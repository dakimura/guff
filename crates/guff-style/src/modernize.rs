//! Port of [`golang.org/x/tools/go/analysis/passes/modernize`](https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/modernize)
//! (golangci-lint wrapper: `linters.settings.modernize.disable`).
//!
//! Implemented checkers (default on):
//! - `any` — `interface{}` → `any` (Go 1.18+)
//! - `plusbuild` — obsolete `// +build` beside `//go:build` (Go 1.18+)
//! - `forvar` — redundant `x := x` in range loops (Go 1.22+)
//! - `rangeint` — `for i := 0; i < n; i++` → `for i := range n` (Go 1.22+; simplified)
//! - `minmax` — if/else → `min`/`max` pattern 1 (Go 1.21+)
//! - `fmtappendf` — `[]byte(fmt.Sprint*)` → `fmt.Append*` (Go 1.19+)
//! - `omitzero` — `json:",omitempty"` on struct fields (Go 1.24+)
//! - `slicessort` — `sort.Slice` with natural order → `slices.Sort` (Go 1.21+)
//! - `stringscutprefix` — `HasPrefix`+`TrimPrefix` → `CutPrefix` (Go 1.20+; pattern 1+2;
//!   strings+bytes)
//! - `slicescontains` — search loop → `slices.Contains`/`ContainsFunc` (Go 1.21+;
//!   return true/false, break body, found=true/false)
//! - `stringsseq` — `range strings.Split/Fields` → `SplitSeq`/`FieldsSeq` (Go 1.24+)
//! - `waitgroupgo` — `Add(1)`+`go`+`Done` → `WaitGroup.Go` (Go 1.25+)
//! - `mapsloop` — `for k, v := range x { m[k] = v }` → `maps.Copy` (Go 1.23+; map→map)
//! - `slicesbackward` — reverse index loop → `slices.Backward` (Go 1.23+; simplified)
//! - `reflecttypefor` — `reflect.TypeOf` → `TypeFor` (Go 1.22+; `(*T)(nil).Elem` + simple vars)
//! - `reflecttypeassert` — `v.Interface().(T)` (comma-ok) → `reflect.TypeAssert[T](v)`
//!   (Go 1.25+)
//! - `testingcontext` — `WithCancel(Background/TODO)`+`defer cancel` → `t.Context` (Go 1.24+)
//! - `unsafefuncs` — `unsafe.Pointer(uintptr(ptr)+…)` → `unsafe.Add` (Go 1.17+)
//! - `importcomment` — obsolete `package p // import "path"` comments
//!   (off by default via Suite parity; see `ModernizeSettings::to_guff_modernize`)
//! - `stringscut` — `Split(N)(…)[0]` → `Cut` (Go 1.18+; strings+bytes Split/SplitN;
//!   off by default — Suite's stringscut is Index→Cut only as of x/tools v0.44)
//! - `newexpr` — `func f(x T) *T { return &x }` → `new(x)` wrappers + call sites
//!   (Go 1.26+; `NewLike` facts)
//! - `errorsastype` — `var e T; if errors.As(err, &e)` → `errors.AsType[T]`
//!   (Go 1.26+; if-stmt only; off by default — not in Suite v0.44)
//! - `stringsbuilder` — `s += x` in a loop → `strings.Builder` (local string vars;
//!   `_test.go` skipped)
//! - `slicesdelete` — `append(s[:i], s[j:]...)` → `slices.Delete` (Go 1.21+;
//!   off by default — commented out upstream as not nil-preserving)
//! - `bloop` — `for … b.N …` → `for b.Loop()` (Go 1.24+; off by default —
//!   commented out upstream)
//! - `stditerators` — `for i := 0; i < x.Len(); i++ { use(x.At(i)) }` →
//!   `for elem := range x.All()` for well-known `go/types`/`reflect` types
//!   (Go 1.24+/1.26+; both C-style and `for i := range x.Len()` forms;
//!   elem-name collision → candidate skipped, DEFERRED fresh-name)
//! - `atomictypes` — `var x int32` + `atomic.AddInt32(&x, …)` → `atomic.Int32`
//!   (Go 1.19+; And/Or need Go 1.23+; Pointer variants / IgnoredFiles gating
//!   DEFERRED)
//!
//! DEFERRED (recognized in `disable` / documented): embedlit,
//! appendclipped (unsafe-by-default upstream),
//! atomictypes Pointer variants / IgnoredFiles,
//! stditerators fresh-name generation on elem collisions / Seq2 dual-component
//! patterns, stringscut Index/Contains
//! patterns, unsafefuncs Slice/String helpers, importcomment Module==nil
//! (GOPATH) skip, mapsloop Insert/Collect (iter.Seq2) / Clone (nil-preserving),
//! slicescontains nested free break/continue analysis full parity,
//! waitgroupgo trailing-Done,
//! stringscutprefix dot-import spelling and `refactor.FreshName` for the
//! `after` / `ok` variables,
//! slicesdelete `int()` conversion / int-shadowing skip, reflecttypefor
//! complicated/unnamed types & unused-var deletion, slicesbackward
//! mutation/non-`s[i]` use analysis full parity, testingcontext sole-use via
//! typeindex, newexpr `new` shadowing / CheckExpr untyped-constant re-typecheck
//! full parity, errorsastype switch/`new(E)`/combined-cond forms, bloop keyed
//! `for i := range b.N`, and remaining rangeint edge cases (post-loop ASSIGN
//! use, ResultVar/PackageVar index) with upstream.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, BinaryExpr, BlockStmt, BranchStmt, CallExpr, CommentGroup, Decl, Expr, Field, File,
    ForStmt, FuncDecl, FuncLit, GenDecl, GoStmt, IfStmt, IncDecStmt, InterfaceType, RangeStmt,
    Spec, Stmt, StructType, UnaryExpr, ValueSpec,
};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::{FileSet, Pos};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::{inspect, typeindex};
// Every checker whose replacement text names a package goes through
// `refactor::add_import` for the prefix *and* the import edits: the file may
// not import the package at all, may import it under an alias, or may shadow
// the name at the fix site. A hard-coded `"slices."` is wrong in all three.
use guff_analysis::refactor;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Fact, FactTypeId, Pass, RunError, RunFn, SuggestedFix,
    TextEdit,
};
use guff_types::alias::unalias_readonly;
use guff_types::api_predicates::{api_identical, api_implements};
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::BasicKind;
use guff_types::map::{map_elem, map_key};
use guff_types::named::named_obj;
use guff_types::object::var::VarKind;
use guff_types::pointer::pointer_elem;
use guff_types::predicates::{is_float, is_integer, is_interface, is_string};
use guff_types::signature::{
    signature_params, signature_recv, signature_results, signature_variadic,
};
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::typestring::type_string;
use guff_types::{ObjectId, OperandMode, TypeId};
use regex::Regex;

use crate::options::ModernizeOptions;

fn enabled(opts: &ModernizeOptions, name: &str) -> bool {
    !opts.disable.iter().any(|d| d == name)
}

/// Give every diagnostic a check just produced its check name, which
/// `format_issue_text` renders as the `name: ` prefix golangci-lint puts in
/// front of every modernize message.
///
/// Two of the twenty-five checks set `category` themselves and the rest left it
/// empty, so `minmax` and `rangeint` shipped their messages bare and
/// `normalize.py` stripped the prefix off upstream's side to keep the tiers
/// green (COMPAT-HARDENING §5, row 6). Stamping it here rather than in each
/// check is the same move the gocritic sweep made when it pushed the checker
/// prefix into `report()`: a new check cannot forget what it never writes.
fn stamp_category(pending: &mut [Diagnostic], from: usize, name: &str) {
    for d in &mut pending[from..] {
        if d.category.is_empty() {
            d.category = name.to_string();
        }
    }
}

fn go_at_least(pass: &Pass<'_>, pos: u32, want: &str) -> bool {
    code::version_compare(&code::stdlib_version(pass, pos), want) >= 0
}

fn ident_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(id) => Some(id.name.as_str()),
        _ => None,
    }
}

/// The source text a node occupies, which is what upstream's `Format` prints.
///
/// [`expr_text`] renders the tree by hand, and a hand-written printer has
/// holes — a call with two arguments, a function literal, a composite literal.
/// A hole there does not merely produce a worse fix: the caller drops the
/// finding, because it cannot build one. Reading the bytes back has no holes.
///
/// syncthing `internal/db/sqlite/folderdb_update.go` is the shape that showed
/// it: `globIdx := slices.IndexFunc(es, func(e fileRow) bool { … })` followed by
/// `if globIdx < 0 { globIdx = 0 }` is a `max`, and the whole diagnostic went
/// missing because the preceding call could not be rendered.
fn node_text(pass: &Pass<'_>, pos: guff::Pos, end: guff::Pos) -> Option<String> {
    let fset = pass.fset();
    let file = fset.file(pos)?;
    let (lo, hi) = (file.offset(pos), file.offset(end));
    if lo < 0 || hi < lo {
        return None;
    }
    let base = std::path::Path::new(file.name())
        .file_name()
        .and_then(|s| s.to_str())?
        .to_string();
    let pkg = pass.pkg();
    let idx = pkg
        .compiled_go_files
        .iter()
        .position(|p| p.file_name().and_then(|s| s.to_str()) == Some(base.as_str()))?;
    let owned;
    let src: &[u8] = match pkg.source_bytes(idx) {
        Some(bytes) => bytes,
        None => match fs::read(&pkg.compiled_go_files[idx]) {
            Ok(bytes) => {
                owned = bytes;
                &owned
            }
            Err(_) => return None,
        },
    };
    let (lo, hi) = (lo as usize, hi as usize);
    if hi > src.len() {
        return None;
    }
    String::from_utf8(src[lo..hi].to_vec()).ok()
}

/// [`expr_text`], falling back to the node's own source text.
fn expr_text_src(pass: &Pass<'_>, expr: &Expr) -> Option<String> {
    expr_text(expr).or_else(|| node_text(pass, expr.pos(), expr.end()))
}

fn expr_text(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::BasicLit(lit) => Some(lit.value.clone()),
        Expr::SelectorExpr(sel) => {
            let x = expr_text(&sel.x)?;
            Some(format!("{x}.{}", sel.sel.name))
        }
        Expr::CallExpr(call) if call.args.len() == 1 => {
            let fun = expr_text(&call.fun)?;
            let arg = expr_text(&call.args[0])?;
            Some(format!("{fun}({arg})"))
        }
        Expr::IndexExpr(ix) => {
            let x = expr_text(&ix.x)?;
            let index = expr_text(&ix.index)?;
            Some(format!("{x}[{index}]"))
        }
        Expr::BinaryExpr(b) => {
            let x = expr_text(&b.x)?;
            let y = expr_text(&b.y)?;
            Some(format!("{x} {} {y}", b.op))
        }
        Expr::UnaryExpr(u) => {
            let x = expr_text(&u.x)?;
            Some(format!("{}{x}", u.op))
        }
        Expr::ParenExpr(p) => expr_text(&p.x).map(|inner| format!("({inner})")),
        _ => None,
    }
}

fn exprs_equal(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(x), Expr::Ident(y)) => x.name == y.name,
        (Expr::BasicLit(x), Expr::BasicLit(y)) => x.kind == y.kind && x.value == y.value,
        (Expr::SelectorExpr(x), Expr::SelectorExpr(y)) => {
            x.sel.name == y.sel.name && exprs_equal(&x.x, &y.x)
        }
        (Expr::ParenExpr(x), Expr::ParenExpr(y)) => exprs_equal(&x.x, &y.x),
        (Expr::IndexExpr(x), Expr::IndexExpr(y)) => {
            exprs_equal(&x.x, &y.x) && exprs_equal(&x.index, &y.index)
        }
        (Expr::BinaryExpr(x), Expr::BinaryExpr(y)) => {
            x.op == y.op && exprs_equal(&x.x, &y.x) && exprs_equal(&x.y, &y.y)
        }
        (Expr::UnaryExpr(x), Expr::UnaryExpr(y)) => x.op == y.op && exprs_equal(&x.x, &y.x),
        (Expr::StarExpr(x), Expr::StarExpr(y)) => exprs_equal(&x.x, &y.x),
        (Expr::CallExpr(x), Expr::CallExpr(y)) => {
            exprs_equal(&x.fun, &y.fun)
                && x.args.len() == y.args.len()
                && x.ellipsis.is_valid() == y.ellipsis.is_valid()
                && x.args
                    .iter()
                    .zip(y.args.iter())
                    .all(|(a, b)| exprs_equal(a, b))
        }
        _ => false,
    }
}

fn is_empty_interface(iface: &InterfaceType) -> bool {
    iface.methods.list.is_empty()
}

fn check_any(pass: &Pass<'_>, iface: &InterfaceType, pending: &mut Vec<Diagnostic>) {
    if !is_empty_interface(iface) {
        return;
    }
    let pos = iface.interface_.0 as u32;
    if !go_at_least(pass, pos, "go1.18") {
        return;
    }
    let end = iface.methods.end().0 as u32;
    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: "interface{} can be replaced by any".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace interface{} by any".into(),
            text_edits: vec![TextEdit {
                pos,
                end,
                new_text: "any".into(),
            }],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

fn check_plusbuild(file: &File, pending: &mut Vec<Diagnostic>) {
    for group in &file.comments {
        let mut saw_go_build = false;
        for c in &group.list {
            let text = c.text.as_str();
            if saw_go_build && text.starts_with("// +build ") {
                let pos = c.slash.0 as u32;
                let end = c.end().0 as u32;
                pending.push(Diagnostic {
                    pos,
                    end,
                    category: String::new(),
                    message: "+build line is no longer needed".into(),
                    suggested_fixes: vec![SuggestedFix {
                        message: "Remove obsolete +build line".into(),
                        text_edits: vec![TextEdit {
                            pos,
                            end,
                            new_text: String::new(),
                        }],
                    }],
                    related: Vec::new(),
                    url: String::new(),
                    severity: String::new(),
                    ..Diagnostic::default()
                });
                break;
            }
            if text.starts_with("//go:build ") {
                saw_go_build = true;
            }
        }
    }
}

fn is_loop_var_redecl(assign: &AssignStmt, loop_vars: &HashSet<&str>) -> bool {
    if assign.tok != Some(Token::DEFINE) || assign.lhs.len() != assign.rhs.len() {
        return false;
    }
    for (lhs, rhs) in assign.lhs.iter().zip(assign.rhs.iter()) {
        let Some(l) = ident_name(lhs) else {
            return false;
        };
        let Some(r) = ident_name(rhs) else {
            return false;
        };
        if l != r || !loop_vars.contains(l) {
            return false;
        }
    }
    true
}

fn check_forvar(pass: &Pass<'_>, range_stmt: &RangeStmt, pending: &mut Vec<Diagnostic>) {
    if range_stmt.tok != Some(Token::DEFINE) {
        return;
    }
    let pos = range_stmt.for_.0 as u32;
    if !go_at_least(pass, pos, "go1.22") {
        return;
    }
    let mut loop_vars = HashSet::new();
    if let Some(name) = range_stmt.key.as_ref().and_then(ident_name) {
        if name != "_" {
            loop_vars.insert(name);
        }
    }
    if let Some(name) = range_stmt.value.as_ref().and_then(ident_name) {
        if name != "_" {
            loop_vars.insert(name);
        }
    }
    if loop_vars.is_empty() {
        return;
    }
    for stmt in &range_stmt.body.list {
        let Stmt::AssignStmt(assign) = stmt else {
            break;
        };
        if !is_loop_var_redecl(assign, &loop_vars) {
            break;
        }
        let pos = assign
            .lhs
            .first()
            .map(|e| e.pos().0 as u32)
            .unwrap_or(assign.tok_pos.0 as u32);
        let end = assign
            .rhs
            .last()
            .map(|e| e.end().0 as u32)
            .unwrap_or(assign.tok_pos.0 as u32);
        pending.push(Diagnostic {
            pos,
            end,
            category: String::new(),
            message: "copying variable is unneeded".into(),
            suggested_fixes: vec![SuggestedFix {
                message: "Remove redundant re-declaration".into(),
                text_edits: vec![TextEdit {
                    pos,
                    end,
                    new_text: String::new(),
                }],
            }],
            related: Vec::new(),
            url: String::new(),
            severity: String::new(),
            ..Diagnostic::default()
        });
    }
}

fn is_simple_inc(post: &Stmt, index_name: &str) -> bool {
    match post {
        Stmt::IncDecStmt(IncDecStmt { x, tok, .. }) => {
            *tok == Token::INC && ident_name(x) == Some(index_name)
        }
        Stmt::AssignStmt(a)
            if a.tok == Some(Token::AddAssign)
                && a.lhs.len() == 1
                && a.rhs.len() == 1
                && ident_name(&a.lhs[0]) == Some(index_name) =>
        {
            matches!(&a.rhs[0], Expr::BasicLit(lit) if lit.value == "1")
        }
        _ => false,
    }
}

fn expr_has_constant_value(pass: &Pass<'_>, expr: &Expr) -> bool {
    pass.types_info()
        .and_then(|info| info.types.get(&expr.id()))
        .and_then(|tv| tv.val.as_ref())
        .is_some()
}

fn is_package_level_obj(pass: &Pass<'_>, obj: ObjectId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(parent) = obj.parent(&artifacts.objects) else {
        return false;
    };
    let Some(obj_pkg) = obj.pkg(&artifacts.objects) else {
        return false;
    };
    parent == artifacts.packages.get(obj_pkg).scope()
}

/// Upstream `isScalarLvalue` over all package uses of `obj`.
///
/// Rejects limits that are assigned or address-taken anywhere (not just in the
/// loop), matching rangeint's typeindex check — e.g. `k := …; for i < k; …;
/// k = …` must not modernize the first loop.
thread_local! {
    /// Objects that are assigned, incremented or address-taken somewhere in the
    /// package this thread is linting — the answer to
    /// [`var_is_scalar_lvalue_anywhere`] for every object at once.
    ///
    /// The question is asked once per `for i := 0; i < N; i++` loop whose limit
    /// is an identifier (modernize's `rangeint`), and each ask used to walk
    /// **every file in the package** looking for that one object. The walk
    /// stops early once it finds a hit, but the common answer is "no" — an
    /// unmodified loop bound — and "no" costs a full package traversal. Same
    /// shape as SA4023's full-file-walk-per-candidate, which was fixed the same
    /// way (PERF_TASKS_V3 V1-13).
    ///
    /// Keyed by `Package::id`: an `ObjectId` indexes its own package's arena,
    /// and a rayon worker moves between packages, so an unkeyed cache would
    /// answer about a different object of the same number.
    static SCALAR_LVALUES: std::cell::RefCell<(String, HashSet<ObjectId>)> =
        std::cell::RefCell::new((String::new(), HashSet::new()));
}

/// Collect every object used as a scalar lvalue anywhere in the package.
///
/// Mirrors the per-object test this replaces arm for arm, including its one
/// asymmetry: for `x, y := …` the object comes from `Uses` (a `:=` that
/// reassigns an existing `x` records it there), and everywhere else from
/// [`ident_obj`], which prefers `Defs`.
fn collect_scalar_lvalues(pass: &Pass<'_>) -> HashSet<ObjectId> {
    let mut out = HashSet::new();
    let Some(info) = pass.types_info() else {
        return out;
    };
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::AssignStmt(a) => {
                    for lhs in &a.lhs {
                        let Expr::Ident(id) = unparen_expr(lhs) else {
                            continue;
                        };
                        if a.tok == Some(Token::DEFINE) {
                            // `x, y := …` reassignment of an existing x appears in Uses.
                            out.extend(info.uses.get(&id.id).copied());
                        } else {
                            out.extend(ident_obj(pass, id));
                        }
                    }
                }
                NodeRef::IncDecStmt(inc) => {
                    if let Expr::Ident(id) = unparen_expr(&inc.x) {
                        out.extend(ident_obj(pass, id));
                    }
                }
                NodeRef::UnaryExpr(u) if u.op == Token::AND => {
                    if let Expr::Ident(id) = unparen_expr(&u.x) {
                        out.extend(ident_obj(pass, id));
                    }
                }
                NodeRef::RangeStmt(rs) if rs.tok == Some(Token::ASSIGN) => {
                    for side in rs.key.iter().chain(rs.value.iter()) {
                        if let Expr::Ident(id) = unparen_expr(side) {
                            out.extend(ident_obj(pass, id));
                        }
                    }
                }
                _ => {}
            }
            true
        });
    }
    out
}

fn var_is_scalar_lvalue_anywhere(pass: &Pass<'_>, obj: ObjectId) -> bool {
    if pass.types_info().is_none() {
        return false;
    }
    let pkg_id = pass.pkg().id.as_str();
    let hit = SCALAR_LVALUES.with(|c| {
        let c = c.borrow();
        (c.0 == pkg_id).then(|| c.1.contains(&obj))
    });
    if let Some(hit) = hit {
        return hit;
    }
    let set = collect_scalar_lvalues(pass);
    let answer = set.contains(&obj);
    SCALAR_LVALUES.with(|c| {
        *c.borrow_mut() = (pkg_id.to_string(), set);
    });
    answer
}

fn limit_ident_is_safe(pass: &Pass<'_>, id: &guff::ast::Ident) -> bool {
    let Some(info) = pass.types_info() else {
        // Without types, keep the old permissive Ident allowance so AST-only
        // runs still suggest safe BasicLit/simple cases; callers already accept
        // BasicLit separately. Conservatively reject bare Idents.
        return false;
    };
    let Some(obj) = info.uses.get(&id.id).copied() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let ObjectData::Var(v) = artifacts.objects.get(obj) else {
        return false;
    };
    // Exported package-level vars may be mutated in other packages.
    if obj.exported(&artifacts.objects) && is_package_level_obj(pass, obj) {
        return false;
    }
    // PackageVar / Field kinds are still OK if unexported and not mutated here.
    let _ = v;
    !var_is_scalar_lvalue_anywhere(pass, obj)
}

fn limit_is_safe(pass: &Pass<'_>, limit: &Expr) -> bool {
    // Upstream rangeint: constant, or local/unexported Ident that is not
    // assigned or address-taken — never field selectors like `s.size`.
    match limit {
        Expr::ParenExpr(p) => limit_is_safe(pass, &p.x),
        Expr::CallExpr(call) => {
            // Allow len(slice) only (not len(map)); then require the slice
            // operand itself to be a safe limit (so `&chks` / `chks =` skip).
            code::is_call_to(pass, call, "len")
                && call.args.len() == 1
                && matches!(type_kind(pass, &call.args[0]), Some(TypeKind::Slice))
                && limit_is_safe(pass, &call.args[0])
        }
        Expr::BasicLit(_) => true,
        other if expr_has_constant_value(pass, other) => true,
        Expr::Ident(id) => limit_ident_is_safe(pass, id),
        _ => false,
    }
}

/// Whether the loop index is *read* anywhere in the loop body.
///
/// Upstream `rangeint` deletes `i := ` from the fix when it is not
/// (`rangeint.go`, `if !used && init.Tok == token.DEFINE`): `for i := 0; i < n;
/// i++` with no use of `i` has to become `for range n`, because a range clause
/// that binds `i` and never reads it is `declared and not used` — the rewrite
/// would not compile.
///
/// Resolution is by object, not by name, so an inner `i` shadowing the loop's
/// does not count as a use. An unresolvable index answers `true`: keeping the
/// variable is the conservative half of this decision.
fn index_used_in_body(pass: &Pass<'_>, body: &guff::ast::BlockStmt, index: &Expr) -> bool {
    let Expr::Ident(index_id) = index else {
        return true;
    };
    let Some(index_obj) = ident_obj(pass, index_id) else {
        return true;
    };
    let Some(info) = pass.types_info() else {
        return true;
    };
    let mut used = false;
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        if used {
            return false;
        }
        let Some(NodeRef::Ident(id)) = n else {
            return true;
        };
        // Upstream tests `info.Uses[id] == v`: a *use*, not the definition.
        if info.uses.get(&id.id).copied() == Some(index_obj) {
            used = true;
            return false;
        }
        true
    });
    used
}

fn index_is_scalar_lvalue_in(body: &guff::ast::BlockStmt, index_name: &str) -> bool {
    let mut found = false;
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        match n {
            NodeRef::AssignStmt(a) if a.tok != Some(Token::DEFINE) => {
                if a.lhs.iter().any(|e| ident_name(e) == Some(index_name)) {
                    found = true;
                }
            }
            NodeRef::IncDecStmt(inc) if ident_name(&inc.x) == Some(index_name) => {
                found = true;
            }
            NodeRef::UnaryExpr(u)
                if u.op == Token::AND && ident_name(&u.x) == Some(index_name) =>
            {
                found = true;
            }
            NodeRef::RangeStmt(rs) if rs.tok == Some(Token::ASSIGN) => {
                if rs
                    .key
                    .as_ref()
                    .is_some_and(|k| ident_name(k) == Some(index_name))
                    || rs
                        .value
                        .as_ref()
                        .is_some_and(|v| ident_name(v) == Some(index_name))
                {
                    found = true;
                }
            }
            _ => {}
        }
        true
    });
    found
}

/// Upstream's post-loop use check, for the `for i = 0; …` spelling only: a
/// range loop leaves `i` holding `limit-1` rather than `limit`, so the rewrite
/// is only offered when nothing reads `i` afterwards. With `i := 0` the
/// variable is scoped to the loop and the question does not arise.
///
/// Keyed on the object rather than the name — upstream walks the loop's
/// enclosing statement list and compares `info.Uses[id]` — so a different `i`
/// in a later function is not mistaken for this one. dapr's
/// `pkg/api/http/directmessaging.go` returns its `i` after the loop.
///
/// DEFERRED: upstream also rejects a `defer` *above* the loop that reads `i`,
/// since it runs after the loop body. Position alone cannot see that one.
fn index_used_after_loop(pass: &Pass<'_>, for_stmt: &ForStmt, index: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Expr::Ident(id) = index else {
        return false;
    };
    let Some(obj) = info.uses.get(&id.id).copied() else {
        return false;
    };
    let loop_end = for_stmt.body.end().0;
    let mut used = false;
    for file in pass.files() {
        guff::walk::preorder(guff::walk::NodeRef::File(file), |n| {
            if used {
                return false;
            }
            if let guff::walk::NodeRef::Ident(ident) = n {
                if ident.pos().0 > loop_end && info.uses.get(&ident.id).copied() == Some(obj) {
                    used = true;
                    return false;
                }
            }
            true
        });
        if used {
            break;
        }
    }
    used
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TypeKind {
    Slice,
    Array,
    String,
    Struct,
    Map,
    Other,
}

fn type_kind(pass: &Pass<'_>, expr: &Expr) -> Option<TypeKind> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = info.types.get(&expr.id())?.typ;
    let typ = unalias_readonly(&artifacts.types, typ);
    let under = typ.underlying(&artifacts.types);
    Some(match artifacts.types.get(under) {
        TypeData::Slice(_) => TypeKind::Slice,
        TypeData::Array(_) => TypeKind::Array,
        TypeData::Struct(_) => TypeKind::Struct,
        TypeData::Map(_) => TypeKind::Map,
        _ if is_string(&artifacts.types, under) => TypeKind::String,
        _ => TypeKind::Other,
    })
}

fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    if let Some(tav) = info.types.get(&expr.id()) {
        return Some(tav.typ);
    }
    // Range variables (and other defs) may only appear in defs/uses.
    let Expr::Ident(id) = expr else {
        return None;
    };
    let obj = ident_obj(pass, id)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    obj.typ(&artifacts.objects)
}

fn types_identical(pass: &Pass<'_>, a: TypeId, b: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    api_identical(&mut types, &artifacts.objects, &artifacts.packages, a, b)
}

fn underlying_map(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = type_of(pass, expr)?;
    let typ = unalias_readonly(&artifacts.types, typ);
    let under = typ.underlying(&artifacts.types);
    match artifacts.types.get(under) {
        TypeData::Map(_) => Some(under),
        _ => None,
    }
}

fn ident_obj(pass: &Pass<'_>, id: &guff::ast::Ident) -> Option<ObjectId> {
    let info = pass.types_info()?;
    info.defs
        .get(&id.id)
        .copied()
        .flatten()
        .or_else(|| info.uses.get(&id.id).copied())
}

fn expr_uses_loop_vars(pass: &Pass<'_>, expr: &Expr, key: &Expr, value: &Expr) -> bool {
    let Some(k_obj) = (match key {
        Expr::Ident(id) => ident_obj(pass, id),
        _ => None,
    }) else {
        return false;
    };
    let Some(v_obj) = (match value {
        Expr::Ident(id) => ident_obj(pass, id),
        _ => None,
    }) else {
        return false;
    };
    let mut used = false;
    walk::inspect(walk::expr_ref(expr), |n| {
        let Some(n) = n else {
            return true;
        };
        if let NodeRef::Ident(id) = n {
            if let Some(obj) = ident_obj(pass, id) {
                if obj == k_obj || obj == v_obj {
                    used = true;
                    return false;
                }
            }
        }
        true
    });
    used
}

fn is_float_expr(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(tav) = info.types.get(&expr.id()) else {
        return false;
    };
    is_float(&artifacts.types, tav.typ.underlying(&artifacts.types))
}

fn field_type_is_struct(pass: &Pass<'_>, field: &Field) -> bool {
    let Some(ty) = field.ty.as_ref() else {
        return false;
    };
    type_kind(pass, ty) == Some(TypeKind::Struct)
}

