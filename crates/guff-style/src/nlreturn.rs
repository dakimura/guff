//! Port of [`github.com/ssgreg/nlreturn`](https://github.com/ssgreg/nlreturn)
//! (golangci-lint wrapper in `pkg/golinters/nlreturn`).
//!
//! Default matches golangci-lint / upstream: `block-size=1`.
//!
//! DEFERRED: `linters.settings.nlreturn` wiring (`block-size`); SuggestedFix.

use std::sync::OnceLock;

use guff::ast::Stmt;
use guff::position::FileSet;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

/// golangci-lint / upstream default for `linters.settings.nlreturn.block-size`.
const BLOCK_SIZE: i64 = 1;

fn line(fset: &FileSet, pos: guff::position::Pos) -> i64 {
    fset.position(pos).line
}

fn branch_name(stmt: &Stmt) -> &'static str {
    match stmt {
        Stmt::ReturnStmt(_) => "return",
        Stmt::BranchStmt(b) => match b.tok {
            Token::BREAK => "break",
            Token::CONTINUE => "continue",
            Token::GOTO => "goto",
            Token::FALLTHROUGH => "fallthrough",
            _ => "unknown",
        },
        _ => "unknown",
    }
}

fn inspect_block(fset: &FileSet, block: &[Stmt], pending: &mut Vec<(u32, String)>) {
    for (i, stmt) in block.iter().enumerate() {
        if !matches!(stmt, Stmt::BranchStmt(_) | Stmt::ReturnStmt(_)) {
            continue;
        }

        if i == 0 || line(fset, stmt.pos()) - line(fset, block[0].pos()) < BLOCK_SIZE {
            return;
        }

        if line(fset, stmt.pos()) - line(fset, block[i - 1].end()) <= 1 {
            pending.push((
                stmt.pos().0 as u32,
                format!("{} with no blank line before", branch_name(stmt)),
            ));
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "nlreturn requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    let fset = pass.fset().clone();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::BlockStmt(b) => inspect_block(&fset, &b.list, &mut pending),
                NodeRef::CaseClause(c) => inspect_block(&fset, &c.body, &mut pending),
                NodeRef::CommClause(c) => inspect_block(&fset, &c.body, &mut pending),
                _ => {}
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
        name: "nlreturn",
        doc: "Checks for a new line before return and branch statements to increase code clarity",
        url: "https://github.com/ssgreg/nlreturn",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
