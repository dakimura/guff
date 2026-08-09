//! `lostcancel` — check for missing calls to context cancel functions.
//!
//! Port of [`lostcancel`](https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/lostcancel).
//!
//! Two reports, both matching upstream's positions:
//!
//! - `ctx, _ := context.WithCancel(…)` — the cancel func is discarded. Reported
//!   on sight, at the `_` (upstream's `ReportRangef(id, …)`).
//! - a `return` is reachable from the `context.With*` call without any
//!   reference to the cancel variable in between. Reported twice: at the
//!   defining `AssignStmt` / `ValueSpec`, and at the return statement.
//!
//! **Any** reference to the variable counts as a use, even inside a nested
//! function literal — upstream searches for a path with no reference at all,
//! not for a call.
//!
//! Upstream walks a `ctrlflow` CFG depth-first, pruning blocks that reference
//! the variable, and reports the first return block it reaches. guff has no
//! CFG, so [`scan_seq`] runs the same search over the statement tree: it walks
//! forward from the defining statement, descends into each branch before the
//! code that follows it (upstream's DFS order), and stops on a reference that
//! every path through that point must execute. `Scan::Blocked` is the
//! statement-tree equivalent of upstream pruning a block.
//!
//! DEFERRED — the first two lose reports, the third can go either way:
//! - `goto` / labeled `break` / `continue` end the scan rather than following
//!   the edge, so a return reachable only through one is missed.
//! - a loop body that references the variable after a conditional `break`
//!   counts as covering the code after the loop.
//! - "this call does not return" is a name-based list ([`is_terminating_call`])
//!   rather than upstream's whole-program `ctrlflow` noreturn facts. A
//!   non-returning call the list does not know about lets the scan walk past it
//!   and report a return upstream considers unreachable; a `log.Fatal` that is
//!   really a method on a local variable named `log` does the opposite.

use std::sync::OnceLock;

use guff::ast::{
    BlockStmt, CaseClause, CommClause, Decl, Expr, FuncType, Ident, Spec, Stmt, ValueSpec,
};
use guff::position::Pos;
use guff::walk::{self, NodeRef};
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::ObjectId;

use crate::govet_util::imports_package;

const WITH_FUNCS: &[&str] = &[
    "WithCancel",
    "WithCancelCause",
    "WithTimeout",
    "WithTimeoutCause",
    "WithDeadline",
    "WithDeadlineCause",
];

/// Calls that never return, so the statements after them are unreachable and
/// the CFG block holding them has no successors.
///
/// Name-based stand-in for the `ctrlflow` noreturn facts (see module DEFERRED).
fn is_terminating_call(expr: &Expr) -> bool {
    let Expr::CallExpr(call) = expr else {
        return false;
    };
    match &*call.fun {
        Expr::Ident(id) => id.name == "panic",
        Expr::SelectorExpr(sel) => {
            let Expr::Ident(pkg) = sel.x.as_ref() else {
                return false;
            };
            match (pkg.name.as_str(), sel.sel.name.as_str()) {
                ("os", "Exit") => true,
                ("runtime", "Goexit") => true,
                ("log", m) => {
                    m.starts_with("Fatal") || m.starts_with("Panic")
                }
                _ => false,
            }
        }
        _ => false,
    }
}

fn is_context_with_cancel(pass: &Pass<'_>, e: &Expr) -> Option<String> {
    let Expr::SelectorExpr(sel) = e else {
        return None;
    };
    if !WITH_FUNCS.contains(&sel.sel.name.as_str()) {
        return None;
    }
    let Expr::Ident(pkg) = sel.x.as_ref() else {
        return None;
    };
    if pkg.name != "context" {
        return None;
    }
    // Import failed? Upstream falls back to the local package name.
    if let (Some(info), Some(artifacts)) = (pass.types_info(), pass.pkg().type_artifacts.as_ref()) {
        if let Some(obj) = info.uses.get(&pkg.id).copied() {
            if let guff_types::arena::ObjectData::PkgName(pn) = artifacts.objects.get(obj) {
                if artifacts.packages.get(pn.imported()).path() != "context" {
                    return None;
                }
            }
        }
    }
    Some(sel.sel.name.clone())
}

