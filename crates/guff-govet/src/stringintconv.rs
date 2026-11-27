//! `stringintconv` — check for int-to-string conversions that yield one rune.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/stringintconv`, whose logic
//! moved to `internal/analysis/intconv.Check` in x/tools v0.52.
//!
//! In the conversion `T(v)` of a value of type `V`, upstream looks for a `T0` in
//! the type set of `T` whose underlying type is `string` and a `V0` in the type
//! set of `V` that is a non-byte, non-rune integer, so type parameters are
//! reported too.
//!
//! Fixes (v0.52): the first must preserve behaviour — `string(rune(x))` when
//! `rune(x)` is exact, else `fmt.Sprintf("%c", x)` — and when neither applies
//! no fix is offered at all. The second, `fmt.Sprintf("%d", x)`, changes
//! behaviour. golangci's fixer takes the edits of *every* `SuggestedFix` on an
//! issue, so for a type at most 32 bits wide the two compose
//! (`fmt.Sprintf("%d", rune(x))`), and for a wider one they write the same
//! range, which golangci treats as a govet self-conflict and drops every govet
//! edit in the file. guff's fixer applies the same rule (`guff-lint/src/fix.rs`).

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, Ident, SelectorExpr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{
    refactor, AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::alias::unalias_readonly;
use guff_types::api_predicates::api_convertible_to;
use guff_types::arena::{ObjectArena, ObjectData, TypeArena, TypeData};
use guff_types::basic::{BasicKind, IS_INTEGER, IS_UNTYPED};
use guff_types::{ObjectId, TypeId};

/// `typeName`: a basic type's name (possibly "untyped int"), the name of a
/// named type, alias or type parameter, and "" for anything else.
fn type_name(types: &TypeArena, objects: &ObjectArena, t: TypeId) -> String {
    match types.get(t) {
        TypeData::Basic(b) => b.name().to_string(),
        TypeData::Named(n) => n.obj().name(objects).to_string(),
        TypeData::Alias(a) => a.obj().name(objects).to_string(),
        TypeData::TypeParam(tp) => tp.obj().name(objects).to_string(),
        _ => String::new(),
    }
}

/// `describe`: `typ` as found within the type set of `in_type`, named
/// `in_name` when that is non-empty (an alias's name is not reachable from the
/// type itself).
fn describe(
    types: &TypeArena,
    objects: &ObjectArena,
    typ: TypeId,
    in_type: TypeId,
    in_name: &str,
) -> String {
    let mut name = in_name.to_string();
    if typ != in_type {
        name = type_name(types, objects, typ);
    }
    if name.is_empty() {
        return String::new();
    }
    let mut parentheticals = Vec::new();
    let under_name = type_name(types, objects, typ.underlying(types));
    if !under_name.is_empty() && under_name != name {
        parentheticals.push(under_name);
    }
    if typ != in_type && !in_name.is_empty() && in_name != name {
        parentheticals.push(format!("in {in_name}"));
    }
    if !parentheticals.is_empty() {
        name = format!("{name} ({})", parentheticals.join(", "));
    }
    name
}

/// `structuralTypes`: a type parameter's structural terms (none for a
/// constraint without type terms), or the type itself.
fn structural_types(
    scratch: &mut Option<TypeArena>,
    pass: &Pass<'_>,
    t: TypeId,
) -> Option<Vec<TypeId>> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let types = &artifacts.types;
    let u = unalias_readonly(types, t);
    if !matches!(types.get(u), TypeData::TypeParam(_)) {
        return Some(vec![t]);
    }
    let arena = scratch.get_or_insert_with(|| artifacts.types.clone());
    let iface =
        guff_types::type_param_iface(arena, &artifacts.objects, &artifacts.packages, u);
    let tset =
        guff_types::interface_typeset(arena, &artifacts.objects, &artifacts.packages, iface);
    Some(tset.term_types().unwrap_or_default())
}

/// `types.Unalias(t).Underlying()` is a string.
fn is_string_based(types: &TypeArena, t: TypeId) -> bool {
    matches!(
        types.get(t.underlying(types)),
        TypeData::Basic(b) if b.kind() == BasicKind::String
    )
}

