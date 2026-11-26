//! Port of [`github.com/jgautheron/goconst`](https://github.com/jgautheron/goconst)
//! v1.11.0 (`visitor.go`, `api.go` `RunWithConfig`), as golangci-lint 2.14's
//! wrapper drives it: one run per package, over every file of it.
//!
//! v1.11 changed what a finding *is*: occurrences are counted per scope (test
//! files apart from the rest), each file reports its **smallest** position,
//! a non-test finding only names a non-test constant, duplicate constants are
//! grouped per scope by value, and `ignore-map-keys` / `eval-const-expressions`
//! / `ignore-functions` exist.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{BasicLit, CallExpr, CompositeLit, Expr, GenDecl, Spec};
use guff::commentmap::{node_end, node_pos};
use guff::position::{Pos, Position};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_constant::Kind;
use guff_types::arena::{ObjectData, TypeData};
use regex::Regex;

use crate::options::{GoconstExcludeType, GoconstOptions};

const TEST_SUFFIX: &str = "_test.go";

/// `ExtendedPos`: where a literal was seen, with the raw position kept for
/// reporting.
#[derive(Clone)]
struct StrPos {
    position: Position,
    pos: u32,
}

/// `ConstType`.
#[derive(Clone)]
struct ConstEntry {
    name: String,
    position: Position,
    pos: u32,
    /// `valueKey`: the exact value, so constants whose display values are
    /// approximations (high-precision numbers) are not grouped together.
    value_key: String,
}

/// `lessPosition`: filename, then line, then column.
fn position_key(p: &Position) -> (&str, i64, i64) {
    (p.filename.as_str(), p.line, p.column)
}

/// The literal's *value*, the way upstream gets it:
///
/// ```go
/// if unquotedStr, err = strconv.Unquote(str); err != nil {
///     // If unquoting fails, manually strip quotes
///     unquotedStr = str[1 : len(str)-1]
/// }
/// ```
///
/// Both halves matter. `min-len` is `utf8.RuneCountInString` of *this* string,
/// so a hand-rolled unquote that leaves `\xc5` as four characters reports a
/// three-occurrence literal that upstream measures as one rune and drops
/// (pyroscope `pkg/validation/validate_test.go:71`). And the value is the map
/// key, so `"\x61bc"` and `"abc"` are one string with six occurrences upstream
/// and two strings with three each here. Anything not starting with a quote
/// (a number, a constant's `String()`) is taken as is.
fn unquote(v: &str) -> String {
    if !(v.starts_with('"') || v.starts_with('`')) {
        return v.to_string();
    }
    match guff_gostd::strconv::unquote(v) {
        Ok(s) => s,
        Err(_) if v.len() >= 2 => v[1..v.len() - 1].to_string(),
        Err(_) => v.to_string(),
    }
}

/// Join ignore patterns with OR, wrapping each in `(...)` like upstream
/// `NewWithIgnorePatterns`. Invalid patterns are skipped.
fn compile_ignore_strings(patterns: &[String]) -> Option<Regex> {
    if patterns.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for p in patterns {
        if p.is_empty() {
            continue;
        }
        // Validate each pattern individually so one bad entry doesn't drop all.
        if Regex::new(p).is_err() {
            continue;
        }
        parts.push(format!("({p})"));
    }
    if parts.is_empty() {
        return None;
    }
    Regex::new(&parts.join("|")).ok()
}

/// The run's state: `Parser` and `treeVisitor` in one.
struct Goconst<'p, 'a> {
    pass: &'p Pass<'a>,
    options: &'p GoconstOptions,
    ignore: Option<Regex>,
    excluded: HashSet<GoconstExcludeType>,
    ignore_functions: HashSet<String>,
    strs: HashMap<String, Vec<StrPos>>,
    string_count: HashMap<String, usize>,
    consts: HashMap<String, Vec<ConstEntry>>,
    /// `skipNodes`: map-key expression subtrees (by source range) the walk
    /// prunes under `ignore-map-keys`.
    skip: Vec<(i64, i64)>,
}

