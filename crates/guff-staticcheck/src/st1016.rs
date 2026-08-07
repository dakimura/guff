//! ST1016 — use consistent method receiver names.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1016`.
//! AST-based (upstream uses buildir + IntuitiveMethodSet); methods declared
//! in this package are grouped by receiver type name, which naturally skips
//! embedded methods from other types.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use guff::ast::Expr;
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::is_generated_at;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn recv_type_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StarExpr(s) => recv_type_name(&s.x),
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::IndexExpr(i) => recv_type_name(&i.x), // generic `T[K]`
        Expr::IndexListExpr(i) => recv_type_name(&i.x),
        Expr::ParenExpr(p) => recv_type_name(&p.x),
        _ => None,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ST1016 requires inspect analyzer".to_string())?
        .clone();

    // type name → (first method pos, counts of receiver names)
    let mut by_type: BTreeMap<String, (u32, BTreeMap<String, usize>)> = BTreeMap::new();

    inspect.preorder_typed(node_mask!(FuncDecl), pass.files(), |node| {
        let NodeRef::FuncDecl(fd) = node else {
            return;
        };
        let Some(recv) = &fd.recv else {
            return;
        };
        let Some(field) = recv.list.first() else {
            return;
        };
        let Some(ty) = &field.ty else {
            return;
        };
        let Some(type_name) = recv_type_name(ty) else {
            return;
        };
        let recv_name = field.names.first().map(|n| n.name.as_str()).unwrap_or("");
        if recv_name.is_empty() || recv_name == "_" {
            return;
        }
        let pos = field
            .names
            .first()
            .map(|n| n.pos().0 as u32)
            .unwrap_or_else(|| ty.pos().0 as u32);
        if is_generated_at(pass, pos) {
            return;
        }

        // Upstream reports the first method's *name*, not its receiver.
        let report_pos = fd.name.pos().0 as u32;
        let entry = by_type
            .entry(type_name)
            .or_insert_with(|| (report_pos, BTreeMap::new()));
        *entry.1.entry(recv_name.to_string()).or_insert(0) += 1;
    });

    let mut pending = Vec::new();
    for (_ty, (first_pos, names)) in by_type {
        if names.len() <= 1 {
            continue;
        }
        let mut seen: Vec<String> = names
            .iter()
            .map(|(name, count)| format!("{count}x {name:?}"))
            .collect();
        seen.sort();
        pending.push((
            first_pos,
            format!(
                "methods on the same type should have the same receiver name (seen {})",
                seen.join(", ")
            ),
        ));
    }

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn st1016_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1016",
        doc: "use consistent method receiver names",
        url: "https://staticcheck.dev/docs/checks/#ST1016",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1016_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1016_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
