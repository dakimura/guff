//! S1021 — merge variable declaration with assignment on the next line.
//!
//! Port of `honnef.co/go/tools/simple/s1021`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, BlockStmt, DeclStmt, Expr, GenDecl, Stmt, ValueSpec};
use guff::node_mask;
use guff::token::Token;
use guff::walk::{inspect as walk_inspect, NodeRef};
use guff_analysis::code::{object_of, refers_to};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

/// Upstream `hasMultipleAssignments`: a full `ast.Inspect` over the block,
/// counting every `AssignStmt` whose LHS mentions `obj`. A hand-rolled
/// statement walker misses shapes like `select { case <-c: err = f() }`
/// (prometheus `discovery/kubernetes.retryOnError`) and then reports a merge
/// that would change behaviour.
fn has_multiple_assignments(
    pass: &Pass<'_>,
    block: &BlockStmt,
    obj: guff_types::arena::ObjectId,
) -> bool {
    let mut num = 0usize;
    walk_inspect(NodeRef::BlockStmt(block), |node| {
        let Some(node) = node else {
            return true;
        };
        if num >= 2 {
            return false;
        }
        let NodeRef::AssignStmt(assign) = node else {
            return true;
        };
        for lhs in &assign.lhs {
            let Expr::Ident(ident) = lhs else {
                continue;
            };
            if object_of(pass, ident) == Some(obj) {
                num += 1;
            }
        }
        true
    });
    num >= 2
}

fn check_block(pass: &Pass<'_>, block: &BlockStmt) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    if block.list.len() < 2 {
        return out;
    }
    for i in 0..block.list.len() - 1 {
        let Stmt::DeclStmt(DeclStmt { decl, .. }) = &block.list[i] else {
            continue;
        };
        let guff::ast::Decl::GenDecl(GenDecl { tok_pos, tok, specs, .. }) = decl else {
            continue;
        };
        if *tok != Some(Token::VAR) || specs.len() != 1 {
            continue;
        }
        let guff::ast::Spec::ValueSpec(ValueSpec { names, ty, values, .. }) = &specs[0] else {
            continue;
        };
        if names.len() != 1 || ty.is_none() || !values.is_empty() {
            continue;
        }
        let Stmt::AssignStmt(AssignStmt { tok: assign_tok, lhs, rhs, .. }) = &block.list[i + 1]
        else {
            continue;
        };
        if *assign_tok != Some(Token::ASSIGN) || lhs.len() != 1 || rhs.len() != 1 {
            continue;
        }
        let Expr::Ident(lhs_id) = &lhs[0] else {
            continue;
        };
        let Some(decl_obj) = object_of(pass, &names[0]) else {
            continue;
        };
        let Some(lhs_obj) = object_of(pass, lhs_id) else {
            continue;
        };
        if decl_obj != lhs_obj {
            continue;
        }
        if refers_to(pass, &rhs[0], lhs_obj) {
            continue;
        }
        if has_multiple_assignments(pass, block, decl_obj) {
            continue;
        }
        // Upstream reports on `decl`, i.e. the `var` keyword — not the name.
        out.push((
            tok_pos.0 as u32,
            "should merge variable declaration with assignment on next line".into(),
        ));
    }
    out
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1021 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(BlockStmt), pass.files(), |n| {
        let NodeRef::BlockStmt(block) = n else {
            return;
        };
        pending.extend(check_block(pass, block));
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1021_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1021",
        doc: "merge variable declaration and assignment",
        url: "https://staticcheck.dev/docs/checks/#S1021",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1021_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1021_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
