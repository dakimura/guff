//! `stringintconv` — check for int-to-string conversions that yield one rune.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/stringintconv` (suggested fixes omitted).

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, Ident, ParenExpr, SelectorExpr, StarExpr};
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::{ObjectData, TypeData};
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
    inspect.preorder(pass.files(), |n| {
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
        pending.push((
            call.pos().0 as u32,
            format!(
                "conversion from {source} to {target} yields a string of one rune, not a string of digits"
            ),
        ));
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
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
