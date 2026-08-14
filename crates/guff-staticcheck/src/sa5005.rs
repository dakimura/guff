//! SA5005 — finalizer references the finalized object.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5005`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::token::Token;
use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::instr::{InstrData, UnOp};
use guff_ssa::value::Value;

/// Upstream's conditions are exact, and all three have to hold:
///
/// 1. the object argument is a **load of an `Alloc`** — a variable still in
///    memory, which is what being captured by a closure forces;
/// 2. the finalizer argument is a **`MakeClosure`**;
/// 3. one of that closure's bindings **is that same `Alloc`**.
///
/// Both arguments are `any`, so both arrive boxed; upstream strips the
/// `MakeInterface` first, and `callcheck` here does the same before a rule sees
/// them.
///
/// Measured on 2026-08-14: golangci-lint 2.12.2 reports **nothing** for this
/// check, on any shape tried — including the example in its own documentation
/// (`x := &Foo{}; runtime.SetFinalizer(x, func(y *Foo) { … x … })`). The AST
/// approximation that used to live here reported all of them.
///
/// **Known unverifiable difference:** upstream's message ends with `(at %s)`
/// naming the closure's position. Since no upstream finding can be produced to
/// compare against, that suffix is left off rather than guessed at.
fn check_set_finalizer(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let Some(obj) = call.args.first() else {
        return;
    };
    let Some(fin) = call.args.get(1) else {
        return;
    };

    // The object must be a load of an addressed local.
    let Value::Instr(load_id) = obj.value.value() else {
        return;
    };
    let InstrData::UnOp(UnOp { op: Token::MUL, x: loaded, .. }) = ctx.caller.instrs.get(load_id)
    else {
        return;
    };
    let Value::Instr(alloc_id) = *loaded else {
        return;
    };
    if !matches!(ctx.caller.instrs.get(alloc_id), InstrData::Alloc(_)) {
        return;
    }

    // The finalizer must be a closure that binds that same cell.
    let Value::Instr(mc_id) = fin.value.value() else {
        return;
    };
    let InstrData::MakeClosure(mc) = ctx.caller.instrs.get(mc_id) else {
        return;
    };
    if mc.bindings.iter().any(|b| *b == *loaded) {
        call.invalid(
            "the finalizer closes over the object, preventing the finalizer from ever running",
        );
    }
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([(
            "runtime.SetFinalizer",
            check_set_finalizer as callcheck::CheckFn,
        )])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA5005 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa5005_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA5005",
        doc: "the finalizer references the finalized object, preventing garbage collection",
        url: "https://staticcheck.dev/docs/checks/#SA5005",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa5005_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa5005_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
