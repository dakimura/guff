//! S1016 — use a type conversion instead of manually copying struct fields.
//!
//! Port of `honnef.co/go/tools/simple/s1016`.

use std::sync::OnceLock;

use guff::ast::{CompositeLit, Expr, SelectorExpr};
use guff::token::Token;
use guff::walk::{preorder_stack, NodeRef};
use guff_analysis::code::object_of;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::alias::unalias_readonly;
use guff_types::arena::ObjectData;
use guff_types::named::named_obj;
use guff_types::r#struct::struct_field;
use guff_types::{TypeData, TypeId};

fn render_type(pass: &Pass<'_>, typ: TypeId) -> Option<String> {
    let a = pass.pkg().type_artifacts.as_ref()?;
    Some(guff_types::typestring::type_string(
        &a.types,
        &a.objects,
        &a.packages,
        typ,
        None,
    ))
}

fn as_named(pass: &Pass<'_>, typ: TypeId) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Named(_) => Some(typ),
        _ => None,
    }
}

fn struct_field_count(pass: &Pass<'_>, typ: TypeId) -> Option<usize> {
    let types = &pass.pkg().type_artifacts.as_ref()?.types;
    match types.get(typ.underlying(types)) {
        TypeData::Struct(s) => Some(s.num_fields()),
        _ => None,
    }
}

fn field_name_at(pass: &Pass<'_>, typ: TypeId, i: usize) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let types = &artifacts.types;
    let field = struct_field(types, typ.underlying(types), i);
    Some(field.name(&artifacts.objects).to_string())
}

fn named_pkg(pass: &Pass<'_>, named: TypeId) -> Option<guff_types::PackageId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    named_obj(&artifacts.types, named).pkg(&artifacts.objects)
}

fn get_sel_source(pass: &Pass<'_>, sel: &SelectorExpr) -> Option<(TypeId, guff_types::ObjectId)> {
    let Expr::Ident(ident) = &*sel.x else {
        return None;
    };
    let typ = pass.types_info()?.types.get(&sel.x.id()).map(|tv| tv.typ)?;
    let obj = object_of(pass, ident)?;
    Some((typ, obj))
}

fn check_composite_lit(pass: &Pass<'_>, lit: &CompositeLit) -> Option<String> {
    let ty_expr = lit.ty.as_ref()?;
    let dst_typ_raw = pass.types_info()?.types.get(&ty_expr.id()).map(|tv| tv.typ)?;
    let dst_named = as_named(pass, dst_typ_raw)?;
    let field_count = struct_field_count(pass, dst_named)?;
    if lit.elts.is_empty() || field_count != lit.elts.len() {
        return None;
    }

    let mut src_typ = None;
    let mut src_obj = None;

    for (i, elt) in lit.elts.iter().enumerate() {
        let (sel, field_name) = match elt {
            Expr::SelectorExpr(s) => (s, s.sel.name.as_str()),
            Expr::KeyValueExpr(kv) => {
                let Expr::Ident(key) = &*kv.key else {
                    return None;
                };
                let Expr::SelectorExpr(s) = &*kv.value else {
                    return None;
                };
                if key.name != s.sel.name {
                    return None;
                }
                (s, s.sel.name.as_str())
            }
            _ => return None,
        };
        if field_name_at(pass, dst_named, i).as_deref() != Some(field_name) {
            return None;
        }
        let (t, obj) = get_sel_source(pass, sel)?;
        if let Some(prev) = src_obj {
            if prev != obj {
                return None;
            }
        } else {
            src_obj = Some(obj);
        }
        // Upstream only accepts Named (or Alias→Named) source types — not pointers.
        let Some(named) = as_named(pass, t) else {
            return None;
        };
        if let Some(prev) = src_typ {
            if prev != named {
                return None;
            }
        } else {
            src_typ = Some(named);
        }
    }

    let src_named = src_typ?;
    let src_obj = src_obj?;
    if dst_named == src_named {
        return None;
    }

    // Do not suggest conversions across packages (coincidence / fragility).
    if named_pkg(pass, dst_named) != named_pkg(pass, src_named) {
        return None;
    }

    // Field names already matched 1:1; also require underlying struct shapes to
    // agree (types + exported/embedded). Tags are ignored (Go ≥ 1.8), matching
    // upstream `types.IdenticalIgnoreTags`.
    if !structs_convertible(pass, dst_named, src_named) {
        return None;
    }

    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let ident_name = src_obj.name(&artifacts.objects).to_string();
    Some(format!(
        "should convert {ident_name} (type {}) to {} instead of using struct literal",
        crate::render::type_string_rel(pass, src_named)?,
        crate::render::type_string_rel(pass, dst_named)?
    ))
}

fn structs_convertible(pass: &Pass<'_>, dst: TypeId, src: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let types = &artifacts.types;
    let objects = &artifacts.objects;
    let (TypeData::Struct(s1), TypeData::Struct(s2)) =
        (types.get(dst.underlying(types)), types.get(src.underlying(types)))
    else {
        return false;
    };
    if s1.num_fields() != s2.num_fields() {
        return false;
    }
    for i in 0..s1.num_fields() {
        let f1 = struct_field(types, dst.underlying(types), i);
        let f2 = struct_field(types, src.underlying(types), i);
        if f1.name(objects) != f2.name(objects) {
            return false;
        }
        let (ObjectData::Var(v1), ObjectData::Var(v2)) = (objects.get(f1), objects.get(f2)) else {
            return false;
        };
        if v1.embedded() != v2.embedded() {
            return false;
        }
        // Compare field types by TypeId; same checker session shares ids for
        // identical types. Tags intentionally ignored.
        if v1.typ() != v2.typ() {
            return false;
        }
    }
    true
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        let mut stack = Vec::new();
        preorder_stack(NodeRef::File(file), &mut stack, |n, stk| {
            let NodeRef::CompositeLit(lit) = n else {
                return true;
            };
            // Upstream: do not suggest type conversion between pointers (`&T{...}`).
            // `stk` excludes the current node; index 0 from the end is the parent.
            if matches!(
                stk.last(),
                Some(NodeRef::UnaryExpr(u)) if u.op == Token::AND
            ) {
                return true;
            }
            if let Some(msg) = check_composite_lit(pass, lit) {
                pending.push((lit.lbrace.0 as u32, msg));
            }
            true
        });
    }

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1016_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1016",
        doc: "use a type conversion instead of manually copying struct fields",
        url: "https://staticcheck.dev/docs/checks/#S1016",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1016_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1016_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
