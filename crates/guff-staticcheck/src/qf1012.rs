//! QF1012 — use `fmt.Fprintf(x, ...)` instead of `x.Write(fmt.Sprintf(...))`.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1012`.
//!
//! Uses `types.Implements` against imported `io.Writer` / `io.StringWriter`.
//! Named non-interface receivers are checked via `*T` (larger method set),
//! matching upstream (https://staticcheck.dev/issues/1097).

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to_any, is_method_val};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::alias::unalias_readonly;
use guff_types::arena::TypeData;
use guff_types::check_lookup::implements;
use guff_types::new_pointer;
use guff_types::scope::lookup;
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

fn expr_type(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn imported_type(pass: &Pass<'_>, import_path: &str, name: &str) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let pkg_id = artifacts.packages.find_by_path(import_path)?;
    let scope = artifacts.packages.get(pkg_id).scope();
    let obj = lookup(&artifacts.scopes, scope, name)?;
    obj.typ(&artifacts.objects)
}

fn method_result_count(pass: &Pass<'_>, typ: TypeId, name: &str) -> Option<usize> {
    use guff_types::arena::ObjectData;
    use guff_types::lookup::{lookup_field_or_method, LookupResult};
    use guff_types::signature::signature_results;
    use guff_types::tuple::tuple_len;

    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let mut types = artifacts.types.clone();
    let resolved = unalias_readonly(&artifacts.types, typ);
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

/// True Implements against `io.Writer` / `io.StringWriter` when the package is
/// in the type graph; otherwise fall back to method-arity (Write/WriteString
/// returning 2 values), which covers fixtures that only declare local ifaces.
fn implements_iface(
    pass: &Pass<'_>,
    recv: &Expr,
    iface_path: &str,
    iface_name: &str,
    method: &str,
) -> bool {
    let Some(typ) = expr_type(pass, recv) else {
        return false;
    };
    if let Some(iface) = imported_type(pass, iface_path, iface_name) {
        if let Some(artifacts) = pass.pkg().type_artifacts.as_ref() {
            let resolved = unalias_readonly(&artifacts.types, typ);
            let mut types = artifacts.types.clone();
            let v = if matches!(types.get(resolved), TypeData::Named(_))
                && !matches!(
                    types.get(resolved.underlying(&types)),
                    TypeData::Interface(_)
                ) {
                new_pointer(&mut types, resolved)
            } else {
                typ
            };
            if implements(
                &mut types,
                &artifacts.objects,
                &artifacts.packages,
                v,
                iface,
                false,
            )
            .is_ok()
            {
                return true;
            }
        }
    }
    method_result_count(pass, typ, method) == Some(2)
}

fn implements_writer(pass: &Pass<'_>, recv: &Expr) -> bool {
    implements_iface(pass, recv, "io", "Writer", "Write")
}

fn implements_string_writer(pass: &Pass<'_>, recv: &Expr) -> bool {
    implements_iface(pass, recv, "io", "StringWriter", "WriteString")
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
) -> Option<(&'a Expr, &'static str, &'static str, &'a [Expr])> {
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
    Some((&sel.x, fprint, "Write", &inner.args))
}

fn match_write_string_sprintf<'a>(
    pass: &Pass<'_>,
    call: &'a CallExpr,
) -> Option<(&'a Expr, &'static str, &'static str, &'a [Expr])> {
    let Expr::SelectorExpr(sel) = &*call.fun else {
        return None;
    };
    if sel.sel.name != "WriteString" || call.args.len() != 1 {
        return None;
    }
    let Expr::CallExpr(inner) = &call.args[0] else {
        return None;
    };

    // Resolve Sprint* — typed call_name first, then AST `fmt.Sprint*`.
    let fprint = if is_call_to_any(pass, inner, SPRINT_FNS) {
        let name = guff_analysis::code::call_name(pass, &inner.fun)?;
        sprint_to_fprint(&name)?
    } else {
        let Expr::SelectorExpr(inner_sel) = inner.fun.as_ref() else {
            return None;
        };
        if !matches!(inner_sel.x.as_ref(), Expr::Ident(id) if id.name == "fmt") {
            return None;
        }
        match inner_sel.sel.name.as_str() {
            "Sprint" => "Fprint",
            "Sprintf" => "Fprintf",
            "Sprintln" => "Fprintln",
            _ => return None,
        }
    };
    let args = inner.args.as_slice();

    // Ideal: MethodVal + io.StringWriter + io.Writer.
    if is_method_val(pass, sel, "WriteString")
        && implements_string_writer(pass, &sel.x)
        && implements_writer(pass, &sel.x)
    {
        return Some((&sel.x, fprint, "WriteString", args));
    }

    None
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

fn fprint_to_sprint(fprint: &str) -> &'static str {
    match fprint {
        "Fprint" => "Sprint",
        "Fprintf" => "Sprintf",
        "Fprintln" => "Sprintln",
        _ => "Sprint*",
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1012 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, String, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        // Upstream renders the argument as written, so the `[]byte(...)`
        // conversion of the Write form stays in the message; WriteString has
        // none. Track which matcher fired rather than dropping it.
        let (matched, byte_conv) = match match_write_bytes_sprintf(pass, call) {
            Some(m) => (Some(m), true),
            None => (match_write_string_sprintf(pass, call), false),
        };
        let Some((recv, fprint, write_method, args)) = matched else {
            return;
        };
        let recv_s = render_expr(recv);
        let args_s = render_args(args);
        let replacement = if args_s.is_empty() {
            format!("fmt.{fprint}({recv_s})")
        } else {
            format!("fmt.{fprint}({recv_s}, {args_s})")
        };
        // Match upstream / golangci phrasing exactly (compat finding-set keys).
        let sprint = fprint_to_sprint(fprint);
        let inner = format!("fmt.{sprint}(...)");
        let arg = if byte_conv {
            format!("[]byte({inner})")
        } else {
            inner
        };
        let msg = format!("Use fmt.{fprint}(...) instead of {write_method}({arg})");
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
        run_despite_errors: true,
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
