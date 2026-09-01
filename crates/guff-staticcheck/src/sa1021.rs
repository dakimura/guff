//! SA1021 — using `bytes.Equal` to compare two `net.IP`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1021`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

/// `isConvertedFrom(v, "net.IP")`: the value must **be** a conversion, and the
/// thing it converts must be a `net.IP`. Asking instead whether the argument's
/// type is `net.IP` — which is what this did while `ssa_value_type` peeled
/// `ChangeType` for every rule — answers yes for a plain `net.IP` operand that
/// was never converted at all.
fn is_converted_from_net_ip(ctx: &CallContext<'_>, value: callcheck::SsaValue) -> bool {
    let Some(typ) = callcheck::change_type_operand_type(ctx.prog, ctx.caller, value) else {
        return false;
    };
    // `types.TypeString(types.Unalias(change.X.Type()), nil)`: an alias of
    // `net.IP` is a `net.IP`. Without the unalias, `type myIP = net.IP`
    // renders as the alias's own name and the comparison fails.
    let typ = guff_types::alias::unalias_readonly(&ctx.prog.type_arena, typ);
    callcheck::render_type(
        &ctx.prog.type_arena,
        &ctx.prog.object_arena,
        &ctx.prog.package_arena,
        typ,
    ) == "net.IP"
}

fn check(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let (Some(a), Some(b)) = (call.args.get(0), call.args.get(1)) else {
        return;
    };
    if is_converted_from_net_ip(ctx, a.value) && is_converted_from_net_ip(ctx, b.value) {
        call.invalid("use net.IP.Equal to compare net.IPs, not bytes.Equal");
    }
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| HashMap::from([("bytes.Equal", check as callcheck::CheckFn)]))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1021 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1021_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1021",
        doc: "using bytes.Equal to compare two net.IP",
        url: "https://staticcheck.dev/docs/checks/#SA1021",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1021 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1021_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1021_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
