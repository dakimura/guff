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
use guff_types::alias::unalias_readonly;
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

/// The type an imported package declares under `name`.
///
/// A third local copy of what `qf1010` and `qf1012` each keep — they are the
/// same six lines, and consolidating them is a separate change from this one.
fn imported_type(pass: &Pass<'_>, import_path: &str, name: &str) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let pkg_id = artifacts.packages.find_by_path(import_path)?;
    let scope = artifacts.packages.get(pkg_id).scope();
    let obj = guff_types::scope::lookup(&artifacts.scopes, scope, name)?;
    obj.typ(&artifacts.objects)
}

/// `types.Implements(typ, knowledge.Interfaces["fmt.Stringer"])`.
///
/// Not `IsTypeWithName(typ, "fmt.Stringer")`, which is what this used to ask —
/// that is true only of a value whose static type *is* the interface, so the
/// Stringer branch never fired for a concrete type and the branches below it
/// answered instead. boundary's `credential.Password` is `string` with a
/// `String` method: upstream says "should use String()", guff said "the
/// argument's underlying type is a string".
fn implements_fmt_stringer(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(iface) = imported_type(pass, "fmt", "Stringer") else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    guff_types::check_lookup::implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        iface,
        false,
    )
    .is_ok()
}

/// Upstream's `isFormatter`: the type's method set holds a `Format` taking two
/// parameters and returning nothing.
///
/// Deliberately looser than `types.Implements(fmt.Formatter)` — upstream does
/// not check the parameter types, and says so in a TODO. It is a *skip*: such a
/// type may render `%s` however it likes, so neither branch below applies.
///
/// This skip is why the fix above cannot land alone. Before it, a type with
/// both `Format` and `String` was silent only because the Stringer branch never
/// fired; making that branch work would have started reporting it.
fn is_formatter(pass: &Pass<'_>, typ: TypeId) -> bool {
    use guff_types::arena::ObjectData;
    use guff_types::lookup::{lookup_field_or_method, LookupResult};
    use guff_types::signature::{signature_params, signature_results};
    use guff_types::tuple::tuple_len;

    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    // `msCache.MethodSet(T)` — the method set of the argument's own type, so a
    // pointer-receiver `Format` does not count for a value.
    let found = lookup_field_or_method(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        false,
        None,
        "Format",
    );
    let LookupResult::Found { obj, .. } = found else {
        return false;
    };
    if !matches!(artifacts.objects.get(obj), ObjectData::Func(_)) {
        return false;
    }
    let Some(sig) = obj.typ(&artifacts.objects) else {
        return false;
    };
    let params = signature_params(&artifacts.types, sig);
    let results = signature_results(&artifacts.types, sig);
    tuple_len(&artifacts.types, params) == 2 && tuple_len(&artifacts.types, results) == 0
}

/// `code.IsOfStringConvertibleByteSlice`.
///
/// ```go
/// typ, ok := pass.TypesInfo.TypeOf(expr).Underlying().(*types.Slice)
/// if !ok { return false }
/// elem := types.Unalias(typ.Elem())
/// if version.Compare(LanguageVersion(pass, expr), "go1.18") >= 0 {
///     elem = elem.Underlying()
/// }
/// return types.Identical(elem, types.Typ[types.Byte])
/// ```
///
/// The go1.18 gate is carried for shape rather than for effect: the effective
/// file version is `max(fileVersion, go1.21)`, so the unwrapping always
/// happens. Before Go 1.18 a `[]T` with `type T byte` could not be converted to
/// string directly (golang/go#23536).
fn is_string_convertible_byte_slice(pass: &Pass<'_>, expr: &Expr, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let types = &artifacts.types;
    let under = typ.underlying(types);
    let TypeData::Slice(_) = types.get(under) else {
        return false;
    };
    let mut elem = unalias_readonly(types, guff_types::slice::slice_elem(types, under));
    if code::version_compare(
        &code::effective_file_go_version(pass, expr.pos().0 as u32),
        "go1.18",
    ) >= 0
    {
        elem = elem.underlying(types);
    }
    matches!(types.get(elem), TypeData::Basic(b) if b.kind() == guff_types::basic::BYTE)
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
            // printing with %s produces output different from using the String
            // method
            return;
        }
        if is_formatter(pass, typ) {
            // the type may choose to handle %s in arbitrary ways
            return;
        }

        // Each branch replaces the whole `fmt.Sprintf` call, and each builds a
        // different node from the same argument: `x.String()`, `x` itself, or
        // `string(x)`.
        let arg = &call.args[1];
        let (msg, fix_msg, replacement): (&str, &str, fn(&str) -> String) =
            if implements_fmt_stringer(pass, typ) {
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
            } else if is_string_convertible_byte_slice(pass, arg, typ) {
                (
                    "the argument's underlying type is a slice of bytes, should use a simple conversion instead of fmt.Sprintf",
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