/// A `cancel` variable defined by `ctx, cancel := context.With*(…)`.
struct CancelDef {
    /// The variable the second LHS ident resolves to. Uses are matched against
    /// it, so the defining ident itself is never mistaken for a use.
    obj: ObjectId,
    name: String,
    /// Upstream's `ReportRangef(stmt, …)`: the `AssignStmt` or the `ValueSpec`
    /// (not the enclosing `DeclStmt` — `var ctx, cancel = …` reports at `ctx`).
    stmt_pos: u32,
    /// A naked `return` counts as a use when the variable is a named result.
    is_named_result: bool,
}

enum Def {
    /// `ctx, _ := context.WithCancel(…)`, with the position of the `_`.
    Discarded { pos: u32, with_name: String },
    Var(CancelDef),
}

/// The enclosing function: everything the search needs that is not per-variable.
struct Ctx<'a, 'p> {
    pass: &'a Pass<'p>,
    ty: &'a FuncType,
    body: &'a BlockStmt,
    /// Objects of the function's named results.
    named_results: Vec<ObjectId>,
}

impl<'a, 'p> Ctx<'a, 'p> {
    fn new(pass: &'a Pass<'p>, ty: &'a FuncType, body: &'a BlockStmt) -> Self {
        let mut named_results = Vec::new();
        if let (Some(info), Some(results)) = (pass.types_info(), ty.results.as_ref()) {
            for field in &results.list {
                for name in &field.names {
                    if let Some(Some(obj)) = info.defs.get(&name.id) {
                        named_results.push(*obj);
                    }
                }
            }
        }
        Self {
            pass,
            ty,
            body,
            named_results,
        }
    }

    fn line(&self, pos: u32) -> i64 {
        self.pass.fset().position(Pos(pos as i64)).line
    }

    /// Upstream's `funcScope.Contains(v.Pos())`: a variable declared outside the
    /// function may be used by code this analysis cannot see, so it is skipped.
    fn declares(&self, obj: ObjectId) -> bool {
        let Some(info) = self.pass.types_info() else {
            return false;
        };
        let mut found = false;
        for root in [
            NodeRef::FuncType(self.ty),
            NodeRef::BlockStmt(self.body),
        ] {
            if found {
                break;
            }
            walk::inspect(root, |n| {
                if let Some(NodeRef::Ident(id)) = n {
                    if info.defs.get(&id.id) == Some(&Some(obj)) {
                        found = true;
                        return false;
                    }
                }
                !found
            });
        }
        found
    }
}

/// Labels do not affect the flow through a statement; the search looks past them.
fn unlabel(stmt: &Stmt) -> &Stmt {
    let mut cur = stmt;
    while let Stmt::LabeledStmt(l) = cur {
        cur = &l.stmt;
    }
    cur
}

/// `context.With*` calls directly under `stmt`, paired with the cancel variable
/// they assign — upstream's `[{AssignStmt,ValueSpec} CallExpr SelectorExpr]`
/// stack shape.
fn defs_from_stmt(ctx: &Ctx<'_, '_>, stmt: &Stmt) -> Vec<Def> {
    let mut out = Vec::new();
    match unlabel(stmt) {
        Stmt::AssignStmt(a) if a.lhs.len() > 1 => {
            if let Some(with_name) = a.rhs.iter().find_map(|r| call_to_with(ctx.pass, r)) {
                if let Expr::Ident(id) = &a.lhs[1] {
                    push_def(ctx, id, with_name, stmt.pos().0 as u32, &mut out);
                }
            }
        }
        Stmt::DeclStmt(ds) => {
            let Decl::GenDecl(gd) = &ds.decl else {
                return out;
            };
            for spec in &gd.specs {
                let Spec::ValueSpec(ValueSpec { names, values, .. }) = spec else {
                    continue;
                };
                if names.len() < 2 {
                    continue;
                }
                if let Some(with_name) = values.iter().find_map(|v| call_to_with(ctx.pass, v)) {
                    // The ValueSpec, not the DeclStmt: `var ctx, cancel = …`
                    // reports at `ctx`, past the `var` keyword.
                    let spec_pos = names[0].pos().0 as u32;
                    push_def(ctx, &names[1], with_name, spec_pos, &mut out);
                }
            }
        }
        _ => {}
    }
    out
}

