//! SA1027 — 64-bit atomic access must be 64-bit aligned on 32-bit platforms.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1027`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext, SsaValue};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::TypeData;
use guff_types::pointer::pointer_elem;
use guff_types::r#struct::{struct_field, struct_num_fields};

const ATOMIC_INT64_FUNCS: &[&str] = &[
    "sync/atomic.AddInt64",
    "sync/atomic.AddUint64",
    "sync/atomic.CompareAndSwapInt64",
    "sync/atomic.CompareAndSwapUint64",
    "sync/atomic.LoadInt64",
    "sync/atomic.LoadUint64",
    "sync/atomic.StoreInt64",
    "sync/atomic.StoreUint64",
    "sync/atomic.SwapInt64",
    "sync/atomic.SwapUint64",
];

fn check_atomic_alignment(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    if ctx.sizes.word_size != 4 {
        return;
    }
    let Some(arg) = call.args.first_mut() else {
        return;
    };
    let Some((struct_ptr, field_index)) =
        callcheck::field_addr_from_value(ctx.caller, arg.value)
    else {
        return;
    };

    let arena = &ctx.prog.type_arena;
    let objects = &ctx.prog.object_arena;
    let ptr_typ = callcheck::ssa_value_type(ctx.prog, ctx.caller, SsaValue::new(struct_ptr));
    let TypeData::Pointer(_) = arena.get(ptr_typ.underlying(arena)) else {
        return;
    };
    let elem = pointer_elem(arena, ptr_typ.underlying(arena));
    let u_struct = elem.underlying(arena);
    let TypeData::Struct(_) = arena.get(u_struct) else {
        return;
    };

    let n = struct_num_fields(arena, u_struct);
    if field_index >= n {
        return;
    }
    let fields: Vec<_> = (0..=field_index)
        .map(|i| struct_field(arena, u_struct, i))
        .collect();
    let offsets = ctx
        .sizes
        .offsetsof(arena, objects, &ctx.prog.package_arena, &fields);
    let off = offsets[field_index];
    if off < 0 || off % 8 != 0 {
        let field_name = struct_field(arena, u_struct, field_index).name(objects);
        let func = callcheck::call_target_name(ctx, call.common).unwrap_or_else(|| {
            "sync/atomic.*".to_string()
        });
        call.invalid(format!(
            "address of non 64-bit aligned field {field_name} passed to {func}"
        ));
    }
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        ATOMIC_INT64_FUNCS
            .iter()
            .map(|&name| (name, check_atomic_alignment as callcheck::CheckFn))
            .collect()
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1027 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1027_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1027",
        doc: "atomic access to 64-bit variable must be 64-bit aligned",
        url: "https://staticcheck.dev/docs/checks/#SA1027",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1027 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1027_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1027_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
