//! SA5011 — possible nil pointer dereference.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5011`.
//!
//! Upstream relies on SSA sigma nodes so that `if x != nil { *x }` uses a
//! different value inside the branch. When the same value is reused (no
//! sigma), we additionally suppress reports when the deref is dominated by
//! both the nil-check and its non-nil successor (guarded use after the check).

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::token::Token;
use guff_analysis::callcheck::{flatten_ssa_value, is_pointer_or_interface_type, is_slice_type};
use guff_analysis::passes::buildir;
use guff_analysis::{
    is_nil_const, AnalysisResult, Analyzer, Diagnostic, RelatedInformation, RunError, RunFn, Pass,
};
use guff_ssa::function::Function;
use guff_ssa::ids::{BlockId, InstrId};
use guff_ssa::instr::{BinOp, FieldAddr, If, IndexAddr, InstrData, Store, UnOp};
use guff_ssa::program::{value_type_of, Program};
use guff_ssa::value::Value;

const MSG: &str = "possible nil pointer dereference";
const RELATED: &str = "this check suggests that the pointer can be nil";

fn is_nil_const_operand(prog: &Program, caller: &Function, value: Value) -> bool {
    is_nil_const(prog, caller, value)
}

fn peel_load(func: &Function, v: Value) -> Value {
    let v = flatten_ssa_value(func, v);
    let Value::Instr(iid) = v else {
        return v;
    };
    match func.instrs.get(iid) {
        InstrData::UnOp(UnOp {
            op: Token::MUL, x, ..
        }) => *x,
        _ => v,
    }
}

fn is_nil_pointer_const(prog: &Program, v: Value) -> bool {
    let Value::Const(id) = v else {
        return false;
    };
    let c = prog.constants.get(id);
    if c.val.is_some() {
        return false;
    }
    is_pointer_or_interface_type(&prog.type_arena, c.typ)
}

fn ptr_keys_equal(prog: &Program, func: &Function, a: Value, b: Value) -> bool {
    let a = peel_load(func, a);
    let b = peel_load(func, b);
    if a == b {
        return true;
    }
    is_nil_pointer_const(prog, a) && is_nil_pointer_const(prog, b)
}

struct NilCheck {
    bin_op: InstrId,
    check_block: BlockId,
    non_nil_block: Option<BlockId>,
}

fn nil_check_partner(func: &Function, cond: Value) -> Option<(InstrId, Value, Value, bool)> {
    let Value::Instr(iid) = cond else {
        return None;
    };
    let InstrData::BinOp(BinOp { op, x, y, .. }) = func.instrs.get(iid) else {
        return None;
    };
    let eq_nil = match *op {
        Token::EQL => true,
        Token::NEQ => false,
        _ => return None,
    };
    Some((iid, *x, *y, eq_nil))
}

fn collect_maybe_nil(prog: &Program, func: &Function) -> HashMap<Value, Vec<NilCheck>> {
    let mut maybe_nil: HashMap<Value, Vec<NilCheck>> = HashMap::new();
    for (bid, block) in func.live_blocks() {
        for &iid in &block.instrs {
            let InstrData::If(If { cond }) = func.instrs.get(iid) else {
                continue;
            };
            let Some((bin_id, x, y, eq_nil)) = nil_check_partner(func, *cond) else {
                continue;
            };
            let non_nil_block = if block.succs.len() >= 2 {
                Some(if eq_nil {
                    block.succs[1]
                } else {
                    block.succs[0]
                })
            } else {
                None
            };
            // Only pointer/interface nil-checks mark a value as maybe-nil.
            // `if *m == nil` on a `*map`/`*slice` compares the map/slice value
            // (not the pointer) — do not peel that load, or later `*m = …` is
            // falsely reported (prometheus `(*Annotations).Add` / `Merge`).
            // When the compared value *is* a pointer/interface, peel loads so
            // Alloc'd locals (`var x *T`) unify across distinct load instrs.
            let consider = |prog: &Program, func: &Function, v: Value| -> Option<Value> {
                let v = flatten_ssa_value(func, v);
                let typ = value_type_of(prog, func, v);
                if !is_pointer_or_interface_type(&prog.type_arena, typ) {
                    return None;
                }
                Some(peel_load(func, v))
            };
            let push = |maybe_nil: &mut HashMap<Value, Vec<NilCheck>>, key: Value| {
                maybe_nil.entry(key).or_default().push(NilCheck {
                    bin_op: bin_id,
                    check_block: bid,
                    non_nil_block,
                });
            };
            if is_nil_const_operand(prog, func, x) {
                if let Some(key) = consider(prog, func, y) {
                    push(&mut maybe_nil, key);
                }
            }
            if is_nil_const_operand(prog, func, y) {
                if let Some(key) = consider(prog, func, x) {
                    push(&mut maybe_nil, key);
                }
            }
        }
    }
    maybe_nil
}

