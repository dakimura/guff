//! S1016 — use a type conversion instead of manually copying struct fields.
//!
//! Port of `honnef.co/go/tools/simple/s1016`.

use std::sync::OnceLock;

use guff::ast::{CompositeLit, Expr, Ident, SelectorExpr};
use guff::walk::NodeRef;
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
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

fn get_sel_source(pass: &Pass<'_>, expr: &Expr) -> Option<(TypeId, guff_types::ObjectId)> {
    let Expr::SelectorExpr(SelectorExpr { x, .. }) = expr else {
        return None;
    };
    let Expr::Ident(ident) = &**x else {
        return None;
    };
    let typ = pass.types_info()?.types.get(&x.id()).map(|tv| tv.typ)?;
    let obj = object_of(pass, ident)?;
    Some((typ, obj))
}

fn check_composite_lit(pass: &Pass<'_>, lit: &CompositeLit) -> Option<String> {
    let ty_expr = lit.ty.as_ref()?;
    let dst_typ = pass.types_info()?.types.get(&ty_expr.id()).map(|tv| tv.typ)?;
    let field_count = struct_field_count(pass, dst_typ)?;
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
        if field_name_at(pass, dst_typ, i).as_deref() != Some(field_name) {
            return None;
        }
        let (t, obj) = get_sel_source(pass, &Expr::SelectorExpr(sel.clone()))?;
        if let Some(prev) = src_obj {
            if prev != obj {
                return None;
            }
        } else {
            src_obj = Some(obj);
        }
        if let Some(prev) = src_typ {
            if prev != t {
                return None;
            }
        } else {
            src_typ = Some(t);
        }
    }

    let src_typ = src_typ?;
    let src_obj = src_obj?;
    if dst_typ == src_typ {
        return None;
    }

    let ident_name = {
        let artifacts = pass.pkg().type_artifacts.as_ref()?;
        src_obj.name(&artifacts.objects).to_string()
    };
    Some(format!(
        "should convert {ident_name} (type {}) to {} instead of using struct literal",
        render_type(pass, src_typ)?,
        render_type(pass, dst_typ)?
    ))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1016 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::CompositeLit(lit) = n else {
            return;
        };
        if let Some(msg) = check_composite_lit(pass, lit) {
            pending.push((lit.lbrace.0 as u32, msg));
        }
    });

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
        requires: vec![inspect::analyzer()],
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
