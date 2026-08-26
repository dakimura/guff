//! SA1006 — `Printf` with dynamic first argument and no further arguments.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1006`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to, is_call_to_any};
use guff_analysis::passes::inspect;
use guff_analysis::code;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass, Diagnostic, SuggestedFix, TextEdit};

use crate::render::render_node;
use guff_types::arena::TypeData;

const PRINTF_ONE_ARG: &[&str] = &[
    "fmt.Errorf",
    "fmt.Printf",
    "fmt.Sprintf",
    "log.Fatalf",
    "log.Panicf",
    "log.Printf",
    "(*log.Logger).Printf",
    "(*testing.common).Logf",
    "(*testing.common).Errorf",
    "(*testing.common).Fatalf",
    "(*testing.common).Skipf",
    "(testing.TB).Logf",
    "(testing.TB).Errorf",
    "(testing.TB).Fatalf",
    "(testing.TB).Skipf",
];

/// The format argument as the check actually sees it.
///
/// Upstream matches the call with `pattern.MustParse` and then type-switches on
/// `m.State["format"]` for `*ast.CallExpr` / `*ast.Ident`, which reads as "the
/// parenthesized form is skipped". It is not: `pattern.match` strips
/// `*ast.ParenExpr` from **both** sides before any binding is made
/// (`pattern/match.go`, the two `case *ast.ParenExpr` arms sit above the
/// `matcher` dispatch), so what the type switch inspects is already unwrapped
/// and `fmt.Printf((s))` is reported.
///
/// Which way a matcher goes has to be read off the matcher every time — the
/// same honnef tree strips parens here and refuses to in `astutil.Equal`
/// (QF1003), and revive's rules type-assert raw. Three ports of this check have
/// been wrong in three different directions.
fn format_arg(expr: &Expr) -> &Expr {
    match expr {
        Expr::ParenExpr(p) => format_arg(&p.x),
        other => other,
    }
}

fn is_dynamic_format_arg(expr: &Expr) -> bool {
    matches!(expr, Expr::CallExpr(_) | Expr::Ident(_))
}

fn is_splatted_tuple(pass: &Pass<'_>, expr: &Expr) -> bool {
    let info = match pass.types_info() {
        Some(i) => i,
        None => return false,
    };
    let typ = match info.types.get(&expr.id()) {
        Some(tv) => tv.typ,
        None => return false,
    };
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return false,
    };
    matches!(
        artifacts.types.get(typ.underlying(&artifacts.types)),
        TypeData::Tuple(_)
    )
}

fn check_printf_call(
    pass: &Pass<'_>,
    call: &CallExpr,
    pending: &mut Vec<(u32, String, Option<TextEdit>)>,
) {
    let format = if is_call_to(pass, call, "fmt.Fprintf") {
        if call.args.len() != 2 {
            return;
        }
        &call.args[1]
    } else if is_call_to_any(pass, call, PRINTF_ONE_ARG) {
        if call.args.len() != 1 {
            return;
        }
        &call.args[0]
    } else {
        return;
    };

    let format = format_arg(format);
    if !is_dynamic_format_arg(format) || is_splatted_tuple(pass, format) {
        return;
    }
    // `edit.ReplaceWithString(call.Fun, alt)`. For everything but `fmt.Errorf`,
    // `alt` is the rendered callee with its final byte removed — upstream's
    // comment notes the callee can be an arbitrary selector like
    // `foo.bar[0].Printf`, and dropping the trailing `f` works for all of them.
    let alt = if is_call_to(pass, call, "fmt.Errorf") {
        Some("errors.New".to_string())
    } else {
        render_node(pass, &call.fun).and_then(|mut t| {
            t.pop()?;
            Some(t)
        })
    };
    let edit = alt.map(|new_text| TextEdit {
        pos: call.fun.pos().0 as u32,
        end: call.fun.end().0 as u32,
        new_text,
    });
    pending.push((
        call.fun.pos().0 as u32,
        "printf-style function with dynamic format string and no further arguments should use print-style function instead"
            .into(),
        edit,
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA1006 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String, Option<TextEdit>)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        check_printf_call(pass, call, &mut pending);
    });

    for (pos, message, edit) in pending {
        let Some(edit) = edit else {
            pass.report_unless_generated(pos, message);
            continue;
        };
        if code::is_generated_at(pass, pos) {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Use print-style function".into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn sa1006_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1006",
        doc: "Printf with dynamic first argument and no further arguments",
        url: "https://staticcheck.dev/docs/checks/#SA1006",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// SA1006 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1006_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1006_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
