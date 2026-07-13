//! SA6002 — storing non-pointer values in `sync.Pool` allocates memory.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa6002`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::TypeData;

fn check_put(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let Some(arg) = call.args.get_mut(0) else {
        return;
    };
    let typ = callcheck::ssa_value_type(ctx.prog, ctx.caller, arg.value);
    let arena = &ctx.prog.type_arena;
    let u = typ.underlying(arena);
    let is_slice = matches!(arena.get(u), TypeData::Slice(_));
    if !callcheck::is_pointer_or_interface_type(arena, typ) || is_slice {
        arg.invalid("argument should be pointer-like to avoid allocations");
    }
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([(
            "(*sync.Pool).Put",
            check_put as callcheck::CheckFn,
        )])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA6002 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa6002_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA6002",
        doc: "storing non-pointer values in sync.Pool allocates memory",
        url: "https://staticcheck.dev/docs/checks/#SA6002",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa6002_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa6002_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
