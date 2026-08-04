//! SA4020 — unreachable case clause in a type switch.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4020`.

use std::sync::OnceLock;

use guff::ast::{CaseClause, Expr, Stmt, TypeSwitchStmt};
use guff::node_mask;
use guff::walk::NodeRef;

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::TypeData;
use guff_types::check_lookup::implements;
use guff_types::typestring::type_string;

fn subsumes_safe(pass: &Pass<'_>, iface: guff_types::TypeId, concrete: guff_types::TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let iface_u = iface.underlying(&artifacts.types);
    if !matches!(artifacts.types.get(iface_u), TypeData::Interface(_)) {
        return false;
    }
    // Must use the package type arena — a fresh Checker only has the universe
    // (~34 types) and panics on package TypeIds (rclone SA4020).
    let mut types = artifacts.types.clone();
    implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        concrete,
        iface,
        false,
    )
    .is_ok()
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4020 requires inspect analyzer".to_string())?
        .clone();
    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(TypeSwitchStmt), pass.files(), |node| {
        let NodeRef::TypeSwitchStmt(ts) = node else {
            return;
        };
        collect_unreachable(pass, ts, &mut pending);
    });
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn collect_unreachable(pass: &Pass<'_>, ts: &TypeSwitchStmt, pending: &mut Vec<(u32, String)>) {
    let info = match pass.types_info() {
        Some(i) => i,
        None => return,
    };
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return,
    };
    let mut cases: Vec<(&CaseClause, Vec<guff_types::TypeId>)> = Vec::new();
    for stmt in &ts.body.list {
        let Stmt::CaseClause(cc) = stmt else {
            continue;
        };
        if cc.list.is_empty() {
            continue;
        }
        let mut types = Vec::new();
        for e in &cc.list {
            if matches!(e, Expr::Ident(id) if id.name == "nil") {
                continue;
            }
            if let Some(tav) = info.types.get(&e.id()) {
                types.push(tav.typ);
            }
        }
        cases.push((cc, types));
    }
    for i in 0..cases.len().saturating_sub(1) {
        let (_, ref earlier) = cases[i];
        for (cc, ref later) in &cases[i + 1..] {
            for &t in earlier {
                for &v in later {
                    if is_empty_interface(pass, &artifacts.types, t) {
                        let vname = type_string(
                            &artifacts.types,
                            &artifacts.objects,
                            &artifacts.packages,
                            v,
                            None,
                        );
                        pending.push((
                            cc.case.0 as u32,
                            format!("unreachable case clause: earlier case will always match before {vname}"),
                        ));
                        continue;
                    }
                    if subsumes_safe(pass, t, v) {
                        let tname = type_string(
                            &artifacts.types,
                            &artifacts.objects,
                            &artifacts.packages,
                            t,
                            None,
                        );
                        let vname = type_string(
                            &artifacts.types,
                            &artifacts.objects,
                            &artifacts.packages,
                            v,
                            None,
                        );
                        pending.push((
                            cc.case.0 as u32,
                            format!("unreachable case clause: {tname} will always match before {vname}"),
                        ));
                    }
                }
            }
        }
    }
}

fn is_empty_interface(
    pass: &Pass<'_>,
    artifacts: &guff_types::arena::TypeArena,
    typ: guff_types::TypeId,
) -> bool {
    let name = type_string(
        artifacts,
        &pass.pkg().type_artifacts.as_ref().unwrap().objects,
        &pass.pkg().type_artifacts.as_ref().unwrap().packages,
        typ,
        None,
    );
    name == "any" || name == "interface{}"
}

fn sa4020_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4020",
        doc: "unreachable case clause in a type switch",
        url: "https://staticcheck.dev/docs/checks/#SA4020",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4020_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4020_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
