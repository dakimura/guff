//! `add-constant` — suggest named constants for magic numbers and repeated string literals.
//!
//! A port of revive v1.17.1 `rule/add_constant.go`. The walk is upstream's
//! `ast.Walk` visitor plus `checkFunc`'s `ast.Inspect`, so the order literals
//! are counted in — which decides which occurrence of a string is the one
//! reported — is the same preorder.

use std::collections::{HashMap, HashSet};

use guff::ast::{BasicLit, CallExpr, Expr, StructType};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;
use regex::Regex;

use crate::config::rule_option_matches;
use crate::failure::Failure;
use crate::settings::RuleArgument;
use crate::util::basic_lit_string_value;

const RULE: &str = "add-constant";
const DEFAULT_STR_LIT_LIMIT: i64 = 2;

/// `AddConstantRule` after `Configure`.
struct Config {
    allow_ints: HashSet<String>,
    allow_floats: HashSet<String>,
    allow_strs: HashSet<String>,
    str_lit_limit: i64,
    ignore_funcs: Vec<Regex>,
}

/// Upstream `Configure`. Every list is empty by default — not even `0` or `1`
/// is allowed — and the limit is 2.
///
/// Upstream rejects a non-string value, a non-integer `maxLitCount`, and an
/// empty or uncompilable `ignoreFuncs` entry with a configuration error. guff
/// does not surface revive's configuration errors (as for the other rules), so
/// such a key is skipped and the rest of the map still applies.
fn configure(pass: &Pass<'_>) -> Config {
    let mut cfg = Config {
        allow_ints: HashSet::new(),
        allow_floats: HashSet::new(),
        allow_strs: HashSet::new(),
        str_lit_limit: DEFAULT_STR_LIT_LIMIT,
        ignore_funcs: Vec::new(),
    };
    let Some(map) = crate::config::rule_arg_map(pass, RULE, 0) else {
        return cfg;
    };
    for (k, v) in &map {
        let RuleArgument::String(s) = v else {
            continue;
        };
        // `allowList.add` splits on "," and does not trim: "0, 1" allows "0"
        // and " 1", so `1` stays reported.
        let list = if rule_option_matches(k, "allowFloats") {
            Some(&mut cfg.allow_floats)
        } else if rule_option_matches(k, "allowInts") {
            Some(&mut cfg.allow_ints)
        } else if rule_option_matches(k, "allowStrs") {
            Some(&mut cfg.allow_strs)
        } else {
            None
        };
        if let Some(list) = list {
            list.extend(s.split(',').map(str::to_string));
        } else if rule_option_matches(k, "maxLitCount") {
            // strconv.Atoi: an optional sign and decimal digits only.
            if let Ok(n) = s.parse::<i64>() {
                cfg.str_lit_limit = n;
            }
        } else if rule_option_matches(k, "ignoreFuncs") {
            for exclude in s.split(',') {
                let exclude = exclude.trim_matches(' ');
                if exclude.is_empty() {
                    continue;
                }
                if let Ok(re) = Regex::new(exclude) {
                    cfg.ignore_funcs.push(re);
                }
            }
        }
    }
    cfg
}

struct Walker<'c> {
    cfg: &'c Config,
    /// Per file: upstream builds a fresh `lintAddConstantRule` in each `Apply`.
    str_lits: HashMap<String, i64>,
    struct_tags: HashSet<i64>,
    failures: Vec<Failure>,
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let cfg = configure(pass);
    let mut failures = Vec::new();
    for file in pass.files() {
        let mut w = Walker {
            cfg: &cfg,
            str_lits: HashMap::new(),
            struct_tags: HashSet::new(),
            failures: Vec::new(),
        };
        w.walk(NodeRef::File(file));
        failures.append(&mut w.failures);
    }
    failures
}

