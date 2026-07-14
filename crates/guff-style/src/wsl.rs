//! Port of [`github.com/bombsimon/wsl/v4`](https://github.com/bombsimon/wsl)
//! (golangci-lint wrapper `wsl` / deprecated name; use `wsl_v5` upstream for v5).
//!
//! Implements golangci-lint v4 default cuddle rules (assignment/if/decl/return/
//! branch/append/defer/range/for/switch) plus leading/trailing block whitespace.
//!
//! DEFERRED: full v4 parity (comment-map nuance, ForceCuddleErrCheck, force-case
//! whitespace, AllowSeparatedLeadingComment, nested func-lit edge cases);
//! SuggestedFix; `linters.settings.wsl` wiring; `wsl_v5` analyzer.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{BlockStmt, Decl, Expr, Spec, Stmt};
use guff::position::{FileSet, Pos};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

const REASON_APPEND: &str = "append only allowed to cuddle with appended value";
const REASON_ASSIGNS: &str = "assignments should only be cuddled with other assignments";
const REASON_BLOCK_START: &str = "block should not start with a whitespace";
const REASON_BLOCK_END: &str = "block should not end with a whitespace (or comment)";
const REASON_BRANCH: &str = "branch statements should not be cuddled if block has more than two lines";
const REASON_DECL: &str = "declarations should never be cuddled";
const REASON_IF_ASSIGN: &str = "if statements should only be cuddled with assignments";
const REASON_IF_USED: &str =
    "if statements should only be cuddled with assignments used in the if statement itself";
const REASON_ONE_IF: &str = "only one cuddle assignment allowed before if statement";
const REASON_ONE_DEFER: &str = "only one cuddle assignment allowed before defer statement";
const REASON_ONE_RANGE: &str = "only one cuddle assignment allowed before range statement";
const REASON_ONE_FOR: &str = "only one cuddle assignment allowed before for statement";
const REASON_RANGE: &str = "ranges should only be cuddled with assignments used in the iteration";
const REASON_FOR: &str = "for statements should only be cuddled with assignments used in the iteration";
const REASON_RETURN: &str = "return statements should not be cuddled if block has more than two lines";
const REASON_EXPR_BLOCK: &str = "expressions should not be cuddled with blocks";
const REASON_EXPR_DECL: &str = "expressions should not be cuddled with declarations or returns";
const REASON_EXPR_UNUSED: &str =
    "only cuddled expressions if assigning variable or using from line above";
const REASON_DEFER: &str = "defer statements should only be cuddled with expressions on same variable";
const REASON_SWITCH: &str = "switch statements should only be cuddled with variables switched";
const REASON_ANON_SWITCH: &str = "anonymous switch statements should never be cuddled";

fn line(fset: &FileSet, pos: Pos) -> i64 {
    fset.position(pos).line
}

fn stmt_start(fset: &FileSet, s: &Stmt) -> i64 {
    line(fset, s.pos())
}

fn stmt_end(fset: &FileSet, s: &Stmt) -> i64 {
    line(fset, s.end())
}

fn lists_overlap(a: &[String], b: &[String]) -> bool {
    let set: HashSet<&str> = a.iter().map(|s| s.as_str()).collect();
    b.iter().any(|x| set.contains(x.as_str()))
}

fn find_lhs(stmt: &Stmt) -> Vec<String> {
    match stmt {
        Stmt::AssignStmt(a) => a
            .lhs
            .iter()
            .flat_map(expr_idents)
            .collect(),
        Stmt::IncDecStmt(i) => expr_idents(&i.x),
        Stmt::DeclStmt(d) => match &d.decl {
            Decl::GenDecl(g) => g
                .specs
                .iter()
                .flat_map(|sp| match sp {
                    Spec::ValueSpec(vs) => vs.names.iter().map(|n| n.name.clone()).collect::<Vec<_>>(),
                    _ => Vec::new(),
                })
                .collect(),
            _ => Vec::new(),
        },
        Stmt::IfStmt(i) => expr_idents(&i.cond),
        _ => Vec::new(),
    }
}