impl Goconst<'_, '_> {
    /// `isSupported`: strings, and numbers when `numbers` is set.
    fn is_supported(&self, lit: &BasicLit) -> bool {
        match lit.kind {
            Some(Token::STRING) => true,
            Some(Token::INT) | Some(Token::FLOAT) => self.options.numbers,
            _ => false,
        }
    }

    /// `isSupportedKind`.
    fn is_supported_kind(&self, kind: Kind) -> bool {
        match kind {
            Kind::String => true,
            Kind::Complex | Kind::Float | Kind::Int => self.options.numbers,
            _ => false,
        }
    }

    /// `numberMin`/`numberMax` against anything `strconv.ParseInt(s, 0, 0)`
    /// accepts — even with `numbers` off, so golangci's defaults `min=3,
    /// max=3` drop `"443"`.
    fn out_of_number_range(&self, s: &str) -> bool {
        let (min, max) = (self.options.number_min, self.options.number_max);
        if min == 0 && max == 0 {
            return false;
        }
        match guff_gostd::strconv::parse_int(s, 0, 64) {
            Ok(i) => (min != 0 && i < min) || (max != 0 && i > max),
            Err(_) => false,
        }
    }

    fn position(&self, pos: u32) -> Position {
        self.pass.fset().position(Pos(pos as i64))
    }

    /// `addString`.
    fn add_string(&mut self, lit: &BasicLit, typ: GoconstExcludeType) {
        if self.excluded.contains(&typ) {
            return;
        }
        let s = unquote(&lit.value);
        if s.is_empty() || s.chars().count() < self.options.min_len {
            return;
        }
        if self.ignore.as_ref().is_some_and(|re| re.is_match(&s)) {
            return;
        }
        if self.out_of_number_range(&s) {
            return;
        }
        *self.string_count.entry(s.clone()).or_default() += 1;
        let pos = lit.value_pos.0 as u32;
        let position = self.position(pos);
        self.strs.entry(s).or_default().push(StrPos { position, pos });
    }

    fn add_expr(&mut self, e: &Expr, typ: GoconstExcludeType) {
        if let Expr::BasicLit(lit) = e {
            if self.is_supported(lit) {
                self.add_string(lit, typ);
            }
        }
    }

    /// `addConst`.
    fn add_const(&mut self, name: &str, val: &str, pos: u32, value_key: Option<String>) {
        let v = unquote(val);
        if v.chars().count() < self.options.min_len {
            return;
        }
        if self.ignore.as_ref().is_some_and(|re| re.is_match(&v)) {
            return;
        }
        let position = self.position(pos);
        let value_key = value_key.filter(|k| !k.is_empty()).unwrap_or_else(|| v.clone());
        let o = self.options;
        let entry = self.consts.entry(v);
        let new = matches!(entry, std::collections::hash_map::Entry::Vacant(_));
        if new || o.find_duplicates || o.match_constant {
            entry.or_default().push(ConstEntry {
                name: name.to_string(),
                position,
                pos,
                value_key,
            });
        }
    }

    /// `constValueStrings`: the display value and the exact-value key.
    fn const_value_strings(v: &guff_constant::Value) -> (String, String) {
        if v.kind() == Kind::String {
            return (
                v.exact_string(),
                format!("string:{}", guff_constant::string_val_lossy(v)),
            );
        }
        (v.to_string(), format!("{:?}:{}", v.kind(), v.exact_string()))
    }

