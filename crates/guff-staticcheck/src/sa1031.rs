//! SA1031 — overlapping `dst` and `src` byte slices in encoders.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1031`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_overlapping(call: &mut Call<'_>, ctx: &CallContext<'_>, dst_arg: usize, src_arg: usize) {
    let dst_value = call.args.get(dst_arg).map(|a| a.value);
    let src_value = call.args.get(src_arg).map(|a| a.value);
    let (Some(dst_value), Some(src_value)) = (dst_value, src_value) else {
        return;
    };
    if callcheck::is_ssa_const(ctx.caller, dst_value)
        || callcheck::is_ssa_const(ctx.caller, src_value)
    {
        return;
    }
    if dst_value.value() == src_value.value() {
        call.args[dst_arg].invalid("overlapping dst and src");
        return;
    }
    let Some(dst_slice) = callcheck::slice_from_value(ctx.caller, dst_value) else {
        return;
    };
    let Some(src_slice) = callcheck::slice_from_value(ctx.caller, src_value) else {
        return;
    };
    let dst_x = callcheck::flatten_ir_value(ctx.caller, dst_slice.x).unwrap_or(dst_slice.x);
    let src_x = callcheck::flatten_ir_value(ctx.caller, src_slice.x).unwrap_or(src_slice.x);
    if dst_x != src_x {
        return;
    }
    if callcheck::slice_bounds_equal(ctx.prog, ctx.caller, dst_slice.low, src_slice.low) {
        call.args[dst_arg].invalid("overlapping dst and src");
    }
}

fn check_hex_encode(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_overlapping(call, ctx, 0, 1);
}

fn check_ascii85_encode(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_overlapping(call, ctx, 0, 1);
}

fn check_base32_encode(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_overlapping(call, ctx, 1, 2);
}

fn check_base64_encode(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_overlapping(call, ctx, 1, 2);
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([
            ("encoding/ascii85.Encode", check_ascii85_encode as callcheck::CheckFn),
            (
                "(*encoding/base32.Encoding).Encode",
                check_base32_encode as callcheck::CheckFn,
            ),
            (
                "(*encoding/base64.Encoding).Encode",
                check_base64_encode as callcheck::CheckFn,
            ),
            ("encoding/hex.Encode", check_hex_encode as callcheck::CheckFn),
        ])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1031 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1031_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1031",
        doc: "overlapping byte slices passed to an encoder",
        url: "https://staticcheck.dev/docs/checks/#SA1031",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1031 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1031_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1031_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
