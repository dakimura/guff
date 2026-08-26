//! S1025 — don't use `fmt.Sprintf("%s", x)` unnecessarily.
//!
//! Port of `honnef.co/go/tools/simple/s1025`.

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{self, expr_to_string, is_call_to, type_with_name};
use guff_analysis::passes::inspect;
use guff_analysis::{
    match_pos, AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::render::render_node;
use guff_types::basic::BasicKind;
use guff_types::{Basic, TypeData, TypeId};

fn expr_type(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    pass.types_info()?.types.get(&expr.id()).map(|tv| tv.typ)
}

fn is_string_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let types = &artifacts.types;
    matches!(
        types.get(typ),
        TypeData::Basic(b) if b.kind() == BasicKind::String
    )
}

fn underlying_is_string(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    is_string_type(pass, typ.underlying(&artifacts.types))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1025 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String, &'static str, Option<TextEdit>)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if !is_call_to(pass, call, "fmt.Sprintf") || call.args.len() != 2 {
            return;
        };
        let Some(s) = expr_to_string(pass, &call.args[0]) else {
            return;
        };
        if s != "%s" {
            return;
        }
        let Some(typ) = expr_type(pass, &call.args[1]) else {
            return;
        };
        if type_with_name(pass, typ, "reflect.Value") {
            return;
        }

        // Each branch replaces the whole `fmt.Sprintf` call, and each builds a
        // different node from the same argument: `x.String()`, `x` itself, or
        // `string(x)`.
        let arg = &call.args[1];
        let (msg, fix_msg, replacement): (&str, &str, fn(&str) -> String) =
            if type_with_name(pass, typ, "fmt.Stringer") {
                (
                    "should use String() instead of fmt.Sprintf",
                    "Replace with call to String method",
                    |a| format!("{a}.String()"),
                )
            } else if is_string_type(pass, typ) {
                (
                    "the argument is already a string, there's no need to use fmt.Sprintf",
                    "Remove unnecessary call to fmt.Sprintf",
                    |a| a.to_string(),
                )
            } else if underlying_is_string(pass, typ) {
                (
                    "the argument's underlying type is a string, should use a simple conversion instead of fmt.Sprintf",
                    "Replace with conversion to string",
                    |a| format!("string({a})"),
                )
            } else {
                return;
            };
        let edit = render_node(pass, arg).map(|a| TextEdit {
            pos: call.pos().0 as u32,
            end: call.end().0 as u32,
            new_text: replacement(&a),
        });
        pending.push((match_pos(node), msg.into(), fix_msg, edit));
    });
    for (pos, message, fix_msg, edit) in pending {
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
                message: fix_msg.into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn s1025_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1025",
        doc: "don't use fmt.Sprintf(\"%s\", x) unnecessarily",
        url: "https://staticcheck.dev/docs/checks/#S1025",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1025_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1025_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
