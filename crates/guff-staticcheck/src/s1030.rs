//! S1030 — use `bytes.Buffer.String` or `bytes.Buffer.Bytes`.
//!
//! Port of `honnef.co/go/tools/simple/s1030`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, SelectorExpr};
use guff::walk::NodeRef;
use guff_analysis::code::type_func_name;
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn method_name(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    let Expr::SelectorExpr(SelectorExpr { sel, .. }) = &*call.fun else {
        return None;
    };
    let obj = pass.types_info()?.uses.get(&sel.id).copied()?;
    let a = pass.pkg().type_artifacts.as_ref()?;
    Some(type_func_name(
        &a.types,
        &a.objects,
        &a.packages,
        obj,
    ))
}

fn is_builtin_ident(fun: &Expr, name: &str) -> bool {
    matches!(fun, Expr::Ident(id) if id.name == name)
}

fn is_buffer_method(name: &str) -> bool {
    matches!(name, "(bytes.Buffer).Bytes" | "(bytes.Buffer).String")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let pkg = pass.pkg().pkg_path.as_str();
    if pkg == "bytes" || pkg == "bytes_test" {
        return Ok(None);
    }

    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1030 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if call.args.len() != 1 {
            return;
        }
        let Expr::CallExpr(inner_call) = &call.args[0] else {
            return;
        };
        let Some(inner_name) = method_name(pass, inner_call) else {
            return;
        };
        if !is_buffer_method(&inner_name) {
            return;
        }

        if is_builtin_ident(&call.fun, "string") && inner_name.ends_with(".Bytes") {
            pending.push((
                match_pos(node),
                "should use buf.String() instead of string(buf.Bytes())".into(),
            ));
        } else if is_builtin_ident(&call.fun, "[]byte") && inner_name.ends_with(".String") {
            pending.push((
                match_pos(node),
                "should use buf.Bytes() instead of []byte(buf.String())".into(),
            ));
        }
    });

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1030_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1030",
        doc: "use bytes.Buffer.String or bytes.Buffer.Bytes",
        url: "https://staticcheck.dev/docs/checks/#S1030",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1030_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1030_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