fn cannot_be_nil_source(func: &Function, ptr: Value) -> bool {
    let Value::Instr(iid) = ptr else {
        return false;
    };
    matches!(
        func.instrs.get(iid),
        InstrData::Alloc(_) | InstrData::FieldAddr(_) | InstrData::IndexAddr(_)
    )
}

fn is_index_addr_on_slice(
    prog: &Program,
    caller: &Function,
    arena: &guff_types::arena::TypeArena,
    ia: &IndexAddr,
) -> bool {
    let x_typ = value_type_of(prog, caller, ia.x);
    is_slice_type(arena, x_typ)
}

fn is_guarded_by_non_nil(func: &Function, check: &NilCheck, deref_block: BlockId) -> bool {
    let Some(non_nil_block) = check.non_nil_block else {
        return false;
    };
    let check_bb = func.blocks.get(check.check_block);
    let non_nil = func.blocks.get(non_nil_block);
    let deref = func.blocks.get(deref_block);
    check_bb.dominates(deref) && (non_nil.dominates(deref) || non_nil_block == deref_block)
}

/// True when any recorded nil-check of this pointer guards `deref_block`.
///
/// A function often has several `if p != nil` / `if p == nil { return }` on the
/// same pointer (caddy `respHeaderOps`, short-circuit `a != nil && a.F`). A
/// single HashMap slot used to keep only the last check, so an earlier guarded
/// deref was matched against a later check and falsely reported.
fn any_check_guards(func: &Function, checks: &[NilCheck], deref_block: BlockId) -> bool {
    checks
        .iter()
        .any(|check| is_guarded_by_non_nil(func, check, deref_block))
}

fn lookup_maybe_nil<'a>(
    prog: &Program,
    func: &Function,
    maybe_nil: &'a HashMap<Value, Vec<NilCheck>>,
    ptr: Value,
    deref_block: BlockId,
) -> Option<&'a NilCheck> {
    let key = peel_load(func, ptr);
    if let Some(checks) = maybe_nil.get(&key) {
        if any_check_guards(func, checks, deref_block) {
            return None;
        }
        // Not guarded by any check — relate to the first (earliest) check.
        return checks.first();
    }
    // Nil-pointer-const keys may not be pointer-identical. Only then scan.
    if !is_nil_pointer_const(prog, key) {
        return None;
    }
    maybe_nil.iter().find_map(|(&k, checks)| {
        if !ptr_keys_equal(prog, func, k, key) {
            return None;
        }
        if any_check_guards(func, checks, deref_block) {
            return None;
        }
        checks.first()
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut reports = Vec::new();
    {
        let ir = pass
            .result_of::<buildir::BuildIrResult>(buildir::analyzer())
            .ok_or_else(|| "SA5011 requires buildir analyzer".to_string())?;
        let arena = &ir.prog.type_arena;

        for &fid in &ir.src_funcs {
            let func = ir.prog.functions.get(fid);
            let maybe_nil = collect_maybe_nil(&ir.prog, func);
            if maybe_nil.is_empty() {
                continue;
            }

            for (bid, block) in func.live_blocks() {
                for &iid in &block.instrs {
                    let ptr = match func.instrs.get(iid) {
                        InstrData::UnOp(UnOp {
                            op: Token::MUL, x, ..
                        }) => Some(*x),
                        InstrData::Store(Store { addr, .. }) => Some(*addr),
                        InstrData::IndexAddr(ia) => {
                            if is_index_addr_on_slice(&ir.prog, func, arena, ia) {
                                continue;
                            }
                            Some(ia.x)
                        }
                        InstrData::FieldAddr(FieldAddr { x, .. }) => Some(*x),
                        _ => None,
                    };
                    let Some(ptr) = ptr else {
                        continue;
                    };
                    if cannot_be_nil_source(func, ptr) {
                        continue;
                    }
                    let Some(nil_check) = lookup_maybe_nil(&ir.prog, func, &maybe_nil, ptr, bid)
                    else {
                        continue;
                    };
                    reports.push((func.pos(iid).0 as u32, func.pos(nil_check.bin_op).0 as u32));
                }
            }
        }
    }
    for (pos, related_pos) in reports {
        // Hybrid SSA sometimes yields NoPos (0); never emit unlocated diagnostics.
        if pos == 0 {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message: MSG.into(),
            related: vec![RelatedInformation {
                pos: related_pos,
                end: 0,
                message: RELATED.into(),
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn sa5011_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA5011",
        doc: "possible nil pointer dereference",
        url: "https://staticcheck.dev/docs/checks/#SA5011",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa5011_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa5011_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