impl Walker<'_> {
    /// `ast.Walk(w, node)` with upstream's `Visit`.
    fn walk(&mut self, node: NodeRef<'_>) {
        walk::inspect(node, |n| match n {
            Some(NodeRef::CallExpr(call)) => {
                self.check_func(call);
                false
            }
            Some(NodeRef::GenDecl(_)) => false, // skip declarations
            Some(NodeRef::BasicLit(lit)) => {
                if !self.is_struct_tag(lit) {
                    self.check_lit(lit);
                }
                true
            }
            Some(NodeRef::StructType(st)) => {
                self.collect_struct_tags(st);
                true
            }
            _ => true,
        });
    }

    /// Upstream `checkFunc`: the call's `Fun` and every argument are inspected
    /// in full. A nested call applies its own `ignoreFuncs` match, a FuncLit is
    /// walked with the visitor (so not under this call's match at all), and a
    /// literal anywhere else under the call — in a composite literal, a binary
    /// expression, an index — is checked unless this call is ignored.
    fn check_func(&mut self, call: &CallExpr) {
        let ignored = self.is_ignored_func(&func_name(call));
        let inspect = |w: &mut Self, e: &Expr| {
            walk::inspect(walk::expr_ref(e), |n| match n {
                Some(NodeRef::CallExpr(inner)) => {
                    w.check_func(inner);
                    false
                }
                Some(fl @ NodeRef::FuncLit(_)) => {
                    w.walk(fl);
                    false
                }
                Some(NodeRef::StructType(st)) => {
                    w.collect_struct_tags(st);
                    true
                }
                Some(NodeRef::BasicLit(lit)) => {
                    if !ignored && !w.is_struct_tag(lit) {
                        w.check_lit(lit);
                    }
                    false
                }
                _ => true,
            });
        };
        inspect(self, &call.fun);
        for arg in &call.args {
            inspect(self, arg);
        }
    }

    fn collect_struct_tags(&mut self, st: &StructType) {
        for field in &st.fields.list {
            if let Some(tag) = &field.tag {
                self.struct_tags.insert(tag.pos().0);
            }
        }
    }

    fn is_struct_tag(&self, lit: &BasicLit) -> bool {
        self.struct_tags.contains(&lit.pos().0)
    }

    fn is_ignored_func(&self, name: &str) -> bool {
        self.cfg.ignore_funcs.iter().any(|re| re.is_match(name))
    }

    fn check_lit(&mut self, lit: &BasicLit) {
        match lit.kind {
            Some(Token::INT) => self.check_num_lit(lit, Token::INT),
            Some(Token::FLOAT) => self.check_num_lit(lit, Token::FLOAT),
            Some(Token::STRING) => self.check_str_lit(lit),
            _ => {}
        }
    }

    fn check_num_lit(&mut self, lit: &BasicLit, kind: Token) {
        let allow = if kind == Token::INT {
            &self.cfg.allow_ints
        } else {
            &self.cfg.allow_floats
        };
        if allow.contains(&lit.value) {
            return;
        }
        self.failures.push(Failure {
            rule: RULE,
            pos: lit.pos().0 as u32,
            message: format!(
                "avoid magic numbers like '{}', create a named constant for it",
                lit.value
            ),
            ..Failure::default()
        });
    }

    fn check_str_lit(&mut self, lit: &BasicLit) {
        const IGNORE_MARKER: i64 = -1;

        if self.cfg.allow_strs.contains(&lit.value) {
            return;
        }
        let count = self.str_lits.entry(lit.value.clone()).or_insert(0);
        if *count <= IGNORE_MARKER {
            return;
        }
        *count += 1;
        if basic_lit_string_value(lit).is_some_and(|s| s.is_empty()) {
            return;
        }
        if *count > self.cfg.str_lit_limit {
            self.failures.push(Failure {
                rule: RULE,
                pos: lit.pos().0 as u32,
                message: format!(
                    "string literal {} appears, at least, {} times, create a named constant for it",
                    lit.value, *count
                ),
                ..Failure::default()
            });
            *count = IGNORE_MARKER; // mark it to avoid failing again on the same literal
        }
    }
}

/// Upstream `getFuncName`: `pkg.F` for a selector on an identifier, `.F` for a
/// selector on a call (`fn().Info`), `F` for an identifier, and "" otherwise
/// (a parenthesized or indexed callee matches only a pattern that matches "").
fn func_name(call: &CallExpr) -> String {
    match call.fun.as_ref() {
        Expr::SelectorExpr(sel) => match sel.x.as_ref() {
            Expr::Ident(x) => format!("{}.{}", x.name, sel.sel.name),
            Expr::CallExpr(_) => format!(".{}", sel.sel.name),
            _ => String::new(),
        },
        Expr::Ident(id) => id.name.clone(),
        _ => String::new(),
    }
}
