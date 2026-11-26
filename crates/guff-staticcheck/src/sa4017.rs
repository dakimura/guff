//! SA4017 — discarding return value of pure function call
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4017`.
//!
//! Purity itself lives in the shared fact analyzer
//! [`guff_analysis::passes::facts::purity`], which ports upstream's
//! `analysis/facts/purity`: the `pureStdlib` table *plus* inference over the
//! package's own function bodies. Before that existed this check carried a
//! 26-name list of its own, which both over- and under-reported (`strconv.Itoa`
//! is not pure to upstream; `time.Parse` and every `(time.Time)` method are).
//!
//! Upstream does not flag `_ = pure()` / `x := pure()` — only calls whose
//! results are unused as expression statements (no assignment). guff's blank
//! `LValue::store` is a no-op (like go/ssa), so SSA referrers alone would
//! still report blank-assigns; we skip CallExprs that appear as assign RHS.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::Expr;
use guff::walk::{preorder, NodeRef};
use guff_analysis::code::is_in_test_at;
use guff_analysis::passes::buildir;
use guff_analysis::passes::facts::purity;
use guff_analysis::{call_object, has_non_debug_referrer, referrers, short_call_name};
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_ssa::function::Function;
use guff_ssa::instr::{Call, InstrData};
use guff_ssa::program::Program;
use guff_ssa::value::Value;
use guff_types::TypeData;

fn call_pos(expr: &Expr) -> Option<u32> {
    match expr {
        Expr::CallExpr(c) => Some(c.lparen.0 as u32),
        Expr::ParenExpr(p) => call_pos(&p.x),
        _ => None,
    }
}

/// Positions of CallExprs that are (part of) an assignment RHS or return
/// value — not ExprStmt discards — plus the `( -> CallExpr.Pos()` map this
/// check needs to report where upstream does. Both come out of one walk;
/// `call_node_starts` exists for checks that do not already walk the AST.
fn assigned_call_positions(pass: &Pass<'_>) -> (HashSet<u32>, HashMap<u32, u32>) {
    let mut out = HashSet::new();
    let mut starts = HashMap::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::CallExpr(c) = n {
                starts.insert(c.lparen.0 as u32, c.pos().0 as u32);
            }
            match n {
                NodeRef::AssignStmt(a) => {
                    for r in &a.rhs {
                        if let Some(pos) = call_pos(r) {
                            out.insert(pos);
                        }
                    }
                }
                NodeRef::ReturnStmt(r) => {
                    for e in &r.results {
                        if let Some(pos) = call_pos(e) {
                            out.insert(pos);
                        }
                    }
                }
                NodeRef::ValueSpec(vs) => {
                    for v in &vs.values {
                        if let Some(pos) = call_pos(v) {
                            out.insert(pos);
                        }
                    }
                }
                _ => {}
            }
            true
        });
    }
    (out, starts)
}

/// Port of `typeutil.IsPointerToTypeWithName(typ, "testing.B")`.
///
/// Runs over every parameter of every source function, so it settles the common
/// case on the type's shape and only renders a name once the object is already
/// called `B`.
fn is_pointer_to_testing_b(prog: &Program, typ: guff_types::TypeId) -> bool {
    let typ = guff_types::alias::unalias_readonly(&prog.type_arena, typ);
    let TypeData::Pointer(p) = prog.type_arena.get(typ) else {
        return false;
    };
    let elem = guff_types::alias::unalias_readonly(&prog.type_arena, p.elem());
    let TypeData::Named(n) = prog.type_arena.get(elem) else {
        return false;
    };
    let obj = n.obj();
    if obj.name(&prog.object_arena) != "B" {
        return false;
    }
    obj.pkg(&prog.object_arena)
        .is_some_and(|pkg| prog.package_arena.get(pkg).path() == "testing")
}

/// Upstream skips a whole function in a `_test.go` file when it takes a
/// `*testing.B`, rather than matching `BenchmarkFoo` by name: a benchmark often
/// hands the real work to a helper, and discarding a pure call is exactly what
/// you do to measure it. gin's `BenchmarkBytesConvStrToBytesRaw` is the case
/// that surfaced this.
///
/// Upstream tests `IsInTest` first; guff's is a linear scan over the package's
/// files rather than a `FileSet` lookup, so the parameter test — which rejects
/// almost every function on the shape of its first parameter — goes first. Both
/// conditions have to hold, so the order is not observable.
fn skipped_as_benchmark(pass: &Pass<'_>, prog: &Program, func: &Function) -> bool {
    let Some(obj) = func.object else {
        return false;
    };
    // `fn.Signature.Params()` — the receiver is not among them.
    let Some(params) = func
        .signature
        .and_then(|sig| guff_types::signature::signature_params(&prog.type_arena, sig))
    else {
        return false;
    };
    let TypeData::Tuple(t) = prog.type_arena.get(params) else {
        return false;
    };
    let takes_b = (0..t.len()).any(|i| {
        t.at(i)
            .typ(&prog.object_arena)
            .is_some_and(|ty| is_pointer_to_testing_b(prog, ty))
    });
    takes_b && is_in_test_at(pass, obj.pos(&prog.object_arena))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let ir = pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .ok_or_else(|| "SA4017 requires buildir analyzer".to_string())?;
    let pure = pass
        .result_of::<purity::PurityResult>(purity::analyzer())
        .ok_or_else(|| "SA4017 requires the purity fact analyzer".to_string())?;
    let (assigned, call_starts) = assigned_call_positions(pass);
    let in_fmt_test = pass.pkg().pkg_path == "fmt_test";
    let mut pending = Vec::new();
    // Upstream's SrcFuncs includes methods; guff's shared list may not.
    for &fid in ir.src_funcs_with_methods() {
        let func = ir.prog.functions.get(fid);
        if skipped_as_benchmark(pass, &ir.prog, func) {
            continue;
        }
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                let InstrData::Call(Call { call, .. }) = func.instrs.get(iid) else {
                    continue;
                };
                let val = Value::Instr(iid);
                if has_non_debug_referrer(referrers(func, val), func) {
                    continue;
                }
                let pos = func.pos(iid).0 as u32;
                if assigned.contains(&pos) {
                    continue;
                }
                // Upstream requires a static callee with a type-checker object:
                // interface invokes and anonymous functions are out of scope.
                let Some(callee) = call_object(&ir.prog, call) else {
                    continue;
                };
                if call.method.is_some() {
                    continue;
                }
                if !pure.is_pure(&ir.prog, callee) {
                    continue;
                }
                // Upstream's one hardcoded exemption: fmt's own benchmarks
                // discard `fmt.Sprintf` on purpose.
                if in_fmt_test
                    && guff_analysis::code::type_func_name(
                        &ir.prog.type_arena,
                        &ir.prog.object_arena,
                        &ir.prog.package_arena,
                        callee,
                    ) == "fmt.Sprintf"
                {
                    continue;
                }
                let name = short_call_name(&ir.prog, call).unwrap_or_default();
                pending.push((
                    call_starts.get(&pos).copied().unwrap_or(pos),
                    format!("{name} doesn't have side effects and its return value is ignored"),
                ));
            }
        }
    }
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4017_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4017",
        doc: "discarding return value of pure function call",
        url: "https://staticcheck.dev/docs/checks/#SA4017",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer(), purity::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4017_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4017_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