/// An integer that is not `byte`, `rune` or an untyped rune.
fn is_problematic_int(types: &TypeArena, t: TypeId) -> bool {
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

/// The `*types.TypeName` that `call.Fun` uses — only a bare identifier or a
/// selector, as upstream: `(string)(x)` is not looked at.
fn conversion_type_name(pass: &Pass<'_>, call: &CallExpr) -> Option<ObjectId> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let obj = match &*call.fun {
        Expr::Ident(Ident { id, .. }) => info.uses.get(id).copied()?,
        Expr::SelectorExpr(SelectorExpr { sel, .. }) => info.uses.get(&sel.id).copied()?,
        _ => return None,
    };
    matches!(artifacts.objects.get(obj), ObjectData::TypeName(_)).then_some(obj)
}

/// `types.ConvertibleTo(t, types.Typ[types.Rune])`.
fn convertible_to_rune(scratch: &mut Option<TypeArena>, pass: &Pass<'_>, t: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(rune) = guff_types::basic::lookup_basic(&artifacts.types, BasicKind::Int32) else {
        return false;
    };
    let arena = scratch.get_or_insert_with(|| artifacts.types.clone());
    api_convertible_to(arena, &artifacts.objects, &artifacts.packages, t, rune)
}

/// `types.NewMethodSet(V0).Len() == 0` for a type whose underlying type is an
/// integer: no embedding is possible, so the value method set is the methods
/// declared with a value receiver.
fn method_set_empty(types: &TypeArena, objects: &ObjectArena, t: TypeId) -> bool {
    let t = unalias_readonly(types, t);
    if !matches!(types.get(t), TypeData::Named(_)) {
        return true;
    }
    let origin = guff_types::named::named_origin(types, t);
    for i in 0..guff_types::named::named_num_methods(types, origin) {
        let m = guff_types::named::named_method(types, origin, i);
        let ObjectData::Func(f) = objects.get(m) else {
            return false;
        };
        let recv_typ = f
            .typ()
            .and_then(|sig| match types.get(sig) {
                TypeData::Signature(s) => s.recv(),
                _ => None,
            })
            .and_then(|r| match objects.get(r) {
                ObjectData::Var(v) => Some(v.typ()),
                _ => None,
            });
        match recv_typ {
            Some(rt) if matches!(types.get(unalias_readonly(types, rt)), TypeData::Pointer(_)) => {}
            _ => return false,
        }
    }
    true
}

/// `runeIsExact`: `string(rune(x))` equals `string(x)` — `x` is a constant
/// that fits in 32 bits, or every type in its type set is at most 32 bits wide.
fn rune_is_exact(
    types: &TypeArena,
    val: Option<&guff_constant::Value>,
    vtypes: &[TypeId],
) -> bool {
    if let Some(v) = val {
        let (n, exact) = guff_constant::int64_val(&guff_constant::to_int(v.clone()));
        return exact && i64::from(i32::MIN) <= n && n <= i64::from(i32::MAX);
    }
    vtypes.iter().all(|&t| match types.get(t.underlying(types)) {
        TypeData::Basic(b) => matches!(
            b.kind(),
            BasicKind::Int8
                | BasicKind::Int16
                | BasicKind::Int32
                | BasicKind::Uint8
                | BasicKind::Uint16
                | BasicKind::Uint32
        ),
        _ => false,
    })
}

/// `isUntypedConst`.
fn is_untyped_const(types: &TypeArena, typ: TypeId, val: Option<&guff_constant::Value>) -> bool {
    val.is_some() && matches!(types.get(typ), TypeData::Basic(b) if b.info().0 & IS_UNTYPED.0 != 0)
}