fn call_to_with(pass: &Pass<'_>, expr: &Expr) -> Option<String> {
    let Expr::CallExpr(call) = expr else {
        return None;
    };
    is_context_with_cancel(pass, &call.fun)
}

fn push_def(
    ctx: &Ctx<'_, '_>,
    id: &Ident,
    with_name: String,
    stmt_pos: u32,
    out: &mut Vec<Def>,
) {
    if id.name == "_" {
        out.push(Def::Discarded {
            pos: id.pos().0 as u32,
            with_name,
        });
        return;
    }
    let Some(info) = ctx.pass.types_info() else {
        return;
    };
    // `ctx, cancel := …` defines the variable; `ctx, cancel = …` uses one that
    // must belong to this function for the search to see all of its references.
    let obj = match info.defs.get(&id.id) {
        Some(Some(obj)) => *obj,
        _ => match info.uses.get(&id.id).copied() {
            Some(obj) if ctx.declares(obj) => obj,
            _ => return,
        },
    };
    out.push(Def::Var(CancelDef {
        obj,
        name: id.name.clone(),
        stmt_pos,
        is_named_result: ctx.named_results.contains(&obj),
    }));
}

/// How far the cancel variable's reference reaches over a statement sequence.
enum Scan {
    /// A return statement is reachable without a reference (its position).
    Bad(u32),
    /// Every path onward references the variable, or leaves the function:
    /// whatever follows is unreachable without a reference. Upstream prunes.
    Blocked,
    /// Control reaches the next statement with the variable still unreferenced.
    Fell,
}

/// Does any ident in `node` resolve to the cancel variable?
fn uses_node(ctx: &Ctx<'_, '_>, def: &CancelDef, node: NodeRef<'_>) -> bool {
    let Some(info) = ctx.pass.types_info() else {
        return false;
    };
    let mut found = false;
    walk::inspect(node, |n| {
        if found {
            return false;
        }
        match n {
            Some(NodeRef::Ident(id)) => {
                if info.uses.get(&id.id) == Some(&def.obj) {
                    found = true;
                    return false;
                }
            }
            // Upstream: a naked return counts as a use of the named results.
            Some(NodeRef::ReturnStmt(r)) if def.is_named_result && r.results.is_empty() => {
                found = true;
                return false;
            }
            _ => {}
        }
        true
    });
    found
}

fn uses_stmt(ctx: &Ctx<'_, '_>, def: &CancelDef, stmt: &Stmt) -> bool {
    uses_node(ctx, def, walk::stmt_ref(stmt))
}

fn uses_expr(ctx: &Ctx<'_, '_>, def: &CancelDef, expr: &Expr) -> bool {
    uses_node(ctx, def, walk::expr_ref(expr))
}

fn uses_opt_stmt(ctx: &Ctx<'_, '_>, def: &CancelDef, stmt: &Option<Box<Stmt>>) -> bool {
    stmt.as_ref().is_some_and(|s| uses_stmt(ctx, def, s))
}

