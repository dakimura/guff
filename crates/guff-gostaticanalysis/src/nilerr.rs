//! Port of [`github.com/gostaticanalysis/nilerr`](https://github.com/gostaticanalysis/nilerr).
//!
//! Flags returning `nil` after `err != nil`, or returning `err` after `err == nil`.
//!
//! DEFERRED: `//lint:ignore nilerr` via commentmap (guff `//nolint` covers the
//! common case at the runner layer).

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::token::Token;
use guff_analysis::callcheck::{is_nil_const, SsaValue};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, Diagnostic, RunError, RunFn, Pass};
use guff_ssa::function::Function;
use guff_ssa::ids::BlockId;
use guff_ssa::instr::{BinOp, If, InstrData, Return};
use guff_ssa::program::{value_type_of, Program};
use guff_ssa::value::Value;
use guff_types::api_predicates::api_implements;
use guff_types::arena::ObjectData;
use guff_types::TypeId;

struct ErrTypeCache {
    error_typ: TypeId,
    types: guff_types::TypeArena,
    cache: HashMap<TypeId, bool>,
}

impl ErrTypeCache {
    fn new(prog: &Program) -> Option<Self> {
        let error_typ = universe_error(prog)?;
        Some(Self {
            error_typ,
            types: prog.type_arena.clone(),
            cache: HashMap::new(),
        })
    }

    fn is_err_type(&mut self, prog: &Program, typ: TypeId) -> bool {
        if let Some(&cached) = self.cache.get(&typ) {
            return cached;
        }
        let ok = api_implements(
            &mut self.types,
            &prog.object_arena,
            &prog.package_arena,
            typ,
            self.error_typ,
        );
        self.cache.insert(typ, ok);
        ok
    }
}

fn universe_error(prog: &Program) -> Option<TypeId> {
    for oid in prog.object_arena.ids() {
        let ObjectData::TypeName(tn) = prog.object_arena.get(oid) else {
            continue;
        };
        if tn.name() != "error" {
            continue;
        }
        if oid.pkg(&prog.object_arena).is_some() {
            continue;
        }
        return tn.typ();
    }
    None
}

fn bin_op_err_nil(
    prog: &Program,
    func: &Function,
    cache: &mut ErrTypeCache,
    bid: BlockId,
    op: Token,
) -> Option<Value> {
    let block = func.blocks.get(bid);
    let &last = block.instrs.last()?;
    let InstrData::If(If { cond }) = func.instrs.get(last) else {
        return None;
    };
    let Value::Instr(binop_id) = *cond else {
        return None;
    };
    let InstrData::BinOp(BinOp {
        op: bop, x, y, ..
    }) = func.instrs.get(binop_id)
    else {
        return None;
    };
    if *bop != op {
        return None;
    }
    let xt = value_type_of(prog, func, *x);
    let yt = value_type_of(prog, func, *y);
    let x_err = cache.is_err_type(prog, xt);
    let y_err = cache.is_err_type(prog, yt);
    let x_nil = is_nil_const(prog, func, SsaValue::new(*x));
    let y_nil = is_nil_const(prog, func, SsaValue::new(*y));
    // Prefer typed-error vs nil (same as nilnesserr). Upstream also requires
    // Implements on both sides; untyped-nil consts often fail that in guff.
    match (x_err, y_nil, y_err, x_nil) {
        (true, true, _, _) => Some(*x),
        (_, _, true, true) => Some(*y),
        _ => None,
    }
}

fn is_return_nil(
    prog: &Program,
    func: &Function,
    cache: &mut ErrTypeCache,
    bid: BlockId,
) -> Option<u32> {
    let block = func.blocks.get(bid);
    let &last = block.instrs.last()?;
    let InstrData::Return(Return { results }) = func.instrs.get(last) else {
        return None;
    };
    let mut error_returns = 0;
    for &res in results {
        let typ = value_type_of(prog, func, res);
        let is_nil = is_nil_const(prog, func, SsaValue::new(res));
        let is_err = cache.is_err_type(prog, typ);
        if is_err {
            error_returns += 1;
            if !is_nil {
                return None;
            }
        } else if is_nil {
            // guff may type `return nil` as untyped nil (go/ssa usually types it
            // as error). Count it as a nil error return.
            error_returns += 1;
        }
    }
    if error_returns == 0 {
        return None;
    }
    let pos = func.pos(last);
    if !pos.is_valid() {
        return None;
    }
    Some(pos.0 as u32)
}

