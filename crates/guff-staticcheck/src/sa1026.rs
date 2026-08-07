//! SA1026 — cannot marshal channels or functions (JSON/XML).
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1026`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

use crate::fakejson;

fn check_marshal(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let Some(arg) = call.args.first() else {
        return;
    };
    let typ = callcheck::ssa_value_type(ctx.prog, ctx.caller, arg.value);
    let arena = &ctx.prog.type_arena;
    let objects = &ctx.prog.object_arena;
    let packages = &ctx.prog.package_arena;
    if let Some(err) = fakejson::marshal(arena, objects, packages, typ) {
        call.args[0].invalid(fakejson::format_marshal_error(
            arena, objects, packages, &err,
        ));
    }
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        // Upstream's rule table is exactly these four. `MarshalIndent` is not
        // on it — checking it too made consul's
        // `json.MarshalIndent(bound, …)` a guff-only finding.
        HashMap::from([
            ("encoding/json.Marshal", check_marshal as callcheck::CheckFn),
            (
                "(*encoding/json.Encoder).Encode",
                check_marshal as callcheck::CheckFn,
            ),
            ("encoding/xml.Marshal", check_marshal as callcheck::CheckFn),
            (
                "(*encoding/xml.Encoder).Encode",
                check_marshal as callcheck::CheckFn,
            ),
        ])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1026 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1026_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1026",
        doc: "cannot marshal channels or functions",
        url: "https://staticcheck.dev/docs/checks/#SA1026",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1026 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1026_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1026_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