/// References that every path through `stmt` must execute: the whole subtree of
/// a simple statement, or only the header of a compound one (its branches are
/// visited separately, since a path may skip them).
fn unavoidable_use(ctx: &Ctx<'_, '_>, def: &CancelDef, stmt: &Stmt) -> bool {
    match stmt {
        Stmt::IfStmt(s) => uses_opt_stmt(ctx, def, &s.init) || uses_expr(ctx, def, &s.cond),
        Stmt::ForStmt(s) => {
            uses_opt_stmt(ctx, def, &s.init)
                || s.cond.as_ref().is_some_and(|c| uses_expr(ctx, def, c))
                || uses_opt_stmt(ctx, def, &s.post)
        }
        Stmt::RangeStmt(s) => {
            s.key.as_ref().is_some_and(|k| uses_expr(ctx, def, k))
                || s.value.as_ref().is_some_and(|v| uses_expr(ctx, def, v))
                || uses_expr(ctx, def, &s.x)
        }
        Stmt::SwitchStmt(s) => {
            uses_opt_stmt(ctx, def, &s.init)
                || s.tag.as_ref().is_some_and(|t| uses_expr(ctx, def, t))
        }
        Stmt::TypeSwitchStmt(s) => {
            uses_opt_stmt(ctx, def, &s.init) || uses_stmt(ctx, def, &s.assign)
        }
        Stmt::SelectStmt(_) | Stmt::BlockStmt(_) => false,
        other => uses_stmt(ctx, def, other),
    }
}

fn scan_seq(ctx: &Ctx<'_, '_>, def: &CancelDef, stmts: &[Stmt]) -> Scan {
    for raw in stmts {
        let stmt = unlabel(raw);
        if unavoidable_use(ctx, def, stmt) {
            return Scan::Blocked;
        }
        match stmt {
            Stmt::ReturnStmt(r) => return Scan::Bad(r.return_.0 as u32),
            // break / continue / goto / fallthrough: the edge is not followed.
            Stmt::BranchStmt(_) => return Scan::Blocked,
            Stmt::ExprStmt(e) if is_terminating_call(&e.x) => return Scan::Blocked,
            Stmt::BlockStmt(b) => match scan_seq(ctx, def, &b.list) {
                Scan::Bad(p) => return Scan::Bad(p),
                Scan::Blocked => return Scan::Blocked,
                Scan::Fell => {}
            },
            Stmt::IfStmt(s) => {
                let then = match scan_seq(ctx, def, &s.body.list) {
                    Scan::Bad(p) => return Scan::Bad(p),
                    other => other,
                };
                let Some(else_) = &s.else_ else {
                    // No else: the code after the `if` is always reachable.
                    continue;
                };
                let else_scan = match scan_seq(ctx, def, std::slice::from_ref(else_.as_ref())) {
                    Scan::Bad(p) => return Scan::Bad(p),
                    other => other,
                };
                if matches!(then, Scan::Blocked) && matches!(else_scan, Scan::Blocked) {
                    return Scan::Blocked;
                }
            }
            Stmt::ForStmt(s) => match scan_seq(ctx, def, &s.body.list) {
                Scan::Bad(p) => return Scan::Bad(p),
                // `for {}` is left only by a `break` or a `return`, and the scan
                // does not follow `break` edges, so what comes after it does not
                // count as reachable. A condition can skip the body entirely.
                _ if s.cond.is_none() => return Scan::Blocked,
                _ => {}
            },
            Stmt::RangeStmt(s) => {
                if let Scan::Bad(p) = scan_seq(ctx, def, &s.body.list) {
                    return Scan::Bad(p);
                }
            }
            Stmt::SwitchStmt(s) => match scan_cases(ctx, def, &s.body.list) {
                Scan::Bad(p) => return Scan::Bad(p),
                Scan::Blocked => return Scan::Blocked,
                Scan::Fell => {}
            },
            Stmt::TypeSwitchStmt(s) => match scan_cases(ctx, def, &s.body.list) {
                Scan::Bad(p) => return Scan::Bad(p),
                Scan::Blocked => return Scan::Blocked,
                Scan::Fell => {}
            },
            Stmt::SelectStmt(s) => match scan_comms(ctx, def, &s.body.list) {
                Scan::Bad(p) => return Scan::Bad(p),
                Scan::Blocked => return Scan::Blocked,
                Scan::Fell => {}
            },
            _ => {}
        }
    }
    Scan::Fell
}