/// `types.Identical(T, types.Typ[types.String])`.
fn is_exactly_string(types: &TypeArena, t: TypeId) -> bool {
    matches!(
        types.get(unalias_readonly(types, t)),
        TypeData::Basic(b) if b.kind() == BasicKind::String
    )
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
    let orig_types = &artifacts.types;
    let objects = &artifacts.objects;

    // A scratch copy of the arena for the queries that compute lazily (type
    // sets, convertibility); made at most once, on the first hit. Once it
    // exists every lookup reads it: a type set computed there may name types
    // the original arena does not have.
    let mut scratch: Option<TypeArena> = None;
    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if call.args.len() != 1 {
            return;
        }
        let arg = &call.args[0];

        let Some(tname) = conversion_type_name(pass, call) else {
            return;
        };
        let ObjectData::TypeName(tn) = objects.get(tname) else {
            return;
        };
        let Some(t) = tn.typ() else {
            return;
        };
        let Some(ttypes) = structural_types(&mut scratch, pass, t) else {
            return;
        };
        let Some(&t0) = ttypes
            .iter()
            .find(|&&tt| is_string_based(scratch.as_ref().unwrap_or(orig_types), tt)) else {
            return;
        };

        let Some(tv) = info.types.get(&arg.id()) else {
            return;
        };
        let v = tv.typ;
        let val = tv.val.as_ref();
        let Some(vtypes) = structural_types(&mut scratch, pass, v) else {
            return;
        };
        let Some(&v0) = vtypes
            .iter()
            .find(|&&vt| is_problematic_int(scratch.as_ref().unwrap_or(orig_types), vt)) else {
            return;
        };

        let convertible = vtypes
            .iter()
            .all(|&vt| convertible_to_rune(&mut scratch, pass, vt));
        let types = scratch.as_ref().unwrap_or(orig_types);

        let target = describe(types, objects, t0, t, tn.name());
        let source = describe(types, objects, v0, v, &type_name(types, objects, v));
        if target.is_empty() || source.is_empty() {
            return;
        }
        let message = format!(
            "conversion from {source} to {target} yields a string of one rune, not a string of digits"
        );
        if !convertible {
            pending.push((call.pos().0 as u32, message, Vec::new()));
            return;
        }

        let (arg_pos, arg_end) = (arg.pos().0 as u32, arg.end().0 as u32);
        // `fmt.Sprintf("%verb", x)`, keeping the conversion unless T is exactly
        // string. None only when the file cannot be found.
        let sprintf = |verb: &str| -> Option<Vec<TextEdit>> {
            let (start, end) = if is_exactly_string(types, t) {
                (call.pos().0 as u32, call.end().0 as u32)
            } else {
                (arg_pos, arg_end)
            };
            let file = refactor::enclosing_file(pass, call.pos().0 as u32)?;
            let (prefix, mut edits) =
                refactor::add_import(pass, file, "fmt", "fmt", "Sprintf", arg_pos)?;
            edits.push(TextEdit {
                pos: start,
                end: arg_pos,
                new_text: format!("{prefix}Sprintf(\"%{verb}\", "),
            });
            edits.push(TextEdit {
                pos: arg_end,
                end,
                new_text: ")".into(),
            });
            Some(edits)
        };

        // Type parameters have too many combinations, and a method (such as
        // Format) may change what fmt prints.
        let sprintf_ok = ttypes.len() == 1 && vtypes.len() == 1 && method_set_empty(types, objects, v0);

        let mut fixes: Vec<(String, Vec<TextEdit>)> = Vec::new();
        if rune_is_exact(types, val, &vtypes) {
            fixes.push((
                "Convert a single rune to a string".into(),
                vec![
                    TextEdit {
                        pos: arg_pos,
                        end: arg_pos,
                        new_text: "rune(".into(),
                    },
                    TextEdit {
                        pos: arg_end,
                        end: arg_end,
                        new_text: ")".into(),
                    },
                ],
            ));
        } else if sprintf_ok
            && !matches!(types.get(unalias_readonly(types, v)), TypeData::TypeParam(_))
            && !is_untyped_const(types, v, val)
        {
            match sprintf("c") {
                Some(edits) => {
                    fixes.push(("Convert single rune to string (preserves behavior)".into(), edits))
                }
                None => {
                    pending.push((call.pos().0 as u32, message, Vec::new()));
                    return;
                }
            }
        } else {
            // No behaviour-preserving fix, so no fixes.
            pending.push((call.pos().0 as u32, message, Vec::new()));
            return;
        }
        if sprintf_ok {
            if let Some(edits) = sprintf("d") {
                fixes.push(("Format number as decimal (changes behavior)".into(), edits));
            }
        }
        pending.push((call.pos().0 as u32, message, fixes));
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
