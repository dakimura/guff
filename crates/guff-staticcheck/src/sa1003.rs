//! SA1003 — unsupported argument to `encoding/binary.Write`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1003`.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, render_type, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::alias::unalias_readonly;
use guff_types::arena::{ObjectArena, TypeArena, TypeData};
use guff_types::basic::BasicKind;
use guff_types::TypeId;

fn can_binary_marshal(arena: &TypeArena, objects: &ObjectArena, typ: TypeId) -> bool {
    let mut seen = HashSet::new();
    valid_encoding_binary_type(arena, objects, typ, &mut seen)
}

fn valid_encoding_binary_type(
    arena: &TypeArena,
    objects: &ObjectArena,
    typ: TypeId,
    seen: &mut HashSet<TypeId>,
) -> bool {
    if !seen.insert(typ) {
        return true;
    }

    let mut cur = unalias_readonly(arena, typ);
    if let TypeData::Pointer(p) = arena.get(cur) {
        cur = p.elem().underlying(arena);
    }
    if let TypeData::Array(a) = arena.get(cur) {
        cur = a.elem().underlying(arena);
    } else if let TypeData::Slice(s) = arena.get(cur) {
        cur = s.elem().underlying(arena);
    }

    let u = cur.underlying(arena);
    match arena.get(u) {
        TypeData::Basic(b) => match b.kind() {
            BasicKind::Uint8
            | BasicKind::Uint16
            | BasicKind::Uint32
            | BasicKind::Uint64
            | BasicKind::Int8
            | BasicKind::Int16
            | BasicKind::Int32
            | BasicKind::Int64
            | BasicKind::Float32
            | BasicKind::Float64
            | BasicKind::Complex64
            | BasicKind::Complex128
            | BasicKind::Invalid => true,
            BasicKind::Bool => true,
            _ => false,
        },
        TypeData::Struct(s) => {
            for i in 0..s.num_fields() {
                let f = s.field(i);
                let ftyp = f.typ(objects).expect("field type");
                if !valid_encoding_binary_type(arena, objects, ftyp, seen) {
                    return false;
                }
            }
            true
        }
        TypeData::Array(a) => valid_encoding_binary_type(arena, objects, a.elem(), seen),
        TypeData::Interface(_) => true,
        _ => false,
    }
}

fn check_binary_write(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let Some(arg) = call.args.get_mut(2) else {
        return;
    };
    let typ = callcheck::ssa_value_type(ctx.prog, ctx.caller, arg.value);
    let arena = &ctx.prog.type_arena;
    let objects = &ctx.prog.object_arena;
    let packages = &ctx.prog.package_arena;
    if !can_binary_marshal(arena, objects, typ) {
        let name = render_type(arena, objects, packages, typ);
        arg.invalid(format!(
            "value of type {name} cannot be used with binary.Write"
        ));
    }
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([(
            "encoding/binary.Write",
            check_binary_write as callcheck::CheckFn,
        )])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1003 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1003_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1003",
        doc: "unsupported argument to functions in encoding/binary",
        url: "https://staticcheck.dev/docs/checks/#SA1003",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1003 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1003_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1003_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