    fn gen_decl(&mut self, g: &GenDecl) {
        if !self.options.match_constant && !self.options.find_duplicates {
            return;
        }
        if g.tok != Some(Token::CONST) {
            return;
        }
        let info = self.pass.types_info();
        let objects = self.pass.pkg().type_artifacts.as_ref().map(|a| &a.objects);
        let eval = self.options.eval_const_expressions && info.is_some();
        for spec in &g.specs {
            let Spec::ValueSpec(val) = spec else {
                continue;
            };
            if eval {
                // `typeInfo.Defs[name].(*types.Const)`: every name, including
                // the implicit repetitions of an iota block.
                let mut added = false;
                for name in &val.names {
                    let c = info
                        .and_then(|i| i.defs.get(&name.id).copied().flatten())
                        .zip(objects)
                        .and_then(|(o, objs)| match objs.get(o) {
                            ObjectData::Const(c) => Some(c.val().clone()),
                            _ => None,
                        });
                    let Some(c) = c else {
                        continue;
                    };
                    if !self.is_supported_kind(c.kind()) {
                        continue;
                    }
                    let (display, key) = Self::const_value_strings(&c);
                    self.add_const(&name.name, &display, name.name_pos.0 as u32, Some(key));
                    added = true;
                }
                if added || val.values.is_empty() {
                    continue;
                }
            }
            for (i, value) in val.values.iter().enumerate() {
                let Some(name) = val.names.get(i) else {
                    continue;
                };
                if eval {
                    let tv = info.and_then(|inf| inf.types.get(&value.id()));
                    let Some(c) = tv.and_then(|tv| tv.val.as_ref()) else {
                        continue;
                    };
                    if !self.is_supported_kind(c.kind()) {
                        continue;
                    }
                    let (display, key) = Self::const_value_strings(c);
                    self.add_const(&name.name, &display, value.pos().0 as u32, Some(key));
                    continue;
                }
                let Expr::BasicLit(lit) = value else {
                    continue;
                };
                if !self.is_supported(lit) {
                    continue;
                }
                self.add_const(&name.name, &lit.value, name.name_pos.0 as u32, None);
            }
        }
    }

    /// `shouldIgnoreCall`: `f(…)` or `pkg.f(…)` named in `ignore-functions`.
    fn should_ignore_call(&self, call: &CallExpr) -> bool {
        if self.ignore_functions.is_empty() {
            return false;
        }
        let name = match call.fun.as_ref() {
            Expr::Ident(id) => id.name.clone(),
            Expr::SelectorExpr(sel) => match sel.x.as_ref() {
                Expr::Ident(x) => format!("{}.{}", x.name, sel.sel.name),
                _ => return false,
            },
            _ => return false,
        };
        self.ignore_functions.contains(&name)
    }

    /// `isMapLiteral`: by type when there is type information.
    fn is_map_literal(&self, lit: &CompositeLit) -> bool {
        let tv = self.pass.types_info().and_then(|i| i.types.get(&lit.id));
        if let (Some(tv), Some(a)) = (tv, self.pass.pkg().type_artifacts.as_ref()) {
            return matches!(a.types.get(tv.typ.underlying(&a.types)), TypeData::Map(_));
        }
        matches!(lit.ty.as_deref(), Some(Expr::MapType(_)))
    }

