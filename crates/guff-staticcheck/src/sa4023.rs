//! SA4023 — impossible comparison of interface value with untyped nil.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4023` (the `MakeInterface` case;
//! the `Call`/`Extract` cases defer nilness/typedness analysis of call results).
//!
//! Upstream's rule is entirely an IR one: the left operand of an
//! interface-vs-untyped-nil comparison must *flatten* to a `MakeInterface`.
//! `irutil.Flatten` walks `Phi` edges and gives up when they disagree, which is
//! what keeps a conditional assignment (`var d I; if cond { d = &impl{} }`)
//! quiet — one edge is the zero value, so the interface really can be nil.

use std::sync::OnceLock;

use guff::token::Token;
use guff_analysis::callcheck;
use guff_analysis::passes::buildir;
use guff_analysis::is_nil_const;
use guff_analysis::{call_node_starts, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::instr::{BinOp, InstrData};
use guff_ssa::program::value_type_of;
use guff_ssa::value::Value;
use guff_types::arena::{TypeArena, TypeData, TypeId};
use guff_types::predicates::is_non_type_param_interface;
use guff_types::unalias_readonly;

/// Upstream guards the report with
/// `terms, err := typeparams.NormalTerms(x.X.Type()); len(terms) == 0 || err != nil`.
///
/// For anything but a type parameter `NormalTerms` yields the single term
/// `typ`, so the guard only ever bites inside a generic body: a `T` whose
/// constraint has no *structural* terms (`any`, `comparable`, a method-only
/// interface) can still be nil once boxed, so the comparison is not always
/// true. `~int`, `int | string` and `*T` all have terms and are reported.
fn has_structural_terms(arena: &TypeArena, typ: TypeId) -> bool {
    let t = unalias_readonly(arena, typ);
    let TypeData::TypeParam(tp) = arena.get(t) else {
        return true;
    };
    let Some(bound) = tp.constraint() else {
        return false;
    };
    let u = bound.underlying(arena);
    let TypeData::Interface(iface) = arena.get(u) else {
        // A non-interface bound (`[T *impl]`) is the implicit interface with
        // that one embedded term — `type_param_iface`'s `wrap_in_implicit_interface`
        // — so it always has a term. Except an invalid bound, which stands for
        // the empty interface.
        return !matches!(
            arena.get(u),
            TypeData::Basic(b) if b.kind() == guff_types::BasicKind::Invalid
        );
    };
    // Read the cached type set rather than computing one: an analyzer only has
    // shared access to the arena, and the checker has already computed the set
    // for every constraint it resolved.
    let Some(tset) = iface.cached_typeset() else {
        return false;
    };
    let mut has_term = false;
    tset.is(|_tilde, term| {
        has_term |= term.is_some();
        true
    });
    has_term
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let ir = pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .ok_or_else(|| "SA4023 requires buildir analyzer".to_string())?;
    let mut pending: Vec<(u32, String)> = Vec::new();
    for &fid in ir.src_funcs_with_methods() {
        let func = ir.prog.functions.get(fid);
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                let InstrData::BinOp(BinOp { op, x, y, .. }) = func.instrs.get(iid) else {
                    continue;
                };
                if !matches!(*op, Token::EQL | Token::NEQ) {
                    continue;
                }
                // Upstream tests `binop.X.Type()`, not the `BinOp`'s own type.
                // guff records the type of the `BinaryExpr` node on the
                // instruction, and for a comparison that is `bool` — so asking
                // the instruction whether it is an interface made the whole IR
                // path unreachable, and the AST approximation below it was the
                // only thing that ever fired.
                let x_typ = value_type_of(&ir.prog, func, *x);
                if !is_non_type_param_interface(&ir.prog.type_arena, x_typ) {
                    continue;
                }
                if !is_nil_const(&ir.prog, func, *y) {
                    continue;
                }
                // Upstream has a "TODO support swapped X and Y": `nil != d` is
                // not reported, and is not reported here either — `x` is then
                // the nil constant and `y` the variable.
                let Some(Value::Instr(xid)) = callcheck::flatten_ir_value(func, *x) else {
                    continue;
                };
                let InstrData::MakeInterface(mi) = func.instrs.get(xid) else {
                    continue;
                };
                let boxed = value_type_of(&ir.prog, func, mi.x);
                if !has_structural_terms(&ir.prog.type_arena, boxed) {
                    continue;
                }
                let qualifier = if *op == Token::EQL { "never" } else { "always" };
                // Upstream also attaches related information pointing at the
                // concrete value; golangci-lint renders that as a separate
                // `SA4023(related information)` row, which the compat tiers
                // exclude from the set-diff on both sides.
                pending.push((
                    func.pos(iid).0 as u32,
                    format!("this comparison is {qualifier} true"),
                ));
            }
        }
    }

    if pending.is_empty() {
        return Ok(None);
    }
    // A `BinOp` carries the operator position; upstream reports the
    // `ast.BinaryExpr`, whose `Pos()` is the start of the left operand. Build
    // the map only when there is something to report: SA4023 used to be 57% of
    // all AST scanning in a prometheus run, and almost every package has no
    // finding at all.
    let starts = call_node_starts(pass);
    for (pos, msg) in pending {
        pass.reportf(starts.get(&pos).copied().unwrap_or(pos), msg);
    }
    Ok(None)
}

fn sa4023_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4023",
        doc: "impossible comparison of interface value with untyped nil",
        url: "https://staticcheck.dev/docs/checks/#SA4023",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4023_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4023_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
