//! QF1012 — use `fmt.Fprintf(x, ...)` instead of `x.Write(fmt.Sprintf(...))`.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1012`.
//!
//! Approximates `io.Writer` / `io.StringWriter` by checking that the selected
//! `Write` / `WriteString` method has the expected result arity (Writer must
//! return 2 values). Full `types.Implements` against imported iface types is
//! DEFERRED when the receiver is a named non-interface type that needs the
//! pointer method set expansion for edge cases.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to_any, is_method_val};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::alias::unalias_readonly;
use guff_types::arena::ObjectData;
use guff_types::lookup::{lookup_field_or_method, LookupResult};
use guff_types::signature::signature_results;
use guff_types::tuple::tuple_len;
use guff_types::TypeId;

use crate::render::render_expr;

const SPRINT_FNS: &[&str] = &["fmt.Sprint", "fmt.Sprintf", "fmt.Sprintln"];

fn is_byte_slice_conv(fun: &Expr) -> bool {
    let Expr::ArrayType(arr) = fun else {
        return false;
    };
    if arr.len.is_some() {
        return false;
    }
    matches!(arr.elt.as_ref(), Expr::Ident(id) if id.name == "byte" || id.name == "uint8")
}

fn method_result_count(pass: &Pass<'_>, typ: TypeId, name: &str) -> Option<usize> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let mut types = artifacts.types.clone();
    let resolved = unalias_readonly(&artifacts.types, typ);
    // `address=true` includes pointer method set for addressable named values
    // (e.g. `bytes.Buffer` value receivers that need `*Buffer` methods).
    match lookup_field_or_method(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        resolved,
        true,
        None,
        name,
    ) {
        LookupResult::Found { obj, .. }
            if matches!(artifacts.objects.get(obj), ObjectData::Func(_)) =>
        {
            let sig = obj.typ(&artifacts.objects)?;
            let results = signature_results(&artifacts.types, sig);
            Some(tuple_len(&artifacts.types, results))
        }
        _ => None,
    }
}

fn expr_type(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn implements_writer(pass: &Pass<'_>, recv: &Expr) -> bool {
    let Some(typ) = expr_type(pass, recv) else {
        return false;
    };
    // io.Writer.Write returns (int, error) — 2 results. NotAWriter returns 0.
    method_result_count(pass, typ, "Write") == Some(2)
}

fn implements_string_writer(pass: &Pass<'_>, recv: &Expr) -> bool {
    let Some(typ) = expr_type(pass, recv) else {
        return false;
    };
    method_result_count(pass, typ, "WriteString") == Some(2)
}

fn sprint_to_fprint(name: &str) -> Option<&'static str> {
    match name {
        "fmt.Sprint" => Some("Fprint"),
        "fmt.Sprintf" => Some("Fprintf"),
        "fmt.Sprintln" => Some("Fprintln"),
        _ => None,
    }
}

fn match_write_bytes_sprintf<'a>(
    pass: &Pass<'_>,
    call: &'a CallExpr,
) -> Option<(&'a Expr, &'static str, &'a [Expr])> {
    let Expr::SelectorExpr(sel) = &*call.fun else {
        return None;
    };
    if sel.sel.name != "Write" || !is_method_val(pass, sel, "Write") {
        return None;
    }
    if call.args.len() != 1 {
        return None;
    }
    let Expr::CallExpr(conv) = &call.args[0] else {
        return None;
    };
    if !is_byte_slice_conv(&conv.fun) || conv.args.len() != 1 {
        return None;
    }
    let Expr::CallExpr(inner) = &conv.args[0] else {
        return None;
    };
    if !is_call_to_any(pass, inner, SPRINT_FNS) {
        return None;
    }
    let name = guff_analysis::code::call_name(pass, &inner.fun)?;
    let fprint = sprint_to_fprint(&name)?;
    if !implements_writer(pass, &sel.x) {
        return None;
    }
    Some((&sel.x, fprint, &inner.args))
}

fn match_write_string_sprintf<'a>(
    pass: &Pass<'_>,
    call: &'a CallExpr,
) -> Option<(&'a Expr, &'static str, &'a [Expr])> {
    let Expr::SelectorExpr(sel) = &*call.fun else {
        return None;
    };
    if sel.sel.name != "WriteString" || !is_method_val(pass, sel, "WriteString") {
        return None;
    }
    if call.args.len() != 1 {
        return None;
    }
    let Expr::CallExpr(inner) = &call.args[0] else {
        return None;
    };
    if !is_call_to_any(pass, inner, SPRINT_FNS) {
        return None;
    }
    let name = guff_analysis::code::call_name(pass, &inner.fun)?;
    let fprint = sprint_to_fprint(&name)?;
    // Needs both StringWriter and Writer.
    if !implements_string_writer(pass, &sel.x) || !implements_writer(pass, &sel.x) {
        return None;
    }
    Some((&sel.x, fprint, &inner.args))
}

fn render_args(args: &[Expr]) -> String {
    let mut s = String::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&render_expr(a));
    }
    s
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1012 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, String, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        let matched = match_write_bytes_sprintf(pass, call)
            .or_else(|| match_write_string_sprintf(pass, call));
        let Some((recv, fprint, args)) = matched else {
            return;
        };
        let recv_s = render_expr(recv);
        let args_s = render_args(args);
        let replacement = if args_s.is_empty() {
            format!("fmt.{fprint}({recv_s})")
        } else {
            format!("fmt.{fprint}({recv_s}, {args_s})")
        };
        let msg = format!("Use fmt.{fprint}(...) instead of Write/WriteString(fmt.Sprint*(...))");
        pending.push((
            call.pos().0 as u32,
            call.end().0 as u32,
            msg,
            replacement,
        ));
    });

    for (pos, end, message, replacement) in pending {
        pass.report(Diagnostic {
            pos,
            end,
            message: message.clone(),
            suggested_fixes: vec![SuggestedFix {
                message,
                text_edits: vec![TextEdit {
                    pos,
                    end,
                    new_text: replacement,
                }],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn qf1012_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "QF1012",
        doc: "use fmt.Fprintf(x, ...) instead of x.Write(fmt.Sprintf(...))",
        url: "https://staticcheck.dev/docs/checks/#QF1012",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(qf1012_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn qf1012_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