/// `switch` clauses. Without a `default` the whole statement can be skipped, so
/// the code after it stays reachable however the clauses behave.
fn scan_cases(ctx: &Ctx<'_, '_>, def: &CancelDef, clauses: &[Stmt]) -> Scan {
    let mut has_default = false;
    let mut any_fell = false;
    for clause in clauses {
        let Stmt::CaseClause(CaseClause { list, body, .. }) = clause else {
            continue;
        };
        if list.is_empty() {
            has_default = true;
        }
        match scan_seq(ctx, def, body) {
            Scan::Bad(p) => return Scan::Bad(p),
            Scan::Fell => any_fell = true,
            Scan::Blocked => {}
        }
    }
    if !has_default || any_fell {
        Scan::Fell
    } else {
        Scan::Blocked
    }
}

/// `select` clauses. Exactly one clause runs, so the code after the statement is
/// reachable only through a clause that does not reference the variable.
fn scan_comms(ctx: &Ctx<'_, '_>, def: &CancelDef, clauses: &[Stmt]) -> Scan {
    let mut any_fell = false;
    for clause in clauses {
        let Stmt::CommClause(CommClause { comm, body, .. }) = clause else {
            continue;
        };
        if comm.as_ref().is_some_and(|c| uses_stmt(ctx, def, c)) {
            continue;
        }
        match scan_seq(ctx, def, body) {
            Scan::Bad(p) => return Scan::Bad(p),
            Scan::Fell => any_fell = true,
            Scan::Blocked => {}
        }
    }
    if any_fell {
        Scan::Fell
    } else {
        Scan::Blocked
    }
}

/// What runs after the statement currently being walked, as a chain of
/// enclosing statement sequences.
///
/// A stack-allocated list rather than a `Vec`: the walk visits every statement of
/// every function in a package that imports `context`, and only the rare
/// statement that defines a cancel variable ever reads the chain.
struct Tail<'s, 'p> {
    stmts: &'s [Stmt],
    /// The sequence that runs once `stmts` finishes, or `None` at the end of the
    /// function body — or at the edge of a loop that is never left.
    outer: Option<&'p Tail<'s, 'p>>,
    /// Whether reaching the end of this chain leaves the function.
    exits: bool,
}

/// Upstream's `lostCancelPath`: the first return statement reachable from the
/// defining statement without a reference to the cancel variable.
fn lost_cancel_path(ctx: &Ctx<'_, '_>, def: &CancelDef, tail: &Tail<'_, '_>) -> Option<u32> {
    let mut cur = Some(tail);
    let mut exits = tail.exits;
    while let Some(t) = cur {
        match scan_seq(ctx, def, t.stmts) {
            Scan::Bad(pos) => return Some(pos),
            Scan::Blocked => return None,
            Scan::Fell => {}
        }
        exits = t.exits;
        cur = t.outer;
    }
    if !exits {
        // The statement is inside a loop that is never left, so the end of the
        // function is not reachable from here and neither is its return.
        return None;
    }
    // Falling off the end of the function reaches the synthetic return that
    // upstream's CFG puts there; it is reported at the closing brace.
    Some(ctx.body.rbrace.0 as u32)
}