fn find_rhs(stmt: &Stmt) -> Vec<String> {
    match stmt {
        Stmt::AssignStmt(a) => a.rhs.iter().flat_map(expr_idents).collect(),
        Stmt::ExprStmt(e) => expr_idents(&e.x),
        Stmt::IfStmt(i) => {
            let mut v = expr_idents(&i.cond);
            if let Some(init) = &i.init {
                v.extend(find_rhs(init));
                v.extend(find_lhs(init));
            }
            v
        }
        Stmt::RangeStmt(r) => {
            let mut v = expr_idents(&r.x);
            if let Some(k) = &r.key {
                v.extend(expr_idents(k));
            }
            if let Some(val) = &r.value {
                v.extend(expr_idents(val));
            }
            v
        }
        Stmt::ForStmt(f) => f.cond.as_ref().map(expr_idents).unwrap_or_default(),
        Stmt::DeferStmt(d) => call_idents(&d.call),
        Stmt::GoStmt(g) => call_idents(&g.call),
        Stmt::ReturnStmt(r) => r.results.iter().flat_map(expr_idents).collect(),
        Stmt::SwitchStmt(s) => s.tag.as_ref().map(expr_idents).unwrap_or_default(),
        Stmt::IncDecStmt(i) => expr_idents(&i.x),
        _ => Vec::new(),
    }
}

fn call_idents(c: &guff::ast::CallExpr) -> Vec<String> {
    let mut out = Vec::new();
    collect_idents(&c.fun, &mut out);
    for a in &c.args {
        collect_idents(a, &mut out);
    }
    out
}

fn expr_idents(e: &Expr) -> Vec<String> {
    let mut out = Vec::new();
    collect_idents(e, &mut out);
    out
}

fn collect_idents(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Ident(i) => out.push(i.name.clone()),
        Expr::SelectorExpr(s) => {
            collect_idents(&s.x, out);
            out.push(s.sel.name.clone());
        }
        Expr::CallExpr(c) => {
            collect_idents(&c.fun, out);
            for a in &c.args {
                collect_idents(a, out);
            }
        }
        Expr::ParenExpr(p) => collect_idents(&p.x, out),
        Expr::UnaryExpr(u) => collect_idents(&u.x, out),
        Expr::StarExpr(s) => collect_idents(&s.x, out),
        Expr::BinaryExpr(b) => {
            collect_idents(&b.x, out);
            collect_idents(&b.y, out);
        }
        Expr::IndexExpr(i) => {
            collect_idents(&i.x, out);
            collect_idents(&i.index, out);
        }
        Expr::SliceExpr(s) => {
            collect_idents(&s.x, out);
            if let Some(l) = &s.low {
                collect_idents(l, out);
            }
            if let Some(h) = &s.high {
                collect_idents(h, out);
            }
        }
        Expr::TypeAssertExpr(t) => collect_idents(&t.x, out),
        Expr::KeyValueExpr(kv) => {
            collect_idents(&kv.key, out);
            collect_idents(&kv.value, out);
        }
        Expr::CompositeLit(c) => {
            for elt in &c.elts {
                collect_idents(elt, out);
            }
        }
        Expr::FuncLit(f) => {
            for s in &f.body.list {
                out.extend(find_lhs(s));
                out.extend(find_rhs(s));
            }
        }
        _ => {}
    }
}

fn first_body_stmt(stmt: &Stmt) -> Option<&Stmt> {
    match stmt {
        Stmt::IfStmt(i) => i.body.list.first(),
        Stmt::RangeStmt(r) => r.body.list.first(),
        Stmt::ForStmt(f) => f.body.list.first(),
        Stmt::SwitchStmt(s) => {
            if let Some(Stmt::CaseClause(c)) = s.body.list.first() {
                c.body.first()
            } else {
                None
            }
        }
        Stmt::TypeSwitchStmt(s) => {
            if let Some(Stmt::CaseClause(c)) = s.body.list.first() {
                c.body.first()
            } else {
                None
            }
        }
        Stmt::DeferStmt(d) => {
            if let Expr::FuncLit(fl) = &*d.call.fun {
                fl.body.list.first()
            } else {
                None
            }
        }
        _ => None,
    }
}

