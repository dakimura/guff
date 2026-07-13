//! SA1029 — inappropriate key in `context.WithValue`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1029`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_with_value_key(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let Some(arg) = call.args.get(1) else {
        return;
    };
    let arena = &ctx.prog.type_arena;
    let objects = &ctx.prog.object_arena;
    let packages = &ctx.prog.package_arena;
    let typ = callcheck::ssa_value_type(ctx.prog, ctx.caller, arg.value);

    if let Some((basic, alias)) =
        callcheck::builtin_key_type(arena, objects, packages, typ)
    {
        let msg = if let Some(alias) = alias {
            format!(
                "should not use built-in type {basic} (via alias {alias}) as key for value; define your own type to avoid collisions"
            )
        } else {
            format!(
                "should not use built-in type {basic} as key for value; define your own type to avoid collisions"
            )
        };
        call.args[1].invalid(msg);
        return;
    }

    if callcheck::is_empty_struct_type(arena, typ) {
        call.args[1].invalid(
            "should not use empty anonymous struct as key for value; define your own type to avoid collisions",
        );
        return;
    }

    let mut seen = std::collections::HashSet::new();
    if !callcheck::is_comparable_type(arena, objects, typ, &mut seen) {
        let name = callcheck::render_type(arena, objects, packages, typ);
        call.args[1].invalid(format!(
            "keys used with context.WithValue must be comparable, but type {name} is not comparable"
        ));
    }
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([(
            "context.WithValue",
            check_with_value_key as callcheck::CheckFn,
        )])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1029 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1029_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1029",
        doc: "inappropriate key in call to context.WithValue",
        url: "https://staticcheck.dev/docs/checks/#SA1029",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1029 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1029_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1029_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
