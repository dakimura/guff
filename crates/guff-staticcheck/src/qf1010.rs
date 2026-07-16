//! QF1010 — convert slice of bytes to string when printing it.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1010`.
//!
//! DEFERRED: skip args that implement `fmt.Stringer` (needs interface lookup
//! against a `fmt.Stringer` type in the universe / imported package).

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to_any, is_method_val};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::alias::unalias_readonly;
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;
use guff_types::TypeId;

use crate::render::render_expr;

const PRINT_FNS: &[&str] = &[
    "fmt.Print",
    "fmt.Println",
    "fmt.Sprint",
    "fmt.Sprintln",
    "log.Fatal",
    "log.Fatalln",
    "log.Panic",
    "log.Panicln",
    "log.Print",
    "log.Println",
];

const FPRINT_FNS: &[&str] = &["fmt.Fprint", "fmt.Fprintln"];

const LOGGER_METHODS: &[&str] = &[
    "Fatal", "Fatalln", "Panic", "Panicln", "Print", "Println",
];

fn is_string_convertible_byte_slice(pass: &Pass<'_>, expr: &Expr) -> bool {
    // AST-level `[]byte(...)` conversion (types may omit a TypeAndValue entry
    // for some conversion forms in our checker).
    if let Expr::CallExpr(call) = expr {
        if call.args.len() == 1 {
            let Expr::ArrayType(arr) = &*call.fun else {
                // fall through to type-based check
                return type_is_byte_slice(pass, expr);
            };
            if arr.len.is_none()
                && matches!(arr.elt.as_ref(), Expr::Ident(id) if id.name == "byte" || id.name == "uint8")
            {
                return true;
            }
        }
    }
    type_is_byte_slice(pass, expr)
}

fn type_is_byte_slice(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(tav) = info.types.get(&expr.id()) else {
        return false;
    };
    is_byte_slice_type(pass, tav.typ)
}

fn is_byte_slice_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let under = unalias_readonly(&artifacts.types, typ).underlying(&artifacts.types);
    let TypeData::Slice(s) = artifacts.types.get(under) else {
        return false;
    };
    // Go 1.18+: allow named byte aliases via Underlying on the element.
    let elem = unalias_readonly(&artifacts.types, s.elem()).underlying(&artifacts.types);
    matches!(
        artifacts.types.get(elem),
        TypeData::Basic(b) if b.kind() == BasicKind::Uint8
    )
}

fn print_args<'a>(pass: &Pass<'_>, call: &'a CallExpr) -> Option<&'a [Expr]> {
    if is_call_to_any(pass, call, PRINT_FNS) {
        return Some(&call.args);
    }
    if is_call_to_any(pass, call, FPRINT_FNS) {
        if call.args.is_empty() {
            return Some(&[]);
        }
        return Some(&call.args[1..]);
    }
    // (*log.Logger).Print* methods
    if let Expr::SelectorExpr(sel) = &*call.fun {
        if LOGGER_METHODS.contains(&sel.sel.name.as_str()) && is_method_val(pass, sel, &sel.sel.name)
        {
            return Some(&call.args);
        }
    }
    None
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1010 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        let Some(args) = print_args(pass, call) else {
            return;
        };
        for arg in args {
            if !is_string_convertible_byte_slice(pass, arg) {
                continue;
            }
            // DEFERRED: skip if arg implements fmt.Stringer.
            let replacement = format!("string({})", render_expr(arg));
            pending.push((
                arg.pos().0 as u32,
                arg.end().0 as u32,
                replacement,
            ));
        }
    });

    for (pos, end, replacement) in pending {
        pass.report(Diagnostic {
            pos,
            end,
            message: "could convert argument to string".into(),
            suggested_fixes: vec![SuggestedFix {
                message: "Convert argument to string".into(),
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

fn qf1010_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "QF1010",
        doc: "convert slice of bytes to string when printing it",
        url: "https://staticcheck.dev/docs/checks/#QF1010",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(qf1010_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn qf1010_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