fn is_return_error(func: &Function, bid: BlockId, err_val: Value) -> Option<u32> {
    let block = func.blocks.get(bid);
    let &last = block.instrs.last()?;
    let InstrData::Return(Return { results }) = func.instrs.get(last) else {
        return None;
    };
    if !results.iter().any(|&v| v == err_val) {
        return None;
    }
    let pos = func.pos(last);
    if !pos.is_valid() {
        return None;
    }
    Some(pos.0 as u32)
}

fn peel_value(func: &Function, value: Value) -> Value {
    let mut cur = value;
    for _ in 0..8 {
        let Value::Instr(iid) = cur else {
            return cur;
        };
        match func.instrs.get(iid) {
            InstrData::ChangeType(ct) => cur = ct.x,
            InstrData::UnOp(u) if u.op == Token::MUL => cur = u.x,
            _ => return cur,
        }
    }
    cur
}

fn is_used_in_value(func: &Function, value: Value, looked_for: Value) -> bool {
    if value == looked_for || peel_value(func, value) == peel_value(func, looked_for) {
        return true;
    }
    let Value::Instr(iid) = value else {
        return false;
    };
    match func.instrs.get(iid) {
        InstrData::ChangeType(ct) => is_used_in_value(func, ct.x, looked_for),
        InstrData::Call(c) => {
            if c.call.method.is_some() && is_used_in_value(func, c.call.value, looked_for) {
                return true;
            }
            c.call
                .args
                .iter()
                .any(|&a| is_used_in_value(func, a, looked_for))
        }
        _ => false,
    }
}

fn uses_error_value(func: &Function, bid: BlockId, err_val: Value) -> bool {
    let block = func.blocks.get(bid);
    for &iid in &block.instrs {
        match func.instrs.get(iid) {
            InstrData::Call(call) => {
                if call.call.method.is_some()
                    && is_used_in_value(func, call.call.value, err_val)
                {
                    return true;
                }
                for &arg in &call.call.args {
                    if is_used_in_value(func, arg, err_val) {
                        return true;
                    }
                }
            }
            InstrData::UnOp(u) if is_used_in_value(func, u.x, err_val) => {
                // err.Error() may lower as call; also count other uses.
                return true;
            }
            _ => {}
        }
    }
    false
}

fn err_line_hint(prog: &Program, func: &Function, v: Value) -> String {
    let mut cur = v;
    if let Value::Instr(iid) = cur {
        if let InstrData::Extract(ex) = func.instrs.get(iid) {
            cur = ex.tuple;
        }
    }
    let Value::Instr(iid) = cur else {
        return "unknown".into();
    };
    let pos = func.pos(iid);
    if !pos.is_valid() {
        return "unknown".into();
    }
    if let Some(fset) = prog.fset.as_ref() {
        let p = fset.position(pos);
        if p.is_valid() {
            return format!("line {}", p.line);
        }
    }
    format!("pos {}", pos.0)
}

fn run_func(
    prog: &Program,
    func: &Function,
    cache: &mut ErrTypeCache,
    out: &mut Vec<(u32, String)>,
) {
    for (bid, block) in func.live_blocks() {
        if let Some(v) = bin_op_err_nil(prog, func, cache, bid, Token::NEQ) {
            if block.succs.is_empty() {
                continue;
            }
            let then_b = block.succs[0];
            if let Some(pos) = is_return_nil(prog, func, cache, then_b) {
                if !uses_error_value(func, then_b, v) {
                    let hint = err_line_hint(prog, func, v);
                    out.push((
                        pos,
                        format!("error is not nil ({hint}) but it returns nil"),
                    ));
                }
            }
        } else if let Some(v) = bin_op_err_nil(prog, func, cache, bid, Token::EQL) {
            if block.succs.is_empty() {
                continue;
            }
            let then_b = block.succs[0];
            if func.blocks.get(then_b).preds.len() == 1 {
                if let Some(pos) = is_return_error(func, then_b, v) {
                    let hint = err_line_hint(prog, func, v);
                    out.push((
                        pos,
                        format!("error is nil ({hint}) but it returns error"),
                    ));
                }
            }
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut reports = Vec::new();
    {
        let ir = pass
            .result_of::<buildir::BuildIrResult>(buildir::analyzer())
            .ok_or_else(|| "nilerr requires buildir analyzer".to_string())?;
        let Some(mut cache) = ErrTypeCache::new(&ir.prog) else {
            return Ok(None);
        };
        for &fid in &ir.src_funcs {
            let func = ir.prog.functions.get(fid);
            run_func(&ir.prog, func, &mut cache, &mut reports);
        }
    }
    for (pos, message) in reports {
        if pos == 0 {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

/// The `nilerr` analyzer.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "nilerr",
        doc: "Finds code that returns nil even though it checks that error is not nil.",
        url: "https://github.com/gostaticanalysis/nilerr",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    })
}
