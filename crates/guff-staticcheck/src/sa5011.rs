//! SA5011 — possible nil pointer dereference.
//!
//! Simplified port of `honnef.co/go/tools/staticcheck/sa5011`.
//! Upstream disables this check due to false positives; we implement the same
//! trivial SSA walk (no phi/sigma propagation).

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::token::Token;
use guff_analysis::callcheck::{flatten_ssa_value, is_pointer_or_interface_type, is_slice_type};
use guff_analysis::passes::buildir;
use guff_analysis::{is_nil_const, AnalysisResult, Analyzer, Diagnostic, RelatedInformation, RunError, RunFn, Pass};
use guff_ssa::function::Function;
use guff_ssa::ids::InstrId;
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
        InstrData::UnOp(UnOp { op: Token::MUL, x, .. }) => *x,
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

fn lookup_maybe_nil<'a>(
    prog: &Program,
    func: &Function,
    maybe_nil: &'a HashMap<Value, InstrId>,
    ptr: Value,
) -> Option<&'a InstrId> {
    let key = peel_load(func, ptr);
    if let Some(id) = maybe_nil.get(&key) {
        return Some(id);
    }
    maybe_nil
        .iter()
        .find(|(&k, _)| ptr_keys_equal(prog, func, k, key))
        .map(|(_, id)| id)
}

fn nil_check_partner(
    func: &Function,
    cond: Value,
) -> Option<(InstrId, Value, Value)> {
    let Value::Instr(iid) = cond else {
        return None;
    };
    let InstrData::BinOp(BinOp { op, x, y, .. }) = func.instrs.get(iid) else {
        return None;
    };
    if *op != Token::EQL && *op != Token::NEQ {
        return None;
    }
    Some((iid, *x, *y))
}

fn collect_maybe_nil(
    prog: &Program,
    func: &Function,
) -> HashMap<Value, InstrId> {
    let mut maybe_nil = HashMap::new();
    for (_, block) in func.blocks.iter() {
        for &iid in &block.instrs {
            let InstrData::If(If { cond }) = func.instrs.get(iid) else {
                continue;
            };
            let Some((bin_id, x, y)) = nil_check_partner(func, *cond) else {
                continue;
            };
            if is_nil_const_operand(prog, func, x) {
                maybe_nil.insert(peel_load(func, y), bin_id);
            }
            if is_nil_const_operand(prog, func, y) {
                maybe_nil.insert(peel_load(func, x), bin_id);
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

            for (_, block) in func.blocks.iter() {
                for &iid in &block.instrs {
                    let ptr = match func.instrs.get(iid) {
                        InstrData::UnOp(UnOp { op: Token::MUL, x, .. }) => Some(*x),
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
                    let Some(nil_check) = lookup_maybe_nil(&ir.prog, func, &maybe_nil, ptr) else {
                        continue;
                    };
                    reports.push((
                        func.pos(iid).0 as u32,
                        func.pos(*nil_check).0 as u32,
                    ));
                }
            }
        }
    }
    for (pos, related_pos) in reports {
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
