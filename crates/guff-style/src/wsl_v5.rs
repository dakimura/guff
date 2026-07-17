//! Port of [`github.com/bombsimon/wsl/v5`](https://github.com/bombsimon/wsl)
//! (golangci-lint wrapper `wsl_v5`).
//!
//! Implements default cuddle checks with v5 message format and core settings
//! (`allow-first-in-block` / `allow-whole-block` / `branch-max-lines` /
//! `cuddle-max-statements` / `default`+`enable`/`disable`).
//!
//! DEFERRED: SuggestedFix; `case-max-lines` trailing newlines; `after-block` /
//! `after-decl` / `after-defer` / `after-expr` / `after-go`; `assign-exclusive` /
//! `assign-expr` / `cuddle-group`; full `err` type Implements; decl grouping
//! (`maybeGroupDecl`); Lock/Unlock heuristics; comment-map nuance.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{BlockStmt, Decl, Expr, Spec, Stmt};
use guff::position::{FileSet, Pos};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::{WslV5Check, WslV5Options};

const MSG_ABOVE: &str = "missing whitespace above this line";
const MSG_REMOVE: &str = "unnecessary whitespace";
const MSG_BLOCK_START: &str = "unnecessary whitespace (leading-whitespace)";
const MSG_BLOCK_END: &str = "unnecessary whitespace (trailing-whitespace)";

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
        Stmt::AssignStmt(a) => a.lhs.iter().flat_map(expr_idents).collect(),
        Stmt::IncDecStmt(i) => expr_idents(&i.x),
        Stmt::DeclStmt(d) => match &d.decl {
            Decl::GenDecl(g) => g
                .specs
                .iter()
                .flat_map(|sp| match sp {
                    Spec::ValueSpec(vs) => {
                        vs.names.iter().map(|n| n.name.clone()).collect::<Vec<_>>()
                    }
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
        Stmt::TypeSwitchStmt(s) => {
            let mut v = find_lhs(&s.assign);
            v.extend(find_rhs(&s.assign));
            v
        }
        Stmt::SelectStmt(_) => Vec::new(),
        Stmt::SendStmt(s) => {
            let mut v = expr_idents(&s.chan_);
            v.extend(expr_idents(&s.value));
            v
        }
        Stmt::IncDecStmt(i) => expr_idents(&i.x),
        Stmt::LabeledStmt(l) => find_rhs(&l.stmt),
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
        Stmt::SelectStmt(s) => {
            if let Some(Stmt::CommClause(c)) = s.body.list.first() {
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
        Stmt::GoStmt(g) => {
            if let Expr::FuncLit(fl) = &*g.call.fun {
                fl.body.list.first()
            } else {
                None
            }
        }
        _ => None,
    }
}

fn whole_block_idents(stmt: &Stmt) -> Vec<String> {
    let mut out = find_lhs(stmt);
    out.extend(find_rhs(stmt));
    match stmt {
        Stmt::IfStmt(i) => {
            for s in &i.body.list {
                out.extend(find_lhs(s));
                out.extend(find_rhs(s));
            }
        }
        Stmt::RangeStmt(r) => {
            for s in &r.body.list {
                out.extend(find_lhs(s));
                out.extend(find_rhs(s));
            }
        }
        Stmt::ForStmt(f) => {
            for s in &f.body.list {
                out.extend(find_lhs(s));
                out.extend(find_rhs(s));
            }
        }
        _ => {}
    }
    out
}

fn is_assign_or_inc(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::AssignStmt(_) | Stmt::IncDecStmt(_))
}

fn is_assign_decl_or_inc(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::AssignStmt(_) | Stmt::DeclStmt(_) | Stmt::IncDecStmt(_)
    )
}

fn cuddle_target(
    stmt: &Stmt,
    options: &WslV5Options,
) -> Vec<String> {
    if options.allow_whole_block {
        return whole_block_idents(stmt);
    }
    let mut v = find_lhs(stmt);
    v.extend(find_rhs(stmt));
    if options.allow_first_in_block {
        if let Some(first) = first_body_stmt(stmt) {
            v.extend(find_lhs(first));
            v.extend(find_rhs(first));
        }
    }
    v
}

fn n_cuddled_before(fset: &FileSet, stmts: &[Stmt], i: usize) -> usize {
    let mut n = 0;
    let mut cur = i;
    while cur > 0 {
        let prev = &stmts[cur - 1];
        let curr = &stmts[cur];
        if stmt_end(fset, prev) + 1 != stmt_start(fset, curr) {
            break;
        }
        n += 1;
        cur -= 1;
    }
    n
}

fn check_enabled(options: &WslV5Options, check: WslV5Check) -> bool {
    options.checks.contains(&check)
}

fn msg_invalid(check: &str) -> String {
    format!("{MSG_ABOVE} (invalid statement above {check})")
}

fn msg_no_shared(check: &str) -> String {
    format!("{MSG_ABOVE} (no shared variables above {check})")
}

fn msg_too_many(check: &str) -> String {
    format!("{MSG_ABOVE} (too many statements above {check})")
}

fn msg_too_many_lines(check: &str) -> String {
    format!("{MSG_ABOVE} (too many lines above {check})")
}

fn msg_never(check: &str) -> String {
    format!("{MSG_ABOVE} (never cuddle {check})")
}

fn is_err_not_nil_check(stmt: &Stmt) -> Option<String> {
    let Stmt::IfStmt(ifs) = stmt else {
        return None;
    };
    if ifs.init.is_some() {
        return None;
    }
    let Expr::BinaryExpr(b) = &ifs.cond else {
        return None;
    };
    if b.op != guff::token::Token::NEQ {
        return None;
    }
    // err != nil  /  nil != err
    let (err_side, nil_side) = if matches!(&*b.y, Expr::Ident(id) if id.name == "nil") {
        (&*b.x, &*b.y)
    } else if matches!(&*b.x, Expr::Ident(id) if id.name == "nil") {
        (&*b.y, &*b.x)
    } else {
        return None;
    };
    let _ = nil_side;
    let Expr::Ident(err) = err_side else {
        return None;
    };
    // Upstream checks Implements(error); we approximate with name "err" / "*err*".
    if err.name == "err" || err.name.ends_with("Err") || err.name.starts_with("err") {
        Some(err.name.clone())
    } else {
        None
    }
}

fn assign_defines_err(stmt: &Stmt, err_name: &str) -> bool {
    find_lhs(stmt).iter().any(|n| n == err_name)
}

fn check_leading_trailing(
    fset: &FileSet,
    block: &BlockStmt,
    options: &WslV5Options,
    pending: &mut Vec<(u32, String)>,
) {
    if block.list.is_empty() {
        return;
    }
    let start = line(fset, Pos(block.lbrace.0 + 1));
    let end = line(fset, block.rbrace);
    if start == end {
        return;
    }
    let first = &block.list[0];
    if check_enabled(options, WslV5Check::LeadingWhitespace)
        && stmt_start(fset, first) > start + 1
    {
        pending.push((block.lbrace.0 as u32 + 1, MSG_BLOCK_START.into()));
    }
    let last = block.list.last().unwrap();
    if check_enabled(options, WslV5Check::TrailingWhitespace)
        && end > stmt_end(fset, last) + 1
    {
        pending.push((block.rbrace.0 as u32, MSG_BLOCK_END.into()));
    }
}

fn check_err_cuddle(
    fset: &FileSet,
    stmts: &[Stmt],
    i: usize,
    options: &WslV5Options,
    pending: &mut Vec<(u32, String)>,
) {
    if !check_enabled(options, WslV5Check::Err) || i == 0 {
        return;
    }
    let stmt = &stmts[i];
    let Some(err_name) = is_err_not_nil_check(stmt) else {
        return;
    };
    let prev = &stmts[i - 1];
    if !assign_defines_err(prev, &err_name) {
        return;
    }
    // Gap between err assign and if → remove whitespace.
    if stmt_end(fset, prev) + 1 < stmt_start(fset, stmt) {
        pending.push((
            prev.pos().0 as u32,
            format!("{MSG_REMOVE} (err)"),
        ));
    }
}

fn check_cuddle_blockish(
    fset: &FileSet,
    stmts: &[Stmt],
    i: usize,
    check_name: &str,
    options: &WslV5Options,
    pending: &mut Vec<(u32, String)>,
) {
    let stmt = &stmts[i];
    let prev = &stmts[i - 1];
    let n_above = n_cuddled_before(fset, stmts, i);
    if n_above == 0 {
        return;
    }

    if !is_assign_decl_or_inc(prev)
        && !matches!(stmt, Stmt::DeferStmt(_) | Stmt::GoStmt(_))
    {
        pending.push((stmt.pos().0 as u32, msg_invalid(check_name)));
        return;
    }

    let assigned_above = find_lhs(prev);
    let target = cuddle_target(stmt, options);
    if !lists_overlap(&assigned_above, &target)
        && !lists_overlap(&find_rhs(prev), &target)
    {
        // Decl above may also contribute names via find_lhs.
        pending.push((stmt.pos().0 as u32, msg_no_shared(check_name)));
        return;
    }

    // err check always wins over cuddle-max when enabled.
    if check_enabled(options, WslV5Check::Err) {
        if let Some(err_name) = is_err_not_nil_check(stmt) {
            if assign_defines_err(prev, &err_name) {
                if n_above > 1 {
                    if let Some(extra) = stmts.get(i.saturating_sub(2)) {
                        pending.push((extra.pos().0 as u32, msg_too_many(check_name)));
                    }
                }
                return;
            }
        }
    }

    let max = options.cuddle_max_statements;
    if max == 0 {
        pending.push((stmt.pos().0 as u32, msg_too_many(check_name)));
        return;
    }
    if n_above > max {
        let idx = i - max;
        pending.push((stmts[idx].pos().0 as u32, msg_too_many(check_name)));
    }
}

fn check_statements(
    fset: &FileSet,
    stmts: &[Stmt],
    options: &WslV5Options,
    pending: &mut Vec<(u32, String)>,
) {
    for (i, stmt) in stmts.iter().enumerate() {
        match stmt {
            Stmt::IfStmt(ifs) => {
                check_block(fset, &ifs.body, options, pending);
                if let Some(else_) = &ifs.else_ {
                    walk_else(fset, else_, options, pending);
                }
            }
            Stmt::RangeStmt(r) => check_block(fset, &r.body, options, pending),
            Stmt::ForStmt(f) => check_block(fset, &f.body, options, pending),
            Stmt::SwitchStmt(s) => {
                for c in &s.body.list {
                    if let Stmt::CaseClause(cc) = c {
                        check_statements(fset, &cc.body, options, pending);
                    }
                }
            }
            Stmt::TypeSwitchStmt(s) => {
                for c in &s.body.list {
                    if let Stmt::CaseClause(cc) = c {
                        check_statements(fset, &cc.body, options, pending);
                    }
                }
            }
            Stmt::SelectStmt(s) => {
                for c in &s.body.list {
                    if let Stmt::CommClause(cc) = c {
                        check_statements(fset, &cc.body, options, pending);
                    }
                }
            }
            Stmt::AssignStmt(a) => {
                for r in &a.rhs {
                    if let Expr::FuncLit(fl) = r {
                        check_block(fset, &fl.body, options, pending);
                    }
                }
            }
            Stmt::ExprStmt(e) => {
                if let Expr::CallExpr(c) = &e.x {
                    if let Expr::FuncLit(fl) = &*c.fun {
                        check_block(fset, &fl.body, options, pending);
                    }
                }
            }
            Stmt::DeferStmt(d) => {
                if let Expr::FuncLit(fl) = &*d.call.fun {
                    check_block(fset, &fl.body, options, pending);
                }
            }
            Stmt::GoStmt(g) => {
                if let Expr::FuncLit(fl) = &*g.call.fun {
                    check_block(fset, &fl.body, options, pending);
                }
            }
            Stmt::LabeledStmt(l) => {
                // Labels recurse into labeled statement.
                check_statements(fset, std::slice::from_ref(l.stmt.as_ref()), options, pending);
            }
            _ => {}
        }

        if i == 0 {
            continue;
        }
        check_err_cuddle(fset, stmts, i, options, pending);

        let prev = &stmts[i - 1];
        let cuddled = stmt_end(fset, prev) + 1 == stmt_start(fset, stmt);
        if !cuddled {
            continue;
        }

        match stmt {
            Stmt::IfStmt(_) if check_enabled(options, WslV5Check::If) => {
                check_cuddle_blockish(fset, stmts, i, "if", options, pending);
            }
            Stmt::ReturnStmt(_) if check_enabled(options, WslV5Check::Return) => {
                if stmts.is_empty() {
                    continue;
                }
                let first = &stmts[0];
                let last = stmts.last().unwrap();
                if stmt_end(fset, last) - stmt_start(fset, first) < options.branch_max_lines as i64
                {
                    continue;
                }
                pending.push((stmt.pos().0 as u32, msg_too_many_lines("return")));
            }
            Stmt::BranchStmt(_) if check_enabled(options, WslV5Check::Branch) => {
                let first = &stmts[0];
                let last = stmts.last().unwrap();
                if stmt_end(fset, last) - stmt_start(fset, first) < options.branch_max_lines as i64
                {
                    continue;
                }
                pending.push((stmt.pos().0 as u32, msg_too_many_lines("branch")));
            }
            Stmt::AssignStmt(a) => {
                let is_append = a.rhs.iter().any(|e| {
                    matches!(
                        e,
                        Expr::CallExpr(c)
                            if matches!(&*c.fun, Expr::Ident(id) if id.name == "append")
                    )
                });
                if is_append && check_enabled(options, WslV5Check::Append) {
                    let assigned_above = find_lhs(prev);
                    let called_above = find_rhs(prev);
                    let rhs = find_rhs(stmt);
                    let mut above = assigned_above;
                    above.extend(called_above);
                    if !lists_overlap(&above, &rhs) {
                        pending.push((stmt.pos().0 as u32, msg_no_shared("append")));
                    }
                    continue;
                }
                if !check_enabled(options, WslV5Check::Assign) {
                    continue;
                }
                if is_assign_or_inc(prev) {
                    continue;
                }
                if matches!(prev, Stmt::ExprStmt(_)) {
                    // Consecutive expr→assign is invalid under default assign.
                    pending.push((stmt.pos().0 as u32, msg_invalid("assign")));
                    continue;
                }
                if is_assign_decl_or_inc(prev) {
                    continue;
                }
                // After blocks etc.
                if matches!(
                    prev,
                    Stmt::IfStmt(_)
                        | Stmt::ForStmt(_)
                        | Stmt::RangeStmt(_)
                        | Stmt::SwitchStmt(_)
                        | Stmt::TypeSwitchStmt(_)
                        | Stmt::SelectStmt(_)
                ) {
                    pending.push((stmt.pos().0 as u32, msg_invalid("assign")));
                }
            }
            Stmt::IncDecStmt(_) if check_enabled(options, WslV5Check::IncDec) => {
                if is_assign_or_inc(prev) {
                    continue;
                }
                pending.push((stmt.pos().0 as u32, msg_invalid("inc-dec")));
            }
            Stmt::DeclStmt(_) if check_enabled(options, WslV5Check::Decl) => {
                // Simplified: never cuddle decl (grouping DEFERRED).
                pending.push((stmt.pos().0 as u32, msg_never("decl")));
            }
            Stmt::ExprStmt(_) if check_enabled(options, WslV5Check::Expr) => {
                if matches!(prev, Stmt::ExprStmt(_)) {
                    continue;
                }
                check_cuddle_blockish(fset, stmts, i, "expr", options, pending);
            }
            Stmt::DeferStmt(_) if check_enabled(options, WslV5Check::Defer) => {
                if matches!(prev, Stmt::DeferStmt(_)) {
                    continue;
                }
                // Allow: assign / if-err / defer chain with shared var.
                if matches!(prev, Stmt::IfStmt(_)) {
                    let n = n_cuddled_before(fset, stmts, i);
                    if n >= 2 {
                        let before_if = &stmts[i - 2];
                        let target = find_rhs(stmt);
                        if lists_overlap(&find_lhs(before_if), &target)
                            || lists_overlap(&find_rhs(before_if), &target)
                        {
                            continue;
                        }
                    }
                }
                check_cuddle_blockish(fset, stmts, i, "defer", options, pending);
            }
            Stmt::GoStmt(_) if check_enabled(options, WslV5Check::Go) => {
                if matches!(prev, Stmt::GoStmt(_)) {
                    continue;
                }
                check_cuddle_blockish(fset, stmts, i, "go", options, pending);
            }
            Stmt::RangeStmt(_) if check_enabled(options, WslV5Check::Range) => {
                check_cuddle_blockish(fset, stmts, i, "range", options, pending);
            }
            Stmt::ForStmt(_) if check_enabled(options, WslV5Check::For) => {
                check_cuddle_blockish(fset, stmts, i, "for", options, pending);
            }
            Stmt::SwitchStmt(_) if check_enabled(options, WslV5Check::Switch) => {
                check_cuddle_blockish(fset, stmts, i, "switch", options, pending);
            }
            Stmt::TypeSwitchStmt(_) if check_enabled(options, WslV5Check::TypeSwitch) => {
                check_cuddle_blockish(fset, stmts, i, "type-switch", options, pending);
            }
            Stmt::SelectStmt(_) if check_enabled(options, WslV5Check::Select) => {
                check_cuddle_blockish(fset, stmts, i, "select", options, pending);
            }
            Stmt::SendStmt(_) if check_enabled(options, WslV5Check::Send) => {
                check_cuddle_blockish(fset, stmts, i, "send", options, pending);
            }
            Stmt::LabeledStmt(_) if check_enabled(options, WslV5Check::Label) => {
                pending.push((stmt.pos().0 as u32, msg_never("label")));
            }
            _ => {}
        }
    }
}

fn walk_else(fset: &FileSet, else_: &Stmt, options: &WslV5Options, pending: &mut Vec<(u32, String)>) {
    match else_ {
        Stmt::BlockStmt(b) => check_block(fset, b, options, pending),
        Stmt::IfStmt(e) => {
            check_block(fset, &e.body, options, pending);
            if let Some(next) = &e.else_ {
                walk_else(fset, next, options, pending);
            }
        }
        _ => {}
    }
}

fn check_block(
    fset: &FileSet,
    block: &BlockStmt,
    options: &WslV5Options,
    pending: &mut Vec<(u32, String)>,
) {
    check_leading_trailing(fset, block, options, pending);
    check_statements(fset, &block.list, options, pending);
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "wsl_v5 requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<WslV5Options>("wsl_v5")
        .cloned()
        .unwrap_or_default();

    let mut pending = Vec::new();
    let fset = pass.fset().clone();
    for file in pass.files() {
        for decl in &file.decls {
            if let Decl::FuncDecl(f) = decl {
                if let Some(body) = &f.body {
                    check_block(&fset, body, &options, &mut pending);
                }
            }
        }
        // Also walk package-level FuncLit assigned in vars? Upstream walks
        // FuncLit via Inspect; we cover nested FuncLits inside function bodies.
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "wsl_v5",
        doc: "add or remove empty lines",
        url: "https://github.com/bombsimon/wsl",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_checks_include_if_and_err() {
        let o = WslV5Options::default();
        assert!(o.checks.contains(&WslV5Check::If));
        assert!(o.checks.contains(&WslV5Check::Err));
        assert!(!o.checks.contains(&WslV5Check::AfterBlock));
        assert!(o.allow_first_in_block);
        assert!(!o.allow_whole_block);
        assert_eq!(o.branch_max_lines, 2);
        assert_eq!(o.cuddle_max_statements, 1);
    }

    #[test]
    fn analyzer_name() {
        assert_eq!(analyzer().name, "wsl_v5");
    }
}
