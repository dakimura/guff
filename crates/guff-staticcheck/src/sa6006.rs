//! SA6006 — using `io.WriteString` to write `[]byte`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa6006`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to_any, unparen};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;

fn is_byte_slice(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(tav) = info.types.get(&expr.id()) else {
        return false;
    };
    let TypeData::Slice(s) = artifacts.types.get(tav.typ.underlying(&artifacts.types)) else {
        return false;
    };
    let elem = s.elem().underlying(&artifacts.types);
    matches!(
        artifacts.types.get(elem),
        TypeData::Basic(b) if b.kind() == BasicKind::Uint8
    )
}

/// Upstream matches `(CallExpr (Builtin "string") [arg])`, and `pattern.match`
/// strips `*ast.ParenExpr` at every level before binding — so
/// `io.WriteString(w, (string(b)))` and `io.WriteString(w, ((string))(b))` both
/// match, and `arg` binds to the unparenthesized operand.
fn string_bytes_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::CallExpr(call) = unparen(expr) else {
        return None;
    };
    let Expr::Ident(id) = unparen(call.fun.as_ref()) else {
        return None;
    };
    if id.name != "string" || call.args.len() != 1 {
        return None;
    }
    Some(unparen(&call.args[0]))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA6006 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if !is_call_to_any(pass, call, &["io.WriteString"]) || call.args.len() != 2 {
            return;
        }
        let Some(arg) = string_bytes_arg(&call.args[1]) else {
            return;
        };
        if is_byte_slice(pass, arg) {
            pending.push((
                match_pos(node),
                "use io.Writer.Write instead of converting from []byte to string to use io.WriteString"
                    .into(),
            ));
        }
    });
    for (pos, msg) in pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn sa6006_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA6006",
        doc: "using io.WriteString to write []byte",
        url: "https://staticcheck.dev/docs/checks/#SA6006",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa6006_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa6006_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
