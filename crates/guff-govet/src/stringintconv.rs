//! `stringintconv` — check for int-to-string conversions that yield one rune.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/stringintconv`.
//!
//! Both of upstream's alternative fixes are offered — `fmt.Sprint(x)` and
//! `string(rune(x))`. Their spans do not overlap (one rewrites the conversion's
//! type name, the other brackets its argument), and golangci's fixer takes the
//! edits of *every* `SuggestedFix` on an issue, so applying both is what
//! upstream ends up writing: `fmt.Sprint(rune(65))`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, Ident, ParenExpr, SelectorExpr, StarExpr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{
    refactor, AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::arena::{ObjectData, TypeData};
use guff_types::api_predicates::api_convertible_to;
use guff_types::basic::{BasicKind, IS_INTEGER};
use guff_types::TypeId;

fn is_string_type(types: &guff_types::arena::TypeArena, t: TypeId) -> bool {
    matches!(
        types.get(t.underlying(types)),
        TypeData::Basic(b) if b.kind() == BasicKind::String
    )
}

fn is_problematic_int(types: &guff_types::arena::TypeArena, t: TypeId) -> bool {
    let TypeData::Basic(b) = types.get(t.underlying(types)) else {
        return false;
    };
    if b.info().0 & IS_INTEGER.0 == 0 {
        return false;
    }
    !matches!(
        b.kind(),
        BasicKind::Uint8 | BasicKind::Int32 | BasicKind::UntypedRune
    )
}

fn conversion_target_type(pass: &Pass<'_>, call: &CallExpr) -> Option<TypeId> {
    conversion_target_type_from_expr(pass, &call.fun)
}

fn conversion_target_type_from_expr(pass: &Pass<'_>, e: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    match e {
        Expr::Ident(Ident { id: node_id, .. }) => {
            let obj = info.uses.get(node_id).copied()?;
            type_from_object(&artifacts.objects, obj)
        }
        Expr::SelectorExpr(SelectorExpr { sel, .. }) => {
            let obj = info.uses.get(&sel.id).copied()?;
            type_from_object(&artifacts.objects, obj)
        }
        Expr::ParenExpr(ParenExpr { x, .. }) => conversion_target_type_from_expr(pass, x),
        Expr::StarExpr(StarExpr { x, .. }) => conversion_target_type_from_expr(pass, x),
        _ => None,
    }
}

fn type_from_object(
    objects: &guff_types::arena::ObjectArena,
    obj: guff_types::ObjectId,
) -> Option<TypeId> {
    match objects.get(obj) {
        ObjectData::TypeName(tn) => tn.typ(),
        _ => None,
    }
}

/// Whether offering `fmt.Sprint(x)` could change what the program prints.
///
/// Upstream's guard is `types.NewMethodSet(V0).Len() == 0`: a `String`,
/// `GoString` or `Format` method makes `fmt.Sprint` produce something other
/// than the digits, so the "fix" would be a silent behaviour change.
///
/// guff has no method-set computation, so this counts the methods *declared* on
/// a named type instead. That is stricter than upstream — a type whose only
/// methods take a pointer receiver has an empty value method set, and upstream
/// would offer the fix where this declines. Declining writes less than
/// upstream, which the pending ledger can hold; guessing the other way would
/// rewrite `string(myStringer)` and change its output.
/// Upstream requires *every* term of the source type set to be convertible to
/// `rune` before offering `string(rune(x))`; with no type parameters that is
/// the single source type.
fn convertible_to_rune(pass: &Pass<'_>, t: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(rune) = lookup_basic_kind(&artifacts.types, BasicKind::Int32) else {
        return false;
    };
    api_convertible_to(
        &mut artifacts.types.clone(),
        &artifacts.objects,
        &artifacts.packages,
        t,
        rune,
    )
}

fn lookup_basic_kind(types: &guff_types::arena::TypeArena, kind: BasicKind) -> Option<TypeId> {
    guff_types::basic::lookup_basic(types, kind)
}

/// `types.Identical(T0, types.Typ[types.String])`: exactly `string`, not a
/// defined type whose underlying type is string.
fn is_unnamed_string(types: &guff_types::arena::TypeArena, t: TypeId) -> bool {
    matches!(types.get(t), TypeData::Basic(b) if b.kind() == BasicKind::String)
}