fn is_assign_or_inc(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::AssignStmt(_) | Stmt::IncDecStmt(_))
}

fn is_allow_cuddle_call(names: &[String]) -> bool {
    names.iter().any(|n| n == "Lock" || n == "RLock")
}

fn is_allow_cuddle_rhs(names: &[String]) -> bool {
    names.iter().any(|n| n == "Unlock" || n == "RUnlock")
}

fn check_leading_trailing(fset: &FileSet, block: &BlockStmt, pending: &mut Vec<(u32, String)>) {
    if block.list.is_empty() {
        return;
    }
    let start = line(fset, Pos(block.lbrace.0 + 1));
    let end = line(fset, block.rbrace);
    if start == end {
        return;
    }
    let first = &block.list[0];
    if stmt_start(fset, first) > start + 1 {
        pending.push((block.lbrace.0 as u32 + 1, REASON_BLOCK_START.into()));
    }
    let last = block.list.last().unwrap();
    if end > stmt_end(fset, last) + 1 {
        pending.push((block.rbrace.0 as u32, REASON_BLOCK_END.into()));
    }
}

fn n_cuddled_before(fset: &FileSet, stmts: &[Stmt], i: usize, n: usize) -> bool {
    if i < n {
        return false;
    }
    for j in 1..n {
        let s1 = &stmts[i - j];
        let s2 = &stmts[i - (j + 1)];
        if stmt_start(fset, s1) - 1 != stmt_end(fset, s2) {
            return false;
        }
    }
    true
}

fn short_two_line_return(fset: &FileSet, stmts: &[Stmt], i: usize) -> bool {
    if stmts.len() != 2 || i != 1 {
        return false;
    }
    stmt_end(fset, &stmts[1]) - stmt_start(fset, &stmts[0]) <= 2
}

