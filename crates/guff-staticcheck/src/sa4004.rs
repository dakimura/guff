//! SA4004 — loop exits unconditionally after one iteration.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4004`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{BranchStmt, Expr, ReturnStmt, Stmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::TypeData;

/// Upstream skips ranging over maps (valid "pick one element" pattern) and
/// over function values (unknown iteration semantics).
fn range_x_is_flaggable(pass: &Pass<'_>, x: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return true;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return true;
    };
    let Some(tav) = info.types.get(&x.id()) else {
        return true;
    };
    match artifacts.types.get(tav.typ.underlying(&artifacts.types)) {
        TypeData::Map(_) | TypeData::Signature(_) => false,
        _ => true,
    }
}

/// The `for` keyword's position, used as the loop's identity when a labelled
/// `break` / `continue` has to be matched against *this* loop.
fn loop_key(stmt: &Stmt) -> Option<u32> {
    match stmt {
        Stmt::ForStmt(f) => Some(f.for_.0 as u32),
        Stmt::RangeStmt(r) => Some(r.for_.0 as u32),
        _ => None,
    }
}

/// Does this branch statement refer to the loop identified by `loop_key`?
///
/// Upstream: `stmt.Label == nil || labels[pass.TypesInfo.ObjectOf(stmt.Label)] == loop`.
/// Labels are function-scoped in Go, and the walk is per function, so matching
/// on the name is the same relation as matching on the object.
fn targets_loop(label: Option<&guff::ast::Ident>, labels: &HashMap<String, u32>, key: u32) -> bool {
    match label {
        None => true,
        Some(id) => labels.get(&id.name).copied() == Some(key),
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4004 requires inspect analyzer".to_string())?;
    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(FuncDecl, FuncLit), pass.files(), |node| {
        let body = match node {
            NodeRef::FuncDecl(f) => f.body.as_ref(),
            NodeRef::FuncLit(f) => Some(&f.body),
            _ => None,
        };
        let Some(body) = body else {
            return;
        };

        // `labels[pass.TypesInfo.ObjectOf(label.Label)] = label.Stmt`, kept for
        // loops only: a label on anything else can never equal `loop`.
        let mut labels: HashMap<String, u32> = HashMap::new();
        walk::preorder_prune(NodeRef::BlockStmt(body), |n| {
            if let NodeRef::LabeledStmt(l) = n {
                if let Some(key) = loop_key(&l.stmt) {
                    labels.insert(l.label.name.clone(), key);
                }
            }
            true
        });

        // Upstream walks the whole body with `ast.Inspect`, so a loop nested in
        // an `if` — or in any block at all — is examined like a top-level one.
        // Looking only at the function body's own statements missed opentofu's
        // `internal/legacy/helper/schema/resource_timeout.go:144`, where the
        // loop pair sits inside `if raw, ok := c.Config[…]; ok {`.
        walk::preorder_prune(NodeRef::BlockStmt(body), |n| {
            let (key, loop_body) = match n {
                NodeRef::ForStmt(f) => (f.for_.0 as u32, &f.body.list),
                NodeRef::RangeStmt(r) => {
                    if !range_x_is_flaggable(pass, &r.x) {
                        return true;
                    }
                    (r.for_.0 as u32, &r.body.list)
                }
                _ => return true,
            };
            if loop_body.len() < 2 {
                // Upstream keeps descending here: the one-statement range loop
                // that grabs the first element is not a finding, but a loop
                // *inside* it may be.
                return true;
            }
            let mut unconditional: Option<u32> = None;
            let mut has_branching = false;
            for s in loop_body {
                match s {
                    Stmt::BranchStmt(BranchStmt {
                        tok: Token::BREAK,
                        label,
                        tok_pos,
                        ..
                    }) => {
                        if targets_loop(label.as_ref(), &labels, key) {
                            unconditional = Some(tok_pos.0 as u32);
                        }
                    }
                    Stmt::BranchStmt(BranchStmt {
                        tok: Token::CONTINUE,
                        label,
                        ..
                    }) => {
                        if targets_loop(label.as_ref(), &labels, key) {
                            // The loop is not unconditionally terminated, and
                            // upstream stops descending here.
                            return false;
                        }
                    }
                    Stmt::ReturnStmt(ReturnStmt { return_, .. }) => {
                        unconditional = Some(return_.0 as u32)
                    }
                    Stmt::IfStmt(_)
                    | Stmt::ForStmt(_)
                    | Stmt::RangeStmt(_)
                    | Stmt::SwitchStmt(_)
                    | Stmt::SelectStmt(_) => {
                        has_branching = true;
                    }
                    _ => {}
                }
            }
            if unconditional.is_none() || !has_branching {
                return false;
            }
            // Upstream second pass: a `goto` anywhere, or a `continue` that is
            // unlabelled or names this loop, cancels the finding — "even if it
            // is in another loop or closure".
            let mut cancelled = false;
            for s in loop_body {
                walk::preorder(walk::stmt_ref(s), |n| {
                    if cancelled {
                        return false;
                    }
                    if let NodeRef::BranchStmt(b) = n {
                        match b.tok {
                            Token::GOTO => cancelled = true,
                            Token::CONTINUE => {
                                if targets_loop(b.label.as_ref(), &labels, key) {
                                    cancelled = true;
                                }
                            }
                            _ => {}
                        }
                    }
                    true
                });
            }
            if !cancelled {
                if let Some(pos) = unconditional {
                    pending.push((
                        pos,
                        "the surrounding loop is unconditionally terminated".into(),
                    ));
                }
            }
            true
        });
    });
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4004_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4004",
        doc: "the loop exits unconditionally after one iteration",
        url: "https://staticcheck.dev/docs/checks/#SA4004",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4004_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4004_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