    fn composite_lit(&mut self, lit: &CompositeLit) {
        let is_map = self.options.ignore_map_keys && self.is_map_literal(lit);
        for item in &lit.elts {
            match item {
                Expr::BasicLit(l) if self.is_supported(l) => {
                    self.add_string(l, GoconstExcludeType::CompositeLit)
                }
                Expr::KeyValueExpr(kv) => {
                    if let Expr::BasicLit(key) = kv.key.as_ref() {
                        // A string literal key can only be a map key, so it
                        // goes even without type information; numeric keys
                        // stay.
                        if self.is_supported(key)
                            && (!self.options.ignore_map_keys || key.kind != Some(Token::STRING))
                        {
                            self.add_string(key, GoconstExcludeType::CompositeLit);
                        }
                    } else if self.options.ignore_map_keys && is_map {
                        // `NamedString("key")`: the whole key subtree goes.
                        let k = walk::expr_ref(&kv.key);
                        self.skip.push((node_pos(k).0, node_end(k).0));
                    }
                    if let Expr::BasicLit(v) = kv.value.as_ref() {
                        if self.is_supported(v) {
                            self.add_string(v, GoconstExcludeType::CompositeLit);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn visit(&mut self, n: NodeRef<'_>) -> bool {
        if !self.skip.is_empty() {
            let (p, e) = (node_pos(n).0, node_end(n).0);
            if self.skip.iter().any(|&(s, t)| s <= p && e <= t) {
                return false;
            }
        }
        match n {
            NodeRef::GenDecl(g) => self.gen_decl(g),
            NodeRef::AssignStmt(a) => {
                for rhs in &a.rhs {
                    self.add_expr(rhs, GoconstExcludeType::Assignment);
                }
            }
            NodeRef::BinaryExpr(b) if b.op == Token::EQL || b.op == Token::NEQ => {
                self.add_expr(&b.x, GoconstExcludeType::Binary);
                self.add_expr(&b.y, GoconstExcludeType::Binary);
            }
            NodeRef::CaseClause(c) => {
                for item in &c.list {
                    self.add_expr(item, GoconstExcludeType::Case);
                }
            }
            NodeRef::ReturnStmt(r) => {
                for item in &r.results {
                    self.add_expr(item, GoconstExcludeType::Return);
                }
            }
            NodeRef::CallExpr(c) => {
                if !self.should_ignore_call(c) {
                    for arg in &c.args {
                        self.add_expr(arg, GoconstExcludeType::Call);
                    }
                }
            }
            NodeRef::CompositeLit(cl) => self.composite_lit(cl),
            _ => {}
        }
        true
    }
}

fn format_message(key: &str, count: usize, matching_const: Option<&str>) -> String {
    // `internal.FormatCode` — `%#q` since golangci-lint 2.14, so a string
    // holding a newline is double-quoted with its escapes.
    let quoted = guff_analysis::golinters::format_code(key);
    if let Some(name) = matching_const {
        let name = guff_analysis::golinters::format_code(name);
        format!(
            "string {quoted} has {count} occurrences, but such constant {name} already exists"
        )
    } else {
        format!("string {quoted} has {count} occurrences, make it a constant")
    }
}

/// `DuplicatePos.String()` — golangci-lint prints the file relative to the
/// working directory.
fn format_duplicate_message(name: &str, first_pos: &Position) -> String {
    let name = guff_analysis::golinters::format_code(name);
    let mut at = first_pos.clone();
    at.filename = guff_analysis::golinters::shortest_rel_path(&at.filename);
    format!("This constant is a duplicate of {name} at {at}")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "goconst requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<GoconstOptions>("goconst")
        .cloned()
        .unwrap_or_default();
    // golangci's `toType` fails the run on an unknown `exclude-types` entry.
    if let Some(bad) = &options.exclude_types_error {
        return Err(format!("unknown type {bad}").into());
    }

    let mut issues: Vec<(u32, String)> = Vec::new();
    {
        let mut g = Goconst {
            pass: &*pass,
            options: &options,
            ignore: compile_ignore_strings(&options.ignore_strings),
            excluded: options.exclude_types.iter().copied().collect(),
            ignore_functions: options
                .ignore_functions
                .iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            strs: HashMap::new(),
            string_count: HashMap::new(),
            consts: HashMap::new(),
            skip: Vec::new(),
        };
        let fset = pass.fset();
        for file in pass.files() {
            if options.ignore_tests && fset.position(file.pos()).filename.ends_with(TEST_SUFFIX) {
                continue;
            }
            g.skip.clear();
            walk::inspect(NodeRef::File(file), |n| match n {
                Some(n) => g.visit(n),
                None => true,
            });
        }
        collect_issues(&g, &mut issues);
    }
    for (pos, msg) in issues {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

/// `RunWithConfig` after the walk: `ProcessResults`, the per-scope string
/// issues, then the per-scope duplicate constants.
fn collect_issues(g: &Goconst<'_, '_>, out: &mut Vec<(u32, String)>) {
    let options = g.options;
    let mut keys: Vec<&String> = g
        .strs
        .keys()
        .filter(|s| g.string_count.get(*s).copied().unwrap_or(0) >= options.min_occurrences)
        .filter(|s| !g.out_of_number_range(s))
        .collect();
    keys.sort();

    for s in keys {
        let mut positions = g.strs[s].clone();
        if positions.is_empty() {
            continue;
        }
        positions.sort_by(|a, b| position_key(&a.position).cmp(&position_key(&b.position)));
        let test_count = positions
            .iter()
            .filter(|p| p.position.filename.ends_with(TEST_SUFFIX))
            .count();
        let non_test_count = positions.len() - test_count;

        // A non-test issue never names a test-only constant; a test issue
        // may name any.
        let (mut any_const, mut non_test_const) = (None, None);
        if options.match_constant {
            if let Some(raw) = g.consts.get(s).filter(|c| !c.is_empty()) {
                let mut csts = raw.clone();
                csts.sort_by(|a, b| position_key(&a.position).cmp(&position_key(&b.position)));
                any_const = Some(csts[0].name.clone());
                non_test_const = csts
                    .iter()
                    .find(|c| !c.position.filename.ends_with(TEST_SUFFIX))
                    .map(|c| c.name.clone());
            }
        }

        let mut seen: HashSet<&str> = HashSet::new();
        for p in &positions {
            if !seen.insert(p.position.filename.as_str()) {
                continue;
            }
            let is_test = p.position.filename.ends_with(TEST_SUFFIX);
            let scope_count = if is_test { test_count } else { non_test_count };
            if scope_count < options.min_occurrences {
                continue;
            }
            let matching = match (&non_test_const, is_test) {
                (Some(c), _) => Some(c.as_str()),
                (None, true) => any_const.as_deref(),
                (None, false) => None,
            };
            out.push((p.pos, format_message(s, scope_count, matching)));
        }
    }

    if !options.find_duplicates {
        return;
    }
    // `duplicateConstGroups`: by exact value, each group shown under its
    // smallest display value, in key order.
    let mut groups: HashMap<&str, (&str, Vec<&ConstEntry>)> = HashMap::new();
    for (display, values) in &g.consts {
        for c in values {
            let key = if c.value_key.is_empty() { display.as_str() } else { c.value_key.as_str() };
            let group = groups.entry(key).or_insert((display.as_str(), Vec::new()));
            if display.as_str() < group.0 {
                group.0 = display.as_str();
            }
            group.1.push(c);
        }
    }
    let mut keys: Vec<&str> = groups
        .iter()
        .filter(|(_, (_, c))| c.len() > 1)
        .map(|(k, _)| *k)
        .collect();
    keys.sort_unstable();
    for key in keys {
        let (_, consts) = &groups[key];
        let (test, non_test): (Vec<&ConstEntry>, Vec<&ConstEntry>) = consts
            .iter()
            .partition(|c| c.position.filename.ends_with(TEST_SUFFIX));
        for mut scope in [non_test, test] {
            scope.sort_by(|a, b| position_key(&a.position).cmp(&position_key(&b.position)));
            let Some(first) = scope.first() else {
                continue;
            };
            for dup in &scope[1..] {
                out.push((dup.pos, format_duplicate_message(&first.name, &first.position)));
            }
        }
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "goconst",
        doc: "Finds repeated strings that could be replaced by a constant",
        url: "https://github.com/jgautheron/goconst",
        run: run as RunFn,
        // AST-only (like upstream jgautheron/goconst). Still useful on
        // packages guff typechecks imperfectly — cobra OSS hunt regression.
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