fn source_has_methods(types: &guff_types::arena::TypeArena, t: TypeId) -> bool {
    match types.get(t) {
        TypeData::Named(_) => guff_types::named::named_num_methods(types, t) > 0,
        _ => false,
    }
}

/// Type parameters are out of scope for the fix.
///
/// Upstream only offers it when the type sets of both sides have exactly one
/// term. A single-term type parameter would pass that test and fails this one,
/// so guff declines a little more often — again the direction the ledger can
/// hold.
fn is_type_param(types: &guff_types::arena::TypeArena, t: TypeId) -> bool {
    matches!(types.get(t), TypeData::TypeParam(_))
}

fn type_name(
    types: &guff_types::arena::TypeArena,
    objects: &guff_types::arena::ObjectArena,
    t: TypeId,
) -> String {
    match types.get(t) {
        TypeData::Basic(b) => b.name().to_string(),
        TypeData::Named(n) => n.obj().name(objects).to_string(),
        _ => "type".to_string(),
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "stringintconv requires inspect analyzer".to_string())?
        .clone();
    let artifacts = pass
        .pkg()
        .type_artifacts
        .as_ref()
        .ok_or_else(|| "stringintconv requires type artifacts".to_string())?;
    let info = pass
        .types_info()
        .ok_or_else(|| "stringintconv requires types info".to_string())?;

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if call.args.len() != 1 {
            return;
        }
        let Some(target_typ) = conversion_target_type(pass, call) else {
            return;
        };
        if !is_string_type(&artifacts.types, target_typ) {
            return;
        }
        let arg = &call.args[0];
        let Some(arg_typ) = info.types.get(&arg.id()).map(|tv| tv.typ) else {
            return;
        };
        if !is_problematic_int(&artifacts.types, arg_typ) {
            return;
        }
        let source = type_name(&artifacts.types, &artifacts.objects, arg_typ);
        let target = type_name(&artifacts.types, &artifacts.objects, target_typ);

        let mut fixes: Vec<(String, Vec<TextEdit>)> = Vec::new();
        let eligible = !is_type_param(&artifacts.types, arg_typ)
            && !is_type_param(&artifacts.types, target_typ)
            && !source_has_methods(&artifacts.types, arg_typ);
        if eligible {
            if let Some((prefix, import_edits)) = refactor::enclosing_file(pass, call.pos().0 as u32)
                .and_then(|file| {
                    refactor::add_import(pass, file, "fmt", "fmt", "Sprint", arg.pos().0 as u32)
                })
            {
                // `string(x)` replaces the type name; a named string type has to
                // keep its conversion, so the call is wrapped instead.
                let mut edits = import_edits;
                if is_unnamed_string(&artifacts.types, target_typ) {
                    edits.push(TextEdit {
                        pos: call.fun.pos().0 as u32,
                        end: call.fun.end().0 as u32,
                        new_text: format!("{prefix}Sprint"),
                    });
                } else {
                    let lparen = (call.lparen.0 + 1) as u32;
                    edits.push(TextEdit {
                        pos: lparen,
                        end: lparen,
                        new_text: format!("{prefix}Sprint("),
                    });
                    edits.push(TextEdit {
                        pos: call.rparen.0 as u32,
                        end: call.rparen.0 as u32,
                        new_text: ")".into(),
                    });
                }
                fixes.push(("Format the number as a decimal".into(), edits));
            }
        }
        if convertible_to_rune(pass, arg_typ) {
            let (a, b) = (arg.pos().0 as u32, arg.end().0 as u32);
            fixes.push((
                "Convert a single rune to a string".into(),
                vec![
                    TextEdit {
                        pos: a,
                        end: a,
                        new_text: "rune(".into(),
                    },
                    TextEdit {
                        pos: b,
                        end: b,
                        new_text: ")".into(),
                    },
                ],
            ));
        }

        pending.push((
            call.pos().0 as u32,
            format!(
                "conversion from {source} to {target} yields a string of one rune, not a string of digits"
            ),
            fixes,
        ));
    });

    for (pos, message, fixes) in pending {
        if fixes.is_empty() {
            pass.reportf(pos, message);
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: fixes
                .into_iter()
                .map(|(message, text_edits)| SuggestedFix {
                    message,
                    text_edits,
                })
                .collect(),
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "stringintconv",
        doc: "check for string(int) conversions that yield one rune",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/stringintconv",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