fn check_statements(fset: &FileSet, stmts: &[Stmt], pending: &mut Vec<(u32, String)>) {
    for (i, stmt) in stmts.iter().enumerate() {
        // Recurse into nested blocks (if/for/range/switch/func lits).
        match stmt {
            Stmt::IfStmt(ifs) => {
                check_block(fset, &ifs.body, pending);
                if let Some(else_) = &ifs.else_ {
                    match else_.as_ref() {
                        Stmt::BlockStmt(b) => check_block(fset, b, pending),
                        Stmt::IfStmt(e) => {
                            check_block(fset, &e.body, pending);
                            // Nested else-if chain: walk remaining else arms via recursion
                            // through the statements path when we hit the else if as a Stmt.
                            let mut cur = e.else_.as_deref();
                            while let Some(s) = cur {
                                match s {
                                    Stmt::BlockStmt(b) => {
                                        check_block(fset, b, pending);
                                        break;
                                    }
                                    Stmt::IfStmt(inner) => {
                                        check_block(fset, &inner.body, pending);
                                        cur = inner.else_.as_deref();
                                    }
                                    _ => break,
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Stmt::RangeStmt(r) => check_block(fset, &r.body, pending),
            Stmt::ForStmt(f) => check_block(fset, &f.body, pending),
            Stmt::SwitchStmt(s) => {
                for c in &s.body.list {
                    if let Stmt::CaseClause(cc) = c {
                        check_statements(fset, &cc.body, pending);
                    }
                }
            }
            Stmt::TypeSwitchStmt(s) => {
                for c in &s.body.list {
                    if let Stmt::CaseClause(cc) = c {
                        check_statements(fset, &cc.body, pending);
                    }
                }
            }
            Stmt::SelectStmt(s) => {
                for c in &s.body.list {
                    if let Stmt::CommClause(cc) = c {
                        check_statements(fset, &cc.body, pending);
                    }
                }
            }
            Stmt::AssignStmt(a) => {
                for r in &a.rhs {
                    if let Expr::FuncLit(fl) = r {
                        check_block(fset, &fl.body, pending);
                    }
                }
            }
            Stmt::ExprStmt(e) => {
                if let Expr::CallExpr(c) = &e.x {
                    if let Expr::FuncLit(fl) = &*c.fun {
                        check_block(fset, &fl.body, pending);
                    }
                }
            }
            Stmt::DeferStmt(d) => {
                if let Expr::FuncLit(fl) = &*d.call.fun {
                    check_block(fset, &fl.body, pending);
                }
            }
            Stmt::GoStmt(g) => {
                if let Expr::FuncLit(fl) = &*g.call.fun {
                    check_block(fset, &fl.body, pending);
                }
            }
            _ => {}
        }

        if i == 0 {
            continue;
        }
        let prev = &stmts[i - 1];
        let cuddled = stmt_end(fset, prev) + 1 == stmt_start(fset, stmt);
        if !cuddled {
            continue;
        }

        // Multi-line previous stmt: only AssignStmt may contribute LHS
        // (AllowMultiLineAssignCuddle=true, matching bombsimon/wsl v4 defaults).
        let cuddled_with_multiline =
            cuddled && stmt_start(fset, prev) != stmt_start(fset, stmt) - 1;
        let (assigned_above, called_above) = if !cuddled_with_multiline {
            (find_lhs(prev), find_rhs(prev))
        } else if matches!(prev, Stmt::AssignStmt(_)) {
            (find_lhs(prev), Vec::new())
        } else {
            (Vec::new(), Vec::new())
        };
        let lhs = find_lhs(stmt);
        let rhs = find_rhs(stmt);
        let both = {
            let mut v = lhs.clone();
            v.extend(rhs.clone());
            v
        };
        let called_or_assigned_above = {
            let mut v = called_above.clone();
            v.extend(assigned_above.clone());
            v
        };
        let first_in_block: Vec<String> = first_body_stmt(stmt)
            .map(|s| {
                let mut v = find_lhs(s);
                v.extend(find_rhs(s));
                v
            })
            .unwrap_or_default();

        if is_allow_cuddle_call(&called_above) || is_allow_cuddle_rhs(&rhs) {
            continue;
        }

        match stmt {
            Stmt::IfStmt(_) => {
                if assigned_above.is_empty() {
                    pending.push((stmt.pos().0 as u32, REASON_IF_ASSIGN.into()));
                    continue;
                }
                if n_cuddled_before(fset, stmts, i, 2) {
                    pending.push((stmt.pos().0 as u32, REASON_ONE_IF.into()));
                    continue;
                }
                if lists_overlap(&both, &assigned_above)
                    || lists_overlap(&assigned_above, &first_in_block)
                {
                    continue;
                }
                pending.push((stmt.pos().0 as u32, REASON_IF_USED.into()));
            }
            Stmt::ReturnStmt(_) => {
                if short_two_line_return(fset, stmts, i) {
                    continue;
                }
                pending.push((stmt.pos().0 as u32, REASON_RETURN.into()));
            }
            Stmt::BranchStmt(_) => {
                if short_two_line_return(fset, stmts, i) {
                    continue;
                }
                pending.push((stmt.pos().0 as u32, REASON_BRANCH.into()));
            }
            Stmt::AssignStmt(a) => {
                let is_append = a.rhs.iter().any(|e| {
                    matches!(e, Expr::CallExpr(c) if matches!(&*c.fun, Expr::Ident(id) if id.name == "append"))
                });
                if is_append {
                    if !lists_overlap(&called_or_assigned_above, &rhs) {
                        pending.push((stmt.pos().0 as u32, REASON_APPEND.into()));
                    }
                    continue;
                }
                if is_assign_or_inc(prev) {
                    continue;
                }
                if lists_overlap(&called_or_assigned_above, &both) {
                    continue;
                }
                pending.push((stmt.pos().0 as u32, REASON_ASSIGNS.into()));
            }
            Stmt::IncDecStmt(_) => {
                if is_assign_or_inc(prev) {
                    continue;
                }
                pending.push((stmt.pos().0 as u32, REASON_ASSIGNS.into()));
            }
            Stmt::DeclStmt(_) => {
                pending.push((stmt.pos().0 as u32, REASON_DECL.into()));
            }
            Stmt::ExprStmt(_) => {
                if matches!(prev, Stmt::DeclStmt(_) | Stmt::ReturnStmt(_)) {
                    pending.push((stmt.pos().0 as u32, REASON_EXPR_DECL.into()));
                    continue;
                }
                if matches!(
                    prev,
                    Stmt::IfStmt(_) | Stmt::RangeStmt(_) | Stmt::SwitchStmt(_) | Stmt::ForStmt(_)
                ) {
                    pending.push((stmt.pos().0 as u32, REASON_EXPR_BLOCK.into()));
                    continue;
                }
                if lists_overlap(&called_or_assigned_above, &both) {
                    continue;
                }
                if !assigned_above.is_empty() && !lists_overlap(&both, &assigned_above) {
                    pending.push((stmt.pos().0 as u32, REASON_EXPR_UNUSED.into()));
                }
            }
            Stmt::DeferStmt(_) => {
                if matches!(prev, Stmt::DeferStmt(_)) {
                    continue;
                }
                if n_cuddled_before(fset, stmts, i, 2) {
                    pending.push((stmt.pos().0 as u32, REASON_ONE_DEFER.into()));
                    continue;
                }
                if lists_overlap(&rhs, &called_above)
                    || lists_overlap(&assigned_above, &first_in_block)
                    || lists_overlap(&both, &assigned_above)
                {
                    continue;
                }
                pending.push((stmt.pos().0 as u32, REASON_DEFER.into()));
            }
            Stmt::RangeStmt(_) => {
                if n_cuddled_before(fset, stmts, i, 2) {
                    pending.push((stmt.pos().0 as u32, REASON_ONE_RANGE.into()));
                    continue;
                }
                if lists_overlap(&both, &assigned_above)
                    || lists_overlap(&assigned_above, &first_in_block)
                {
                    continue;
                }
                pending.push((stmt.pos().0 as u32, REASON_RANGE.into()));
            }
            Stmt::ForStmt(_) => {
                if n_cuddled_before(fset, stmts, i, 2) {
                    pending.push((stmt.pos().0 as u32, REASON_ONE_FOR.into()));
                    continue;
                }
                if lists_overlap(&both, &assigned_above)
                    || lists_overlap(&assigned_above, &first_in_block)
                {
                    continue;
                }
                pending.push((stmt.pos().0 as u32, REASON_FOR.into()));
            }
            Stmt::SwitchStmt(s) => {
                if s.tag.is_none() {
                    pending.push((stmt.pos().0 as u32, REASON_ANON_SWITCH.into()));
                    continue;
                }
                if !lists_overlap(&both, &assigned_above) {
                    pending.push((stmt.pos().0 as u32, REASON_SWITCH.into()));
                }
            }
            _ => {}
        }
    }
}

fn check_block(fset: &FileSet, block: &BlockStmt, pending: &mut Vec<(u32, String)>) {
    check_leading_trailing(fset, block, pending);
    check_statements(fset, &block.list, pending);
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "wsl requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    let fset = pass.fset().clone();
    for file in pass.files() {
        for decl in &file.decls {
            if let Decl::FuncDecl(f) = decl {
                if let Some(body) = &f.body {
                    check_block(&fset, body, &mut pending);
                }
            }
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "wsl",
        doc: "add or remove empty lines",
        url: "https://github.com/bombsimon/wsl",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
