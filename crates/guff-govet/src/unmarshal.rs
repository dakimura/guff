//! `unmarshal` — check encoding/json and related Unmarshal/Decode calls.

use std::sync::OnceLock;

use guff::ast::CallExpr;
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::TypeData;

use crate::govet_util::{expr_type, is_type_named, receiver_named_type, static_callee};

fn skip_pkg(pass: &Pass<'_>) -> bool {
    matches!(
        pass.pkg().pkg_path.as_str(),
        "encoding/gob" | "encoding/json" | "encoding/xml" | "encoding/asn1"
    )
}

fn unmarshal_arg_index(pass: &Pass<'_>, call: &CallExpr) -> Option<(usize, &'static str)> {
    let obj = static_callee(pass, call)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let name = obj.name(&artifacts.objects);
    let path = obj
        .pkg(&artifacts.objects)
        .map(|p| artifacts.packages.get(p).path().to_string())?;
    if name == "Unmarshal" {
        return match path.as_str() {
            "encoding/json" | "encoding/xml" | "encoding/asn1" => Some((1, "Unmarshal")),
            _ => None,
        };
    }
    if name == "Decode" {
        let sig = obj.typ(&artifacts.objects)?;
        let recv = guff_types::signature::signature_recv(&artifacts.types, sig)
            .and_then(|r| r.typ(&artifacts.objects))?;
        if let Some((_, tn)) = receiver_named_type(pass, recv) {
            let tname = tn.name(&artifacts.objects);
            if tname == "Decoder" {
                return match path.as_str() {
                    "encoding/json" | "encoding/xml" | "encoding/gob" => Some((0, "Decode")),
                    _ => None,
                };
            }
        }
    }
    None
}

fn is_pointer_like(pass: &Pass<'_>, typ: guff_types::TypeId) -> bool {
    let artifacts = pass.pkg().type_artifacts.as_ref().expect("artifacts");
    let u = typ.underlying(&artifacts.types);
    matches!(
        artifacts.types.get(u),
        TypeData::Pointer(_) | TypeData::Interface(_) | TypeData::TypeParam(_)
    )
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if skip_pkg(pass) {
        return Ok(None);
    }
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "unmarshal requires inspect analyzer".to_string())?
        .clone();
    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        let Some((idx, name)) = unmarshal_arg_index(pass, call) else {
            return;
        };
        let Some(arg) = call.args.get(idx) else {
            return;
        };
        let Some(typ) = expr_type(pass, arg) else {
            return;
        };
        if is_pointer_like(pass, typ) {
            return;
        }
        let msg = if idx == 0 {
            format!("call of {name} passes non-pointer")
        } else {
            format!("call of {name} passes non-pointer as second argument")
        };
        pending.push((call.lparen.0 as u32, msg));
    });
    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "unmarshal",
        doc: "check encoding/json Unmarshal and Decoder.Decode pointer arguments",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/unmarshal",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
