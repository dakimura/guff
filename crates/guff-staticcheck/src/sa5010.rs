//! SA5010 — impossible type assertion.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5010` (simplified).

use std::sync::OnceLock;

use guff_analysis::callcheck::{render_type, ssa_value_type, SsaValue};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::instr::{InstrData, TypeAssert};
use guff_types::arena::TypeData;
use guff_types::signature::{signature_params, signature_results};
use guff_types::tuple::tuple_at;

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut reports = Vec::new();
    {
        let ir = pass
            .result_of::<buildir::BuildIrResult>(buildir::analyzer())
            .ok_or_else(|| "SA5010 requires buildir analyzer".to_string())?;

        for &fid in &ir.src_funcs {
            let func = ir.prog.functions.get(fid);
            for (_, block) in func.live_blocks() {
                for &iid in &block.instrs {
                    let InstrData::TypeAssert(TypeAssert {
                        x,
                        assert_type,
                        ..
                    }) = func.instrs.get(iid)
                    else {
                        continue;
                    };
                    let left = ssa_value_type(&ir.prog, func, SsaValue::new(*x));
                    let arena = &ir.prog.type_arena;
                    let right_u = assert_type.underlying(arena);
                    let TypeData::Interface(right_iface) = arena.get(right_u) else {
                        continue;
                    };
                    let left_u = left.underlying(arena);
                    let TypeData::Interface(left_iface) = arena.get(left_u) else {
                        continue;
                    };
                    let mut wrong = Vec::new();
                    for i in 0..right_iface.num_explicit_methods() {
                        let mr = right_iface.explicit_method(i);
                        let mr_name = mr.name(&ir.prog.object_arena);
                        let Some(ml) = (0..left_iface.num_explicit_methods())
                            .map(|j| left_iface.explicit_method(j))
                            .find(|m| m.name(&ir.prog.object_arena) == mr_name)
                        else {
                            continue;
                        };
                        let ml_sig = ml.typ(&ir.prog.object_arena).unwrap();
                        let mr_sig = mr.typ(&ir.prog.object_arena).unwrap();
                        if !signatures_assignable(arena, &ir.prog.object_arena, ml_sig, mr_sig) {
                            wrong.push((ml, mr));
                        }
                    }
                    if wrong.is_empty() {
                        continue;
                    }
                    // The two interface names are rendered with
                    // `types.RelativeTo(pass.Pkg)` — local types appear bare —
                    // but the method signatures below keep the nil qualifier
                    // and stay fully qualified. Verified against
                    // golangci-lint 2.12.2 with a local type in the signature.
                    let left_s = crate::render::type_string_rel(pass, left).unwrap_or_else(|| {
                        render_type(arena, &ir.prog.object_arena, &ir.prog.package_arena, left)
                    });
                    let right_s = crate::render::type_string_rel(pass, *assert_type)
                        .unwrap_or_else(|| {
                            render_type(
                                arena,
                                &ir.prog.object_arena,
                                &ir.prog.package_arena,
                                *assert_type,
                            )
                        });
                    let mut msg = format!(
                        "impossible type assertion; {left_s} and {right_s} contradict each other:"
                    );
                    for (ml, mr) in wrong {
                        msg.push_str(&format!(
                            "\n\twrong type for {} method\n\t\thave {}\n\t\twant {}",
                            ml.name(&ir.prog.object_arena),
                            render_sig(arena, &ir.prog.object_arena, &ir.prog.package_arena, ml),
                            render_sig(arena, &ir.prog.object_arena, &ir.prog.package_arena, mr),
                        ));
                    }
                    reports.push((func.pos(iid).0 as u32, msg));
                }
            }
        }
    }
    // Upstream reports the `*ir.TypeAssert`, whose `Source()` is the
    // TypeAssertExpr, so the finding lands on the start of the asserted operand
    // rather than on the `(` of `.(T)` that guff-ssa stamps.
    let starts = (!reports.is_empty())
        .then(|| guff_analysis::call_node_starts(pass))
        .unwrap_or_default();
    for (pos, msg) in reports {
        pass.reportf(starts.get(&pos).copied().unwrap_or(pos), msg);
    }
    Ok(None)
}

fn render_sig(
    arena: &guff_types::arena::TypeArena,
    objects: &guff_types::arena::ObjectArena,
    packages: &guff_types::arena::PackageArena,
    f: guff_types::ObjectId,
) -> String {
    let sig = f.typ(objects).unwrap();
    render_type(arena, objects, packages, sig)
}

fn signatures_assignable(
    arena: &guff_types::arena::TypeArena,
    objects: &guff_types::arena::ObjectArena,
    have: guff_types::TypeId,
    want: guff_types::TypeId,
) -> bool {
    let have_params = signature_params(arena, have);
    let want_params = signature_params(arena, want);
    let have_results = signature_results(arena, have);
    let want_results = signature_results(arena, want);
    tuple_sig_equal_optional(arena, objects, have_params, want_params)
        && tuple_sig_equal_optional(arena, objects, have_results, want_results)
}

fn tuple_sig_equal_optional(
    arena: &guff_types::arena::TypeArena,
    objects: &guff_types::arena::ObjectArena,
    a: Option<guff_types::TypeId>,
    b: Option<guff_types::TypeId>,
) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => tuple_sig_equal(arena, objects, a, b),
        _ => false,
    }
}

fn tuple_sig_equal(
    arena: &guff_types::arena::TypeArena,
    objects: &guff_types::arena::ObjectArena,
    a: guff_types::TypeId,
    b: guff_types::TypeId,
) -> bool {
    let na = guff_types::tuple::tuple_len(arena, Some(a));
    let nb = guff_types::tuple::tuple_len(arena, Some(b));
    if na != nb {
        return false;
    }
    (0..na).all(|i| {
        let ta = tuple_at(arena, a, i).typ(objects);
        let tb = tuple_at(arena, b, i).typ(objects);
        ta == tb
    })
}

fn sa5010_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA5010",
        doc: "impossible type assertion",
        url: "https://staticcheck.dev/docs/checks/#SA5010",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa5010_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa5010_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
