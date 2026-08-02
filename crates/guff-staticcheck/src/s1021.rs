//! S1021 — merge variable declaration with assignment on the next line.
//!
//! Port of `honnef.co/go/tools/simple/s1021`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, BlockStmt, DeclStmt, Expr, GenDecl, Stmt, ValueSpec};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{object_of, refers_to};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn count_assignments_to(pass: &Pass<'_>, stmt: &Stmt, obj: guff_types::arena::ObjectId) -> usize {
    match stmt {
        Stmt::AssignStmt(assign) => {
            let mut n = 0usize;
            for lhs in &assign.lhs {
                let Expr::Ident(ident) = lhs else {
                    continue;
                };
                if object_of(pass, ident) == Some(obj) {
                    n += 1;
                }
            }
            n
        }
        Stmt::IfStmt(i) => {
            let mut n = 0usize;
            if let Some(init) = &i.init {
                n += count_assignments_to(pass, init, obj);
            }
            for s in &i.body.list {
                n += count_assignments_to(pass, s, obj);
            }
            if let Some(else_) = &i.else_ {
                n += count_assignments_to(pass, else_, obj);
            }
            n
        }
        Stmt::BlockStmt(b) => b
            .list
            .iter()
            .map(|s| count_assignments_to(pass, s, obj))
            .sum(),
        Stmt::ForStmt(f) => {
            let mut n = 0usize;
            if let Some(init) = &f.init {
                n += count_assignments_to(pass, init, obj);
            }
            if let Some(post) = &f.post {
                n += count_assignments_to(pass, post, obj);
            }
            for s in &f.body.list {
                n += count_assignments_to(pass, s, obj);
            }
            n
        }
        Stmt::RangeStmt(r) => r
            .body
            .list
            .iter()
            .map(|s| count_assignments_to(pass, s, obj))
            .sum(),
        Stmt::SwitchStmt(s) => {
            let mut n = 0usize;
            if let Some(init) = &s.init {
                n += count_assignments_to(pass, init, obj);
            }
            for c in &s.body.list {
                let Stmt::CaseClause(cc) = c else { continue };
                for s in &cc.body {
                    n += count_assignments_to(pass, s, obj);
                }
            }
            n
        }
        Stmt::TypeSwitchStmt(s) => {
            let mut n = 0usize;
            if let Some(init) = &s.init {
                n += count_assignments_to(pass, init, obj);
            }
            // assign is DeclStmt / AssignStmt on the switch
            n += count_assignments_to(pass, &s.assign, obj);
            for c in &s.body.list {
                let Stmt::CaseClause(cc) = c else { continue };
                for s in &cc.body {
                    n += count_assignments_to(pass, s, obj);
                }
            }
            n
        }
        _ => 0,
    }
}

fn has_multiple_assignments(pass: &Pass<'_>, block: &BlockStmt, obj: guff_types::arena::ObjectId) -> bool {
    let mut count = 0usize;
    for stmt in &block.list {
        count += count_assignments_to(pass, stmt, obj);
        if count >= 2 {
            return true;
        }
    }
    false
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
        let guff::ast::Decl::GenDecl(GenDecl { tok, specs, .. }) = decl else {
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
        out.push((
            names[0].name_pos.0 as u32,
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
