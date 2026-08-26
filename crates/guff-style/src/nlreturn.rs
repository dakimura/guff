//! Port of [`github.com/ssgreg/nlreturn`](https://github.com/ssgreg/nlreturn)
//! (golangci-lint wrapper in `pkg/golinters/nlreturn`).
//!
//! Default matches golangci-lint / upstream: `block-size=1`.
//!
//! The fix is a pure insertion of a newline at the statement's position — the
//! gofmt pass that follows a `--fix` turns it into the blank line the message
//! asks for.

use std::sync::OnceLock;

use guff::ast::Stmt;
use guff::position::FileSet;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::options::NlreturnOptions;

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

fn inspect_block(
    fset: &FileSet,
    block: &[Stmt],
    block_size: i64,
    pending: &mut Vec<(u32, String)>,
) {
    // Upstream's edit is `{Pos: stmt.Pos(), End: stmt.Pos(), NewText: "\n"}` at
    // every report site, so it is built from `pos` alone below.
    for (i, stmt) in block.iter().enumerate() {
        if !matches!(stmt, Stmt::BranchStmt(_) | Stmt::ReturnStmt(_)) {
            continue;
        }

        if i == 0 || line(fset, stmt.pos()) - line(fset, block[0].pos()) < block_size {
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

    let options = pass
        .settings::<NlreturnOptions>("nlreturn")
        .copied()
        .unwrap_or_default();
    let block_size = options.block_size;

    let mut pending = Vec::new();
    let fset = pass.fset().clone();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::BlockStmt(b) => inspect_block(&fset, &b.list, block_size, &mut pending),
                NodeRef::CaseClause(c) => inspect_block(&fset, &c.body, block_size, &mut pending),
                NodeRef::CommClause(c) => inspect_block(&fset, &c.body, block_size, &mut pending),
                _ => {}
            }
            true
        });
    }

    for (pos, message) in pending {
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: String::new(),
                text_edits: vec![TextEdit {
                    pos,
                    end: pos,
                    new_text: "\n".to_string(),
                }],
            }],
            ..Diagnostic::default()
        });
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