fn check_rangeint(pass: &Pass<'_>, for_stmt: &ForStmt, pending: &mut Vec<Diagnostic>) {
    let pos = for_stmt.for_.0 as u32;
    if !go_at_least(pass, pos, "go1.22") {
        return;
    }
    let Some(Stmt::AssignStmt(init)) = for_stmt.init.as_deref() else {
        return;
    };
    if init.lhs.len() != 1 || init.rhs.len() != 1 {
        return;
    }
    if init.tok != Some(Token::DEFINE) && init.tok != Some(Token::ASSIGN) {
        return;
    }
    let Some(index_name) = ident_name(&init.lhs[0]) else {
        return;
    };
    if !code::is_integer_constant(pass, &init.rhs[0], 0) {
        return;
    }
    let Some(Expr::BinaryExpr(BinaryExpr { x, op, y, .. })) = for_stmt.cond.as_ref() else {
        return;
    };
    if *op != Token::LSS || ident_name(x) != Some(index_name) {
        return;
    }
    let Some(post) = for_stmt.post.as_deref() else {
        return;
    };
    if !is_simple_inc(post, index_name) {
        return;
    }
    if !limit_is_safe(pass, y) {
        return;
    }
    // Upstream: reject if the loop index is assigned or address-taken in the body
    // (`for range int` ignores such assignments).
    if index_is_scalar_lvalue_in(&for_stmt.body, index_name) {
        return;
    }
    // Upstream: for `for i = 0; …` (ASSIGN), skip if `i` is used after the loop.
    if init.tok == Some(Token::ASSIGN) && index_used_after_loop(pass, for_stmt, &init.lhs[0]) {
        return;
    }
    let Some(limit_text) = expr_text_src(pass, y) else {
        return;
    };
    // Prefer `range slice` when limit is len(slice); otherwise range-over-int.
    // SuggestedFix uses the concrete range operand; the diagnostic message
    // always says "range over int" (x/tools modernize / golangci parity).
    let range_expr = if let Expr::CallExpr(call) = y.as_ref() {
        if code::is_call_to(pass, call, "len") && call.args.len() == 1 {
            expr_text(&call.args[0]).unwrap_or(limit_text.clone())
        } else {
            limit_text.clone()
        }
    } else {
        limit_text.clone()
    };

    let end = for_stmt
        .post
        .as_ref()
        .map(|p| p.end().0 as u32)
        .unwrap_or(for_stmt.for_.0 as u32);

    let new_text = if init.tok == Some(Token::DEFINE) {
        if index_used_in_body(pass, &for_stmt.body, &init.lhs[0]) {
            format!("for {index_name} := range {range_expr}")
        } else {
            format!("for range {range_expr}")
        }
    } else {
        // `for i = 0; …` reuses a variable declared elsewhere, so there is no
        // declaration to drop.
        format!("for {index_name} = range {range_expr}")
    };

    // Upstream reports the *init statement*, not the `for` keyword
    // (`Pos: init.Pos()`, `End: loop.Post.End()`) — the range of text the fix
    // rewrites. Four columns to the right of where guff was pointing.
    let report_pos = init.lhs[0].pos().0 as u32;

    pending.push(Diagnostic {
        pos: report_pos,
        end,
        category: String::new(),
        message: "for loop can be modernized using range over int".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace 3-clause for with range-over-int".into(),
            text_edits: vec![TextEdit { pos, end, new_text }],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

/// `isSimpleAssign` (x/tools `modernize/minmax.go:275`): `lhs = rhs` **or**
/// `lhs := rhs`, one name and one value.
fn simple_assign(stmt: &Stmt) -> Option<&AssignStmt> {
    match stmt {
        Stmt::AssignStmt(a)
            if matches!(a.tok, Some(Token::ASSIGN | Token::DEFINE))
                && a.lhs.len() == 1
                && a.rhs.len() == 1 =>
        {
            Some(a)
        }
        _ => None,
    }
}

/// minmax's **pattern 2** (x/tools `modernize/minmax.go:139-207`), which guff
/// did not have at all:
///
/// ```go
/// v := x
/// if v > y {
///     v = y
/// }
/// ```
///
/// It needs the statement *above* the `if`, so it runs over blocks rather than
/// over `IfStmt` nodes. Its message says "if statement", where pattern 1's says
/// "if/else statement" — the two are told apart by that word alone, and
/// syncthing writes the second form nine times.
///
/// A `select` comm clause (`case v := <-ch:`) cannot be rewritten and upstream
/// rejects it explicitly; here it is excluded for free, because that assignment
/// is the clause's `Comm`, not a statement in a block's list.
fn check_minmax_block(pass: &Pass<'_>, block: &BlockStmt, pending: &mut Vec<Diagnostic>) {
    for i in 1..block.list.len() {
        let Stmt::IfStmt(if_stmt) = &block.list[i] else {
            continue;
        };
        if if_stmt.init.is_some() || if_stmt.else_.is_some() {
            continue;
        }
        if !go_at_least(pass, if_stmt.if_.0 as u32, "go1.21") {
            continue;
        }
        let Expr::BinaryExpr(compare) = &if_stmt.cond else {
            continue;
        };
        let Some(mut sign) = inequality_sign(compare.op) else {
            continue;
        };
        let Some(tassign) = is_assign_block(&if_stmt.body) else {
            continue;
        };
        let Some(fassign) = simple_assign(&block.list[i - 1]) else {
            continue;
        };
        let lhs = &tassign.lhs[0];
        let rhs = &tassign.rhs[0];
        let lhs0 = &fassign.lhs[0];
        let rhs0 = &fassign.rhs[0];
        if !code::equal_syntax(lhs, lhs0) {
            continue;
        }
        let mut a = compare.x.as_ref();
        let mut b = compare.y.as_ref();
        if code::equal_syntax(rhs, a)
            && (code::equal_syntax(rhs0, b) || code::equal_syntax(lhs0, b))
        {
            // keep sign
        } else if (code::equal_syntax(rhs0, a) || code::equal_syntax(lhs0, a))
            && code::equal_syntax(rhs, b)
        {
            sign = -sign;
        } else {
            continue;
        }
        // `maybeNaN(tLHS)` — upstream asks the *assigned* variable's type, once,
        // before either pattern runs.
        if is_float_expr(pass, lhs) {
            continue;
        }
        // `lhs0` was allowed to stand in for `rhs0` in the matching above, but
        // the fix must not write `v = min(v, y)`: the `=` could have been a `:=`.
        if code::equal_syntax(lhs0, a) {
            a = rhs0;
        } else if code::equal_syntax(lhs0, b) {
            b = rhs0;
        }
        let sym = if sign < 0 { "min" } else { "max" };
        let (Some(lhs_text), Some(a_text), Some(b_text)) = (
            expr_text_src(pass, lhs),
            expr_text_src(pass, a),
            expr_text_src(pass, b),
        ) else {
            continue;
        };
        let tok_text = if fassign.tok == Some(Token::DEFINE) {
            ":="
        } else {
            "="
        };
        let fix_pos = lhs0.pos().0 as u32;
        let fix_end = if_stmt.body.rbrace.0 as u32 + 1;
        pending.push(Diagnostic {
            // `Pos: compare.Pos()`, like pattern 1.
            pos: compare.x.pos().0 as u32,
            end: compare.y.end().0 as u32,
            category: String::new(),
            message: format!("if statement can be modernized using {sym}"),
            suggested_fixes: vec![SuggestedFix {
                // Upstream's two patterns have their fix messages the other way
                // round from their diagnostics; this is theirs, not a slip.
                message: format!("Replace if/else with {sym}"),
                text_edits: vec![TextEdit {
                    pos: fix_pos,
                    end: fix_end,
                    new_text: format!("{lhs_text} {tok_text} {sym}({a_text}, {b_text})"),
                }],
            }],
            related: Vec::new(),
            url: String::new(),
            severity: String::new(),
            ..Diagnostic::default()
        });
    }
}

fn is_assign_block(body: &guff::ast::BlockStmt) -> Option<&AssignStmt> {
    if body.list.len() != 1 {
        return None;
    }
    match &body.list[0] {
        Stmt::AssignStmt(a)
            if a.tok == Some(Token::ASSIGN) && a.lhs.len() == 1 && a.rhs.len() == 1 =>
        {
            Some(a)
        }
        _ => None,
    }
}

fn inequality_sign(op: Token) -> Option<i32> {
    match op {
        Token::LSS | Token::LEQ => Some(-1),
        Token::GTR | Token::GEQ => Some(1),
        _ => None,
    }
}

fn check_minmax(pass: &Pass<'_>, if_stmt: &IfStmt, pending: &mut Vec<Diagnostic>) {
    if if_stmt.init.is_some() {
        return;
    }
    let pos = if_stmt.if_.0 as u32;
    if !go_at_least(pass, pos, "go1.21") {
        return;
    }
    let Expr::BinaryExpr(compare) = &if_stmt.cond else {
        return;
    };
    let Some(mut sign) = inequality_sign(compare.op) else {
        return;
    };
    let Some(tassign) = is_assign_block(&if_stmt.body) else {
        return;
    };
    let Some(Stmt::BlockStmt(fblock)) = if_stmt.else_.as_deref() else {
        return;
    };
    let Some(fassign) = is_assign_block(fblock) else {
        return;
    };
    // `astutil.EqualSyntax`, not "same value": upstream compares the written
    // shape, so `len(a)` written twice matches itself even though a call is not
    // provably pure (x/tools `modernize/minmax.go:95-101`).
    if !code::equal_syntax(&tassign.lhs[0], &fassign.lhs[0]) {
        return;
    }
    let a = compare.x.as_ref();
    let b = compare.y.as_ref();
    let rhs = &tassign.rhs[0];
    let rhs2 = &fassign.rhs[0];
    if code::equal_syntax(rhs, a) && code::equal_syntax(rhs2, b) {
        // keep sign
    } else if code::equal_syntax(rhs2, a) && code::equal_syntax(rhs, b) {
        sign = -sign;
    } else {
        return;
    }
    // Skip floats (NaN concerns).
    if is_float_expr(pass, a) || is_float_expr(pass, b) {
        return;
    }
    let sym = if sign < 0 { "min" } else { "max" };
    let Some(lhs_text) = expr_text_src(pass, &tassign.lhs[0]) else {
        return;
    };
    let Some(a_text) = expr_text_src(pass, a) else {
        return;
    };
    let Some(b_text) = expr_text_src(pass, b) else {
        return;
    };
    let end = if_stmt
        .else_
        .as_ref()
        .map(|e| e.end().0 as u32)
        .unwrap_or(if_stmt.body.rbrace.0 as u32);
    pending.push(Diagnostic {
        // `Pos: compare.Pos()` (x/tools `modernize/minmax.go:121`) — the start of
        // `a < b`, not the `<`. Two columns apart on the same line, which only
        // the golden tier compares.
        pos: compare.x.pos().0 as u32,
        end: compare.y.end().0 as u32,
        category: String::new(),
        message: format!("if/else statement can be modernized using {sym}"),
        suggested_fixes: vec![SuggestedFix {
            message: format!("Replace if statement with {sym}"),
            text_edits: vec![TextEdit {
                pos,
                end,
                new_text: format!("{lhs_text} = {sym}({a_text}, {b_text})"),
            }],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

fn is_byte_slice_type_expr(fun: &Expr) -> bool {
    // `[]byte` is an ArrayType with no length (Go AST convention for slices).
    let Expr::ArrayType(arr) = fun else {
        return false;
    };
    if arr.len.is_some() {
        return false;
    }
    matches!(arr.elt.as_ref(), Expr::Ident(id) if id.name == "byte" || id.name == "uint8")
}

fn is_byte_slice_conversion(pass: &Pass<'_>, call: &CallExpr) -> bool {
    if call.args.len() != 1 {
        return false;
    }
    if is_byte_slice_type_expr(&call.fun) {
        return true;
    }
    // Fallback: typed conversion via types info.
    let info = pass.types_info();
    let artifacts = pass.pkg().type_artifacts.as_ref();
    let (Some(info), Some(artifacts)) = (info, artifacts) else {
        return false;
    };
    let Some(tav) = info.types.get(&call.fun.id()) else {
        return false;
    };
    if tav.mode != OperandMode::TypeExpr {
        return false;
    }
    // `types.Identical(tv.Type, byteSliceType)` — the conversion has to be to
    // `[]byte` itself, not to something whose *underlying* type is. A named
    // byte slice keeps its own methods and its own nil-vs-empty contract, and
    // upstream leaves it alone; guff took `underlying()` on both the slice and
    // its element and rewrote `json.RawMessage(fmt.Sprintf(…))` (k6
    // `cloudapi/logs_test.go`). An alias still qualifies, which is what
    // `unalias_readonly` is for.
    let typ = unalias_readonly(&artifacts.types, tav.typ);
    match artifacts.types.get(typ) {
        TypeData::Slice(s) => {
            let elem = unalias_readonly(&artifacts.types, s.elem());
            matches!(
                artifacts.types.get(elem),
                TypeData::Basic(b) if b.kind() == BasicKind::Uint8
            )
        }
        _ => false,
    }
}

/// `mayFormatEmpty` — can this format string render as the empty string?
///
/// `[]byte(fmt.Sprintf(""))` is an empty but non-nil slice while
/// `fmt.Appendf(nil, "")` is nil, so upstream declines the rewrite whenever the
/// format might come out empty. It decides that by parsing the format and
/// asking two questions: is every byte part of an operation, and is every
/// verb one of `s v x X`.
///
/// Any other verb answers no. `%d` is reported and `%s` is not — measured
/// against golangci-lint on eighteen formats, which is also how the rule was
/// pinned down: upstream's own condition reads
/// `!strings.ContainsRune("svxX", verb) && op.Prec.Fixed != 0`, and `%d`
/// without a precision still takes it.
///
/// A format with no operations at all fails to parse upstream, which the caller
/// reads as "cannot be empty", so `"plain"` is reported.
fn may_format_empty(format: &str) -> bool {
    if format.is_empty() {
        return true;
    }
    let b = format.as_bytes();
    let mut i = 0usize;
    let mut ops_len = 0usize;
    let mut saw_op = false;
    while i < b.len() {
        if b[i] != b'%' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        // flags
        while i < b.len() && matches!(b[i], b'-' | b'+' | b'#' | b' ' | b'0') {
            i += 1;
        }
        // `[n]` argument index
        i = skip_arg_index(b, i);
        // width
        if i < b.len() && b[i] == b'*' {
            i += 1;
            i = skip_arg_index(b, i);
        } else {
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
        // precision
        if i < b.len() && b[i] == b'.' {
            i += 1;
            if i < b.len() && b[i] == b'*' {
                i += 1;
                i = skip_arg_index(b, i);
            } else {
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
        }
        i = skip_arg_index(b, i);
        // verb
        let Some(verb) = format[i..].chars().next() else {
            return false; // trailing `%`: malformed
        };
        i += verb.len_utf8();
        if !matches!(verb, 's' | 'v' | 'x' | 'X') {
            return false;
        }
        saw_op = true;
        ops_len += i - start;
    }
    // No operations at all is a parse error upstream, and a parse error is
    // "cannot be empty".
    saw_op && ops_len == format.len()
}

fn skip_arg_index(b: &[u8], mut i: usize) -> usize {
    if i < b.len() && b[i] == b'[' {
        let start = i;
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i < b.len() && b[i] == b']' {
            return i + 1;
        }
        return start;
    }
    i
}

fn check_fmtappendf(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<Diagnostic>) {
    // Look for []byte(fmt.Sprintf/Sprint/Sprintln(...))
    if !is_byte_slice_conversion(pass, call) {
        return;
    }
    let Expr::CallExpr(inner) = &call.args[0] else {
        return;
    };
    let Some(name) = code::call_name(pass, &inner.fun) else {
        return;
    };
    let append_name = match name.as_str() {
        "fmt.Sprintf" => "Appendf",
        "fmt.Sprint" => "Append",
        "fmt.Sprintln" => "Appendln",
        _ => return,
    };
    let pos = call.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.19") {
        return;
    }
    if inner.args.is_empty() {
        return;
    }
    // `fmt.Sprint` and `fmt.Sprintf` disagree with their `Append` twins on nil
    // when the result is empty, so upstream skips those two whenever the format
    // may render empty. `Sprintln` always writes a newline and is never
    // skipped.
    if matches!(name.as_str(), "fmt.Sprintf" | "fmt.Sprint") {
        if let Some(format) = code::expr_to_string(pass, &inner.args[0]) {
            if may_format_empty(&format) {
                return;
            }
        }
    }
    let args: Option<Vec<String>> = inner.args.iter().map(expr_text).collect();
    let Some(args) = args else {
        return;
    };
    let args_joined = args.join(", ");
    let end = call.end().0 as u32;
    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: format!(
            "Replace []byte(fmt.{}...) with fmt.{append_name}",
            { name.strip_prefix("fmt.").unwrap_or(&name) }
        ),
        suggested_fixes: vec![SuggestedFix {
            message: format!(
                "Replace []byte(fmt.{}...) with fmt.{append_name}",
                { name.strip_prefix("fmt.").unwrap_or(&name) }
            ),
            text_edits: vec![TextEdit {
                pos,
                end,
                new_text: format!("fmt.{append_name}(nil, {args_joined})"),
            }],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

/// Upstream's `omitemptyRegex` (`modernize.go`), matched against the tag
/// literal's *unquoted* value. Group 1 is the `,omitempty` run itself, which is
/// what both suggested fixes are cut from.
fn omitempty_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?:^json| json):"[^"]*(,omitempty)(?:"|,[^"]*")\s?"#).unwrap()
    })
}

/// Go's `strconv.UnquoteChar`: one character off the front of a literal's
/// interior, plus the rest.
///
/// `quote` is the literal's delimiter, and a backtick is passed through here
/// exactly as upstream passes it — which means a backslash inside a *raw*
/// string is decoded as an escape even though Go itself would not. That is
/// upstream's behaviour in `walkStringLiteral` and this is a position mapper,
/// so matching it is the point; a struct tag with a backslash in a raw string
/// would otherwise get fix spans golangci-lint does not produce.
fn unquote_char(s: &str, quote: u8) -> Option<(char, &str)> {
    let bytes = s.as_bytes();
    let first = *bytes.first()?;
    if first != b'\\' {
        let c = s.chars().next()?;
        return Some((c, &s[c.len_utf8()..]));
    }
    let esc = *bytes.get(1)?;
    let simple = |c: char, n: usize| Some((c, &s[n..]));
    match esc {
        b'a' => simple('\u{7}', 2),
        b'b' => simple('\u{8}', 2),
        b'f' => simple('\u{c}', 2),
        b'n' => simple('\n', 2),
        b'r' => simple('\r', 2),
        b't' => simple('\t', 2),
        b'v' => simple('\u{b}', 2),
        b'\\' => simple('\\', 2),
        // Go rejects `\'` in a double-quoted string and `\"` in a single-quoted
        // one; both are accepted here because upstream *ignores* UnquoteChar's
        // error and walks on with a zero rune, which would silently stop the
        // mapping mid-literal instead of at the offset asked for.
        b'\'' | b'"' => simple(esc as char, 2),
        b'x' | b'u' | b'U' => {
            let width = match esc {
                b'x' => 2,
                b'u' => 4,
                _ => 8,
            };
            let digits = s.get(2..2 + width)?;
            let v = u32::from_str_radix(digits, 16).ok()?;
            let c = if esc == b'x' {
                // \xNN is a *byte*, not a rune; only the ASCII range can be
                // one char, and a tag outside it is not worth guessing at.
                char::from_u32(v).filter(|c| c.is_ascii())?
            } else {
                char::from_u32(v)?
            };
            Some((c, &s[2 + width..]))
        }
        b'0'..=b'7' => {
            let digits = s.get(1..4)?;
            let v = u32::from_str_radix(digits, 8).ok()?;
            Some((char::from_u32(v)?, &s[4..]))
        }
        _ => None,
    }
}

/// Go's `strconv.Unquote` for the two literal forms a struct tag can take.
///
/// A raw string keeps every byte except carriage returns; an interpreted one is
/// walked escape by escape, which is the same decoding the position mapper
/// above does — the two have to agree or an offset means different things to
/// each of them.
fn unquote_literal(value: &str) -> Option<String> {
    if value.len() < 2 {
        return None;
    }
    let quote = value.as_bytes()[0];
    let body = &value[1..value.len() - 1];
    if quote == b'`' {
        if !value.ends_with('`') {
            return None;
        }
        return Some(body.replace('\r', ""));
    }
    if quote != b'"' || !value.ends_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut rest = body;
    while !rest.is_empty() {
        let (c, next) = unquote_char(rest, b'"')?;
        out.push(c);
        rest = next;
    }
    Some(out)
}

/// Port of `internal/astutil.PosInStringLiteral`: map a byte offset in a string
/// literal's cooked value back to a position in the source literal.
///
/// The two are not the same offset the moment the literal contains an escape —
/// `\"` is two bytes of source and one of value — and a struct tag written as an
/// interpreted string (`"json:\"a,omitempty\""`) is exactly that shape. Cutting
/// the fix out of the cooked offsets would land it several bytes early.
fn pos_in_string_literal(value: &str, lit_pos: u32, lit_end: u32, offset: usize) -> Option<u32> {
    if value.len() < 2 {
        return None;
    }
    let quote = value.as_bytes()[0];
    // `norm`: the source span is longer than the literal's value, which happens
    // when a raw string's \r\n was normalized to \n by the scanner.
    let norm = (lit_end - lit_pos) as usize > value.len();
    let mut raw = &value[1..value.len() - 1];
    let mut i = 0usize;
    let mut pos = lit_pos + 1;
    while !raw.is_empty() {
        let (r, rest) = unquote_char(raw, quote)?;
        let sz = (raw.len() - rest.len()) as u32;
        let mut next_pos = pos + sz;
        if norm && r == '\n' {
            next_pos += 1;
        }
        let next_i = i + r.len_utf8();
        if next_pos > lit_end || next_i > offset {
            break;
        }
        raw = rest;
        i = next_i;
        pos = next_pos;
    }
    Some(pos)
}

/// Go's `reflect.StructTag.Get`, which upstream uses to ask whether the json tag
/// is *exactly* `,omitempty`.
///
/// Deliberately not `musttag`'s `lookup_struct_tag`, which reads a value as
/// `trim_matches('"')`: that is an approximation good enough for a name lookup
/// and wrong for an equality test, because it cannot tell `json:",omitempty"`
/// from a value that merely contains quotes.
fn struct_tag_get(tag: &str, key: &str) -> Option<String> {
    let mut rest = tag;
    loop {
        rest = rest.trim_start_matches(' ');
        if rest.is_empty() {
            return None;
        }
        let name_end = rest.find(':')?;
        let name = &rest[..name_end];
        if name.is_empty() || name.contains(' ') || name.contains('\t') || name.contains('"') {
            return None;
        }
        rest = &rest[name_end + 1..];
        if !rest.starts_with('"') {
            return None;
        }
        // Scan to the closing quote, skipping escaped ones.
        let bytes = rest.as_bytes();
        let mut i = 1;
        while i < bytes.len() && bytes[i] != b'"' {
            i += if bytes[i] == b'\\' { 2 } else { 1 };
        }
        if i >= bytes.len() {
            return None;
        }
        let quoted = &rest[..i + 1];
        rest = &rest[i + 1..];
        if name == key {
            // The value is a Go quoted string; unquote it the same way.
            let mut out = String::new();
            let mut body = &quoted[1..quoted.len() - 1];
            while !body.is_empty() {
                let (c, next) = unquote_char(body, b'"')?;
                out.push(c);
                body = next;
            }
            return Some(out);
        }
    }
}

/// Upstream's `usesKubebuilder`: does any comment in the *package* contain
/// `+kubebuilder:`?
///
/// kubebuilder has its own interpretation of `omitzero` (go.dev/issue/76649),
/// so `omitzero` reports nothing at all for such a package — not even the
/// fields whose tags have nothing to do with a marker. dapr's `pkg/apis/**` are
/// kubebuilder CRD types, which is 24 findings golangci-lint does not make.
///
/// Reads the retained source rather than `file.comments`, which the production
/// load leaves empty (it parses without `PARSE_COMMENTS`), and scans comment
/// text only — a string literal that happens to contain the marker is not one.
fn package_uses_kubebuilder(pass: &Pass<'_>) -> bool {
    let pkg = pass.pkg();
    for i in 0..pkg.compiled_go_files.len() {
        let path = &pkg.compiled_go_files[i];
        // Retained source when the typechecker kept it, the file otherwise —
        // `check_importcomment` re-reads the same way for the same reason.
        let owned;
        let src: &[u8] = match pkg.source_bytes(i) {
            Some(bytes) => bytes,
            None => match fs::read(path) {
                Ok(bytes) => {
                    owned = bytes;
                    &owned
                }
                Err(_) => continue,
            },
        };
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let re_fset = FileSet::new();
        let Ok(parsed) = parse_file(&re_fset, name, src, PARSE_COMMENTS) else {
            continue;
        };
        for group in &parsed.comments {
            for c in &group.list {
                if c.text.contains("+kubebuilder:") {
                    return true;
                }
            }
        }
    }
    false
}

fn check_omitzero(pass: &Pass<'_>, field: &Field, pending: &mut Vec<Diagnostic>) {
    if !field_type_is_struct(pass, field) {
        return;
    }
    let Some(tag) = field.tag.as_ref() else {
        return;
    };
    let pos = tag.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.24") {
        return;
    }
    let end = tag.end().0 as u32;

    // `strconv.Unquote(tag.Value)` — upstream ignores the error, which leaves
    // an empty string that the regex cannot match, so failing is the same as
    // not matching.
    let Some(tagconv) = unquote_literal(&tag.value) else {
        return;
    };
    let Some(caps) = omitempty_regex().captures(&tagconv) else {
        return;
    };
    let whole = caps.get(0).map(|m| m.range()).unwrap_or_default();
    let Some(omitempty) = caps.get(1).map(|m| m.range()) else {
        return;
    };

    let Some(oe_pos) = pos_in_string_literal(&tag.value, pos, end, omitempty.start) else {
        return;
    };
    let Some(oe_end) = pos_in_string_literal(&tag.value, pos, end, omitempty.end) else {
        return;
    };

    // Two alternatives on the same span, and that is the whole point: the
    // deletion and the replacement overlap, so golangci-lint's fixer sees a
    // conflict and drops *every* modernize edit in the file
    // (`pkg/result/processors/fixer.go`, ported in guff-lint/src/fix.rs).
    // Emitting only the `omitzero` half — which guff did — turned a finding
    // upstream never acts on into a silent rewrite of a struct tag, and
    // `omitempty` -> `omitzero` changes what the encoder puts on the wire.
    let mut remove = (oe_pos, oe_end);
    if struct_tag_get(&tagconv, "json").as_deref() == Some(",omitempty") {
        if whole.len() == tagconv.len() {
            // json is the only tag: take the literal, quotes and all.
            remove = (pos, end);
        } else {
            match (
                pos_in_string_literal(&tag.value, pos, end, whole.start),
                pos_in_string_literal(&tag.value, pos, end, whole.end),
            ) {
                (Some(a), Some(b)) => remove = (a, b),
                _ => return,
            }
        }
    }

    pending.push(Diagnostic {
        pos,
        end,
        category: "omitzero".into(),
        message: "Omitempty has no effect on nested struct fields".into(),
        suggested_fixes: vec![
            SuggestedFix {
                message: "Remove redundant omitempty tag".into(),
                text_edits: vec![TextEdit {
                    pos: remove.0,
                    end: remove.1,
                    new_text: String::new(),
                }],
            },
            SuggestedFix {
                message: "Replace omitempty with omitzero (behavior change)".into(),
                text_edits: vec![TextEdit {
                    pos: oe_pos,
                    end: oe_end,
                    new_text: ",omitzero".into(),
                }],
            },
        ],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

fn is_natural_less(pass: &Pass<'_>, lit: &FuncLit, slice: &Expr) -> bool {
    if lit.body.list.len() != 1 {
        return false;
    }
    let Stmt::ReturnStmt(ret) = &lit.body.list[0] else {
        return false;
    };
    if ret.results.len() != 1 {
        return false;
    }
    let Expr::BinaryExpr(BinaryExpr { x, op, y, .. }) = &ret.results[0] else {
        return false;
    };
    if *op != Token::LSS {
        return false;
    }
    let (Expr::IndexExpr(ix), Expr::IndexExpr(iy)) = (x.as_ref(), y.as_ref()) else {
        return false;
    };
    code::same_non_dynamic(pass, &ix.x, slice)
        && code::same_non_dynamic(pass, &iy.x, slice)
        && ident_name(&ix.index) == Some("i")
        && ident_name(&iy.index) == Some("j")
}

fn check_slicessort(
    pass: &Pass<'_>,
    file: &File,
    call: &CallExpr,
    pending: &mut Vec<Diagnostic>,
) {
    if !code::is_call_to(pass, call, "sort.Slice") || call.args.len() != 2 {
        return;
    }
    let pos = call.fun.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.21") {
        return;
    }
    let Expr::FuncLit(lit) = &call.args[1] else {
        return;
    };
    if !is_natural_less(pass, lit, &call.args[0]) {
        return;
    }
    let Some(slice_text) = expr_text_src(pass, &call.args[0]) else {
        return;
    };
    let end = call.end().0 as u32;
    // Upstream keys the import on the whole call, not on `sort.Slice`; the two
    // are in the same scope, so this only matters for staying literal.
    let Some((prefix, import_edits)) =
        refactor::add_import(pass, file, "slices", "slices", "Sort", call.pos().0 as u32)
    else {
        return;
    };
    pending.push(Diagnostic {
        pos,
        end: call.fun.end().0 as u32,
        category: String::new(),
        message: "sort.Slice can be modernized using slices.Sort".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace sort.Slice call by slices.Sort".into(),
            text_edits: with_imports(
                &import_edits,
                vec![TextEdit {
                    pos,
                    end,
                    new_text: format!("{prefix}Sort({slice_text})"),
                }],
            ),
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

/// Split `i±k` into `(i, signed_k)`; otherwise `(e, 0)`.
fn split_index_offset<'a>(pass: &Pass<'_>, e: &'a Expr) -> (&'a Expr, i64) {
    if let Expr::BinaryExpr(bin) = e {
        if bin.op == Token::ADD || bin.op == Token::SUB {
            if let Some(k) = code::expr_to_int(pass, &bin.y) {
                let signed = if bin.op == Token::SUB { -k } else { k };
                return (&bin.x, signed);
            }
        }
    }
    (e, 0)
}

/// Reports whether we can verify `a < b` for slice indices (upstream
/// `increasingSliceIndices`).
fn increasing_slice_indices(pass: &Pass<'_>, a: &Expr, b: &Expr) -> bool {
    let ak = code::expr_to_int(pass, a);
    let bk = code::expr_to_int(pass, b);
    if ak.is_some() || bk.is_some() {
        return matches!((ak, bk), (Some(a), Some(b)) if a < b);
    }
    let (ai, ak) = split_index_offset(pass, a);
    let (bi, bk) = split_index_offset(pass, b);
    exprs_equal(ai, bi) && ak < bk
}

/// Port of modernize `slicesdelete`: `append(s[:a], s[b:]...)` → `slices.Delete`.
fn check_slicesdelete(
    pass: &Pass<'_>,
    file: &File,
    call: &CallExpr,
    pending: &mut Vec<Diagnostic>,
) {
    let path = pass.pkg().pkg_path.as_str();
    if path == "slices"
        || path.starts_with("slices/")
        || path == "runtime"
        || path.starts_with("runtime/")
    {
        return;
    }
    if !code::is_call_to(pass, call, "append") || call.args.len() != 2 {
        return;
    }
    if !call.ellipsis.is_valid() {
        return;
    }
    let pos = call.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.21") {
        return;
    }
    let Expr::SliceExpr(slice1) = &call.args[0] else {
        return;
    };
    let Expr::SliceExpr(slice2) = &call.args[1] else {
        return;
    };
    if slice1.low.is_some() || slice1.slice3 || slice1.high.is_none() {
        return;
    }
    if slice2.high.is_some() || slice2.slice3 || slice2.low.is_none() {
        return;
    }
    if !exprs_equal(&slice1.x, &slice2.x) {
        return;
    }
    if expr_has_effects(pass, &slice1.x) {
        return;
    }
    let high = slice1.high.as_ref().expect("checked above");
    let low = slice2.low.as_ref().expect("checked above");
    if !increasing_slice_indices(pass, high, low) {
        return;
    }
    let Some(x_text) = expr_text_src(pass, &slice1.x) else {
        return;
    };
    let Some(high_text) = expr_text_src(pass, high) else {
        return;
    };
    let Some(low_text) = expr_text_src(pass, low) else {
        return;
    };
    // DEFERRED: `int()` wrap when the indices are non-int, and the int-shadowed
    // skip.
    let end = call.end().0 as u32;
    let Some((prefix, import_edits)) =
        refactor::add_import(pass, file, "slices", "slices", "Delete", call.pos().0 as u32)
    else {
        return;
    };
    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: "Replace append with slices.Delete".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace append with slices.Delete".into(),
            text_edits: with_imports(
                &import_edits,
                vec![TextEdit {
                    pos,
                    end,
                    new_text: format!("{prefix}Delete({x_text}, {high_text}, {low_text})"),
                }],
            ),
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

fn find_first_call_named<'a>(pass: &Pass<'_>, stmt: &'a Stmt, name: &str) -> Option<&'a CallExpr> {
    let mut found = None;
    walk::inspect(walk::stmt_ref(stmt), |n| {
        let Some(n) = n else {
            return true;
        };
        if found.is_some() {
            return false;
        }
        if let NodeRef::CallExpr(c) = n {
            if code::is_call_to(pass, c, name) {
                found = Some(c);
                return false;
            }
        }
        true
    });
    found
}

fn cutprefix_kind(pass: &Pass<'_>, call: &CallExpr) -> Option<(&'static str, &'static str, bool)> {
    // (pkg, cut_name, is_prefix)
    if code::is_call_to(pass, call, "strings.HasPrefix") {
        Some(("strings", "CutPrefix", true))
    } else if code::is_call_to(pass, call, "strings.HasSuffix") {
        Some(("strings", "CutSuffix", false))
    } else if code::is_call_to(pass, call, "bytes.HasPrefix") {
        Some(("bytes", "CutPrefix", true))
    } else if code::is_call_to(pass, call, "bytes.HasSuffix") {
        Some(("bytes", "CutSuffix", false))
    } else {
        None
    }
}

fn trim_kind(pass: &Pass<'_>, call: &CallExpr) -> Option<(&'static str, &'static str, bool)> {
    // (pkg, cut_name, is_prefix)
    if code::is_call_to(pass, call, "strings.TrimPrefix") {
        Some(("strings", "CutPrefix", true))
    } else if code::is_call_to(pass, call, "strings.TrimSuffix") {
        Some(("strings", "CutSuffix", false))
    } else if code::is_call_to(pass, call, "bytes.TrimPrefix") {
        Some(("bytes", "CutPrefix", true))
    } else if code::is_call_to(pass, call, "bytes.TrimSuffix") {
        Some(("bytes", "CutSuffix", false))
    } else {
        None
    }
}

fn check_stringscutprefix(
    pass: &Pass<'_>,
    file: &File,
    if_stmt: &IfStmt,
    pending: &mut Vec<Diagnostic>,
) {
    // Pattern 1: if pkg.HasPrefix(s, affix) { use(pkg.TrimPrefix(s, affix)) }
    if if_stmt.init.is_none() && !if_stmt.body.list.is_empty() {
        if let Expr::CallExpr(has_call) = &if_stmt.cond {
            let pos = has_call.pos().0 as u32;
            if go_at_least(pass, pos, "go1.20") && has_call.args.len() == 2 {
                if let Some((pkg, cut_name, is_prefix)) = cutprefix_kind(pass, has_call) {
                    let trim_name = if is_prefix {
                        format!("{pkg}.TrimPrefix")
                    } else {
                        format!("{pkg}.TrimSuffix")
                    };
                    if let Some(trim_call) =
                        find_first_call_named(pass, &if_stmt.body.list[0], &trim_name)
                    {
                        if trim_call.args.len() == 2
                            && code::same_non_dynamic(pass, &has_call.args[0], &trim_call.args[0])
                            && code::same_non_dynamic(pass, &has_call.args[1], &trim_call.args[1])
                        {
                            if let (Some(s_text), Some(affix_text)) =
                                (expr_text(&has_call.args[0]), expr_text(&has_call.args[1]))
                            {
                                let var_name = if is_prefix { "after" } else { "before" };
                                let (message, fix_message) = if is_prefix {
                                    (
                                        "HasPrefix + TrimPrefix can be simplified to CutPrefix",
                                        "Replace HasPrefix/TrimPrefix with CutPrefix",
                                    )
                                } else {
                                    (
                                        "HasSuffix + TrimSuffix can be simplified to CutSuffix",
                                        "Replace HasSuffix/TrimSuffix with CutSuffix",
                                    )
                                };
                                let end = has_call.end().0 as u32;
                                let Some((prefix, import_edits)) = refactor::add_import(
                                    pass, file, pkg, pkg, cut_name, pos,
                                ) else {
                                    return;
                                };
                                pending.push(Diagnostic {
                                    pos,
                                    end,
                                    category: String::new(),
                                    message: message.into(),
                                    suggested_fixes: vec![SuggestedFix {
                                        message: fix_message.into(),
                                        text_edits: with_imports(
                                            &import_edits,
                                            vec![
                                                TextEdit {
                                                    pos,
                                                    end,
                                                    new_text: format!(
                                                        "{var_name}, ok := {prefix}{cut_name}({s_text}, {affix_text}); ok"
                                                    ),
                                                },
                                                TextEdit {
                                                    pos: trim_call.pos().0 as u32,
                                                    end: trim_call.end().0 as u32,
                                                    new_text: var_name.into(),
                                                },
                                            ],
                                        ),
                                    }],
                                    related: Vec::new(),
                                    url: String::new(),
                                    severity: String::new(),
                                    ..Diagnostic::default()
                                });
                                return;
                            }
                        }
                    }
                }
            }
        }
    }

    // Pattern 2: if after := pkg.TrimPrefix(s, affix); after != s { use(after) }
    let Some(Stmt::AssignStmt(init)) = if_stmt.init.as_deref() else {
        return;
    };
    if init.tok != Some(Token::DEFINE) || init.lhs.len() != 1 || init.rhs.len() != 1 {
        return;
    }
    let Expr::CallExpr(trim_call) = &init.rhs[0] else {
        return;
    };
    let Some((pkg, cut_name, is_prefix)) = trim_kind(pass, trim_call) else {
        return;
    };
    if trim_call.args.len() != 2 {
        return;
    }
    let Expr::BinaryExpr(bin) = &if_stmt.cond else {
        return;
    };
    if bin.op != Token::NEQ {
        return;
    }
    let lhs = &init.lhs[0];
    let s_arg = &trim_call.args[0];
    let cond_ok = (code::same_non_dynamic(pass, lhs, &bin.x)
        && code::same_non_dynamic(pass, s_arg, &bin.y))
        || (code::same_non_dynamic(pass, lhs, &bin.y)
            && code::same_non_dynamic(pass, s_arg, &bin.x));
    if !cond_ok {
        return;
    }
    let pos = init.lhs[0].pos().0 as u32;
    if !go_at_least(pass, pos, "go1.20") {
        return;
    }
    // DEFERRED: upstream handles the dot-import spelling here, using AddImport
    // purely to compute the (empty) prefix.
    let Expr::SelectorExpr(_) = trim_call.fun.as_ref() else {
        return;
    };
    // The existing import already satisfies this call, so AddImport adds
    // nothing — it is here for the local name, which need not be `strings`.
    let Some((prefix, import_edits)) =
        refactor::add_import(pass, file, pkg, pkg, cut_name, trim_call.pos().0 as u32)
    else {
        return;
    };
    let (message, fix_message) = if is_prefix {
        (
            "TrimPrefix can be simplified to CutPrefix",
            "Replace TrimPrefix with CutPrefix",
        )
    } else {
        (
            "TrimSuffix can be simplified to CutSuffix",
            "Replace TrimSuffix with CutSuffix",
        )
    };
    let end = if_stmt.cond.end().0 as u32;
    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: message.into(),
        suggested_fixes: vec![SuggestedFix {
            message: fix_message.into(),
            text_edits: with_imports(
                &import_edits,
                vec![
                    TextEdit {
                        pos: init.lhs[0].end().0 as u32,
                        end: init.lhs[0].end().0 as u32,
                        new_text: ", ok".into(),
                    },
                    // Upstream replaces the whole `pkg.TrimPrefix` selector,
                    // not just the `TrimPrefix` half, so the prefix it computed
                    // is the one that lands.
                    TextEdit {
                        pos: trim_call.fun.pos().0 as u32,
                        end: trim_call.fun.end().0 as u32,
                        new_text: format!("{prefix}{cut_name}"),
                    },
                    TextEdit {
                        pos: if_stmt.cond.pos().0 as u32,
                        end: if_stmt.cond.end().0 as u32,
                        new_text: "ok".into(),
                    },
                ],
            ),
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

fn is_true_or_false_lit(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::Ident(id) if id.name == "true" => Some(true),
        Expr::Ident(id) if id.name == "false" => Some(false),
        _ => None,
    }
}

fn is_blank_ident(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(id) if id.name == "_")
}

/// Whether `e` is the range element (`elem`) or `s[i]`.
fn is_slice_elem(pass: &Pass<'_>, rng: &RangeStmt, e: &Expr) -> bool {
    if let Some(val) = rng.value.as_ref() {
        if !is_blank_ident(val) && code::same_non_dynamic(pass, e, val) {
            return true;
        }
    }
    if let (Some(key), Expr::IndexExpr(ix)) = (rng.key.as_ref(), e) {
        if !is_blank_ident(key)
            && code::same_non_dynamic(pass, &ix.x, &rng.x)
            && code::same_non_dynamic(pass, &ix.index, key)
        {
            return true;
        }
    }
    false
}

fn range_var_objs(pass: &Pass<'_>, rng: &RangeStmt) -> Vec<ObjectId> {
    let mut objs = Vec::new();
    for opt in [&rng.key, &rng.value] {
        let Some(Expr::Ident(id)) = opt.as_ref() else {
            continue;
        };
        if id.name == "_" {
            continue;
        }
        if let Some(obj) = ident_obj(pass, id) {
            objs.push(obj);
        }
    }
    objs
}

fn node_uses_objs(pass: &Pass<'_>, node: NodeRef<'_>, objs: &[ObjectId]) -> bool {
    if objs.is_empty() {
        return false;
    }
    let mut used = false;
    walk::inspect(node, |n| {
        let Some(n) = n else {
            return true;
        };
        if let NodeRef::Ident(id) = n {
            if let Some(obj) = ident_obj(pass, id) {
                if objs.contains(&obj) {
                    used = true;
                    return false;
                }
            }
        }
        true
    });
    used
}

fn stmt_uses_range_vars(pass: &Pass<'_>, stmt: &Stmt, rng: &RangeStmt) -> bool {
    node_uses_objs(pass, walk::stmt_ref(stmt), &range_var_objs(pass, rng))
}

fn expr_uses_range_vars(pass: &Pass<'_>, expr: &Expr, rng: &RangeStmt) -> bool {
    node_uses_objs(pass, walk::expr_ref(expr), &range_var_objs(pass, rng))
}

/// Side-effect heuristic: reject call / unary / composite needles for Contains.
fn expr_may_have_effects(expr: &Expr) -> bool {
    match expr {
        Expr::CallExpr(_) | Expr::UnaryExpr(_) | Expr::CompositeLit(_) | Expr::FuncLit(_) => true,
        Expr::ParenExpr(p) => expr_may_have_effects(&p.x),
        Expr::SelectorExpr(sel) => expr_may_have_effects(&sel.x),
        Expr::IndexExpr(ix) => expr_may_have_effects(&ix.x) || expr_may_have_effects(&ix.index),
        Expr::BinaryExpr(b) => expr_may_have_effects(&b.x) || expr_may_have_effects(&b.y),
        _ => false,
    }
}

/// The `ContainsFunc` half of upstream's signature check: not variadic, and the
/// sole parameter's type identical to the ranged slice's element type.
///
/// A callee whose type is not a signature at all falls through, as upstream's
/// `if isSignature` does.
fn predicate_signature_matches_elem(pass: &Pass<'_>, rng: &RangeStmt, fun: &Expr) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return true;
    };
    let Some(fun_ty) = type_of(pass, fun) else {
        return true;
    };
    let under = unalias_readonly(&artifacts.types, fun_ty).underlying(&artifacts.types);
    let TypeData::Signature(sig) = artifacts.types.get(under) else {
        return true;
    };
    if sig.variadic() {
        return false;
    }
    let Some(params) = sig.params() else {
        return false;
    };
    let TypeData::Tuple(tuple) = artifacts.types.get(params) else {
        return false;
    };
    if tuple.len() != 1 {
        return false;
    }
    let ObjectData::Var(var) = artifacts.objects.get(tuple.at(0)) else {
        return false;
    };
    let param_ty = var.typ();
    let Some(x_ty) = type_of(pass, &rng.x) else {
        return true;
    };
    let x_under = unalias_readonly(&artifacts.types, x_ty).underlying(&artifacts.types);
    let TypeData::Slice(slice) = artifacts.types.get(x_under) else {
        return true;
    };
    types_identical(pass, slice.elem(), param_ty)
}

/// Analyze `if cond` for Contains / ContainsFunc. Returns (func_name, arg2_text).
fn slicescontains_cond(
    pass: &Pass<'_>,
    rng: &RangeStmt,
    cond: &Expr,
) -> Option<(&'static str, String)> {
    match cond {
        Expr::BinaryExpr(BinaryExpr { x, op, y, .. }) if *op == Token::EQL => {
            let (elem, needle) = if is_slice_elem(pass, rng, x) {
                (x.as_ref(), y.as_ref())
            } else if is_slice_elem(pass, rng, y) {
                (y.as_ref(), x.as_ref())
            } else {
                return None;
            };
            let Some(elem_ty) = type_of(pass, elem) else {
                return None;
            };
            let Some(needle_ty) = type_of(pass, needle) else {
                return None;
            };
            if !types_identical(pass, elem_ty, needle_ty) {
                return None;
            }
            if expr_may_have_effects(needle) || expr_uses_range_vars(pass, needle, rng) {
                return None;
            }
            let needle_text = expr_text(needle)?;
            Some(("Contains", needle_text))
        }
        Expr::CallExpr(call) if call.args.len() == 1 && !call.ellipsis.is_valid() => {
            // Skip type conversions: `T(x)`.
            let is_type = pass.types_info().is_some_and(|info| {
                info.types
                    .get(&call.fun.id())
                    .is_some_and(|tav| tav.mode == OperandMode::TypeExpr)
            });
            if is_type {
                return None;
            }
            if !is_slice_elem(pass, rng, &call.args[0]) {
                return None;
            }
            if expr_uses_range_vars(pass, &call.fun, rng) {
                return None;
            }
            // Upstream reads the callee's signature and declines twice: a
            // variadic predicate, and one whose parameter type is not
            // *identical* to the slice's element type.
            //
            //     tElem  = CoreType(info.TypeOf(rng.X)).(*types.Slice).Elem()
            //     tParam = sig.Params().At(0).Type()
            //     if !types.Identical(tElem, tParam) { return }
            //
            // Assignability is not enough, and that is the whole of k6
            // `internal/js/modules/k6/grpc/client.go:445`: `stack` is
            // `[]*sobek.Object` while `SameAs(other Value) bool` takes the
            // interface, so `slices.ContainsFunc(stack, obj.SameAs)` would not
            // even compile. guff checked neither.
            if !predicate_signature_matches_elem(pass, rng, &call.fun) {
                return None;
            }
            let pred_text = expr_text(&call.fun)?;
            if expr_may_have_effects(&call.fun) {
                return None;
            }
            Some(("ContainsFunc", pred_text))
        }
        _ => None,
    }
}

fn is_unlabeled_break(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::BranchStmt(BranchStmt {
            tok: Token::BREAK,
            label: None,
            ..
        })
    )
}

fn body_has_free_branch(stmts: &[Stmt], skip_last: bool) -> bool {
    let end = if skip_last {
        stmts.len().saturating_sub(1)
    } else {
        stmts.len()
    };
    for stmt in &stmts[..end] {
        let mut found = false;
        walk::inspect(walk::stmt_ref(stmt), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::BranchStmt(b)
                    if matches!(b.tok, Token::BREAK | Token::CONTINUE) && b.label.is_none() =>
                {
                    found = true;
                    return false;
                }
                NodeRef::ReturnStmt(_) => {
                    found = true;
                    return false;
                }
                // Don't descend into nested function literals.
                NodeRef::FuncLit(_) => return false,
                _ => {}
            }
            true
        });
        if found {
            return true;
        }
    }
    false
}

/// Upstream builds each fix as `append(importEdits, ...)`; the import edits
/// come first and every fix in the file carries the same copy. golangci's
/// fixer deduplicates equivalent edits, so the import is inserted once.
fn with_imports(import_edits: &[TextEdit], rest: Vec<TextEdit>) -> Vec<TextEdit> {
    let mut edits = import_edits.to_vec();
    edits.extend(rest);
    edits
}

fn check_slicescontains(
    pass: &Pass<'_>,
    file: &File,
    block: &BlockStmt,
    pending: &mut Vec<Diagnostic>,
) {
    if pass.pkg().pkg_path == "slices" || pass.pkg().pkg_path.starts_with("slices/") {
        return;
    }
    for i in 0..block.list.len() {
        let Stmt::RangeStmt(rng) = &block.list[i] else {
            continue;
        };
        let pos = rng.for_.0 as u32;
        if !go_at_least(pass, pos, "go1.21") {
            continue;
        }
        if type_kind(pass, &rng.x) != Some(TypeKind::Slice) {
            continue;
        }
        if rng.tok != Some(Token::DEFINE) {
            continue;
        }
        // Need at least a key ident (may be `_`).
        if !matches!(rng.key.as_ref(), Some(Expr::Ident(_))) {
            continue;
        }
        if rng.body.list.len() != 1 {
            continue;
        }
        let Stmt::IfStmt(if_stmt) = &rng.body.list[0] else {
            continue;
        };
        if if_stmt.init.is_some() || if_stmt.else_.is_some() {
            continue;
        }
        let body = &if_stmt.body.list;
        if body.is_empty() {
            continue;
        }
        let Some((func_name, arg2_text)) = slicescontains_cond(pass, rng, &if_stmt.cond) else {
            continue;
        };
        // Upstream rejects any body use of range vars (including last stmt).
        if body.iter().any(|s| stmt_uses_range_vars(pass, s, rng)) {
            continue;
        }
        if body_has_free_branch(body, true) {
            continue;
        }
        let Some(slice_text) = expr_text_src(pass, &rng.x) else {
            continue;
        };
        let Some((prefix, import_edits)) =
            refactor::add_import(pass, file, "slices", "slices", func_name, pos)
        else {
            continue;
        };
        let contains = format!("{prefix}{func_name}({slice_text}, {arg2_text})");
        let last = body.last().unwrap();
        let msg = format!("Loop can be simplified using slices.{func_name}");

        // Special case: body={ return true/false } next={ return false/true }
        if let Stmt::ReturnStmt(ret_last) = last {
            if body.len() == 1 {
                if let Some(Stmt::ReturnStmt(after)) = block.list.get(i + 1) {
                    let tval = if ret_last.results.len() == 1 {
                        is_true_or_false_lit(&ret_last.results[0])
                    } else {
                        None
                    };
                    let fval = if after.results.len() == 1 {
                        is_true_or_false_lit(&after.results[0])
                    } else {
                        None
                    };
                    if let (Some(t), Some(f)) = (tval, fval) {
                        if t != f {
                            let neg = if t { "" } else { "!" };
                            let end = after
                                .results
                                .last()
                                .map(|e| e.end().0 as u32)
                                .unwrap_or(after.return_.0 as u32);
                            pending.push(Diagnostic {
                                pos,
                                end,
                                category: String::new(),
                                message: msg.clone(),
                                suggested_fixes: vec![SuggestedFix {
                                    message: format!("Replace loop by call to slices.{func_name}"),
                                    text_edits: with_imports(
                                        &import_edits,
                                        vec![TextEdit {
                                            pos,
                                            end,
                                            new_text: format!("return {neg}{contains}"),
                                        }],
                                    ),
                                }],
                                related: Vec::new(),
                                url: String::new(),
                                severity: String::new(),
                                ..Diagnostic::default()
                            });
                            continue;
                        }
                    }
                }
            }
            // General return: for ... { if cond { stmts; return x } } → if Contains { ... }
            let end = rng.body.end().0 as u32;
            pending.push(Diagnostic {
                pos,
                end,
                category: String::new(),
                message: msg,
                suggested_fixes: vec![SuggestedFix {
                    message: format!("Replace loop by call to slices.{func_name}"),
                    text_edits: with_imports(
                        &import_edits,
                        vec![
                            TextEdit {
                                pos,
                                end: if_stmt.body.pos().0 as u32,
                                new_text: format!("if {contains} "),
                            },
                            TextEdit {
                                pos: if_stmt.body.end().0 as u32,
                                end,
                                new_text: String::new(),
                            },
                        ],
                    ),
                }],
                related: Vec::new(),
                url: String::new(),
                severity: String::new(),
                ..Diagnostic::default()
            });
            continue;
        }

        // break variants
        if !is_unlabeled_break(last) {
            continue;
        }
        // Sole break → empty if; skip (upstream #77677).
        if body.len() == 1 {
            continue;
        }

        // Special: prev=`lhs = false`; body=`lhs = true; break`
        if body.len() == 2 {
            if let Stmt::AssignStmt(assign) = &body[0] {
                if assign.tok == Some(Token::ASSIGN)
                    && assign.lhs.len() == 1
                    && assign.rhs.len() == 1
                {
                    if let Some(assign_bool) = is_true_or_false_lit(&assign.rhs[0]) {
                        if let Some(j) = i.checked_sub(1) {
                            if let Stmt::AssignStmt(prev) = &block.list[j] {
                                if (prev.tok == Some(Token::ASSIGN)
                                    || prev.tok == Some(Token::DEFINE))
                                    && prev.lhs.len() == 1
                                    && prev.rhs.len() == 1
                                    && code::same_non_dynamic(pass, &prev.lhs[0], &assign.lhs[0])
                                {
                                    if let Some(prev_bool) = is_true_or_false_lit(&prev.rhs[0]) {
                                        if assign_bool != prev_bool {
                                            let neg = if assign_bool { "" } else { "!" };
                                            let end = rng.body.end().0 as u32;
                                            // Upstream replaces the previous
                                            // assignment's *right-hand side*
                                            // only, so whichever token the
                                            // source used survives. Rewriting
                                            // from the lhs and spelling `=`
                                            // turned `found := false` into
                                            // `found = slices.Contains(…)` —
                                            // `undefined: found`, a fix that
                                            // does not compile.
                                            let rhs_pos = prev.rhs[0].pos().0 as u32;
                                            let rhs_end = prev.rhs[0].end().0 as u32;
                                            pending.push(Diagnostic {
                                                // `Pos: rng.Pos()` for every
                                                // spelling. The fix starts at
                                                // `found := false`; the finding
                                                // does not.
                                                pos,
                                                end,
                                                category: String::new(),
                                                message: msg.clone(),
                                                suggested_fixes: vec![SuggestedFix {
                                                    message: format!(
                                                        "Replace loop by call to slices.{func_name}"
                                                    ),
                                                    text_edits: with_imports(
                                                        &import_edits,
                                                        vec![
                                                            TextEdit {
                                                                pos: rhs_pos,
                                                                end: rhs_end,
                                                                new_text: format!(
                                                                    "{neg}{contains}"
                                                                ),
                                                            },
                                                            // Delete the loop
                                                            // and the space
                                                            // before it.
                                                            TextEdit {
                                                                pos: rhs_end,
                                                                end,
                                                                new_text: String::new(),
                                                            },
                                                        ],
                                                    ),
                                                }],
                                                related: Vec::new(),
                                                url: String::new(),
                                                severity: String::new(),
                                                ..Diagnostic::default()
                                            });
                                            continue;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // General: for ... { if cond { stmts; break } } → if Contains { stmts }
        let end = rng.body.end().0 as u32;
        let before_break_end = body[body.len() - 2].end().0 as u32;
        pending.push(Diagnostic {
            pos,
            end,
            category: String::new(),
            message: msg,
            suggested_fixes: vec![SuggestedFix {
                message: format!("Replace loop by call to slices.{func_name}"),
                text_edits: with_imports(
                    &import_edits,
                    vec![
                        TextEdit {
                            pos,
                            end: if_stmt.body.pos().0 as u32,
                            new_text: format!("if {contains} "),
                        },
                        TextEdit {
                            pos: before_break_end,
                            end: last.end().0 as u32,
                            new_text: String::new(),
                        },
                        TextEdit {
                            pos: if_stmt.body.end().0 as u32,
                            end,
                            new_text: String::new(),
                        },
                    ],
                ),
            }],
            related: Vec::new(),
            url: String::new(),
            severity: String::new(),
            ..Diagnostic::default()
        });
    }
}

fn check_mapsloop(
    pass: &Pass<'_>,
    file: &File,
    range_stmt: &RangeStmt,
    pending: &mut Vec<Diagnostic>,
) {
    // Skip stdlib packages where a maps import would cycle (maps itself).
    if pass.pkg().pkg_path == "maps" || pass.pkg().pkg_path.starts_with("maps/") {
        return;
    }
    let pos = range_stmt.for_.0 as u32;
    if !go_at_least(pass, pos, "go1.23") {
        return;
    }
    if range_stmt.tok != Some(Token::DEFINE) {
        return;
    }
    let Some(key) = range_stmt.key.as_ref() else {
        return;
    };
    let Some(value) = range_stmt.value.as_ref() else {
        return;
    };
    // Body must be a single `m[k] = v` (or `:=`) assignment.
    if range_stmt.body.list.len() != 1 {
        return;
    }
    let Stmt::AssignStmt(assign) = &range_stmt.body.list[0] else {
        return;
    };
    if assign.tok != Some(Token::ASSIGN) && assign.tok != Some(Token::DEFINE) {
        return;
    }
    if assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return;
    }
    let Expr::IndexExpr(index) = &assign.lhs[0] else {
        return;
    };
    if !code::same_non_dynamic(pass, key, &index.index) {
        return;
    }
    if !code::same_non_dynamic(pass, value, &assign.rhs[0]) {
        return;
    }
    // Reject e.g. f(k, v)[k] = v
    if expr_uses_loop_vars(pass, &index.x, key, value) {
        return;
    }
    // Source x must be a map (iter.Seq2 → Insert/Collect is DEFERRED).
    let Some(src_map) = underlying_map(pass, &range_stmt.x) else {
        return;
    };
    let Some(dst_map) = underlying_map(pass, &index.x) else {
        return;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let Some(index_ty) = type_of(pass, &assign.lhs[0]) else {
        return;
    };
    let Some(value_ty) = type_of(pass, value) else {
        return;
    };
    let Some(key_ty) = type_of(pass, key) else {
        return;
    };
    // No implicit conversion of key or value.
    if !types_identical(pass, index_ty, value_ty) {
        return;
    }
    if !types_identical(pass, map_key(&artifacts.types, dst_map), key_ty) {
        return;
    }
    // Also require source map key/elem match (defensive; usually follows from assignability).
    if !types_identical(pass, map_key(&artifacts.types, src_map), key_ty)
        || !types_identical(pass, map_elem(&artifacts.types, src_map), value_ty)
    {
        return;
    }

    let Some(m_text) = expr_text_src(pass, &index.x) else {
        return;
    };
    let Some(x_text) = expr_text_src(pass, &range_stmt.x) else {
        return;
    };
    let end = range_stmt.body.end().0 as u32;
    let report_pos = assign.lhs[0].pos().0 as u32;
    let report_end = assign.lhs[0].end().0 as u32;
    let Some((prefix, mut text_edits)) = refactor::add_import(pass, file, "maps", "maps", "Copy", pos)
    else {
        return;
    };
    text_edits.push(TextEdit {
        pos,
        end,
        new_text: format!("{prefix}Copy({m_text}, {x_text})"),
    });
    pending.push(Diagnostic {
        pos: report_pos,
        end: report_end,
        category: String::new(),
        message: "Replace m[k]=v loop with maps.Copy".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace m[k]=v loop with maps.Copy".into(),
            text_edits,
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

/// The four functions upstream's index looks up: `strings.Split`,
/// `strings.Fields`, **`bytes.Split` and `bytes.Fields`**. `bytes` grew
/// `SplitSeq`/`FieldsSeq` in the same release, and the analyzer's own doc
/// comment lists them; guff had only the `strings` half, so syncthing's
/// `bytes.Split(data, []byte("\n"))` in `cmd/syncthing/crash_reporting.go`
/// went unreported.
fn split_or_fields_seq_name(pass: &Pass<'_>, call: &CallExpr) -> Option<&'static str> {
    if code::is_call_to(pass, call, "strings.Split") || code::is_call_to(pass, call, "bytes.Split")
    {
        Some("SplitSeq")
    } else if code::is_call_to(pass, call, "strings.Fields")
        || code::is_call_to(pass, call, "bytes.Fields")
    {
        Some("FieldsSeq")
    } else {
        None
    }
}

fn check_stringsseq(pass: &Pass<'_>, range_stmt: &RangeStmt, pending: &mut Vec<Diagnostic>) {
    let pos = range_stmt.for_.0 as u32;
    if !go_at_least(pass, pos, "go1.24") {
        return;
    }
    // SplitSeq/FieldsSeq are iter.Seq (value only); reject non-blank keys.
    if let Some(key) = range_stmt.key.as_ref() {
        if ident_name(key) != Some("_") {
            return;
        }
    }
    let Expr::CallExpr(call) = &range_stmt.x else {
        return;
    };
    let Some(seq_name) = split_or_fields_seq_name(pass, call) else {
        return;
    };
    let Some(fun_text) = expr_text_src(pass, &call.fun) else {
        return;
    };
    // strings.Split → strings.SplitSeq (replace the selector leaf).
    let new_fun = if let Some(prefix) = fun_text.rsplit_once('.') {
        format!("{}.{}", prefix.0, seq_name)
    } else {
        seq_name.to_string()
    };
    let _ = new_fun;
    let old_fn_name = fun_text.rsplit_once('.').map_or(fun_text.as_str(), |p| p.1);
    report_stringsseq(range_stmt, call, old_fn_name, seq_name, pending);
}

/// The report both spellings share. Upstream names only the *function*, not the
/// qualified selector: `Ranging over SplitSeq is more efficient`, never
/// `strings.SplitSeq` (x/tools v0.44.0 `modernize/stringsseq.go:126`).
fn report_stringsseq(
    range_stmt: &RangeStmt,
    call: &CallExpr,
    old_fn_name: &str,
    seq_name: &str,
    pending: &mut Vec<Diagnostic>,
) {
    let Expr::SelectorExpr(sel) = &*call.fun else {
        return;
    };
    // The fix first deletes the blank key: `for _, line :=` → `for line :=`,
    // `for _ :=` → `for`.
    let mut text_edits = Vec::new();
    if let Some(key) = range_stmt.key.as_ref() {
        let end = range_stmt
            .value
            .as_ref()
            .map(|v| v.pos())
            .unwrap_or(range_stmt.range_);
        text_edits.push(TextEdit {
            pos: key.pos().0 as u32,
            end: end.0 as u32,
            new_text: String::new(),
        });
    }
    text_edits.push(TextEdit {
        pos: sel.sel.pos().0 as u32,
        end: sel.sel.end().0 as u32,
        new_text: seq_name.to_string(),
    });
    pending.push(Diagnostic {
        pos: call.fun.pos().0 as u32,
        end: call.fun.end().0 as u32,
        category: String::new(),
        message: format!("Ranging over {seq_name} is more efficient"),
        suggested_fixes: vec![SuggestedFix {
            message: format!("Replace {old_fn_name} with {seq_name}"),
            text_edits,
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

/// The *indirect* spelling, which guff did not have at all:
///
/// ```go
/// lines := strings.Split(s, "\n")
/// for _, line := range lines {
/// ```
///
/// Upstream accepts it when the range operand is an identifier defined by the
/// immediately preceding `:=` in the same block, that statement has exactly one
/// name and one value, and the range is the variable's **sole** use — otherwise
/// rewriting the call would change what the other uses see
/// (`stringsseq.go:72-92`, `soleUseIs`). syncthing writes it seven times, and
/// two of those are this form.
fn check_stringsseq_block(pass: &Pass<'_>, block: &BlockStmt, pending: &mut Vec<Diagnostic>) {
    for i in 1..block.list.len() {
        let Stmt::RangeStmt(range_stmt) = &block.list[i] else {
            continue;
        };
        if !go_at_least(pass, range_stmt.for_.0 as u32, "go1.24") {
            continue;
        }
        if let Some(key) = range_stmt.key.as_ref() {
            if ident_name(key) != Some("_") {
                continue;
            }
        }
        let Expr::Ident(operand) = &range_stmt.x else {
            continue;
        };
        let Stmt::AssignStmt(assign) = &block.list[i - 1] else {
            continue;
        };
        if assign.tok != Some(Token::DEFINE) || assign.lhs.len() != 1 || assign.rhs.len() != 1 {
            continue;
        }
        let Expr::Ident(defined) = &assign.lhs[0] else {
            continue;
        };
        let Expr::CallExpr(call) = &assign.rhs[0] else {
            continue;
        };
        let Some(info) = pass.types_info() else {
            return;
        };
        let Some(def_obj) = info.defs.get(&defined.id).copied().flatten() else {
            continue;
        };
        if info.uses.get(&operand.id).copied() != Some(def_obj) {
            continue;
        }
        if !sole_use_is(pass, def_obj, operand.id) {
            continue;
        }
        let Some(seq_name) = split_or_fields_seq_name(pass, call) else {
            continue;
        };
        let Some(fun_text) = expr_text_src(pass, &call.fun) else {
            continue;
        };
        let old_fn_name = fun_text.rsplit_once('.').map_or(fun_text.as_str(), |p| p.1);
        report_stringsseq(range_stmt, call, old_fn_name, seq_name, pending);
    }
}

/// `soleUseIs` (x/tools `modernize/testingcontext.go:162`): `obj` is used, and
/// every use of it is `ident_id`.
fn sole_use_is(pass: &Pass<'_>, obj: ObjectId, ident_id: u32) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let mut seen = false;
    for (id, used) in &info.uses {
        if *used != obj {
            continue;
        }
        if *id != ident_id {
            return false;
        }
        seen = true;
    }
    seen
}

fn is_named_pkg_type(pass: &Pass<'_>, typ: TypeId, pkg: &str, name: &str) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let typ = match artifacts.types.get(typ) {
        TypeData::Pointer(p) => unalias_readonly(&artifacts.types, p.elem()),
        _ => typ,
    };
    let TypeData::Named(_) = artifacts.types.get(typ) else {
        return false;
    };
    let obj = named_obj(&artifacts.types, typ);
    if obj.name(&artifacts.objects) != name {
        return false;
    }
    obj.pkg(&artifacts.objects)
        .is_some_and(|p| artifacts.packages.get(p).path() == pkg)
}

fn is_waitgroup_method(pass: &Pass<'_>, call: &CallExpr, method: &str) -> bool {
    let Expr::SelectorExpr(sel) = &*call.fun else {
        return false;
    };
    if sel.sel.name != method {
        return false;
    }
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(obj_id) = info.uses.get(&sel.sel.id).copied() else {
        return false;
    };
    if !matches!(artifacts.objects.get(obj_id), ObjectData::Func(_)) {
        return false;
    }
    let Some(sig_id) = obj_id.typ(&artifacts.objects) else {
        return false;
    };
    let Some(recv) = signature_recv(&artifacts.types, sig_id) else {
        return false;
    };
    let Some(recv_typ) = recv.typ(&artifacts.objects) else {
        return false;
    };
    is_named_pkg_type(pass, recv_typ, "sync", "WaitGroup")
}

fn waitgroup_recv(call: &CallExpr) -> Option<&Expr> {
    match &*call.fun {
        Expr::SelectorExpr(sel) => Some(sel.x.as_ref()),
        _ => None,
    }
}

fn check_waitgroupgo(
    pass: &Pass<'_>,
    file: &File,
    block: &BlockStmt,
    pending: &mut Vec<Diagnostic>,
) {
    for i in 0..block.list.len().saturating_sub(1) {
        let Stmt::ExprStmt(add_stmt) = &block.list[i] else {
            continue;
        };
        let Expr::CallExpr(add_call) = &add_stmt.x else {
            continue;
        };
        if !is_waitgroup_method(pass, add_call, "Add") || add_call.args.len() != 1 {
            continue;
        }
        if !code::is_integer_constant(pass, &add_call.args[0], 1) {
            continue;
        }
        let Some(add_recv) = waitgroup_recv(add_call) else {
            continue;
        };
        let Stmt::GoStmt(GoStmt { go_, call: go_call }) = &block.list[i + 1] else {
            continue;
        };
        if !go_call.args.is_empty() {
            continue;
        }
        let Expr::FuncLit(lit) = &*go_call.fun else {
            continue;
        };
        if lit.ty.results.as_ref().is_some_and(|f| !f.list.is_empty()) {
            continue;
        }
        if lit.body.list.is_empty() {
            continue;
        }
        let Stmt::DeferStmt(defer_stmt) = &lit.body.list[0] else {
            continue;
        };
        if !is_waitgroup_method(pass, &defer_stmt.call, "Done") {
            continue;
        }
        let Some(done_recv) = waitgroup_recv(&defer_stmt.call) else {
            continue;
        };
        if !code::same_non_dynamic(pass, add_recv, done_recv) {
            continue;
        }
        let pos = go_.0 as u32;
        if !go_at_least(pass, pos, "go1.25") {
            continue;
        }
        let Some(recv_text) = expr_text_src(pass, add_recv) else {
            continue;
        };
        pending.push(Diagnostic {
            pos,
            end: lit.ty.end().0 as u32,
            category: String::new(),
            message: "Goroutine creation can be simplified using WaitGroup.Go".into(),
            suggested_fixes: vec![SuggestedFix {
                message: "Simplify by using WaitGroup.Go".into(),
                text_edits: {
                    // Upstream deletes both statements with
                    // `refactor.DeleteStmt`, which takes the line as well —
                    // leaving the span alone strands a line of whitespace where
                    // `wg.Add(1)` used to be.
                    let src = refactor::file_source(pass, file);
                    let mut edits = refactor::delete_with_line(
                        file,
                        src,
                        add_stmt.x.pos().0 as u32,
                        add_stmt.x.end().0 as u32,
                    );
                    edits.push(TextEdit {
                        pos,
                        end: go_call.pos().0 as u32,
                        new_text: format!("{recv_text}.Go("),
                    });
                    edits.extend(refactor::delete_with_line(
                        file,
                        src,
                        defer_stmt.defer_.0 as u32,
                        defer_stmt.call.end().0 as u32,
                    ));
                    // ... }()
                    //      -
                    // ... } )
                    //
                    // Without this the call the goroutine used to make is left
                    // behind inside the one being built: `wg.Go(func() {…}()`,
                    // which does not parse. A missing edit is invisible to
                    // every finding-set gate — the diagnostic is identical
                    // either way — and shows up only as a tree that stops
                    // compiling (COMPAT-HARDENING, `compat/fix/`).
                    edits.push(TextEdit {
                        pos: go_call.lparen.0 as u32,
                        end: go_call.rparen.0 as u32,
                        new_text: String::new(),
                    });
                    edits
                },
            }],
            related: Vec::new(),
            url: String::new(),
            severity: String::new(),
            ..Diagnostic::default()
        });
    }
}

fn is_simple_dec(post: &Stmt, index_name: &str) -> bool {
    match post {
        Stmt::IncDecStmt(IncDecStmt { x, tok, .. }) => {
            *tok == Token::DEC && ident_name(x) == Some(index_name)
        }
        Stmt::AssignStmt(a)
            if a.tok == Some(Token::SubAssign)
                && a.lhs.len() == 1
                && a.rhs.len() == 1
                && ident_name(&a.lhs[0]) == Some(index_name) =>
        {
            matches!(&a.rhs[0], Expr::BasicLit(lit) if lit.value == "1")
        }
        _ => false,
    }
}

fn index_mutated_in_body(pass: &Pass<'_>, body: &BlockStmt, index_obj: ObjectId) -> bool {
    let mut mutated = false;
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        match n {
            NodeRef::AssignStmt(a) => {
                for lhs in &a.lhs {
                    if let Expr::Ident(id) = lhs {
                        if ident_obj(pass, id) == Some(index_obj) {
                            mutated = true;
                            return false;
                        }
                    }
                }
            }
            NodeRef::IncDecStmt(IncDecStmt { x, .. }) => {
                if let Expr::Ident(id) = x {
                    if ident_obj(pass, id) == Some(index_obj) {
                        mutated = true;
                        return false;
                    }
                }
            }
            NodeRef::UnaryExpr(u) if u.op == Token::AND => {
                if let Expr::Ident(id) = u.x.as_ref() {
                    if ident_obj(pass, id) == Some(index_obj) {
                        mutated = true;
                        return false;
                    }
                }
            }
            _ => {}
        }
        true
    });
    mutated
}

fn check_slicesbackward(
    pass: &Pass<'_>,
    file: &File,
    for_stmt: &ForStmt,
    pending: &mut Vec<Diagnostic>,
) {
    if pass.pkg().pkg_path == "slices" || pass.pkg().pkg_path.starts_with("slices/") {
        return;
    }
    let pos = for_stmt.for_.0 as u32;
    if !go_at_least(pass, pos, "go1.23") {
        return;
    }
    let Some(Stmt::AssignStmt(init)) = for_stmt.init.as_deref() else {
        return;
    };
    if init.lhs.len() != 1 || init.rhs.len() != 1 {
        return;
    }
    if init.tok != Some(Token::DEFINE) && init.tok != Some(Token::ASSIGN) {
        return;
    }
    let Some(index_name) = ident_name(&init.lhs[0]) else {
        return;
    };
    let Expr::BinaryExpr(bin) = &init.rhs[0] else {
        return;
    };
    if bin.op != Token::SUB || !code::is_integer_constant(pass, &bin.y, 1) {
        return;
    }
    let Expr::CallExpr(len_call) = bin.x.as_ref() else {
        return;
    };
    if !code::is_call_to(pass, len_call, "len") || len_call.args.len() != 1 {
        return;
    }
    if type_kind(pass, &len_call.args[0]) != Some(TypeKind::Slice) {
        return;
    }
    let Some(Expr::BinaryExpr(cond)) = for_stmt.cond.as_ref() else {
        return;
    };
    if cond.op != Token::GEQ
        || ident_name(&cond.x) != Some(index_name)
        || !code::is_integer_constant(pass, &cond.y, 0)
    {
        return;
    }
    let Some(post) = for_stmt.post.as_deref() else {
        return;
    };
    if !is_simple_dec(post, index_name) {
        return;
    }
    let Expr::Ident(index_id) = &init.lhs[0] else {
        return;
    };
    let Some(index_obj) = ident_obj(pass, index_id) else {
        return;
    };
    if index_mutated_in_body(pass, &for_stmt.body, index_obj) {
        return;
    }

    let slice_expr = &len_call.args[0];
    let Some(slice_text) = expr_text_src(pass, slice_expr) else {
        return;
    };

    // Classify body uses of i: pure s[i] vs other.
    let mut slice_indexes: Vec<(u32, u32)> = Vec::new();
    let mut other_uses = 0usize;
    walk::inspect(NodeRef::BlockStmt(&for_stmt.body), |n| {
        let Some(n) = n else {
            return true;
        };
        let NodeRef::Ident(id) = n else {
            return true;
        };
        if ident_obj(pass, id) != Some(index_obj) {
            return true;
        }
        // Walk parent is not available; approximate: collect all idents and
        // separately find IndexExpr where index is this ident and x is slice.
        other_uses += 1;
        true
    });
    // Re-scan for s[i] patterns and subtract them from other_uses.
    walk::inspect(NodeRef::BlockStmt(&for_stmt.body), |n| {
        let Some(n) = n else {
            return true;
        };
        if let NodeRef::IndexExpr(ix) = n {
            if code::same_non_dynamic(pass, &ix.x, slice_expr)
                && code::same_non_dynamic(pass, &ix.index, &init.lhs[0])
            {
                slice_indexes.push((ix.x.pos().0 as u32, (ix.rbrack.0 + 1) as u32));
            }
        }
        true
    });
    other_uses = other_uses.saturating_sub(slice_indexes.len());

    let end = for_stmt
        .post
        .as_ref()
        .map(|p| p.end().0 as u32)
        .unwrap_or(pos);
    let header_pos = init.lhs[0].pos().0 as u32;
    let elem_name = "v";
    let Some((prefix, mut text_edits)) =
        refactor::add_import(pass, file, "slices", "slices", "Backward", pos)
    else {
        return;
    };
    let header = if other_uses == 0 && !slice_indexes.is_empty() {
        format!("_, {elem_name} := range {prefix}Backward({slice_text})")
    } else {
        format!("{index_name}, {elem_name} := range {prefix}Backward({slice_text})")
    };
    text_edits.push(TextEdit {
        pos: header_pos,
        end,
        new_text: header,
    });
    if other_uses == 0 {
        for (ipos, iend) in &slice_indexes {
            text_edits.push(TextEdit {
                pos: *ipos,
                end: *iend,
                new_text: elem_name.into(),
            });
        }
    }
    pending.push(Diagnostic {
        pos: header_pos,
        end,
        category: "slicesbackward".into(),
        message: "backward loop over slice can be modernized using slices.Backward".into(),
        suggested_fixes: vec![SuggestedFix {
            // Upstream names the package here literally, not through the
            // prefix: the fix message says slices.Backward even when the edit
            // writes an alias.
            message: format!("Replace with range slices.Backward({slice_text})"),
            text_edits,
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

fn format_type(pass: &Pass<'_>, typ: TypeId) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let pkg = pass.type_pkg();
    let qf = pkg.map(|p| {
        move |id: guff_types::arena::PackageId, parena: &guff_types::arena::PackageArena| {
            if id == p {
                String::new()
            } else {
                parena.get(id).name().to_string()
            }
        }
    });
    let qf_ref = qf.as_ref().map(|f| {
        f as &dyn Fn(guff_types::arena::PackageId, &guff_types::arena::PackageArena) -> String
    });
    Some(type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        qf_ref,
    ))
}

fn is_complicated_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return true;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Named(_) | TypeData::Alias(_) | TypeData::Basic(_) | TypeData::TypeParam(_) => {
            false
        }
        TypeData::Pointer(p) => is_complicated_type(pass, p.elem()),
        TypeData::Slice(s) => is_complicated_type(pass, s.elem()),
        TypeData::Array(a) => is_complicated_type(pass, a.elem()),
        TypeData::Chan(c) => is_complicated_type(pass, c.elem()),
        TypeData::Map(m) => {
            is_complicated_type(pass, m.key()) || is_complicated_type(pass, m.elem())
        }
        TypeData::Struct(_) | TypeData::Interface(_) | TypeData::Signature(_) => true,
        _ => true,
    }
}

fn expr_has_effects(pass: &Pass<'_>, expr: &Expr) -> bool {
    match expr {
        Expr::Ident(_) | Expr::BasicLit(_) | Expr::SelectorExpr(_) | Expr::CompositeLit(_) => false,
        Expr::ParenExpr(p) => expr_has_effects(pass, &p.x),
        Expr::UnaryExpr(u) => {
            // Channel receive (`<-ch`) has side effects.
            if u.op == Token::ARROW {
                return true;
            }
            expr_has_effects(pass, &u.x)
        }
        Expr::StarExpr(s) => expr_has_effects(pass, &s.x),
        Expr::IndexExpr(ix) => expr_has_effects(pass, &ix.x) || expr_has_effects(pass, &ix.index),
        Expr::CallExpr(call) => {
            // Type conversion T(x) is effect-free if x is.
            let info = pass.types_info();
            let is_conv = info
                .and_then(|i| i.types.get(&call.fun.id()))
                .is_some_and(|tav| tav.mode == OperandMode::TypeExpr);
            if is_conv && call.args.len() == 1 {
                return expr_has_effects(pass, &call.args[0]);
            }
            true
        }
        _ => true,
    }
}

fn is_nil_typed_conversion(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Expr::CallExpr(call) = expr else {
        return false;
    };
    if call.args.len() != 1 || !code::is_nil(pass, &call.args[0]) {
        return false;
    }
    pass.types_info()
        .and_then(|i| i.types.get(&call.fun.id()))
        .is_some_and(|tav| tav.mode == OperandMode::TypeExpr)
}

/// Like [`is_named_pkg_type`] but does **not** unwrap pointers — needed so
/// `*reflect.Value` is not treated as `reflect.Value` (upstream leaves those alone).
fn is_exact_named_pkg_type(pass: &Pass<'_>, typ: TypeId, pkg: &str, name: &str) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let TypeData::Named(_) = artifacts.types.get(typ) else {
        return false;
    };
    let obj = named_obj(&artifacts.types, typ);
    if obj.name(&artifacts.objects) != name {
        return false;
    }
    obj.pkg(&artifacts.objects)
        .is_some_and(|p| artifacts.packages.get(p).path() == pkg)
}

/// `x, ok := v.Interface().(T)` → `reflect.TypeAssert[T](v)` (Go 1.25+).
fn check_reflecttypeassert(
    pass: &Pass<'_>,
    file: &File,
    assign: &AssignStmt,
    pending: &mut Vec<Diagnostic>,
) {
    if assign.lhs.len() != 2 || assign.rhs.len() != 1 {
        return;
    }
    if assign.tok != Some(Token::ASSIGN) && assign.tok != Some(Token::DEFINE) {
        return;
    }
    let Expr::TypeAssertExpr(assert) = unparen_expr(&assign.rhs[0]) else {
        return;
    };
    let Some(ty) = assert.ty.as_ref() else {
        return; // type switch
    };
    let Expr::CallExpr(call) = unparen_expr(&assert.x) else {
        return;
    };
    let Expr::SelectorExpr(sel) = unparen_expr(&call.fun) else {
        return; // method expression reflect.Value.Interface(v)
    };
    if sel.sel.name != "Interface" || !call.args.is_empty() {
        return;
    }
    let Some(recv_ty) = type_of(pass, &sel.x) else {
        return;
    };
    // Pointer receiver would need an explicit dereference in the rewrite.
    if !is_exact_named_pkg_type(pass, recv_ty, "reflect", "Value") {
        return;
    }
    let pos = assert.x.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.25") {
        return;
    }
    let Some(tstr) = expr_text_src(pass, ty) else {
        return;
    };
    let end = (assert.rparen.0 + 1) as u32;
    let Some((prefix, import_edits)) =
        refactor::add_import(pass, file, "reflect", "reflect", "TypeAssert", pos)
    else {
        return;
    };
    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: format!(
            "Interface().({tstr}) can be simplified using reflect.TypeAssert"
        ),
        suggested_fixes: vec![SuggestedFix {
            message: format!(
                "Replace Interface().({tstr}) by reflect.TypeAssert[{tstr}]"
            ),
            text_edits: with_imports(
                &import_edits,
                vec![
                    TextEdit {
                        pos,
                        end: sel.x.pos().0 as u32,
                        new_text: format!("{prefix}TypeAssert[{tstr}]("),
                    },
                    TextEdit {
                        pos: sel.x.end().0 as u32,
                        end,
                        new_text: ")".into(),
                    },
                ],
            ),
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

/// Edits deleting the declarations of local variables that `deleted` was the
/// last use of.
///
/// Port of `refactor.DeleteUnusedVars`. Rewriting `reflect.TypeOf(zero)` to
/// `reflect.TypeFor[MyStruct]()` erases the only mention of `zero`, and Go
/// rejects the result with `declared and not used` — the fix has to take the
/// declaration with it.
///
/// Scoped to the shape `var x T` with no initialiser and no other names, which
/// is `deleteVarFromValueSpec`'s `!declaresOtherNames && noRHSEffects` branch
/// reaching `DeleteSpec` -> `DeleteDecl` -> `DeleteStmt`. DEFERRED: the `n:n`
/// assignment and multi-name spec forms, which blank out one name rather than
/// removing a line. Declining there costs a `declared and not used` that guff
/// already had, never a wrong edit.
fn delete_newly_unused_vars(
    pass: &Pass<'_>,
    file: &File,
    deleted: &Expr,
    edits: &mut Vec<TextEdit>,
) {
    let Some(index) = pass.result_of::<typeindex::Index>(typeindex::analyzer()) else {
        return;
    };
    let (Some(info), Some(artifacts)) = (pass.types_info(), pass.pkg().type_artifacts.as_ref())
    else {
        return;
    };

    // How many uses of each local var disappear with `deleted`.
    let mut delcount: HashMap<ObjectId, usize> = HashMap::new();
    walk::inspect(walk::expr_ref(deleted), |n| {
        let Some(NodeRef::Ident(id)) = n else {
            return true;
        };
        if let Some(obj) = info.uses.get(&id.id).copied() {
            if let ObjectData::Var(v) = artifacts.objects.get(obj) {
                if v.kind() == VarKind::Local {
                    *delcount.entry(obj).or_default() += 1;
                }
            }
        }
        true
    });

    // Deterministic order: two vars in one deleted expression would otherwise
    // produce edits in hash order, and the recorded diff has to be stable.
    let mut objs: Vec<ObjectId> = delcount.keys().copied().collect();
    objs.sort_by_key(|o| index.def(*o).unwrap_or(0));

    let src = refactor::file_source(pass, file);
    for obj in objs {
        if index.uses(obj).len() != delcount[&obj] {
            continue; // still used elsewhere
        }
        let Some(def_id) = index.def(obj) else {
            continue;
        };
        if let Some((pos, end)) = sole_var_decl_span(file, def_id) {
            edits.extend(refactor::delete_with_line(file, src, pos, end));
        }
    }
}

/// The span of `var x T` when `def_id` names its sole variable and it has no
/// initialiser; `None` for every other declaration shape.
fn sole_var_decl_span(file: &File, def_id: u32) -> Option<(u32, u32)> {
    let mut found = None;
    walk::inspect(NodeRef::File(file), |n| {
        if found.is_some() {
            return false;
        }
        let Some(NodeRef::DeclStmt(ds)) = n else {
            return true;
        };
        let Decl::GenDecl(gd) = &ds.decl else {
            return true;
        };
        if gd.tok != Some(Token::VAR) || gd.specs.len() != 1 || gd.rparen.is_valid() {
            return true;
        }
        let Spec::ValueSpec(vs) = &gd.specs[0] else {
            return true;
        };
        if vs.names.len() != 1 || !vs.values.is_empty() || vs.names[0].id != def_id {
            return true;
        }
        found = Some((gd.tok_pos.0 as u32, ds.decl.end().0 as u32));
        false
    });
    found
}

fn check_reflecttypefor(
    pass: &Pass<'_>,
    file: &File,
    call: &CallExpr,
    pending: &mut Vec<Diagnostic>,
) {
    if !code::is_call_to(pass, call, "reflect.TypeOf") || call.args.len() != 1 {
        return;
    }
    // Skip `TypeOf((*T)(nil))` / `TypeOf([]T(nil))` — usually paired with `.Elem()`
    // (handled by `check_reflecttypefor_elem`). Reporting both would duplicate.
    if is_nil_typed_conversion(pass, &call.args[0]) {
        return;
    }
    let pos = call.fun.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.22") {
        return;
    }
    if code::is_nil(pass, &call.args[0]) {
        return;
    }
    if expr_has_effects(pass, &call.args[0]) {
        return;
    }
    let Some(arg_ty) = type_of(pass, &call.args[0]) else {
        return;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    // TypeOf(x) where x has an interface type is a dynamic operation; don't
    // transform it to TypeFor (x/tools reflecttypefor). Also skip when the
    // static type is Invalid or an incomplete Named/Alias — under
    // `run_despite_errors`, TypesInfo can carry Invalid for otherwise
    // well-typed interface params (seen on prometheus `parser.Expr`), and
    // suggesting TypeFor would be a false positive.
    let typ = unalias_readonly(&artifacts.types, arg_ty);
    if is_interface(&artifacts.types, typ) {
        return;
    }
    let under = typ.underlying(&artifacts.types);
    match artifacts.types.get(under) {
        TypeData::Basic(b) if b.kind() == BasicKind::Invalid => return,
        TypeData::Named(_) | TypeData::Alias(_) => return,
        _ => {}
    }
    if is_complicated_type(pass, arg_ty) {
        return;
    }
    let Some(tstr) = format_type(pass, arg_ty) else {
        return;
    };
    if tstr.len() >= 16 {
        let old_len = (call.args[0].end().0 - call.args[0].pos().0).max(1) as usize;
        if tstr.len() > 3 * old_len {
            return;
        }
    }
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return; // e.g. dot-import
    };
    let end = call.fun.end().0 as u32;
    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: "reflect.TypeOf call can be simplified using TypeFor".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace TypeOf by TypeFor".into(),
            text_edits: {
                let mut edits = vec![
                    TextEdit {
                        pos: sel.sel.pos().0 as u32,
                        end: sel.sel.end().0 as u32,
                        new_text: format!("TypeFor[{tstr}]"),
                    },
                    TextEdit {
                        pos: (call.lparen.0 + 1) as u32,
                        end: call.rparen.0 as u32,
                        new_text: String::new(),
                    },
                ];
                delete_newly_unused_vars(pass, file, &call.args[0], &mut edits);
                edits
            },
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

fn check_reflecttypefor_elem(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<Diagnostic>) {
    // Match reflect.TypeOf(expr).Elem()
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return;
    };
    if sel.sel.name != "Elem" || !call.args.is_empty() {
        return;
    }
    let Expr::CallExpr(typeof_call) = sel.x.as_ref() else {
        return;
    };
    if !code::is_call_to(pass, typeof_call, "reflect.TypeOf") || typeof_call.args.len() != 1 {
        return;
    }
    let pos = typeof_call.fun.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.22") {
        return;
    }
    if expr_has_effects(pass, &typeof_call.args[0]) {
        return;
    }
    let Some(arg_ty) = type_of(pass, &typeof_call.args[0]) else {
        return;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let under = unalias_readonly(&artifacts.types, arg_ty).underlying(&artifacts.types);
    let elem = match artifacts.types.get(under) {
        TypeData::Pointer(p) => p.elem(),
        TypeData::Slice(s) => s.elem(),
        TypeData::Array(a) => a.elem(),
        TypeData::Chan(c) => c.elem(),
        TypeData::Map(m) => m.elem(),
        _ => return,
    };
    if is_complicated_type(pass, elem) {
        return;
    }
    let Some(tstr) = format_type(pass, elem) else {
        return;
    };
    let Expr::SelectorExpr(typeof_sel) = typeof_call.fun.as_ref() else {
        return;
    };
    pending.push(Diagnostic {
        pos,
        end: call.end().0 as u32,
        category: String::new(),
        message: "reflect.TypeOf call can be simplified using TypeFor".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace TypeOf by TypeFor".into(),
            text_edits: vec![
                TextEdit {
                    pos: typeof_sel.sel.pos().0 as u32,
                    end: typeof_sel.sel.end().0 as u32,
                    new_text: format!("TypeFor[{tstr}]"),
                },
                TextEdit {
                    pos: (typeof_call.lparen.0 + 1) as u32,
                    end: typeof_call.rparen.0 as u32,
                    new_text: String::new(),
                },
                // delete `.Elem()`
                TextEdit {
                    pos: typeof_call.end().0 as u32,
                    end: call.end().0 as u32,
                    new_text: String::new(),
                },
            ],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

fn test_kind_prefix(name: &str) -> Option<&'static str> {
    for prefix in ["Test", "Benchmark", "Fuzz"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            if rest.is_empty() {
                return Some(prefix);
            }
            let first = rest.chars().next()?;
            if first.is_uppercase() {
                return Some(prefix);
            }
        }
    }
    None
}

fn testing_param_name(fd: &FuncDecl) -> Option<&str> {
    test_kind_prefix(&fd.name.name)?;
    let params = fd.ty.params.as_ref()?;
    if params.list.len() != 1 {
        return None;
    }
    if fd.ty.results.as_ref().is_some_and(|r| !r.list.is_empty()) {
        return None;
    }
    let field = &params.list[0];
    if field.names.len() != 1 || field.names[0].name == "_" {
        return None;
    }
    let ty = field.ty.as_ref()?;
    let Expr::StarExpr(star) = ty else {
        return None;
    };
    let Expr::SelectorExpr(sel) = star.x.as_ref() else {
        return None;
    };
    let Expr::Ident(pkg) = sel.x.as_ref() else {
        return None;
    };
    if pkg.name != "testing" {
        return None;
    }
    let want = match test_kind_prefix(&fd.name.name)? {
        "Test" => "T",
        "Benchmark" => "B",
        "Fuzz" => "F",
        _ => return None,
    };
    if sel.sel.name != want {
        return None;
    }
    Some(field.names[0].name.as_str())
}

fn count_ident_uses(pass: &Pass<'_>, body: &BlockStmt, obj: ObjectId) -> usize {
    let mut n = 0;
    walk::inspect(NodeRef::BlockStmt(body), |node| {
        let Some(node) = node else {
            return true;
        };
        if let NodeRef::Ident(id) = node {
            if ident_obj(pass, id) == Some(obj) {
                n += 1;
            }
        }
        true
    });
    n
}

fn check_testingcontext_block(
    pass: &Pass<'_>,
    body: &BlockStmt,
    test_name: &str,
    pending: &mut Vec<Diagnostic>,
) {
    for i in 0..body.list.len().saturating_sub(1) {
        let Stmt::AssignStmt(assign) = &body.list[i] else {
            continue;
        };
        let Stmt::DeferStmt(defr) = &body.list[i + 1] else {
            continue;
        };
        if assign.tok != Some(Token::DEFINE) || assign.lhs.len() != 2 || assign.rhs.len() != 1 {
            continue;
        }
        let Expr::CallExpr(with_cancel) = &assign.rhs[0] else {
            continue;
        };
        if !code::is_call_to(pass, with_cancel, "context.WithCancel") || with_cancel.args.len() != 1
        {
            continue;
        }
        let Expr::CallExpr(bg) = &with_cancel.args[0] else {
            continue;
        };
        if !code::is_call_to_any(pass, bg, &["context.Background", "context.TODO"]) {
            continue;
        }
        let pos = with_cancel.fun.pos().0 as u32;
        if !go_at_least(pass, pos, "go1.24") {
            continue;
        }
        let Some(ctx_name) = ident_name(&assign.lhs[0]) else {
            continue;
        };
        let Expr::Ident(cancel_id) = &assign.lhs[1] else {
            continue;
        };
        if cancel_id.name == "_" {
            continue;
        }
        let Some(cancel_obj) = ident_obj(pass, cancel_id) else {
            continue;
        };
        // defer cancel() — sole use of cancel (assignment lhs + this call = 2).
        let Expr::Ident(defer_fun) = defr.call.fun.as_ref() else {
            continue;
        };
        if ident_obj(pass, defer_fun) != Some(cancel_obj) || !defr.call.args.is_empty() {
            continue;
        }
        if count_ident_uses(pass, body, cancel_obj) != 2 {
            continue;
        }
        pending.push(Diagnostic {
            pos,
            end: with_cancel.fun.end().0 as u32,
            category: String::new(),
            message: format!("context.WithCancel can be modernized using {test_name}.Context"),
            suggested_fixes: vec![SuggestedFix {
                message: format!("Replace context.WithCancel with {test_name}.Context"),
                text_edits: vec![TextEdit {
                    pos: assign.lhs[0].pos().0 as u32,
                    end: defr.call.end().0 as u32,
                    new_text: format!("{ctx_name} := {test_name}.Context()"),
                }],
            }],
            related: Vec::new(),
            url: String::new(),
            severity: String::new(),
            ..Diagnostic::default()
        });
    }
}

fn check_testingcontext(pass: &Pass<'_>, file: &File, pending: &mut Vec<Diagnostic>) {
    for decl in &file.decls {
        let guff::ast::Decl::FuncDecl(fd) = decl else {
            continue;
        };
        let Some(t_name) = testing_param_name(fd) else {
            continue;
        };
        let Some(body) = fd.body.as_ref() else {
            continue;
        };
        check_testingcontext_block(pass, body, t_name, pending);
        // Also scan nested t.Run(..., func(t *testing.T) { ... }) bodies.
        walk::inspect(NodeRef::BlockStmt(body), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::FuncLit(fl) = n {
                if let Some(field) = fl.ty.params.as_ref().and_then(|p| p.list.first()) {
                    if field.names.len() == 1 && field.names[0].name != "_" {
                        if let Some(ty) = field.ty.as_ref() {
                            if let Expr::StarExpr(star) = ty {
                                if let Expr::SelectorExpr(sel) = star.x.as_ref() {
                                    if let Expr::Ident(pkg) = sel.x.as_ref() {
                                        if pkg.name == "testing"
                                            && matches!(sel.sel.name.as_str(), "T" | "B" | "F")
                                        {
                                            check_testingcontext_block(
                                                pass,
                                                &fl.body,
                                                field.names[0].name.as_str(),
                                                pending,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            true
        });
    }
}

/// Port of modernize `bloop`: `for … b.N …` → `for b.Loop()` (Go 1.24+).
///
/// Only modernizes the sole `b.N` loop directly in a `Benchmark*` function
/// (not inside a FuncLit). Preceding `b.{Start,Stop,Reset}Timer` calls in the
/// same function (outside FuncLits) are deleted in the SuggestedFix.
fn check_bloop(pass: &Pass<'_>, file: &File, pending: &mut Vec<Diagnostic>) {
    for decl in &file.decls {
        let Decl::FuncDecl(fd) = decl else {
            continue;
        };
        if !is_benchmark_func(fd) {
            continue;
        }
        let Some(body) = fd.body.as_ref() else {
            continue;
        };
        if count_benchmark_n_refs(pass, body) != 1 {
            continue;
        }
        walk::inspect(NodeRef::BlockStmt(body), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::FuncLit(_) => false, // don't descend: b.Loop must be in Benchmark goroutine
                NodeRef::ForStmt(for_stmt) => {
                    check_bloop_for(pass, body, for_stmt, pending);
                    true
                }
                NodeRef::RangeStmt(range_stmt) => {
                    check_bloop_range(pass, body, range_stmt, pending);
                    true
                }
                _ => true,
            }
        });
    }
}

fn is_benchmark_func(fd: &FuncDecl) -> bool {
    fd.recv.is_none()
        && test_kind_prefix(&fd.name.name) == Some("Benchmark")
        && fd.ty.params.as_ref().is_some_and(|p| p.list.len() == 1)
}

fn is_testing_b(pass: &Pass<'_>, expr: &Expr) -> bool {
    type_of(pass, expr).is_some_and(|typ| is_named_pkg_type(pass, typ, "testing", "B"))
}

fn benchmark_n_recv<'a>(pass: &Pass<'_>, expr: &'a Expr) -> Option<&'a Expr> {
    let Expr::SelectorExpr(sel) = expr else {
        return None;
    };
    if sel.sel.name != "N" || !is_testing_b(pass, &sel.x) {
        return None;
    }
    Some(&sel.x)
}

fn count_benchmark_n_refs(pass: &Pass<'_>, body: &BlockStmt) -> usize {
    let mut n = 0;
    walk::inspect(NodeRef::BlockStmt(body), |node| {
        let Some(node) = node else {
            return true;
        };
        if let NodeRef::SelectorExpr(sel) = node {
            if sel.sel.name == "N" && is_testing_b(pass, &sel.x) {
                n += 1;
            }
        }
        true
    });
    n
}

fn is_testing_b_timer_call(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Expr::SelectorExpr(sel) = &*call.fun else {
        return false;
    };
    if !matches!(
        sel.sel.name.as_str(),
        "StartTimer" | "StopTimer" | "ResetTimer"
    ) {
        return false;
    }
    is_testing_b(pass, &sel.x)
}

fn bloop_timer_edits(pass: &Pass<'_>, body: &BlockStmt, before: u32) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    walk::inspect(NodeRef::BlockStmt(body), |node| {
        let Some(node) = node else {
            return true;
        };
        match node {
            NodeRef::FuncLit(_) => false,
            NodeRef::ExprStmt(es) => {
                if let Expr::CallExpr(call) = &es.x {
                    let pos = es.x.pos().0 as u32;
                    if pos < before && is_testing_b_timer_call(pass, call) {
                        edits.push(TextEdit {
                            pos,
                            end: es.x.end().0 as u32,
                            new_text: String::new(),
                        });
                    }
                }
                true
            }
            _ => true,
        }
    });
    edits
}

fn increment_loop_index(pass: &Pass<'_>, for_stmt: &ForStmt) -> Option<ObjectId> {
    let Stmt::AssignStmt(init) = for_stmt.init.as_deref()? else {
        return None;
    };
    if init.tok != Some(Token::DEFINE) || init.lhs.len() != 1 || init.rhs.len() != 1 {
        return None;
    }
    if !code::is_integer_constant(pass, &init.rhs[0], 0) {
        return None;
    }
    let Expr::Ident(lhs) = &init.lhs[0] else {
        return None;
    };
    let post = for_stmt.post.as_deref()?;
    if !is_simple_inc(post, lhs.name.as_str()) {
        return None;
    }
    ident_obj(pass, lhs)
}

fn check_bloop_for(
    pass: &Pass<'_>,
    fn_body: &BlockStmt,
    for_stmt: &ForStmt,
    pending: &mut Vec<Diagnostic>,
) {
    let pos = for_stmt.for_.0 as u32;
    if !go_at_least(pass, pos, "go1.24") {
        return;
    }
    let Some(cond) = for_stmt.cond.as_ref() else {
        return;
    };
    let Expr::BinaryExpr(cmp) = cond else {
        return;
    };
    if cmp.op != Token::LSS {
        return;
    }
    let Some(b_recv) = benchmark_n_recv(pass, &cmp.y) else {
        return;
    };
    let Some(b_text) = expr_text_src(pass, b_recv) else {
        return;
    };

    let cond_pos = cond.pos().0 as u32;
    let cond_end = cond.end().0 as u32;
    let mut del_pos = cond_pos;
    let mut del_end = cond_end;

    // Eliminate `i := 0; …; i++` when `i` is unused in the body.
    if let Some(idx) = increment_loop_index(pass, for_stmt) {
        if count_ident_uses(pass, &for_stmt.body, idx) == 0 {
            if let (Some(init), Some(post)) = (for_stmt.init.as_ref(), for_stmt.post.as_ref()) {
                del_pos = init.pos().0 as u32;
                del_end = post.end().0 as u32;
            }
        }
    }

    let mut edits = bloop_timer_edits(pass, fn_body, del_pos);
    edits.push(TextEdit {
        pos: del_pos,
        end: del_end,
        new_text: format!("{b_text}.Loop()"),
    });

    pending.push(Diagnostic {
        pos: cond_pos,
        end: cond_end,
        category: String::new(),
        message: "b.N can be modernized using b.Loop()".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace b.N with b.Loop()".into(),
            text_edits: edits,
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

fn check_bloop_range(
    pass: &Pass<'_>,
    fn_body: &BlockStmt,
    range_stmt: &RangeStmt,
    pending: &mut Vec<Diagnostic>,
) {
    // DEFERRED: `for i := range b.N` (keyed form).
    if range_stmt.key.is_some() || range_stmt.value.is_some() {
        return;
    }
    let pos = range_stmt.for_.0 as u32;
    if !go_at_least(pass, pos, "go1.24") {
        return;
    }
    let Some(b_recv) = benchmark_n_recv(pass, &range_stmt.x) else {
        return;
    };
    let Some(b_text) = expr_text_src(pass, b_recv) else {
        return;
    };

    let del_pos = range_stmt.range_.0 as u32;
    let del_end = range_stmt.x.end().0 as u32;
    let mut edits = bloop_timer_edits(pass, fn_body, del_pos);
    edits.push(TextEdit {
        pos: del_pos,
        end: del_end,
        new_text: format!("{b_text}.Loop()"),
    });

    pending.push(Diagnostic {
        pos: del_pos,
        end: del_end,
        category: String::new(),
        message: "b.N can be modernized using b.Loop()".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace b.N with b.Loop()".into(),
            text_edits: edits,
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

fn is_basic_kind(pass: &Pass<'_>, typ: TypeId, kind: BasicKind) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    matches!(artifacts.types.get(typ), TypeData::Basic(b) if b.kind() == kind)
}

fn as_type_conversion<'a>(pass: &Pass<'_>, expr: &'a Expr) -> Option<(TypeId, &'a Expr)> {
    let expr = match expr {
        Expr::ParenExpr(p) => p.x.as_ref(),
        other => other,
    };
    let Expr::CallExpr(call) = expr else {
        return None;
    };
    if call.args.len() != 1 {
        return None;
    }
    let info = pass.types_info()?;
    let tav = info.types.get(&call.fun.id())?;
    if tav.mode != OperandMode::TypeExpr {
        return None;
    }
    Some((tav.typ, &call.args[0]))
}

/// `unsafe.Pointer(uintptr(ptr) + offset)` → `unsafe.Add(ptr, offset)` (Go 1.17+).
fn check_unsafefuncs(
    pass: &Pass<'_>,
    file: &File,
    call: &CallExpr,
    pending: &mut Vec<Diagnostic>,
) {
    if call.args.len() != 1 {
        return;
    }
    let Some(info) = pass.types_info() else {
        return;
    };
    let Some(tav) = info.types.get(&call.fun.id()) else {
        return;
    };
    if tav.mode != OperandMode::TypeExpr {
        return;
    }
    if !is_basic_kind(pass, tav.typ, BasicKind::UnsafePointer) {
        return;
    }
    let pos = call.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.17") {
        return;
    }
    let Expr::BinaryExpr(sum) = &call.args[0] else {
        return;
    };
    if sum.op != Token::ADD {
        return;
    }
    let Some(x_ty) = type_of(pass, &sum.x) else {
        return;
    };
    if !is_basic_kind(pass, x_ty, BasicKind::Uintptr) {
        return;
    }
    let Some((_, ptr_expr)) = as_type_conversion(pass, &sum.x) else {
        return;
    };
    let Some(ptr_ty) = type_of(pass, ptr_expr) else {
        return;
    };
    if !is_basic_kind(pass, ptr_ty, BasicKind::UnsafePointer) {
        return;
    }
    let Some(ptr_text) = expr_text_src(pass, ptr_expr) else {
        return;
    };
    // Drop uintptr(...) around the offset when the conversion target is uintptr.
    let offset_expr = match as_type_conversion(pass, &sum.y) {
        Some((t, inner)) if is_basic_kind(pass, t, BasicKind::Uintptr) => {
            let artifacts = pass.pkg().type_artifacts.as_ref();
            let ok = artifacts.is_some_and(|a| {
                matches!(inner, Expr::BasicLit(_))
                    || type_of(pass, inner).is_some_and(|it| {
                        let under = unalias_readonly(&a.types, it).underlying(&a.types);
                        is_integer(&a.types, under)
                    })
            });
            if ok {
                inner
            } else {
                sum.y.as_ref()
            }
        }
        _ => sum.y.as_ref(),
    };
    let Some(offset_text) = expr_text_src(pass, offset_expr) else {
        return;
    };
    let pos = sum.x.pos().0 as u32;
    let end = sum.y.end().0 as u32;
    let Some((unsafedot, import_edits)) =
        // `sum.Pos()`: a BinaryExpr starts at its left operand.
        refactor::add_import(pass, file, "unsafe", "unsafe", "Add", pos)
    else {
        return;
    };
    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: "pointer + integer can be simplified using unsafe.Add".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Simplify pointer addition using unsafe.Add".into(),
            text_edits: with_imports(
                &import_edits,
                vec![TextEdit {
                    pos: call.pos().0 as u32,
                    end: call.end().0 as u32,
                    new_text: format!("{unsafedot}Add({ptr_text}, {offset_text})"),
                }],
            ),
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

fn is_import_comment(text: &str) -> bool {
    let text = text.trim();
    text.starts_with("import \"") && text.ends_with('"')
}

/// Obsolete `package p // import "path"` comments (ignored in module mode).
///
/// Package-line trailing comments are dropped unless `PARSE_COMMENTS` is set,
/// so we re-parse like gocritic's comment checkers.
fn check_importcomment(
    pass: &Pass<'_>,
    file_idx: usize,
    file: &File,
    pending: &mut Vec<Diagnostic>,
) {
    // DEFERRED: skip when Package.module is None (GOPATH mode), matching upstream.
    let Some(path) = pass.pkg().compiled_go_files.get(file_idx) else {
        return;
    };
    let Ok(src) = fs::read(path) else {
        return;
    };
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return;
    };
    let re_fset = FileSet::new();
    let Ok(parsed) = parse_file(&re_fset, name, &src, PARSE_COMMENTS) else {
        return;
    };
    let pkg_end = parsed.name.end();
    let pkg_line = re_fset.position(pkg_end).line;
    let Some(ft) = pass.fset().file(file.pos()) else {
        return;
    };
    for group in &parsed.comments {
        if group.list.len() != 1 {
            continue;
        }
        let c = &group.list[0];
        if c.pos().0 < pkg_end.0 {
            continue;
        }
        let comment_line = re_fset.position(c.pos()).line;
        if comment_line > pkg_line {
            break;
        }
        if comment_line != pkg_line {
            continue;
        }
        let text = CommentGroup {
            list: group.list.clone(),
        }
        .text();
        if !is_import_comment(&text) {
            continue;
        }
        if pkg_line < 1 || pkg_line as usize > ft.line_count() {
            continue;
        }
        let re_pos = re_fset.position(c.pos());
        let mapped_start =
            ft.line_start(pkg_line as usize).0 as u32 + (re_pos.column as u32).saturating_sub(1);
        let mapped_end = mapped_start + (c.end().0 - c.pos().0) as u32;
        pending.push(Diagnostic {
            pos: mapped_start,
            end: mapped_end,
            category: String::new(),
            message: "canonical import path comment is ignored in module mode".into(),
            suggested_fixes: vec![SuggestedFix {
                message: "Remove obsolete import path comment".into(),
                text_edits: vec![TextEdit {
                    pos: file.name.end().0 as u32,
                    end: mapped_end,
                    new_text: String::new(),
                }],
            }],
            related: Vec::new(),
            url: String::new(),
            severity: String::new(),
            ..Diagnostic::default()
        });
    }
}

/// `x := strings.Split(N)(s, sep[, 2])[0]` → `x, _, _ := strings.Cut(s, sep)`.
fn check_stringscut(pass: &Pass<'_>, assign: &AssignStmt, pending: &mut Vec<Diagnostic>) {
    if assign.tok != Some(Token::DEFINE) || assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return;
    }
    let Some(lhs_name) = ident_name(&assign.lhs[0]) else {
        return;
    };
    if lhs_name == "_" {
        return;
    }
    let Expr::IndexExpr(ix) = &assign.rhs[0] else {
        return;
    };
    if !code::is_integer_constant(pass, &ix.index, 0) {
        return;
    }
    let Expr::CallExpr(call) = ix.x.as_ref() else {
        return;
    };
    let (pkg, split_name, need_n) = if code::is_call_to(pass, call, "strings.Split") {
        ("strings", "Split", false)
    } else if code::is_call_to(pass, call, "strings.SplitN") {
        ("strings", "SplitN", true)
    } else if code::is_call_to(pass, call, "bytes.Split") {
        ("bytes", "Split", false)
    } else if code::is_call_to(pass, call, "bytes.SplitN") {
        ("bytes", "SplitN", true)
    } else {
        return;
    };
    if need_n {
        if call.args.len() != 3 || !code::is_integer_constant(pass, &call.args[2], 2) {
            return;
        }
    } else if call.args.len() != 2 {
        return;
    }
    // strings: require non-empty constant separator; bytes: allow any (often []byte lit).
    if pkg == "strings" {
        let Some(sep) = code::expr_to_string(pass, &call.args[1]) else {
            return;
        };
        if sep.is_empty() {
            return;
        }
    }
    let pos = call.fun.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.18") {
        return;
    }
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return; // e.g. dot-import
    };
    let mut text_edits = vec![
        TextEdit {
            pos: assign.lhs[0].end().0 as u32,
            end: assign.lhs[0].end().0 as u32,
            new_text: ", _, _".into(),
        },
        TextEdit {
            pos: sel.sel.pos().0 as u32,
            end: sel.sel.end().0 as u32,
            new_text: "Cut".into(),
        },
        TextEdit {
            pos: ix.lbrack.0 as u32,
            end: ix.rbrack.0 as u32 + 1,
            new_text: String::new(),
        },
    ];
    if need_n {
        text_edits.push(TextEdit {
            pos: call.args[1].end().0 as u32,
            end: call.rparen.0 as u32,
            new_text: String::new(),
        });
    }
    pending.push(Diagnostic {
        pos,
        end: call.fun.end().0 as u32,
        category: "stringscut".into(),
        message: format!("{pkg}.{split_name} call can be simplified using {pkg}.Cut"),
        suggested_fixes: vec![SuggestedFix {
            message: format!("Replace {pkg}.{split_name} with {pkg}.Cut"),
            text_edits,
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

/// Fact marking a function as "new-like": `func f(x T) *T { return &x }`.
#[derive(Clone, Debug, Default)]
struct NewLikeFact;

impl Fact for NewLikeFact {
    fn fact_type_id(&self) -> FactTypeId {
        FactTypeId::of::<Self>()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn clone_fact(&self) -> Box<dyn Fact> {
        Box::new(self.clone())
    }

    fn type_name(&self) -> &'static str {
        "NewLikeFact"
    }

    fn encode_payload(&self) -> serde_json::Value {
        serde_json::json!({})
    }
}

fn decode_new_like_fact(_payload: serde_json::Value) -> Option<Box<dyn Fact>> {
    Some(Box::new(NewLikeFact))
}

fn ensure_new_like_decoder() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        guff_analysis::register_fact_decoder("NewLikeFact", decode_new_like_fact);
    });
}

fn is_pointer_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    matches!(artifacts.types.get(typ), TypeData::Pointer(_))
}

fn type_name_of(pass: &Pass<'_>, typ: TypeId) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    Some(type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    ))
}

fn has_go_fix_inline(doc: &Option<CommentGroup>) -> bool {
    let Some(doc) = doc else {
        return false;
    };
    for c in &doc.list {
        let text = c.text.trim();
        // Match `//go:fix inline` (and block-comment equivalents).
        let stripped = text
            .trim_start_matches("//")
            .trim_start_matches("/*")
            .trim_end_matches("*/")
            .trim();
        if stripped.starts_with("go:fix") && stripped.contains("inline") {
            return true;
        }
    }
    false
}

fn newlike_unary<'a>(body: &'a BlockStmt) -> Option<&'a UnaryExpr> {
    if body.list.len() != 1 {
        return None;
    }
    let Stmt::ReturnStmt(ret) = &body.list[0] else {
        return None;
    };
    if ret.results.len() != 1 {
        return None;
    }
    let Expr::UnaryExpr(u) = &ret.results[0] else {
        return None;
    };
    if u.op != Token::AND {
        return None;
    }
    Some(u)
}

fn call_callee_object(pass: &Pass<'_>, fun: &Expr) -> Option<ObjectId> {
    match fun {
        Expr::Ident(id) => code::object_of(pass, id),
        Expr::SelectorExpr(sel) => code::object_of(pass, &sel.sel),
        Expr::IndexExpr(ix) => call_callee_object(pass, &ix.x),
        Expr::IndexListExpr(ix) => call_callee_object(pass, &ix.x),
        Expr::ParenExpr(p) => call_callee_object(pass, &p.x),
        _ => None,
    }
}

/// Conservative stand-in for upstream's `types.CheckExpr` on untyped constants:
/// BasicLit defaults must match the pointer element type name, otherwise skip
/// (avoids false positives like `int64Var(123)` where TypesInfo already shows
/// the converted type).
fn untyped_lit_matches_elem(pass: &Pass<'_>, arg: &Expr, elem: TypeId) -> bool {
    let Expr::BasicLit(lit) = arg else {
        // DEFERRED: re-typecheck complex constant expressions via CheckExpr.
        return false;
    };
    let Some(elem_name) = type_name_of(pass, elem) else {
        return false;
    };
    match lit.kind {
        Some(Token::INT) => elem_name == "int",
        Some(Token::STRING) => elem_name == "string",
        Some(Token::FLOAT) => elem_name == "float64",
        Some(Token::CHAR) => elem_name == "rune" || elem_name == "int32",
        _ => false,
    }
}

fn newexpr_arg_ok(pass: &Pass<'_>, arg: &Expr, call_typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let call_typ = unalias_readonly(&artifacts.types, call_typ);
    if !matches!(artifacts.types.get(call_typ), TypeData::Pointer(_)) {
        return false;
    }
    let elem = pointer_elem(&artifacts.types, call_typ);
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(tav) = info.types.get(&arg.id()) else {
        return false;
    };
    if tav.val.is_some() {
        return untyped_lit_matches_elem(pass, arg, elem);
    }
    types_identical(pass, tav.typ, elem)
}

struct NewExprDeclCand {
    obj: ObjectId,
    name: String,
    name_pos: u32,
    name_end: u32,
    decl_pos: u32,
    amp_pos: u32,
    x_end: u32,
    need_inline_comment: bool,
    go126: bool,
}

fn collect_newexpr_decls(pass: &Pass<'_>) -> Vec<NewExprDeclCand> {
    let mut out = Vec::new();
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return out;
    };
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            let Some(body) = fd.body.as_ref() else {
                continue;
            };
            let Some(unary) = newlike_unary(body) else {
                continue;
            };
            let Expr::Ident(x_id) = unary.x.as_ref() else {
                continue;
            };
            let Some(fn_obj) = code::object_of(pass, &fd.name) else {
                continue;
            };
            if !matches!(artifacts.objects.get(fn_obj), ObjectData::Func(_)) {
                continue;
            }
            let Some(sig) = fn_obj.typ(&artifacts.objects) else {
                continue;
            };
            if signature_variadic(&artifacts.types, sig) {
                continue;
            }
            // Methods are fine; Params excludes the receiver.
            let Some(params) = signature_params(&artifacts.types, sig) else {
                continue;
            };
            let Some(results) = signature_results(&artifacts.types, sig) else {
                continue;
            };
            if tuple_len(&artifacts.types, Some(params)) != 1 {
                continue;
            }
            if tuple_len(&artifacts.types, Some(results)) != 1 {
                continue;
            }
            let param = tuple_at(&artifacts.types, params, 0);
            let result = tuple_at(&artifacts.types, results, 0);
            let Some(result_typ) = result.typ(&artifacts.objects) else {
                continue;
            };
            if !is_pointer_type(pass, result_typ) {
                continue;
            }
            let Some(x_obj) = ident_obj(pass, x_id) else {
                continue;
            };
            if x_obj != param {
                continue;
            }
            let pos = fd.name.pos().0 as u32;
            out.push(NewExprDeclCand {
                obj: fn_obj,
                name: fd.name.name.clone(),
                name_pos: pos,
                name_end: fd.name.end().0 as u32,
                decl_pos: fd.ty.func.0 as u32,
                amp_pos: unary.op_pos.0 as u32,
                x_end: unary.x.end().0 as u32,
                need_inline_comment: !has_go_fix_inline(&fd.doc),
                go126: go_at_least(pass, pos, "go1.26"),
            });
        }
    }
    out
}

fn export_newexpr_decls(
    pass: &mut Pass<'_>,
    cands: Vec<NewExprDeclCand>,
    pending: &mut Vec<Diagnostic>,
) {
    for c in cands {
        pass.export_object_fact(c.obj, Box::new(NewLikeFact));
        if !c.go126 {
            continue;
        }
        let mut text_edits = vec![
            TextEdit {
                pos: c.amp_pos,
                end: c.amp_pos + 1,
                new_text: "new(".into(),
            },
            TextEdit {
                pos: c.x_end,
                end: c.x_end,
                new_text: ")".into(),
            },
        ];
        if c.need_inline_comment {
            text_edits.push(TextEdit {
                pos: c.decl_pos,
                end: c.decl_pos,
                new_text: "//go:fix inline\n".into(),
            });
        }
        pending.push(Diagnostic {
            pos: c.name_pos,
            end: c.name_end,
            category: "newexpr".into(),
            message: format!("{} can be an inlinable wrapper around new(expr)", c.name),
            suggested_fixes: vec![SuggestedFix {
                message: format!("Make {} an inlinable wrapper around new(expr)", c.name),
                text_edits,
            }],
            related: Vec::new(),
            url: String::new(),
            severity: String::new(),
            ..Diagnostic::default()
        });
    }
}

fn check_newexpr_call(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<Diagnostic>) {
    if call.args.len() != 1 {
        return;
    }
    let pos = call.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.26") {
        return;
    }
    let Some(fn_obj) = call_callee_object(pass, &call.fun) else {
        return;
    };
    let mut fact = NewLikeFact;
    if !pass.import_object_fact(fn_obj, &mut fact) {
        return;
    }
    let Some(info) = pass.types_info() else {
        return;
    };
    let Some(tav) = info.types.get(&call.id) else {
        return;
    };
    if !newexpr_arg_ok(pass, &call.args[0], tav.typ) {
        return;
    }
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let fname = fn_obj.name(&artifacts.objects).to_string();
    pending.push(Diagnostic {
        pos,
        end: call.end().0 as u32,
        category: "newexpr".into(),
        message: format!("call of {fname}(x) can be simplified to new(x)"),
        suggested_fixes: vec![SuggestedFix {
            message: format!("Simplify {fname}(x) to new(x)"),
            text_edits: vec![TextEdit {
                pos: call.fun.pos().0 as u32,
                end: call.fun.end().0 as u32,
                new_text: "new".into(),
            }],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

fn type_expr_text(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::SelectorExpr(sel) => {
            let x = type_expr_text(&sel.x)?;
            Some(format!("{x}.{}", sel.sel.name))
        }
        Expr::StarExpr(s) => {
            let x = type_expr_text(&s.x)?;
            Some(format!("*{x}"))
        }
        Expr::ParenExpr(p) => type_expr_text(&p.x),
        _ => None,
    }
}

fn universe_error(pass: &Pass<'_>) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    for oid in artifacts.objects.ids() {
        let ObjectData::TypeName(tn) = artifacts.objects.get(oid) else {
            continue;
        };
        if tn.name() != "error" {
            continue;
        }
        if oid.pkg(&artifacts.objects).is_some() {
            continue;
        }
        return tn.typ();
    }
    None
}

fn implements_error(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(err) = universe_error(pass) else {
        return false;
    };
    let mut types = artifacts.types.clone();
    api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        err,
    )
}

fn if_stmt_end(s: &IfStmt) -> Pos {
    s.else_
        .as_ref()
        .map(|e| e.end())
        .unwrap_or_else(|| s.body.end())
}

fn find_simple_var_decl(
    pass: &Pass<'_>,
    file: &File,
    obj: ObjectId,
) -> Option<(u32, u32, String, String)> {
    let mut found = None;
    walk::inspect(NodeRef::File(file), |n| {
        let Some(n) = n else {
            return true;
        };
        let NodeRef::DeclStmt(ds) = n else {
            return true;
        };
        let Decl::GenDecl(gd) = &ds.decl else {
            return true;
        };
        if gd.tok != Some(Token::VAR) || gd.specs.len() != 1 {
            return true;
        }
        let Spec::ValueSpec(vs) = &gd.specs[0] else {
            return true;
        };
        if vs.names.len() != 1 || !vs.values.is_empty() {
            return true;
        }
        let Some(ty) = vs.ty.as_ref() else {
            return true;
        };
        if ident_obj(pass, &vs.names[0]) != Some(obj) {
            return true;
        }
        let Some(type_text) = type_expr_text(ty) else {
            return true;
        };
        found = Some((
            ds.decl.pos().0 as u32,
            ds.decl.end().0 as u32,
            vs.names[0].name.clone(),
            type_text,
        ));
        true
    });
    found
}

fn has_use_outside_if(pass: &Pass<'_>, file: &File, obj: ObjectId, if_stmt: &IfStmt) -> bool {
    let start = if_stmt.if_.0 as u32;
    let end = if_stmt_end(if_stmt).0 as u32;
    let mut outside = false;
    walk::inspect(NodeRef::File(file), |n| {
        let Some(n) = n else {
            return true;
        };
        let NodeRef::Ident(id) = n else {
            return true;
        };
        let Some(info) = pass.types_info() else {
            return true;
        };
        if info.uses.get(&id.id).copied() != Some(obj) {
            return true;
        }
        let p = id.name_pos.0 as u32;
        if p < start || p >= end {
            outside = true;
        }
        true
    });
    outside
}

fn count_var_uses(pass: &Pass<'_>, file: &File, obj: ObjectId) -> usize {
    let mut n = 0;
    walk::inspect(NodeRef::File(file), |node| {
        let Some(node) = node else {
            return true;
        };
        if let NodeRef::Ident(id) = node {
            if pass
                .types_info()
                .and_then(|info| info.uses.get(&id.id).copied())
                == Some(obj)
            {
                n += 1;
            }
        }
        true
    });
    n
}

fn fresh_ok_name(pass: &Pass<'_>, if_stmt: &IfStmt, call_pos: u32) -> String {
    let preferred = "ok";
    let mut conflict = false;
    walk::inspect(NodeRef::IfStmt(if_stmt), |n| {
        let Some(n) = n else {
            return true;
        };
        let NodeRef::Ident(id) = n else {
            return true;
        };
        if id.name != preferred {
            return true;
        }
        let Some(info) = pass.types_info() else {
            return true;
        };
        if info.uses.get(&id.id).is_none() {
            return true;
        }
        if (id.name_pos.0 as u32) >= call_pos {
            conflict = true;
        }
        true
    });
    if !conflict {
        return preferred.to_string();
    }
    for i in 1..100 {
        let candidate = format!("ok{i}");
        let mut used = false;
        walk::inspect(NodeRef::IfStmt(if_stmt), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::Ident(id) = n {
                if id.name == candidate {
                    used = true;
                }
            }
            true
        });
        if !used {
            return candidate;
        }
    }
    "ok99".into()
}

/// Port of modernize `errorsastype`: `var e T; if errors.As(err, &e)` → AsType.
fn check_errorsastype(
    pass: &Pass<'_>,
    file: &File,
    if_stmt: &IfStmt,
    pending: &mut Vec<Diagnostic>,
) {
    if if_stmt.init.is_some() {
        return;
    }

    let mut negated = false;
    let call = match &if_stmt.cond {
        Expr::CallExpr(c) => c,
        Expr::UnaryExpr(u) if u.op == Token::NOT => {
            negated = true;
            match u.x.as_ref() {
                Expr::CallExpr(c) => c,
                _ => return,
            }
        }
        _ => return,
    };

    if !code::is_call_to(pass, call, "errors.As") || call.args.len() < 2 {
        return;
    }

    let pos = call.fun.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.26") {
        return;
    }

    let Expr::UnaryExpr(unary) = &call.args[1] else {
        return;
    };
    if unary.op != Token::AND {
        return;
    }
    let Expr::Ident(target_id) = unary.x.as_ref() else {
        return;
    };
    let Some(obj) = ident_obj(pass, target_id) else {
        return;
    };

    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let Some(typ) = obj.typ(&artifacts.objects) else {
        return;
    };
    if !implements_error(pass, typ) {
        return;
    }

    if has_use_outside_if(pass, file, obj, if_stmt) {
        return;
    }

    let Some((decl_pos, decl_end, var_name, type_text)) = find_simple_var_decl(pass, file, obj)
    else {
        return;
    };

    let as_ident = match call.fun.as_ref() {
        Expr::Ident(id) => id,
        Expr::SelectorExpr(sel) => &sel.sel,
        _ => return,
    };

    let uses_v = count_var_uses(pass, file, obj) > 1;
    let lhs_name = if uses_v { var_name.as_str() } else { "_" };
    let ok_name = fresh_ok_name(pass, if_stmt, call.pos().0 as u32);

    let mut text_edits = vec![
        TextEdit {
            pos: decl_pos,
            end: decl_end,
            new_text: String::new(),
        },
        TextEdit {
            pos: call.pos().0 as u32,
            end: call.pos().0 as u32,
            new_text: format!("{lhs_name}, {ok_name} := "),
        },
        TextEdit {
            pos: as_ident.name_pos.0 as u32,
            end: as_ident.end().0 as u32,
            new_text: format!("AsType[{type_text}]"),
        },
        TextEdit {
            pos: call.args[0].end().0 as u32,
            end: call.args[1].end().0 as u32,
            new_text: String::new(),
        },
        TextEdit {
            pos: call.end().0 as u32,
            end: call.end().0 as u32,
            new_text: format!("; {}{ok_name}", if negated { "!" } else { "" }),
        },
    ];
    if negated {
        if let Expr::UnaryExpr(u) = &if_stmt.cond {
            text_edits.push(TextEdit {
                pos: u.op_pos.0 as u32,
                end: u.x.pos().0 as u32,
                new_text: String::new(),
            });
        }
    }

    let end = call.fun.end().0 as u32;
    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: format!("errors.As can be simplified using AsType[{type_text}]"),
        suggested_fixes: vec![SuggestedFix {
            message: format!("Replace errors.As with AsType[{type_text}]"),
            text_edits,
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

/// Port of modernize `stringsbuilder`: replace `s += x` in a loop with `strings.Builder`.
///
/// SuggestedFix uses a bare `strings.Builder` name (AddImport is DEFERRED).
fn is_builtin_string_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Basic(b) => b.kind() == BasicKind::String,
        _ => false,
    }
}

fn is_local_string_var(pass: &Pass<'_>, obj: ObjectId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let ObjectData::Var(v) = artifacts.objects.get(obj) else {
        return false;
    };
    if v.kind() != VarKind::Local {
        return false;
    }
    is_builtin_string_type(pass, v.typ())
}

fn unparen_expr(expr: &Expr) -> &Expr {
    match expr {
        Expr::ParenExpr(p) => unparen_expr(&p.x),
        other => other,
    }
}

fn expr_is_obj(pass: &Pass<'_>, expr: &Expr, obj: ObjectId) -> bool {
    match unparen_expr(expr) {
        Expr::Ident(id) => ident_obj(pass, id) == Some(obj),
        _ => false,
    }
}

fn is_empty_string_expr(pass: &Pass<'_>, expr: &Expr) -> bool {
    code::expr_to_string(pass, expr).is_some_and(|s| s.is_empty())
}

fn stmt_list_contains_assign<'a>(list: &'a [Stmt], assign: &AssignStmt) -> bool {
    let want = assign.tok_pos.0;
    list.iter().any(|s| match s {
        Stmt::AssignStmt(a) => a.tok_pos.0 == want,
        _ => false,
    })
}

fn short_decl_in_unrestricted_list(file: &File, assign: &AssignStmt) -> bool {
    let mut ok = false;
    walk::inspect(NodeRef::File(file), |n| {
        let Some(n) = n else {
            return true;
        };
        match n {
            NodeRef::BlockStmt(b) if stmt_list_contains_assign(&b.list, assign) => {
                ok = true;
            }
            NodeRef::CaseClause(c) if stmt_list_contains_assign(&c.body, assign) => {
                ok = true;
            }
            NodeRef::CommClause(c) if stmt_list_contains_assign(&c.body, assign) => {
                ok = true;
            }
            _ => {}
        }
        true
    });
    ok
}

enum StringsBuilderDecl<'a> {
    Short {
        assign: &'a AssignStmt,
        empty_init: bool,
    },
    Var {
        decl: &'a GenDecl,
        spec: &'a ValueSpec,
    },
}

fn find_stringsbuilder_decl<'a>(
    pass: &Pass<'_>,
    file: &'a File,
    obj: ObjectId,
) -> Option<StringsBuilderDecl<'a>> {
    let mut found = None;
    walk::inspect(NodeRef::File(file), |n| {
        let Some(n) = n else {
            return true;
        };
        match n {
            NodeRef::AssignStmt(assign)
                if assign.tok == Some(Token::DEFINE)
                    && assign.lhs.len() == 1
                    && assign.rhs.len() == 1
                    && expr_is_obj(pass, &assign.lhs[0], obj) =>
            {
                if short_decl_in_unrestricted_list(file, assign) {
                    found = Some(StringsBuilderDecl::Short {
                        assign,
                        empty_init: is_empty_string_expr(pass, &assign.rhs[0]),
                    });
                }
            }
            NodeRef::DeclStmt(ds) => {
                let Decl::GenDecl(gd) = &ds.decl else {
                    return true;
                };
                if gd.tok != Some(Token::VAR) {
                    return true;
                }
                // Spec containing obj must be the last child (paren form constraint).
                let Some(Spec::ValueSpec(spec)) = gd.specs.last() else {
                    return true;
                };
                if spec.names.len() != 1 || ident_obj(pass, &spec.names[0]) != Some(obj) {
                    return true;
                }
                // Reject multi-name specs earlier in the decl; only last may be ours.
                if gd.specs.len() > 1 {
                    for earlier in &gd.specs[..gd.specs.len() - 1] {
                        if let Spec::ValueSpec(vs) = earlier {
                            if vs.names.iter().any(|n| ident_obj(pass, n) == Some(obj)) {
                                return true;
                            }
                        }
                    }
                }
                found = Some(StringsBuilderDecl::Var { decl: gd, spec });
            }
            _ => {}
        }
        true
    });
    found
}

struct StringsBuilderUses {
    num_loop_assigns: usize,
    first_loop_assign_pos: u32,
    first_loop_assign_end: u32,
    seen_rvalue: bool,
    reject: bool,
    edits: Vec<TextEdit>,
    post_edits: Vec<TextEdit>,
}

fn record_rvalue_use(expr: &Expr, uses: &mut StringsBuilderUses) {
    uses.seen_rvalue = true;
    let end = unparen_expr(expr).end().0 as u32;
    uses.edits.push(TextEdit {
        pos: end,
        end,
        new_text: ".String()".into(),
    });
}

fn walk_expr_for_var_uses(
    pass: &Pass<'_>,
    expr: &Expr,
    obj: ObjectId,
    var_pos: u32,
    in_loop: bool,
    uses: &mut StringsBuilderUses,
) {
    match expr {
        Expr::ParenExpr(p) => walk_expr_for_var_uses(pass, &p.x, obj, var_pos, in_loop, uses),
        Expr::UnaryExpr(u) if u.op == Token::AND => {
            if expr_is_obj(pass, &u.x, obj) {
                uses.reject = true;
                return;
            }
            walk_expr_for_var_uses(pass, &u.x, obj, var_pos, in_loop, uses);
        }
        Expr::Ident(id) => {
            if ident_obj(pass, id) == Some(obj) {
                record_rvalue_use(expr, uses);
            }
        }
        Expr::CallExpr(c) => {
            walk_expr_for_var_uses(pass, &c.fun, obj, var_pos, in_loop, uses);
            for a in &c.args {
                walk_expr_for_var_uses(pass, a, obj, var_pos, in_loop, uses);
            }
        }
        Expr::SelectorExpr(s) => walk_expr_for_var_uses(pass, &s.x, obj, var_pos, in_loop, uses),
        Expr::IndexExpr(i) => {
            walk_expr_for_var_uses(pass, &i.x, obj, var_pos, in_loop, uses);
            walk_expr_for_var_uses(pass, &i.index, obj, var_pos, in_loop, uses);
        }
        Expr::SliceExpr(s) => {
            walk_expr_for_var_uses(pass, &s.x, obj, var_pos, in_loop, uses);
            if let Some(e) = &s.low {
                walk_expr_for_var_uses(pass, e, obj, var_pos, in_loop, uses);
            }
            if let Some(e) = &s.high {
                walk_expr_for_var_uses(pass, e, obj, var_pos, in_loop, uses);
            }
            if let Some(e) = &s.max {
                walk_expr_for_var_uses(pass, e, obj, var_pos, in_loop, uses);
            }
        }
        Expr::BinaryExpr(b) => {
            walk_expr_for_var_uses(pass, &b.x, obj, var_pos, in_loop, uses);
            walk_expr_for_var_uses(pass, &b.y, obj, var_pos, in_loop, uses);
        }
        Expr::StarExpr(s) => walk_expr_for_var_uses(pass, &s.x, obj, var_pos, in_loop, uses),
        Expr::UnaryExpr(u) => walk_expr_for_var_uses(pass, &u.x, obj, var_pos, in_loop, uses),
        Expr::KeyValueExpr(kv) => {
            walk_expr_for_var_uses(pass, &kv.key, obj, var_pos, in_loop, uses);
            walk_expr_for_var_uses(pass, &kv.value, obj, var_pos, in_loop, uses);
        }
        Expr::CompositeLit(c) => {
            if let Some(t) = &c.ty {
                walk_expr_for_var_uses(pass, t, obj, var_pos, in_loop, uses);
            }
            for e in &c.elts {
                walk_expr_for_var_uses(pass, e, obj, var_pos, in_loop, uses);
            }
        }
        Expr::FuncLit(f) => {
            for s in &f.body.list {
                walk_stmt_for_var_uses(pass, s, obj, var_pos, in_loop, uses);
            }
        }
        Expr::TypeAssertExpr(t) => {
            walk_expr_for_var_uses(pass, &t.x, obj, var_pos, in_loop, uses);
            if let Some(ty) = &t.ty {
                walk_expr_for_var_uses(pass, ty, obj, var_pos, in_loop, uses);
            }
        }
        Expr::IndexListExpr(i) => {
            walk_expr_for_var_uses(pass, &i.x, obj, var_pos, in_loop, uses);
            for idx in &i.indices {
                walk_expr_for_var_uses(pass, idx, obj, var_pos, in_loop, uses);
            }
        }
        _ => {}
    }
}

fn walk_stmt_for_var_uses(
    pass: &Pass<'_>,
    stmt: &Stmt,
    obj: ObjectId,
    var_pos: u32,
    in_loop: bool,
    uses: &mut StringsBuilderUses,
) {
    if uses.reject {
        return;
    }
    match stmt {
        Stmt::AssignStmt(assign) => {
            let lhs_is_ours = assign.lhs.len() == 1 && expr_is_obj(pass, &assign.lhs[0], obj);
            if lhs_is_ours {
                if assign.tok == Some(Token::DEFINE) {
                    // Declaration site of `obj` — not a later assignment.
                    return;
                }
                if assign.tok == Some(Token::AddAssign) {
                    if uses.seen_rvalue {
                        uses.reject = true;
                        return;
                    }
                    if in_loop {
                        uses.num_loop_assigns += 1;
                        if uses.first_loop_assign_pos == 0 {
                            uses.first_loop_assign_pos = assign.lhs[0].pos().0 as u32;
                            uses.first_loop_assign_end = assign
                                .rhs
                                .last()
                                .map(|e| e.end().0 as u32)
                                .unwrap_or(assign.tok_pos.0 as u32);
                        }
                    }
                    // s +=          expr  →  s.WriteString(expr)
                    uses.edits.push(TextEdit {
                        pos: assign.lhs[0].end().0 as u32,
                        end: assign.rhs[0].pos().0 as u32,
                        new_text: ".WriteString(".into(),
                    });
                    let assign_end = assign
                        .rhs
                        .last()
                        .map(|e| e.end().0 as u32)
                        .unwrap_or(assign.tok_pos.0 as u32);
                    uses.post_edits.push(TextEdit {
                        pos: assign_end,
                        end: assign_end,
                        new_text: ")".into(),
                    });
                    if assign.rhs.len() == 1 {
                        walk_expr_for_var_uses(pass, &assign.rhs[0], obj, var_pos, in_loop, uses);
                    }
                } else {
                    // Direct assignment of s after decl — reject.
                    uses.reject = true;
                }
                return;
            }
            for e in assign.lhs.iter().chain(assign.rhs.iter()) {
                walk_expr_for_var_uses(pass, e, obj, var_pos, in_loop, uses);
            }
        }
        Stmt::DeclStmt(ds) => {
            // Skip the defining ValueSpec for `obj`; still walk other inits.
            if let Decl::GenDecl(gd) = &ds.decl {
                for spec in &gd.specs {
                    if let Spec::ValueSpec(vs) = spec {
                        let defines_ours = vs.names.iter().any(|n| ident_obj(pass, n) == Some(obj));
                        if defines_ours {
                            continue;
                        }
                        for v in &vs.values {
                            walk_expr_for_var_uses(pass, v, obj, var_pos, in_loop, uses);
                        }
                    }
                }
            }
        }
        Stmt::ExprStmt(e) => walk_expr_for_var_uses(pass, &e.x, obj, var_pos, in_loop, uses),
        Stmt::ReturnStmt(r) => {
            for e in &r.results {
                walk_expr_for_var_uses(pass, e, obj, var_pos, in_loop, uses);
            }
        }
        Stmt::GoStmt(g) => {
            walk_expr_for_var_uses(pass, &g.call.fun, obj, var_pos, in_loop, uses);
            for a in &g.call.args {
                walk_expr_for_var_uses(pass, a, obj, var_pos, in_loop, uses);
            }
        }
        Stmt::DeferStmt(d) => {
            walk_expr_for_var_uses(pass, &d.call.fun, obj, var_pos, in_loop, uses);
            for a in &d.call.args {
                walk_expr_for_var_uses(pass, a, obj, var_pos, in_loop, uses);
            }
        }
        Stmt::SendStmt(s) => {
            walk_expr_for_var_uses(pass, &s.chan_, obj, var_pos, in_loop, uses);
            walk_expr_for_var_uses(pass, &s.value, obj, var_pos, in_loop, uses);
        }
        Stmt::IncDecStmt(i) => walk_expr_for_var_uses(pass, &i.x, obj, var_pos, in_loop, uses),
        Stmt::IfStmt(i) => {
            if let Some(init) = &i.init {
                walk_stmt_for_var_uses(pass, init, obj, var_pos, in_loop, uses);
            }
            walk_expr_for_var_uses(pass, &i.cond, obj, var_pos, in_loop, uses);
            for s in &i.body.list {
                walk_stmt_for_var_uses(pass, s, obj, var_pos, in_loop, uses);
            }
            if let Some(e) = &i.else_ {
                walk_stmt_for_var_uses(pass, e, obj, var_pos, in_loop, uses);
            }
        }
        Stmt::BlockStmt(b) => {
            for s in &b.list {
                walk_stmt_for_var_uses(pass, s, obj, var_pos, in_loop, uses);
            }
        }
        Stmt::ForStmt(f) => {
            let loop_now = in_loop || (f.for_.0 as u32) >= var_pos;
            if let Some(init) = &f.init {
                walk_stmt_for_var_uses(pass, init, obj, var_pos, loop_now, uses);
            }
            if let Some(cond) = &f.cond {
                walk_expr_for_var_uses(pass, cond, obj, var_pos, loop_now, uses);
            }
            if let Some(post) = &f.post {
                walk_stmt_for_var_uses(pass, post, obj, var_pos, loop_now, uses);
            }
            for s in &f.body.list {
                walk_stmt_for_var_uses(pass, s, obj, var_pos, loop_now, uses);
            }
        }
        Stmt::RangeStmt(r) => {
            let loop_now = in_loop || (r.for_.0 as u32) >= var_pos;
            walk_expr_for_var_uses(pass, &r.x, obj, var_pos, loop_now, uses);
            for s in &r.body.list {
                walk_stmt_for_var_uses(pass, s, obj, var_pos, loop_now, uses);
            }
        }
        Stmt::SwitchStmt(s) => {
            if let Some(init) = &s.init {
                walk_stmt_for_var_uses(pass, init, obj, var_pos, in_loop, uses);
            }
            if let Some(tag) = &s.tag {
                walk_expr_for_var_uses(pass, tag, obj, var_pos, in_loop, uses);
            }
            for c in &s.body.list {
                walk_stmt_for_var_uses(pass, c, obj, var_pos, in_loop, uses);
            }
        }
        Stmt::TypeSwitchStmt(s) => {
            if let Some(init) = &s.init {
                walk_stmt_for_var_uses(pass, init, obj, var_pos, in_loop, uses);
            }
            walk_stmt_for_var_uses(pass, &s.assign, obj, var_pos, in_loop, uses);
            for c in &s.body.list {
                walk_stmt_for_var_uses(pass, c, obj, var_pos, in_loop, uses);
            }
        }
        Stmt::CaseClause(c) => {
            for e in &c.list {
                walk_expr_for_var_uses(pass, e, obj, var_pos, in_loop, uses);
            }
            for s in &c.body {
                walk_stmt_for_var_uses(pass, s, obj, var_pos, in_loop, uses);
            }
        }
        Stmt::SelectStmt(s) => {
            for c in &s.body.list {
                walk_stmt_for_var_uses(pass, c, obj, var_pos, in_loop, uses);
            }
        }
        Stmt::CommClause(c) => {
            if let Some(comm) = &c.comm {
                walk_stmt_for_var_uses(pass, comm, obj, var_pos, in_loop, uses);
            }
            for s in &c.body {
                walk_stmt_for_var_uses(pass, s, obj, var_pos, in_loop, uses);
            }
        }
        Stmt::LabeledStmt(l) => {
            walk_stmt_for_var_uses(pass, &l.stmt, obj, var_pos, in_loop, uses);
        }
        _ => {}
    }
}

/// The declaration edits, with the `strings` import edits ahead of them.
///
/// Upstream computes the prefix at the same two points it appends the import
/// edits, so an aliased or shadowed `strings` reaches both the type name and
/// the import spec. Returns `None` when the import cannot be resolved — the
/// replacement names a package, so a fix without it would not compile.
fn build_stringsbuilder_decl_edits(
    pass: &Pass<'_>,
    file: &File,
    decl: &StringsBuilderDecl<'_>,
    var_name: &str,
    var_pos: u32,
) -> Option<Vec<TextEdit>> {
    let (prefix, import_edits) =
        refactor::add_import(pass, file, "strings", "strings", "Builder", var_pos)?;
    let prefix = prefix.as_str();
    let out = match decl {
        StringsBuilderDecl::Short { assign, empty_init } => {
            let assign_pos = assign.lhs[0].pos().0 as u32;
            let assign_end = assign
                .rhs
                .last()
                .map(|e| e.end().0 as u32)
                .unwrap_or(assign.tok_pos.0 as u32);
            if *empty_init {
                vec![TextEdit {
                    pos: assign_pos,
                    end: assign_end,
                    new_text: format!("var {var_name} {prefix}Builder"),
                }]
            } else {
                vec![
                    TextEdit {
                        pos: assign_pos,
                        end: assign.rhs[0].pos().0 as u32,
                        new_text: format!(
                            "var {var_name} {prefix}Builder; {var_name}.WriteString("
                        ),
                    },
                    TextEdit {
                        pos: assign_end,
                        end: assign_end,
                        new_text: ")".into(),
                    },
                ]
            }
        }
        StringsBuilderDecl::Var { decl: gd, spec } => {
            let mut edits = Vec::new();
            let init = if let Some(ty) = &spec.ty {
                ty.end().0 as u32
            } else {
                spec.names[0].end().0 as u32
            };
            edits.push(TextEdit {
                pos: spec.names[0].end().0 as u32,
                end: init,
                new_text: format!(" {prefix}Builder"),
            });
            if !spec.values.is_empty() && !is_empty_string_expr(pass, &spec.values[0]) {
                let gd_end = if gd.rparen.is_valid() {
                    (gd.rparen.0 + 1) as u32
                } else {
                    spec.values[0].end().0 as u32
                };
                if gd.rparen.is_valid() {
                    edits.push(TextEdit {
                        pos: init,
                        end: init,
                        new_text: ")".into(),
                    });
                    edits.push(TextEdit {
                        pos: spec.values[0].end().0 as u32,
                        end: gd_end,
                        new_text: String::new(),
                    });
                }
                edits.push(TextEdit {
                    pos: init,
                    end: spec.values[0].pos().0 as u32,
                    new_text: format!("; {var_name}.WriteString("),
                });
                edits.push(TextEdit {
                    pos: spec.values[0].end().0 as u32,
                    end: spec.values[0].end().0 as u32,
                    new_text: ")".into(),
                });
            } else if !spec.values.is_empty() {
                // delete "= expr" (empty string)
                edits.push(TextEdit {
                    pos: init,
                    end: spec.values[0].end().0 as u32,
                    new_text: String::new(),
                });
            }
            edits
        }
    };
    Some(with_imports(&import_edits, out))
}

fn check_stringsbuilder(pass: &Pass<'_>, file: &File, pending: &mut Vec<Diagnostic>) {
    let pkg = pass.pkg().pkg_path.as_str();
    if pkg == "strings"
        || pkg.starts_with("strings/")
        || pkg == "runtime"
        || pkg.starts_with("runtime/")
    {
        return;
    }
    let filename = pass.fset().position(file.pos()).filename;
    if filename.ends_with("_test.go") {
        return;
    }

    // Candidates: local string vars on the LHS of some `+=`.
    let mut candidates: HashSet<ObjectId> = HashSet::new();
    walk::inspect(NodeRef::File(file), |n| {
        let Some(n) = n else {
            return true;
        };
        let NodeRef::AssignStmt(assign) = n else {
            return true;
        };
        if assign.tok != Some(Token::AddAssign) || assign.lhs.len() != 1 {
            return true;
        }
        let Expr::Ident(id) = unparen_expr(&assign.lhs[0]) else {
            return true;
        };
        let Some(obj) = ident_obj(pass, id) else {
            return true;
        };
        if is_local_string_var(pass, obj) {
            candidates.insert(obj);
        }
        true
    });

    let mut ordered: Vec<ObjectId> = candidates.into_iter().collect();
    if let Some(artifacts) = pass.pkg().type_artifacts.as_ref() {
        ordered.sort_by_key(|o| o.pos(&artifacts.objects));
    }

    let mut last_edit_end: Option<u32> = None;
    for obj in ordered {
        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
            continue;
        };
        let var_pos = obj.pos(&artifacts.objects);
        let ObjectData::Var(v) = artifacts.objects.get(obj) else {
            continue;
        };
        let var_name = v.name().to_string();

        if let Some(end) = last_edit_end {
            if var_pos < end {
                continue; // overlapping fix span
            }
        }

        let Some(decl) = find_stringsbuilder_decl(pass, file, obj) else {
            continue;
        };

        let mut uses = StringsBuilderUses {
            num_loop_assigns: 0,
            first_loop_assign_pos: 0,
            first_loop_assign_end: 0,
            seen_rvalue: false,
            reject: false,
            edits: Vec::new(),
            post_edits: Vec::new(),
        };

        // Walk the enclosing function body (or file decls) for uses.
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::FuncDecl(fd) => {
                    if let Some(body) = &fd.body {
                        for s in &body.list {
                            walk_stmt_for_var_uses(pass, s, obj, var_pos, false, &mut uses);
                        }
                    }
                    false // don't descend via default; we handled body
                }
                NodeRef::FuncLit(_) => true, // handled via stmt walk when nested
                _ => true,
            }
        });

        if uses.reject || !uses.seen_rvalue || uses.num_loop_assigns == 0 {
            continue;
        }

        let Some(mut edits) =
            build_stringsbuilder_decl_edits(pass, file, &decl, &var_name, var_pos)
        else {
            continue;
        };
        edits.append(&mut uses.edits);
        edits.append(&mut uses.post_edits);

        last_edit_end = edits.iter().map(|e| e.end).max();

        pending.push(Diagnostic {
            pos: uses.first_loop_assign_pos,
            end: uses.first_loop_assign_end,
            category: String::new(),
            message: "using string += string in a loop is inefficient".into(),
            suggested_fixes: vec![SuggestedFix {
                message: "Replace string += string with strings.Builder".into(),
                text_edits: edits,
            }],
            related: Vec::new(),
            url: String::new(),
            severity: String::new(),
            ..Diagnostic::default()
        });
    }
}

/// A std type with legacy `T.{Len,At}` iteration methods plus a newer `T.All`
/// iterator method. Port of upstream `stditeratorsTable`.
struct StdIterRow {
    pkgpath: &'static str,
    typename: &'static str,
    lenmethod: &'static str,
    atmethod: &'static str,
    itermethod: &'static str,
    elemname: &'static str,
    /// 1 => `for x`, 2 => `for _, x`.
    seqn: u8,
    /// Go version at which `itermethod` appeared in the stdlib.
    since: &'static str,
}

const STDITERATORS_TABLE: &[StdIterRow] = &[
    StdIterRow {
        pkgpath: "go/types",
        typename: "Interface",
        lenmethod: "NumEmbeddeds",
        atmethod: "EmbeddedType",
        itermethod: "EmbeddedTypes",
        elemname: "etyp",
        seqn: 1,
        since: "go1.24",
    },
    StdIterRow {
        pkgpath: "go/types",
        typename: "Interface",
        lenmethod: "NumExplicitMethods",
        atmethod: "ExplicitMethod",
        itermethod: "ExplicitMethods",
        elemname: "method",
        seqn: 1,
        since: "go1.24",
    },
    StdIterRow {
        pkgpath: "go/types",
        typename: "Interface",
        lenmethod: "NumMethods",
        atmethod: "Method",
        itermethod: "Methods",
        elemname: "method",
        seqn: 1,
        since: "go1.24",
    },
    StdIterRow {
        pkgpath: "go/types",
        typename: "MethodSet",
        lenmethod: "Len",
        atmethod: "At",
        itermethod: "Methods",
        elemname: "method",
        seqn: 1,
        since: "go1.24",
    },
    StdIterRow {
        pkgpath: "go/types",
        typename: "Named",
        lenmethod: "NumMethods",
        atmethod: "Method",
        itermethod: "Methods",
        elemname: "method",
        seqn: 1,
        since: "go1.24",
    },
    StdIterRow {
        pkgpath: "go/types",
        typename: "Scope",
        lenmethod: "NumChildren",
        atmethod: "Child",
        itermethod: "Children",
        elemname: "child",
        seqn: 1,
        since: "go1.24",
    },
    StdIterRow {
        pkgpath: "go/types",
        typename: "Struct",
        lenmethod: "NumFields",
        atmethod: "Field",
        itermethod: "Fields",
        elemname: "field",
        seqn: 1,
        since: "go1.24",
    },
    StdIterRow {
        pkgpath: "go/types",
        typename: "Tuple",
        lenmethod: "Len",
        atmethod: "At",
        itermethod: "Variables",
        elemname: "v",
        seqn: 1,
        since: "go1.24",
    },
    StdIterRow {
        pkgpath: "go/types",
        typename: "TypeList",
        lenmethod: "Len",
        atmethod: "At",
        itermethod: "Types",
        elemname: "t",
        seqn: 1,
        since: "go1.24",
    },
    StdIterRow {
        pkgpath: "go/types",
        typename: "TypeParamList",
        lenmethod: "Len",
        atmethod: "At",
        itermethod: "TypeParams",
        elemname: "tparam",
        seqn: 1,
        since: "go1.24",
    },
    StdIterRow {
        pkgpath: "go/types",
        typename: "Union",
        lenmethod: "Len",
        atmethod: "Term",
        itermethod: "Terms",
        elemname: "term",
        seqn: 1,
        since: "go1.24",
    },
    StdIterRow {
        pkgpath: "reflect",
        typename: "Type",
        lenmethod: "NumField",
        atmethod: "Field",
        itermethod: "Fields",
        elemname: "field",
        seqn: 1,
        since: "go1.26",
    },
    StdIterRow {
        pkgpath: "reflect",
        typename: "Type",
        lenmethod: "NumMethod",
        atmethod: "Method",
        itermethod: "Methods",
        elemname: "method",
        seqn: 1,
        since: "go1.26",
    },
    StdIterRow {
        pkgpath: "reflect",
        typename: "Type",
        lenmethod: "NumIn",
        atmethod: "In",
        itermethod: "Ins",
        elemname: "in",
        seqn: 1,
        since: "go1.26",
    },
    StdIterRow {
        pkgpath: "reflect",
        typename: "Type",
        lenmethod: "NumOut",
        atmethod: "Out",
        itermethod: "Outs",
        elemname: "out",
        seqn: 1,
        since: "go1.26",
    },
    StdIterRow {
        pkgpath: "reflect",
        typename: "Value",
        lenmethod: "NumField",
        atmethod: "Field",
        itermethod: "Fields",
        elemname: "field",
        seqn: 2,
        since: "go1.26",
    },
    StdIterRow {
        pkgpath: "reflect",
        typename: "Value",
        lenmethod: "NumMethod",
        atmethod: "Method",
        itermethod: "Methods",
        elemname: "method",
        seqn: 2,
        since: "go1.26",
    },
];

/// Resolves the package path and type name of a (possibly pointer-to) named type.
fn named_type_pkg_and_name(pass: &Pass<'_>, typ: TypeId) -> Option<(String, String)> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = unalias_readonly(&artifacts.types, typ);
    let typ = if matches!(artifacts.types.get(typ), TypeData::Pointer(_)) {
        unalias_readonly(&artifacts.types, pointer_elem(&artifacts.types, typ))
    } else {
        typ
    };
    let TypeData::Named(_) = artifacts.types.get(typ) else {
        return None;
    };
    let obj = named_obj(&artifacts.types, typ);
    let name = obj.name(&artifacts.objects).to_string();
    let pkg_id = obj.pkg(&artifacts.objects)?;
    let path = artifacts.packages.get(pkg_id).path().to_string();
    Some((path, name))
}

/// Finds the table row whose type matches `recv` and whose `lenmethod` matches
/// the selector name of the `x.Len()` call.
fn stditerators_row_for(
    pass: &Pass<'_>,
    recv: &Expr,
    method_name: &str,
) -> Option<&'static StdIterRow> {
    let typ = type_of(pass, recv)?;
    let (path, name) = named_type_pkg_and_name(pass, typ)?;
    STDITERATORS_TABLE
        .iter()
        .find(|r| r.pkgpath == path && r.typename == name && r.lenmethod == method_name)
}

fn obj_name_is(pass: &Pass<'_>, obj: ObjectId, name: &str) -> bool {
    pass.pkg()
        .type_artifacts
        .as_ref()
        .is_some_and(|a| obj.name(&a.objects) == name)
}

/// Verifies that every use of `loop_var` inside `body` is the sole argument of
/// an `recv.At(loop_var)` call, and returns the edits replacing each such call
/// with `elem`. Returns `None` when a use escapes that pattern, or when `elem`
/// would collide with an existing name referenced in the body (upstream renames;
/// we conservatively decline — DEFERRED).
fn stditerators_at_edits(
    pass: &Pass<'_>,
    body: &BlockStmt,
    recv: &Expr,
    loop_var: ObjectId,
    atmethod: &str,
    elem: &str,
) -> Option<Vec<TextEdit>> {
    let mut at_edits: Vec<TextEdit> = Vec::new();
    let mut allowed: HashSet<u32> = HashSet::new();
    let mut uses: Vec<u32> = Vec::new();
    let mut collision = false;

    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        match n {
            NodeRef::CallExpr(call) => {
                if let Expr::SelectorExpr(sel) = unparen_expr(&call.fun) {
                    if sel.sel.name == atmethod && call.args.len() == 1 && exprs_equal(&sel.x, recv)
                    {
                        if let Expr::Ident(arg) = &call.args[0] {
                            if ident_obj(pass, arg) == Some(loop_var) {
                                allowed.insert(arg.id);
                                at_edits.push(TextEdit {
                                    pos: call.fun.pos().0 as u32,
                                    end: call.rparen.0 as u32 + 1,
                                    new_text: elem.to_string(),
                                });
                            }
                        }
                    }
                }
            }
            NodeRef::Ident(id) => {
                if let Some(obj) = ident_obj(pass, id) {
                    if obj == loop_var {
                        uses.push(id.id);
                    } else if obj_name_is(pass, obj, elem) {
                        collision = true;
                    }
                }
            }
            _ => {}
        }
        true
    });

    if collision {
        return None;
    }
    if uses.iter().any(|u| !allowed.contains(u)) {
        return None;
    }
    Some(at_edits)
}

fn stditerators_message(row: &StdIterRow) -> String {
    // Upstream message text (note: "can simplified", preserved for parity).
    format!(
        "{}/{} loop can simplified using {}.{} iteration",
        row.lenmethod, row.atmethod, row.typename, row.itermethod
    )
}

fn stditerators_fix_message(row: &StdIterRow) -> String {
    format!(
        "Replace {}/{} loop with {}.{} iteration",
        row.lenmethod, row.atmethod, row.typename, row.itermethod
    )
}

/// Pattern 1: `for i := 0; i < x.Len(); i++ { use(x.At(i)) }`.
fn check_stditerators_for(pass: &Pass<'_>, for_stmt: &ForStmt, pending: &mut Vec<Diagnostic>) {
    let Some(Stmt::AssignStmt(init)) = for_stmt.init.as_deref() else {
        return;
    };
    if init.tok != Some(Token::DEFINE) || init.lhs.len() != 1 || init.rhs.len() != 1 {
        return;
    }
    let Some(index_name) = ident_name(&init.lhs[0]) else {
        return;
    };
    if !code::is_integer_constant(pass, &init.rhs[0], 0) {
        return;
    }
    let Some(Expr::BinaryExpr(cmp)) = for_stmt.cond.as_ref() else {
        return;
    };
    if cmp.op != Token::LSS || ident_name(&cmp.x) != Some(index_name) {
        return;
    }
    let Expr::Ident(cmp_x) = cmp.x.as_ref() else {
        return;
    };
    let Expr::CallExpr(len_call) = cmp.y.as_ref() else {
        return;
    };
    if !len_call.args.is_empty() {
        return;
    }
    let Expr::SelectorExpr(len_sel) = unparen_expr(&len_call.fun) else {
        return;
    };
    let Some(post) = for_stmt.post.as_deref() else {
        return;
    };
    if !is_simple_inc(post, index_name) {
        return;
    }

    let recv = &len_sel.x;
    let Some(row) = stditerators_row_for(pass, recv, &len_sel.sel.name) else {
        return;
    };
    if pass.pkg().pkg_path == row.pkgpath {
        return; // don't rewrite within the defining package
    }
    let pos = for_stmt.for_.0 as u32;
    if !go_at_least(pass, pos, row.since) {
        return;
    }
    let Some(loop_var) = ident_obj(pass, cmp_x) else {
        return;
    };
    let elem = row.elemname;
    let Some(mut at_edits) =
        stditerators_at_edits(pass, &for_stmt.body, recv, loop_var, row.atmethod, elem)
    else {
        return;
    };

    let elem_prefix = if row.seqn == 2 { "_, " } else { "" };
    let mut edits = vec![
        TextEdit {
            pos: init.lhs[0].pos().0 as u32,
            end: init.lhs[0].end().0 as u32,
            new_text: format!("{elem_prefix}{elem}"),
        },
        TextEdit {
            pos: init.rhs[0].pos().0 as u32,
            end: cmp.y.pos().0 as u32,
            new_text: "range ".into(),
        },
        TextEdit {
            pos: len_sel.sel.pos().0 as u32,
            end: len_sel.sel.end().0 as u32,
            new_text: row.itermethod.into(),
        },
        TextEdit {
            pos: len_call.rparen.0 as u32 + 1,
            end: post.end().0 as u32,
            new_text: String::new(),
        },
    ];
    edits.append(&mut at_edits);

    pending.push(Diagnostic {
        pos,
        end: post.end().0 as u32,
        category: "stditerators".into(),
        message: stditerators_message(row),
        suggested_fixes: vec![SuggestedFix {
            message: stditerators_fix_message(row),
            text_edits: edits,
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

/// Pattern 2: `for i := range x.Len() { use(x.At(i)) }`.
fn check_stditerators_range(
    pass: &Pass<'_>,
    range_stmt: &RangeStmt,
    pending: &mut Vec<Diagnostic>,
) {
    if range_stmt.tok != Some(Token::DEFINE) || range_stmt.value.is_some() {
        return;
    }
    let Some(key) = range_stmt.key.as_ref() else {
        return;
    };
    let Expr::Ident(key_id) = key else {
        return;
    };
    let Expr::CallExpr(len_call) = &range_stmt.x else {
        return;
    };
    if !len_call.args.is_empty() {
        return;
    }
    let Expr::SelectorExpr(len_sel) = unparen_expr(&len_call.fun) else {
        return;
    };

    let recv = &len_sel.x;
    let Some(row) = stditerators_row_for(pass, recv, &len_sel.sel.name) else {
        return;
    };
    if pass.pkg().pkg_path == row.pkgpath {
        return;
    }
    if !go_at_least(pass, range_stmt.for_.0 as u32, row.since) {
        return;
    }
    let Some(loop_var) = ident_obj(pass, key_id) else {
        return;
    };
    let elem = row.elemname;
    let Some(mut at_edits) =
        stditerators_at_edits(pass, &range_stmt.body, recv, loop_var, row.atmethod, elem)
    else {
        return;
    };

    let elem_prefix = if row.seqn == 2 { "_, " } else { "" };
    let mut edits = vec![
        TextEdit {
            pos: key.pos().0 as u32,
            end: key.end().0 as u32,
            new_text: format!("{elem_prefix}{elem}"),
        },
        TextEdit {
            pos: len_sel.sel.pos().0 as u32,
            end: len_sel.sel.end().0 as u32,
            new_text: row.itermethod.into(),
        },
    ];
    edits.append(&mut at_edits);

    pending.push(Diagnostic {
        pos: range_stmt.range_.0 as u32,
        end: range_stmt.x.end().0 as u32,
        category: "stditerators".into(),
        message: stditerators_message(row),
        suggested_fixes: vec![SuggestedFix {
            message: stditerators_fix_message(row),
            text_edits: edits,
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
        ..Diagnostic::default()
    });
}

/// sync/atomic funcs rewritten by `atomictypes` (Go 1.19+, And/Or Go 1.23+).
const SYNC_ATOMIC_FUNCS: &[&str] = &[
    "AddInt32",
    "AddInt64",
    "AddUint32",
    "AddUint64",
    "AddUintptr",
    "CompareAndSwapInt32",
    "CompareAndSwapInt64",
    "CompareAndSwapUint32",
    "CompareAndSwapUint64",
    "CompareAndSwapUintptr",
    "LoadInt32",
    "LoadInt64",
    "LoadUint32",
    "LoadUint64",
    "LoadUintptr",
    "StoreInt32",
    "StoreInt64",
    "StoreUint32",
    "StoreUint64",
    "StoreUintptr",
    "SwapInt32",
    "SwapInt64",
    "SwapUint32",
    "SwapUint64",
    "SwapUintptr",
    "AndInt32",
    "AndInt64",
    "AndUint32",
    "AndUint64",
    "AndUintptr",
    "OrInt32",
    "OrInt64",
    "OrUint32",
    "OrUint64",
    "OrUintptr",
];

fn atomic_type_name(under: &str) -> Option<&'static str> {
    match under {
        "int32" => Some("Int32"),
        "int64" => Some("Int64"),
        "uint32" => Some("Uint32"),
        "uint64" => Some("Uint64"),
        "uintptr" => Some("Uintptr"),
        _ => None,
    }
}

fn sync_atomic_func_name(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    let name = code::call_name(pass, &call.fun)?;
    let short = name.strip_prefix("sync/atomic.")?;
    if SYNC_ATOMIC_FUNCS.contains(&short) {
        Some(short.to_string())
    } else {
        None
    }
}

/// Resolve `atomic.F(&v)` / `atomic.F(&recv.field)` to the addressed var and
/// the l-value expression (`v` or `recv.field`).
fn var_from_atomic_addr<'a>(
    pass: &Pass<'_>,
    arg: &'a Expr,
) -> Option<(ObjectId, &'a Expr)> {
    let Expr::UnaryExpr(u) = unparen_expr(arg) else {
        return None;
    };
    if u.op != Token::AND {
        return None;
    }
    match u.x.as_ref() {
        Expr::Ident(id) => {
            let obj = ident_obj(pass, id)?;
            Some((obj, u.x.as_ref()))
        }
        Expr::SelectorExpr(sel) => {
            let info = pass.types_info()?;
            let obj = if let Some(seln) = info.selections.get(&sel.id) {
                seln.obj()
            } else {
                ident_obj(pass, &sel.sel)?
            };
            let artifacts = pass.pkg().type_artifacts.as_ref()?;
            if !matches!(artifacts.objects.get(obj), ObjectData::Var(_)) {
                return None;
            }
            Some((obj, u.x.as_ref()))
        }
        _ => None,
    }
}

fn atomictypes_skip_kind(pass: &Pass<'_>, obj: ObjectId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return true;
    };
    if obj.exported(&artifacts.objects) {
        return true;
    }
    let ObjectData::Var(v) = artifacts.objects.get(obj) else {
        return true;
    };
    matches!(
        v.kind(),
        VarKind::Recv | VarKind::Param | VarKind::Result
    )
}

/// Upstream's `isLocal`: the object's scope is four levels deep or more, which
/// is to say it was declared inside a function. Receivers, parameters and
/// results qualify there too, but [`atomictypes_skip_kind`] has already dropped
/// those, so the survivors split cleanly into `Local` and everything else.
fn atomictypes_is_local(pass: &Pass<'_>, obj: ObjectId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let ObjectData::Var(v) = artifacts.objects.get(obj) else {
        return false;
    };
    matches!(v.kind(), VarKind::Local)
}

struct AtomicCand<'a> {
    func_name: String,
    sites: Vec<(&'a CallExpr, &'a Expr)>,
}

enum AtomicDecl<'a> {
    Var {
        name: &'a guff::ast::Ident,
        ty: &'a Expr,
    },
    Field {
        name: &'a guff::ast::Ident,
        ty: &'a Expr,
    },
}

fn find_atomictypes_decl<'a>(
    pass: &'a Pass<'_>,
    obj: ObjectId,
) -> Option<AtomicDecl<'a>> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let ObjectData::Var(v) = artifacts.objects.get(obj) else {
        return None;
    };
    let is_field = v.is_field();
    let field_name = v.name().to_string();

    let mut found = None;
    for file in pass.files() {
        if is_field {
            walk::inspect(NodeRef::File(file), |n| {
                let Some(n) = n else {
                    return true;
                };
                let NodeRef::TypeSpec(ts) = n else {
                    return true;
                };
                let Expr::StructType(st) = &ts.ty else {
                    return true;
                };
                let Some(type_obj) = ident_obj(pass, &ts.name) else {
                    return true;
                };
                let Some(typ) = type_obj.typ(&artifacts.objects) else {
                    return true;
                };
                let typ = unalias_readonly(&artifacts.types, typ);
                let under = typ.underlying(&artifacts.types);
                let nfields = guff_types::r#struct::struct_num_fields(&artifacts.types, under);
                let mut matched = false;
                for i in 0..nfields {
                    if guff_types::r#struct::struct_field(&artifacts.types, under, i) == obj {
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    return true;
                }
                for field in &st.fields.list {
                    if field.names.len() == 1
                        && field.names[0].name == field_name
                        && field.ty.is_some()
                    {
                        found = Some(AtomicDecl::Field {
                            name: &field.names[0],
                            ty: field.ty.as_ref().unwrap(),
                        });
                        return false;
                    }
                }
                true
            });
        } else {
            walk::inspect(NodeRef::File(file), |n| {
                let Some(n) = n else {
                    return true;
                };
                if let NodeRef::ValueSpec(spec) = n {
                    if spec.names.len() == 1
                        && spec.values.is_empty()
                        && spec.ty.is_some()
                        && ident_obj(pass, &spec.names[0]) == Some(obj)
                    {
                        found = Some(AtomicDecl::Var {
                            name: &spec.names[0],
                            ty: spec.ty.as_ref().unwrap(),
                        });
                        return false;
                    }
                }
                true
            });
        }
        if found.is_some() {
            break;
        }
    }
    found
}

fn atomictypes_has_kv_key_use(pass: &Pass<'_>, obj: ObjectId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let ObjectData::Var(v) = artifacts.objects.get(obj) else {
        return false;
    };
    if !v.is_field() {
        return false;
    }
    let field_name = v.name().to_string();
    let mut hit = false;
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            let NodeRef::CompositeLit(lit) = n else {
                return true;
            };
            let Some(info) = pass.types_info() else {
                return true;
            };
            let Some(tav) = info.types.get(&lit.id) else {
                return true;
            };
            // `[]*T{{f: 1}}` elides `&T` — the literal's own type is `*T`, and
            // the field it names still belongs to `T`. Upstream never asks what
            // type the literal has (it walks the *uses of the field object* and
            // rejects any whose parent edge is a KeyValueExpr key), so a
            // pointer here must not end the search. coredns
            // `plugin/errors/errors_test.go` is `[]*pattern{{count: 4, …}}`,
            // and leaving it unpeeled made `count` look like a clean candidate.
            let typ = unalias_readonly(&artifacts.types, tav.typ);
            let mut under = typ.underlying(&artifacts.types);
            if let TypeData::Pointer(p) = artifacts.types.get(under) {
                let elem = unalias_readonly(&artifacts.types, p.elem());
                under = elem.underlying(&artifacts.types);
            }
            if !matches!(artifacts.types.get(under), TypeData::Struct(_)) {
                return true;
            }
            let nfields = guff_types::r#struct::struct_num_fields(&artifacts.types, under);
            let mut owns = false;
            for i in 0..nfields {
                if guff_types::r#struct::struct_field(&artifacts.types, under, i) == obj {
                    owns = true;
                    break;
                }
            }
            if !owns {
                return true;
            }
            for elt in &lit.elts {
                if let Expr::KeyValueExpr(kv) = elt {
                    if let Expr::Ident(id) = kv.key.as_ref() {
                        if id.name == field_name {
                            hit = true;
                        }
                    }
                }
            }
            true
        });
        if hit {
            break;
        }
    }
    hit
}

fn atomictypes_use_count(pass: &Pass<'_>, obj: ObjectId) -> usize {
    let Some(info) = pass.types_info() else {
        return 0;
    };
    info.uses.values().filter(|&&o| o == obj).count()
}

fn underlying_basic_name(pass: &Pass<'_>, typ: TypeId) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = unalias_readonly(&artifacts.types, typ);
    let under = typ.underlying(&artifacts.types);
    match artifacts.types.get(under) {
        TypeData::Basic(b) => Some(b.name().to_string()),
        _ => None,
    }
}

/// Port of modernize `atomictypes`: rewrite `sync/atomic` funcs + basic vars
/// into typed `atomic.Int32` (etc.) wrappers.
fn check_atomictypes(pass: &Pass<'_>, pending: &mut Vec<Diagnostic>) {
    let mut cands: HashMap<ObjectId, AtomicCand<'_>> = HashMap::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            let NodeRef::CallExpr(call) = n else {
                return true;
            };
            let Some(func_name) = sync_atomic_func_name(pass, call) else {
                return true;
            };
            if call.args.is_empty() {
                return true;
            }
            let Some((obj, vexpr)) = var_from_atomic_addr(pass, &call.args[0]) else {
                return true;
            };
            if atomictypes_skip_kind(pass, obj) {
                return true;
            }
            let entry = cands.entry(obj).or_insert_with(|| AtomicCand {
                func_name: func_name.clone(),
                sites: Vec::new(),
            });
            entry.sites.push((call, vexpr));
            true
        });
    }

    // Upstream: `if !isLocal(v) && len(pass.IgnoredFiles) > 0 { continue }`.
    // A package-level var or a struct field can be used from a file the build
    // constraints excluded, which the analyzer cannot see, so the rewrite it
    // would propose might not compile there. A local var cannot be, so it is
    // reported either way.
    //
    // coredns's `plugin/forward` and `plugin/grpc` each carry a
    // `//go:build gofuzz` file, which is what makes their `robin uint32`
    // fields silent upstream and reported here.
    let package_has_ignored_files = !pass.pkg().ignored_files.is_empty();
    for (obj, cand) in cands {
        if package_has_ignored_files && !atomictypes_is_local(pass, obj) {
            continue;
        }
        if atomictypes_has_kv_key_use(pass, obj) {
            continue;
        }
        let use_count = atomictypes_use_count(pass, obj);
        if use_count != cand.sites.len() {
            continue;
        }
        let Some(decl) = find_atomictypes_decl(pass, obj) else {
            continue;
        };
        let (name_ident, ty_expr) = match &decl {
            AtomicDecl::Var { name, ty } | AtomicDecl::Field { name, ty } => (*name, *ty),
        };
        let Some(old_ty) = type_of(pass, ty_expr) else {
            continue;
        };
        let Some(under_name) = underlying_basic_name(pass, old_ty) else {
            continue;
        };
        let Some(new_type) = atomic_type_name(&under_name) else {
            continue;
        };

        let needs_123 = cand.func_name.starts_with("And") || cand.func_name.starts_with("Or");
        let pos = name_ident.pos().0 as u32;
        if needs_123 {
            if !go_at_least(pass, pos, "go1.23") {
                continue;
            }
        } else if !go_at_least(pass, pos, "go1.19") {
            continue;
        }

        // The prefix comes from the *declaration's* file, not from the alias
        // the call site happened to use: a file that spells the package
        // `myatomic` still gets `var x atomic.Int32` when the declaring file
        // imports it plainly. The import edits matter for the same reason —
        // after the fix the need for `sync/atomic` moves from the use to the
        // declaration, which may be in a file that did not import it.
        let Some(decl_file) = refactor::enclosing_file(pass, pos) else {
            continue;
        };
        let Some((prefix, import_edits)) =
            refactor::add_import(pass, decl_file, "atomic", "sync/atomic", "", pos)
        else {
            continue;
        };
        let mut edits = vec![TextEdit {
            pos: ty_expr.pos().0 as u32,
            end: ty_expr.end().0 as u32,
            new_text: format!("{prefix}{new_type}"),
        }];

        for (call, vexpr) in &cand.sites {
            let Some(fn_name) = sync_atomic_func_name(pass, call) else {
                continue;
            };
            let Some(verb) = fn_name.strip_suffix(new_type) else {
                // Mismatched atomic width vs declared type — skip whole var.
                edits.clear();
                break;
            };
            let after = if call.args.len() > 1 {
                call.args[1].pos().0 as u32
            } else {
                vexpr.end().0 as u32
            };
            edits.push(TextEdit {
                pos: call.pos().0 as u32,
                end: vexpr.pos().0 as u32,
                new_text: String::new(),
            });
            edits.push(TextEdit {
                pos: vexpr.end().0 as u32,
                end: after,
                new_text: format!(".{verb}("),
            });
        }
        if edits.len() <= 1 {
            continue;
        }
        let edits = with_imports(&import_edits, edits);

        let var_name = name_ident.name.as_str();
        pending.push(Diagnostic {
            pos,
            end: ty_expr.end().0 as u32,
            category: String::new(),
            message: format!(
                "var {var_name} {under_name} may be simplified using atomic.{new_type}"
            ),
            suggested_fixes: vec![SuggestedFix {
                message: format!("Replace {under_name} by atomic.{new_type}"),
                text_edits: edits,
            }],
            related: Vec::new(),
            url: String::new(),
            severity: String::new(),
            ..Diagnostic::default()
        });
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "modernize requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<ModernizeOptions>("modernize")
        .cloned()
        .unwrap_or_default();

    let mut pending = Vec::new();
    if enabled(&options, "newexpr") {
        let cands = collect_newexpr_decls(pass);
        export_newexpr_decls(pass, cands, &mut pending);
    }
    if enabled(&options, "atomictypes") {
        let _before = pending.len();
        check_atomictypes(pass, &mut pending);
        stamp_category(&mut pending, _before, "atomictypes");
    }
    // Computed once per package, like upstream's `sync.OnceValue`, and only
    // when `omitzero` is on — it re-parses every file for comments.
    let uses_kubebuilder = enabled(&options, "omitzero") && package_uses_kubebuilder(pass);
    for (file_idx, file) in pass.files().iter().enumerate() {
        if enabled(&options, "plusbuild") && go_at_least(pass, file.package.0 as u32, "go1.18") {
            let _before = pending.len();
            check_plusbuild(file, &mut pending);
            stamp_category(&mut pending, _before, "plusbuild");
        }
        if enabled(&options, "testingcontext") {
            let _before = pending.len();
            check_testingcontext(pass, file, &mut pending);
            stamp_category(&mut pending, _before, "testingcontext");
        }
        if enabled(&options, "bloop") {
            let _before = pending.len();
            check_bloop(pass, file, &mut pending);
            stamp_category(&mut pending, _before, "bloop");
        }
        if enabled(&options, "importcomment") {
            let _before = pending.len();
            check_importcomment(pass, file_idx, file, &mut pending);
            stamp_category(&mut pending, _before, "importcomment");
        }
        if enabled(&options, "stringsbuilder") {
            let _before = pending.len();
            check_stringsbuilder(pass, file, &mut pending);
            stamp_category(&mut pending, _before, "stringsbuilder");
        }
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::InterfaceType(iface) if enabled(&options, "any") => {
                    let _before = pending.len();
                    check_any(pass, iface, &mut pending);
                    stamp_category(&mut pending, _before, "any");
                }
                NodeRef::RangeStmt(s) => {
                    if enabled(&options, "forvar") {
                        let _before = pending.len();
                        check_forvar(pass, s, &mut pending);
                        stamp_category(&mut pending, _before, "forvar");
                    }
                    if enabled(&options, "stringsseq") {
                        let _before = pending.len();
                        check_stringsseq(pass, s, &mut pending);
                        stamp_category(&mut pending, _before, "stringsseq");
                    }
                    if enabled(&options, "mapsloop") {
                        let _before = pending.len();
                        check_mapsloop(pass, file, s, &mut pending);
                        stamp_category(&mut pending, _before, "mapsloop");
                    }
                    if enabled(&options, "stditerators") {
                        let _before = pending.len();
                        check_stditerators_range(pass, s, &mut pending);
                        stamp_category(&mut pending, _before, "stditerators");
                    }
                }
                NodeRef::ForStmt(s) => {
                    if enabled(&options, "rangeint") {
                        let _before = pending.len();
                        check_rangeint(pass, s, &mut pending);
                        stamp_category(&mut pending, _before, "rangeint");
                    }
                    if enabled(&options, "slicesbackward") {
                        let _before = pending.len();
                        check_slicesbackward(pass, file, s, &mut pending);
                        stamp_category(&mut pending, _before, "slicesbackward");
                    }
                    if enabled(&options, "stditerators") {
                        let _before = pending.len();
                        check_stditerators_for(pass, s, &mut pending);
                        stamp_category(&mut pending, _before, "stditerators");
                    }
                }
                NodeRef::IfStmt(s) => {
                    if enabled(&options, "minmax") {
                        let _before = pending.len();
                        check_minmax(pass, s, &mut pending);
                        stamp_category(&mut pending, _before, "minmax");
                    }
                    if enabled(&options, "stringscutprefix") {
                        let _before = pending.len();
                        check_stringscutprefix(pass, file, s, &mut pending);
                        stamp_category(&mut pending, _before, "stringscutprefix");
                    }
                    if enabled(&options, "errorsastype") {
                        let _before = pending.len();
                        check_errorsastype(pass, file, s, &mut pending);
                        stamp_category(&mut pending, _before, "errorsastype");
                    }
                }
                NodeRef::BlockStmt(b) => {
                    if enabled(&options, "minmax") {
                        let _before = pending.len();
                        check_minmax_block(pass, b, &mut pending);
                        stamp_category(&mut pending, _before, "minmax");
                    }
                    if enabled(&options, "stringsseq") {
                        let _before = pending.len();
                        check_stringsseq_block(pass, b, &mut pending);
                        stamp_category(&mut pending, _before, "stringsseq");
                    }
                    if enabled(&options, "slicescontains") {
                        let _before = pending.len();
                        check_slicescontains(pass, file, b, &mut pending);
                        stamp_category(&mut pending, _before, "slicescontains");
                    }
                    if enabled(&options, "waitgroupgo") {
                        let _before = pending.len();
                        check_waitgroupgo(pass, file, b, &mut pending);
                        stamp_category(&mut pending, _before, "waitgroupgo");
                    }
                }
                NodeRef::AssignStmt(a) => {
                    if enabled(&options, "stringscut") {
                        let _before = pending.len();
                        check_stringscut(pass, a, &mut pending);
                        stamp_category(&mut pending, _before, "stringscut");
                    }
                    if enabled(&options, "reflecttypeassert") {
                        let _before = pending.len();
                        check_reflecttypeassert(pass, file, a, &mut pending);
                        stamp_category(&mut pending, _before, "reflecttypeassert");
                    }
                }
                NodeRef::CallExpr(c) => {
                    if enabled(&options, "fmtappendf") {
                        let _before = pending.len();
                        check_fmtappendf(pass, c, &mut pending);
                        stamp_category(&mut pending, _before, "fmtappendf");
                    }
                    if enabled(&options, "slicessort") {
                        let _before = pending.len();
                        check_slicessort(pass, file, c, &mut pending);
                        stamp_category(&mut pending, _before, "slicessort");
                    }
                    if enabled(&options, "slicesdelete") {
                        let _before = pending.len();
                        check_slicesdelete(pass, file, c, &mut pending);
                        stamp_category(&mut pending, _before, "slicesdelete");
                    }
                    if enabled(&options, "reflecttypefor") {
                        let _before = pending.len();
                        // Prefer Elem() special-case; plain TypeOf is handled when
                        // this call is not itself the X of a `.Elem()` selector.
                        check_reflecttypefor_elem(pass, c, &mut pending);
                        check_reflecttypefor(pass, file, c, &mut pending);
                        stamp_category(&mut pending, _before, "reflecttypefor");
                    }
                    if enabled(&options, "unsafefuncs") {
                        let _before = pending.len();
                        check_unsafefuncs(pass, file, c, &mut pending);
                        stamp_category(&mut pending, _before, "unsafefuncs");
                    }
                    if enabled(&options, "newexpr") {
                        let _before = pending.len();
                        check_newexpr_call(pass, c, &mut pending);
                        stamp_category(&mut pending, _before, "newexpr");
                    }
                }
                NodeRef::StructType(StructType { fields, .. })
                    if enabled(&options, "omitzero") && !uses_kubebuilder =>
                {
                    for field in &fields.list {
                        check_omitzero(pass, field, &mut pending);
                    }
                }
                _ => {}
            }
            true
        });
    }

    for d in pending {
        pass.report(d);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| {
        ensure_new_like_decoder();
        Analyzer {
            name: "modernize",
            doc: "suggests simplifications to Go code using modern language and library features",
            url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/modernize",
            run: run as RunFn,
            // Still useful on packages guff typechecks incompletely (e.g. missing
            // export-data deps): AST/type-local checks like slicesbackward keep working.
            run_despite_errors: true,
            requires: vec![inspect::analyzer(), typeindex::analyzer()],
            fact_types: vec![FactTypeId::of::<NewLikeFact>()],
        }
    })
}
