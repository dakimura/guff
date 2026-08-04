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
use guff_types::lookup::{lookup_field_or_method, LookupResult};
use guff_types::object::is_exported;
use guff_types::TypeId;

fn has_method(ctx: &CallContext<'_>, typ: TypeId, name: &str) -> bool {
    let mut types = ctx.prog.type_arena.clone();
    match lookup_field_or_method(
        &mut types,
        &ctx.prog.object_arena,
        &ctx.prog.package_arena,
        typ,
        true,
        None,
        name,
    ) {
        LookupResult::Found { obj, .. } => {
            matches!(ctx.prog.object_arena.get(obj), ObjectData::Func(_))
        }
        _ => false,
    }
}

fn has_custom_marshaling(ctx: &CallContext<'_>, typ: TypeId, meths: &[&str]) -> bool {
    meths.iter().any(|m| has_method(ctx, typ, m))
}

fn check_marshal(call: &mut Call<'_>, ctx: &CallContext<'_>, arg_idx: usize, meths: &[&str]) {
    let Some(arg) = call.args.get_mut(arg_idx) else {
        return;
    };
    let typ = callcheck::ssa_value_type(ctx.prog, ctx.caller, arg.value);
    let arena = &ctx.prog.type_arena;
    let objects = &ctx.prog.object_arena;
    // Custom marshaling (MarshalJSON / …) lives on the named type / pointer
    // method set — check before looking at struct fields (upstream SA9005).
    if has_custom_marshaling(ctx, typ, meths) {
        return;
    }
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

fn check_json0(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_marshal(call, ctx, 0, &["MarshalJSON", "MarshalText"]);
}

fn check_xml0(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_marshal(call, ctx, 0, &["MarshalXML", "MarshalText"]);
}

fn check_json_unmarshal(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_marshal(call, ctx, 1, &["UnmarshalJSON", "UnmarshalText"]);
}

fn check_xml_unmarshal(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_marshal(call, ctx, 1, &["UnmarshalXML", "UnmarshalText"]);
}

fn check_json_decode(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_marshal(call, ctx, 0, &["UnmarshalJSON", "UnmarshalText"]);
}

fn check_xml_decode(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_marshal(call, ctx, 0, &["UnmarshalXML", "UnmarshalText"]);
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([
            ("encoding/json.Marshal", check_json0 as callcheck::CheckFn),
            ("encoding/json.MarshalIndent", check_json0 as callcheck::CheckFn),
            ("encoding/xml.Marshal", check_xml0 as callcheck::CheckFn),
            ("encoding/xml.MarshalIndent", check_xml0 as callcheck::CheckFn),
            ("(*encoding/json.Encoder).Encode", check_json0 as callcheck::CheckFn),
            ("(*encoding/xml.Encoder).Encode", check_xml0 as callcheck::CheckFn),
            ("encoding/json.Unmarshal", check_json_unmarshal as callcheck::CheckFn),
            ("encoding/xml.Unmarshal", check_xml_unmarshal as callcheck::CheckFn),
            ("(*encoding/json.Decoder).Decode", check_json_decode as callcheck::CheckFn),
            ("(*encoding/xml.Decoder).Decode", check_xml_decode as callcheck::CheckFn),
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
