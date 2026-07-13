//! SA9005 — trying to marshal a struct with no public fields.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa9005`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, render_type, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::alias::unalias_readonly;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::object::is_exported;

fn check_marshal(call: &mut Call<'_>, ctx: &CallContext<'_>, arg_idx: usize) {
    let Some(arg) = call.args.get_mut(arg_idx) else {
        return;
    };
    let typ = callcheck::ssa_value_type(ctx.prog, ctx.caller, arg.value);
    let arena = &ctx.prog.type_arena;
    let objects = &ctx.prog.object_arena;
    let u = unalias_readonly(arena, typ).underlying(arena);
    let TypeData::Struct(s) = arena.get(u) else {
        return;
    };
    if s.num_fields() == 0 {
        return;
    }
    for i in 0..s.num_fields() {
        let field = s.field(i);
        let ObjectData::Var(v) = objects.get(field) else {
            continue;
        };
        if is_exported(v.name()) {
            return;
        }
    }
    let name = render_type(arena, objects, &ctx.prog.package_arena, typ);
    arg.invalid(format!(
        "struct type '{name}' doesn't have any exported fields, nor custom marshaling"
    ));
}

fn check0(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_marshal(call, ctx, 0);
}

fn check1(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_marshal(call, ctx, 1);
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([
            ("encoding/json.Marshal", check0 as callcheck::CheckFn),
            ("encoding/json.MarshalIndent", check0 as callcheck::CheckFn),
            ("encoding/xml.Marshal", check0 as callcheck::CheckFn),
            ("encoding/xml.MarshalIndent", check0 as callcheck::CheckFn),
            ("(*encoding/json.Encoder).Encode", check0 as callcheck::CheckFn),
            ("(*encoding/xml.Encoder).Encode", check0 as callcheck::CheckFn),
            ("encoding/json.Unmarshal", check1 as callcheck::CheckFn),
            ("encoding/xml.Unmarshal", check1 as callcheck::CheckFn),
            ("(*encoding/json.Decoder).Decode", check0 as callcheck::CheckFn),
            ("(*encoding/xml.Decoder).Decode", check0 as callcheck::CheckFn),
        ])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA9005 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa9005_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA9005",
        doc: "trying to marshal a struct with no public fields nor custom marshaling",
        url: "https://staticcheck.dev/docs/checks/#SA9005",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa9005_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa9005_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
