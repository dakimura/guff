//! SA4005 — field assignment that will never be observed
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4005`.
//!
//! Uses AST only: SSA field-index diagnostics were noisy under hybrid IR
//! (`.8`-style messages) and missed reads that make value-receiver writes
//! observable (method calls, composite literals, return values).

use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, SelectorExpr, Stmt};
use guff::walk::{expr_ref, preorder, NodeRef};
use guff_analysis::code::{object_of, refers_to};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn is_value_receiver(recv: &guff::ast::FieldList) -> bool {
    recv.list.first().is_some_and(|f| {
        match f.ty.as_ref() {
            Some(Expr::StarExpr(_)) => false,
            Some(Expr::UnaryExpr(u)) if u.op == guff::token::Token::MUL => false,
            _ => true,
        }
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4005 requires inspect analyzer".to_string())?
        .clone();
    let mut pending = Vec::new();

    inspect.preorder(pass.files(), |node| {
        let NodeRef::FuncDecl(fd) = node else {
            return;
        };
        let Some(recv) = fd.recv.as_ref() else {
            return;
        };
        if !is_value_receiver(recv) {
            return;
        }
        let Some(recv_name) = recv.list.first().and_then(|f| f.names.first()) else {
            return;
        };
        let Some(recv_obj) = object_of(pass, recv_name) else {
            return;
        };
        let Some(body) = fd.body.as_ref() else {
            return;
        };
        let mut stores: Vec<(u32, String)> = Vec::new();
        let mut reads: Vec<String> = Vec::new();
        for stmt in &body.list {
            walk_stmt(pass, stmt, recv_obj, &mut stores, &mut reads);
        }
        for (pos, field) in stores {
            if reads.iter().any(|f| f == "*" || f == &field) {
                continue;
            }
            pending.push((pos, format!("ineffective assignment to field .{field}")));
        }
    });

    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn walk_stmt(
    pass: &Pass<'_>,
    stmt: &Stmt,
    recv_obj: guff_types::ObjectId,
    stores: &mut Vec<(u32, String)>,
    reads: &mut Vec<String>,
) {
    match stmt {
        Stmt::AssignStmt(AssignStmt { lhs, rhs, .. }) => {
            for lhs in lhs {
                if let Some((pos, field)) = selector_field_on(pass, lhs, recv_obj) {
                    stores.push((pos, field));
                }
            }
            for r in rhs {
                collect_reads(pass, r, recv_obj, reads);
            }
        }
        Stmt::ExprStmt(es) => collect_reads(pass, &es.x, recv_obj, reads),
        Stmt::ReturnStmt(r) => {
            for e in &r.results {
                collect_reads(pass, e, recv_obj, reads);
            }
        }
        Stmt::IfStmt(i) => {
            if let Some(init) = &i.init {
                walk_stmt(pass, init, recv_obj, stores, reads);
            }
            collect_reads(pass, &i.cond, recv_obj, reads);
            for s in &i.body.list {
                walk_stmt(pass, s, recv_obj, stores, reads);
            }
            if let Some(else_) = &i.else_ {
                walk_stmt(pass, else_, recv_obj, stores, reads);
            }
        }
        Stmt::ForStmt(f) => {
            if let Some(init) = &f.init {
                walk_stmt(pass, init, recv_obj, stores, reads);
            }
            if let Some(cond) = &f.cond {
                collect_reads(pass, cond, recv_obj, reads);
            }
            if let Some(post) = &f.post {
                walk_stmt(pass, post, recv_obj, stores, reads);
            }
            for s in &f.body.list {
                walk_stmt(pass, s, recv_obj, stores, reads);
            }
        }
        Stmt::RangeStmt(r) => {
            collect_reads(pass, &r.x, recv_obj, reads);
            for s in &r.body.list {
                walk_stmt(pass, s, recv_obj, stores, reads);
            }
        }
        Stmt::SwitchStmt(s) => {
            if let Some(init) = &s.init {
                walk_stmt(pass, init, recv_obj, stores, reads);
            }
            if let Some(tag) = &s.tag {
                collect_reads(pass, tag, recv_obj, reads);
            }
            for c in &s.body.list {
                let Stmt::CaseClause(cc) = c else { continue };
                for e in &cc.list {
                    collect_reads(pass, e, recv_obj, reads);
                }
                for s in &cc.body {
                    walk_stmt(pass, s, recv_obj, stores, reads);
                }
            }
        }
        Stmt::BlockStmt(b) => {
            for s in &b.list {
                walk_stmt(pass, s, recv_obj, stores, reads);
            }
        }
        Stmt::GoStmt(g) => {
            preorder(NodeRef::CallExpr(&g.call), |n| {
                collect_reads_node(pass, n, recv_obj, reads);
                true
            });
        }
        Stmt::DeferStmt(d) => {
            preorder(NodeRef::CallExpr(&d.call), |n| {
                collect_reads_node(pass, n, recv_obj, reads);
                true
            });
        }
        Stmt::SendStmt(s) => {
            collect_reads(pass, &s.chan_, recv_obj, reads);
            collect_reads(pass, &s.value, recv_obj, reads);
        }
        Stmt::IncDecStmt(i) => collect_reads(pass, &i.x, recv_obj, reads),
        _ => {}
    }
}

/// Collect field reads and treat any other use of the receiver value (method
/// call, passing `h` to a function, etc.) as observing all prior stores.
fn collect_reads(
    pass: &Pass<'_>,
    expr: &Expr,
    recv_obj: guff_types::ObjectId,
    reads: &mut Vec<String>,
) {
    preorder(expr_ref(expr), |n| {
        collect_reads_node(pass, n, recv_obj, reads);
        true
    });
}

fn collect_reads_node(
    pass: &Pass<'_>,
    n: NodeRef<'_>,
    recv_obj: guff_types::ObjectId,
    reads: &mut Vec<String>,
) {
    match n {
        NodeRef::SelectorExpr(sel) => {
            if refers_to(pass, &sel.x, recv_obj) {
                reads.push(sel.sel.name.clone());
            }
        }
        NodeRef::Ident(id) => {
            if object_of(pass, id) == Some(recv_obj) {
                // Bare use of the receiver (method call, argument, return).
                reads.push("*".into());
            }
        }
        _ => {}
    }
}

fn selector_field_on(
    pass: &Pass<'_>,
    expr: &Expr,
    recv_obj: guff_types::ObjectId,
) -> Option<(u32, String)> {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = expr else {
        return None;
    };
    if !refers_to(pass, x, recv_obj) {
        return None;
    }
    Some((sel.name_pos.0 as u32, sel.name.clone()))
}

fn sa4005_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4005",
        doc: "field assignment that will never be observed",
        url: "https://staticcheck.dev/docs/checks/#SA4005",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4005_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4005_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
