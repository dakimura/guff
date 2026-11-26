//! SA1014 — non-pointer value passed to `Unmarshal` / `Decode`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1014`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_pointer_arg(call: &mut Call<'_>, ctx: &CallContext<'_>, arg_idx: usize, name: &str) {
    let Some(arg) = call.args.get(arg_idx) else {
        return;
    };
    let typ = callcheck::ssa_value_type(ctx.prog, ctx.caller, arg.value);
    if !callcheck::is_pointer_or_interface_type(&ctx.prog.type_arena, typ) {
        call.args[arg_idx].invalid(format!(
            "{name} expects to unmarshal into a pointer, but the provided value is not a pointer"
        ));
    }
}

fn check_json_unmarshal(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_pointer_arg(call, ctx, 1, "json.Unmarshal");
}

fn check_json_decode(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_pointer_arg(call, ctx, 0, "Decode");
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([
            ("encoding/json.Unmarshal", check_json_unmarshal as callcheck::CheckFn),
            (
                "(*encoding/json.Decoder).Decode",
                check_json_decode as callcheck::CheckFn,
            ),
        ])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1014 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1014_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1014",
        doc: "non-pointer value passed to Unmarshal or Decode",
        url: "https://staticcheck.dev/docs/checks/#SA1014",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1014 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1014_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1014_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
