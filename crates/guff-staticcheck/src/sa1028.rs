//! SA1028 — `sort.Slice*` called on a non-slice.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1028`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::TypeData;

fn check_slice(call: &mut Call<'_>, ctx: &CallContext<'_>, func: &str) {
    let Some(arg) = call.args.get_mut(0) else {
        return;
    };
    let arena = &ctx.prog.type_arena;
    let typ = callcheck::ssa_value_type(ctx.prog, ctx.caller, arg.value);
    let underlying = typ.underlying(arena);
    match arena.get(underlying) {
        TypeData::Slice(_) => {}
        TypeData::Interface(_) => {
            if callcheck::is_nil_const(ctx.prog, ctx.caller, arg.value) {
                arg.invalid(format!("cannot call {func} on nil literal"));
            }
        }
        other => {
            let name = callcheck::render_type(
                arena,
                &ctx.prog.object_arena,
                &ctx.prog.package_arena,
                underlying,
            );
            let _ = other;
            arg.invalid(format!(
                "{func} must only be called on slices, was called on {name}"
            ));
        }
    }
}

fn check_sort_slice(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_slice(call, ctx, "sort.Slice");
}

fn check_sort_slice_is_sorted(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_slice(call, ctx, "sort.SliceIsSorted");
}

fn check_sort_slice_stable(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_slice(call, ctx, "sort.SliceStable");
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([
            ("sort.Slice", check_sort_slice as callcheck::CheckFn),
            (
                "sort.SliceIsSorted",
                check_sort_slice_is_sorted as callcheck::CheckFn,
            ),
            (
                "sort.SliceStable",
                check_sort_slice_stable as callcheck::CheckFn,
            ),
        ])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1028 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1028_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1028",
        doc: "sort.Slice can only be used on slices",
        url: "https://staticcheck.dev/docs/checks/#SA1028",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1028 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1028_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1028_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
