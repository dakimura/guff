//! `hostport` — `fmt.Sprintf("%s:%d", host, port)` does not work with IPv6.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/hostport`.
//!
//! The check is narrow on purpose: only an address argument of `net.Dial`,
//! `net.DialTimeout` or `(*net.Dialer).Dial` is examined, and only when it is
//! either the `fmt.Sprintf` call itself or a local variable whose *declaration*
//! is one. A `fmt.Sprintf("%s:%d", …)` that never reaches a dial is not a
//! finding.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, Expr, Ident, ValueSpec};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{call_name, expr_to_bytes, unparen};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::ObjectId;

const DIAL_CALLEES: &[&str] = &["net.Dial", "net.DialTimeout", "(*net.Dialer).Dial"];

/// The declaring right-hand side of every local whose declaration has exactly
/// one value, keyed by the declared object.
///
/// Upstream reaches this through the type index (`index.Def(addrVar)` then the
/// declaring node's parent); guff has no such index, so the same map is built
/// by one walk over the file's assignments and value specs.
fn single_value_decls<'a>(
    pass: &Pass<'_>,
    files: &'a [guff::ast::File],
) -> HashMap<ObjectId, &'a Expr> {
    let mut out = HashMap::new();
    let Some(info) = pass.types_info() else {
        return out;
    };
    for file in files {
        // `preorder_prune` keeps the files' lifetime on the yielded nodes, which
        // `inspect`'s masked walk does not — the map has to outlive the walk.
        guff::walk::preorder_prune(NodeRef::File(file), |n| {
            match n {
                NodeRef::AssignStmt(AssignStmt { lhs, rhs, .. }) => {
                    if lhs.len() == 1 && rhs.len() == 1 {
                        if let Expr::Ident(id) = &lhs[0] {
                            if let Some(Some(obj)) = info.defs.get(&id.id) {
                                out.insert(*obj, &rhs[0]);
                            }
                        }
                    }
                }
                NodeRef::ValueSpec(ValueSpec { names, values, .. }) => {
                    if names.len() == 1 && values.len() == 1 {
                        if let Some(Some(obj)) = info.defs.get(&names[0].id) {
                            out.insert(*obj, &values[0]);
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

/// If `e` is `fmt.Sprintf("%s:%d", …)` or `fmt.Sprintf("%s:%s", …)`, the
/// format string's position, end, and text.
fn bad_addr_format(pass: &Pass<'_>, e: &Expr) -> Option<(u32, u32, String)> {
    let Expr::CallExpr(call) = unparen(e) else {
        return None;
    };
    if call.args.len() != 3 {
        return None;
    }
    if call_name(pass, &call.fun).as_deref() != Some("fmt.Sprintf") {
        return None;
    }
    let format_arg = &call.args[0];
    let bytes = expr_to_bytes(pass, format_arg)?;
    let format = String::from_utf8(bytes).ok()?;
    if format != "%s:%d" && format != "%s:%s" {
        return None;
    }
    Some((
        format_arg.pos().0 as u32,
        format_arg.end().0 as u32,
        format,
    ))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "hostport requires inspect analyzer".to_string())?
        .clone();

    let decls = single_value_decls(pass, pass.files());
    let fset = pass.fset().clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if !call_name(pass, &call.fun)
            .is_some_and(|name| DIAL_CALLEES.iter().any(|d| *d == name))
        {
            return;
        }
        let Some(address) = call.args.get(1) else {
            return;
        };
        // `dial_line` is `None` when the Sprintf is the dial argument itself;
        // upstream only names a line when the two are apart.
        let (target, dial_line): (&Expr, Option<i64>) = match unparen(address) {
            // net.Dial("tcp", fmt.Sprintf("%s:%d", …))
            e @ Expr::CallExpr(_) => {
                if call.args.len() != 2 {
                    return; // avoid the spread-call edge case, as upstream does
                }
                (e, None)
            }
            // addr := fmt.Sprintf("%s:%d", …); …; net.Dial("tcp", addr)
            Expr::Ident(Ident { id, .. }) => {
                let Some(info) = pass.types_info() else {
                    return;
                };
                let Some(obj) = info.uses.get(id).copied() else {
                    return;
                };
                let Some(rhs) = decls.get(&obj) else {
                    return;
                };
                (*rhs, Some(fset.position(call.pos()).line as i64))
            }
            _ => return,
        };
        if let Some((pos, _end, format)) = bad_addr_format(pass, target) {
            let suffix = match dial_line {
                Some(line) => format!(" (passed to net.Dial at L{line})"),
                None => String::new(),
            };
            pending.push((
                pos,
                format!("address format {format:?} does not work with IPv6{suffix}"),
            ));
        }
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "hostport",
        doc: "check format of addresses passed to net.Dial",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/hostport",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