/// Walks the function's statement tree, reporting each cancel variable it finds.
fn walk_defs<'s>(
    ctx: &Ctx<'_, '_>,
    stmts: &'s [Stmt],
    outer: Option<&Tail<'s, '_>>,
    exits: bool,
    out: &mut Vec<(u32, String)>,
) {
    for (i, stmt) in stmts.iter().enumerate() {
        let tail = Tail {
            stmts: &stmts[i + 1..],
            outer,
            exits,
        };

        for def in defs_from_stmt(ctx, stmt) {
            match def {
                Def::Discarded { pos, with_name } => out.push((
                    pos,
                    format!(
                        "the cancel function returned by context.{with_name} should be called, not discarded, to avoid a context leak"
                    ),
                )),
                Def::Var(def) => {
                    if let Some(ret_pos) = lost_cancel_path(ctx, &def, &tail) {
                        let line = ctx.line(def.stmt_pos);
                        out.push((
                            def.stmt_pos,
                            format!(
                                "the {} function is not used on all paths (possible context leak)",
                                def.name
                            ),
                        ));
                        out.push((
                            ret_pos,
                            format!(
                                "this return statement may be reached without using the {} var defined on line {}",
                                def.name, line
                            ),
                        ));
                    }
                }
            }
        }

        walk_children(ctx, stmt, &tail, out);
    }
}

/// Descends into the statement sequences nested inside `stmt`, each of which the
/// search enters as its own path.
///
/// `for {}` without a condition is only left by a `break` or a `return`, and the
/// scan does not follow `break` edges, so nothing after such a loop counts as
/// reachable from inside it — including the end of the function.
fn walk_children<'s>(
    ctx: &Ctx<'_, '_>,
    stmt: &'s Stmt,
    tail: &Tail<'s, '_>,
    out: &mut Vec<(u32, String)>,
) {
    match unlabel(stmt) {
        Stmt::BlockStmt(b) => walk_defs(ctx, &b.list, Some(tail), tail.exits, out),
        Stmt::IfStmt(s) => {
            walk_defs(ctx, &s.body.list, Some(tail), tail.exits, out);
            if let Some(else_) = &s.else_ {
                let one = std::slice::from_ref(else_.as_ref());
                walk_defs(ctx, one, Some(tail), tail.exits, out);
            }
        }
        Stmt::RangeStmt(s) => walk_defs(ctx, &s.body.list, Some(tail), tail.exits, out),
        Stmt::SwitchStmt(s) => walk_clauses(ctx, &s.body.list, tail, out),
        Stmt::TypeSwitchStmt(s) => walk_clauses(ctx, &s.body.list, tail, out),
        Stmt::SelectStmt(s) => walk_clauses(ctx, &s.body.list, tail, out),
        Stmt::ForStmt(s) => {
            if s.cond.is_some() {
                walk_defs(ctx, &s.body.list, Some(tail), tail.exits, out);
            } else {
                // Nothing after the loop is reachable from inside it.
                walk_defs(ctx, &s.body.list, None, false, out);
            }
        }
        _ => {}
    }
}

fn walk_clauses<'s>(
    ctx: &Ctx<'_, '_>,
    clauses: &'s [Stmt],
    tail: &Tail<'s, '_>,
    out: &mut Vec<(u32, String)>,
) {
    for clause in clauses {
        let body = match clause {
            Stmt::CaseClause(cc) => &cc.body[..],
            Stmt::CommClause(cc) => &cc.body[..],
            _ => continue,
        };
        walk_defs(ctx, body, Some(tail), tail.exits, out);
    }
}

fn check_func(pass: &Pass<'_>, ty: &FuncType, body: &BlockStmt, out: &mut Vec<(u32, String)>) {
    let ctx = Ctx::new(pass, ty, body);
    walk_defs(&ctx, &body.list, None, true, out);
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if !imports_package(pass, "context") {
        return Ok(None);
    }
    let in_main = pass.pkg().name == "main";
    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            let Some(body) = &f.body else {
                continue;
            };
            // Returning from main.main terminates the process, so there is no
            // need to cancel contexts.
            if in_main && f.recv.is_none() && f.name.name == "main" {
                continue;
            }
            check_func(pass, &f.ty, body, &mut pending);
        }
        // Function literals are analyzed as functions of their own, including
        // the ones inside main.main.
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(NodeRef::FuncLit(lit)) = n {
                check_func(pass, &lit.ty, &lit.body, &mut pending);
            }
            true
        });
    }
    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "lostcancel",
        doc: "check for missing calls to context cancel functions",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/lostcancel",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    })
}
