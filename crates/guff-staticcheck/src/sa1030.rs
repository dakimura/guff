//! SA1030 — invalid argument in call to a `strconv` function.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1030`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn validate_discrete_bit_size(call: &mut Call<'_>, ctx: &CallContext<'_>, idx: usize, a: i64, b: i64) {
    let Some(arg) = call.args.get_mut(idx) else {
        return;
    };
    let Some(n) = callcheck::extract_const_int(ctx.prog, ctx.caller, arg.value) else {
        return;
    };
    if n != a && n != b {
        arg.invalid(format!(
            "'bitSize' argument is invalid, must be either {a} or {b}"
        ));
    }
}

fn validate_continuous_bit_size(
    call: &mut Call<'_>,
    ctx: &CallContext<'_>,
    idx: usize,
    min: i64,
    max: i64,
) {
    let Some(arg) = call.args.get_mut(idx) else {
        return;
    };
    let Some(n) = callcheck::extract_const_int(ctx.prog, ctx.caller, arg.value) else {
        return;
    };
    if n < min || n > max {
        arg.invalid(format!(
            "'bitSize' argument is invalid, must be within {min} and {max}"
        ));
    }
}

fn validate_int_base(call: &mut Call<'_>, ctx: &CallContext<'_>, idx: usize) {
    let Some(arg) = call.args.get_mut(idx) else {
        return;
    };
    let Some(n) = callcheck::extract_const_int(ctx.prog, ctx.caller, arg.value) else {
        return;
    };
    if n < 2 {
        arg.invalid("'base' must not be smaller than 2");
    }
    if n > 36 {
        arg.invalid("'base' must not be larger than 36");
    }
}

fn validate_int_base_allow_zero(call: &mut Call<'_>, ctx: &CallContext<'_>, idx: usize) {
    let Some(arg) = call.args.get_mut(idx) else {
        return;
    };
    let Some(n) = callcheck::extract_const_int(ctx.prog, ctx.caller, arg.value) else {
        return;
    };
    if n < 2 && n != 0 {
        arg.invalid("'base' must not be smaller than 2, unless it is 0");
    }
    if n > 36 {
        arg.invalid("'base' must not be larger than 36");
    }
}

fn validate_float_format(call: &mut Call<'_>, ctx: &CallContext<'_>, idx: usize) {
    let Some(arg) = call.args.get_mut(idx) else {
        return;
    };
    let Some(n) = callcheck::extract_const_int(ctx.prog, ctx.caller, arg.value) else {
        return;
    };
    match n as u8 as char {
        'b' | 'e' | 'E' | 'f' | 'g' | 'G' | 'x' | 'X' => {}
        c => arg.invalid(format!("'fmt' argument is invalid: unknown format '{c}'")),
    }
}

fn check_parse_complex(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    validate_discrete_bit_size(call, ctx, 1, 64, 128);
}

fn check_parse_float(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    validate_discrete_bit_size(call, ctx, 1, 32, 64);
}

fn check_parse_int(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    validate_continuous_bit_size(call, ctx, 2, 0, 64);
    validate_int_base_allow_zero(call, ctx, 1);
}

fn check_parse_uint(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    validate_continuous_bit_size(call, ctx, 2, 0, 64);
    validate_int_base_allow_zero(call, ctx, 1);
}

fn check_format_complex(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    validate_float_format(call, ctx, 1);
    validate_discrete_bit_size(call, ctx, 3, 64, 128);
}

fn check_format_float(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    validate_float_format(call, ctx, 1);
    validate_discrete_bit_size(call, ctx, 3, 32, 64);
}

fn check_format_int(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    validate_int_base(call, ctx, 1);
}

fn check_format_uint(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    validate_int_base(call, ctx, 1);
}

fn check_append_float(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    validate_float_format(call, ctx, 2);
    validate_discrete_bit_size(call, ctx, 4, 32, 64);
}

fn check_append_int(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    validate_int_base(call, ctx, 2);
}

fn check_append_uint(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    validate_int_base(call, ctx, 2);
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([
            ("strconv.ParseComplex", check_parse_complex as callcheck::CheckFn),
            ("strconv.ParseFloat", check_parse_float as callcheck::CheckFn),
            ("strconv.ParseInt", check_parse_int as callcheck::CheckFn),
            ("strconv.ParseUint", check_parse_uint as callcheck::CheckFn),
            ("strconv.FormatComplex", check_format_complex as callcheck::CheckFn),
            ("strconv.FormatFloat", check_format_float as callcheck::CheckFn),
            ("strconv.FormatInt", check_format_int as callcheck::CheckFn),
            ("strconv.FormatUint", check_format_uint as callcheck::CheckFn),
            ("strconv.AppendFloat", check_append_float as callcheck::CheckFn),
            ("strconv.AppendInt", check_append_int as callcheck::CheckFn),
            ("strconv.AppendUint", check_append_uint as callcheck::CheckFn),
        ])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1030 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1030_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1030",
        doc: "invalid argument in call to a strconv function",
        url: "https://staticcheck.dev/docs/checks/#SA1030",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1030 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1030_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1030_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
