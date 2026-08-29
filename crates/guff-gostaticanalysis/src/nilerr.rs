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
    // Hint lines come from the error *value* (gostaticanalysis/nilerr
    // getValueLineNumbers), e.g. the `err := do()` assignment — not the `!=`.
    match (x_err, y_nil, y_err, x_nil) {
        (true, true, _, _) => Some(*x),
        (_, _, true, true) => Some(*y),
        _ => None,
    }
}

/// Is the function's `i`-th result an error?
///
/// Stands in for the type go/ssa would have given the returned constant.
fn result_position_is_error(
    prog: &Program,
    func: &Function,
    cache: &mut ErrTypeCache,
    i: usize,
) -> bool {
    let Some(sig) = func.signature else {
        return false;
    };
    let results = guff_types::signature::signature_results(&prog.type_arena, sig);
    if guff_types::tuple::tuple_len(&prog.type_arena, results) <= i {
        return false;
    }
    let Some(results) = results else {
        return false;
    };
    let obj = guff_types::tuple::tuple_at(&prog.type_arena, results, i);
    let Some(typ) = obj.typ(&prog.object_arena) else {
        return false;
    };
    cache.is_err_type(prog, typ)
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
    for (i, &res) in results.iter().enumerate() {
        let typ = value_type_of(prog, func, res);
        let is_nil = is_nil_const(prog, func, SsaValue::new(res));
        let is_err = cache.is_err_type(prog, typ);
        if is_err {
            error_returns += 1;
            if !is_nil {
                return None;
            }
        } else if is_nil {
            // go/ssa types the constant in `return nil, err` as `error`;
            // guff sometimes leaves it untyped, so the value's own type cannot
            // answer "is this the error result". Ask the *signature* instead
            // of counting every nil, which is what this used to do — and which
            // made `return nil, false` out of a function with no error result
            // at all look like a swallowed error. jaeger's
            // `internal/storage/elasticsearch/esclient/aggregation.go` returns
            // `(*float64, bool)` six times over.
            if result_position_is_error(prog, func, cache, i) {
                error_returns += 1;
            }
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

/// Port of `isUsedInValue`, which is three cases and nothing else:
///
/// ```go
/// case *ssa.ChangeInterface: return isUsedInValue(value.X, lookedFor)
/// case *ssa.MakeInterface:   return isUsedInValue(value.X, lookedFor)
/// case *ssa.Call:            if value.Call.IsInvoke() { … }
/// ```
///
/// `MakeInterface` is the load-bearing one. Passing an `error` to anything
/// variadic — `fmt.Sprintf("failed: %v", err)`, `fmt.Errorf("…: %w", err)` —
/// boxes it first, so without peeling the box the block looks as though it
/// never mentions the error and `nilerr` reports a return that deliberately
/// swallowed it. dapr writes 25 of them.
fn is_used_in_value(func: &Function, value: Value, looked_for: Value) -> bool {
    if value == looked_for {
        return true;
    }
    let Value::Instr(iid) = value else {
        return false;
    };
    match func.instrs.get(iid) {
        InstrData::ChangeInterface(ci) => is_used_in_value(func, ci.x, looked_for),
        InstrData::MakeInterface(mi) => is_used_in_value(func, mi.x, looked_for),
        // `value.Call.IsInvoke()` — an interface method call, whose receiver is
        // `Call.Value`. A static call is not looked through at all.
        InstrData::Call(c) if c.call.method.is_some() => {
            is_used_in_value(func, c.call.value, looked_for)
        }
        _ => false,
    }
}

fn uses_error_value(func: &Function, bid: BlockId, err_val: Value) -> bool {
    let block = func.blocks.get(bid);
    for &iid in &block.instrs {
        match func.instrs.get(iid) {
            InstrData::Call(call) => {
                // Upstream looks at `callInstr.Call.Args` and nothing else. An
                // *invoke* call keeps its receiver in `Call.Value`, not in
                // `Args`, so `err.Error()` on its own does **not** count as
                // using the error — while `x.Foo()` on a concrete receiver
                // does, because there the receiver is `Args[0]`. That
                // asymmetry is upstream's, and treating the invoke receiver as
                // a use silenced a finding golangci-lint makes.
                for &arg in &call.call.args {
                    if is_used_in_value(func, arg, err_val) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Position of an SSA value for line hints (gostaticanalysis/nilerr
/// `getValueLineNumbers`). Peels `Extract` to the defining call/tuple.
fn value_pos(prog: &Program, func: &Function, v: Value) -> guff::Pos {
    // Upstream is one step, not a walk:
    //
    //     value := v
    //     if extract, ok := value.(*ssa.Extract); ok { value = extract.Tuple }
    //     pos := value.Pos()
    //
    // So the value's *own* position wins whenever it has one. That matters for
    // an `err` captured by a closure: go/ssa loads it through a FreeVar and
    // puts the identifier's position on the load, while peeling the load down
    // to the FreeVar — which carries no position — printed "unknown", a word
    // upstream has no way to produce.
    {
        let mut cur = v;
        if let Value::Instr(iid) = cur {
            if let InstrData::Extract(ex) = func.instrs.get(iid) {
                cur = ex.tuple;
            }
        }
        if let Value::Instr(iid) = cur {
            let pos = func.pos(iid);
            if pos.is_valid() {
                return pos;
            }
        }
    }

    // guff's SSA does not always carry a position where go/ssa does. Peeling
    // the value chain is a fallback for those, not the rule.
    let mut cur = v;
    for _ in 0..8 {
        let Value::Instr(iid) = cur else {
            break;
        };
        match func.instrs.get(iid) {
            InstrData::Extract(ex) => cur = ex.tuple,
            InstrData::ChangeType(ct) => cur = ct.x,
            InstrData::UnOp(u) if u.op == Token::MUL => cur = u.x,
            _ => break,
        }
    }
    match cur {
        Value::Instr(iid) => {
            let pos = func.pos(iid);
            if pos.is_valid() {
                return pos;
            }
            // Alloc often carries the `err` Ident position when a Load has none.
            if let InstrData::Alloc(_) = func.instrs.get(iid) {
                return pos;
            }
            pos
        }
        Value::Param(pid) => {
            let p = func.params.get(pid);
            p.object
                .map(|oid| guff::Pos(oid.pos(&prog.object_arena) as i64))
                .unwrap_or(guff::NO_POS)
        }
        _ => guff::NO_POS,
    }
}

fn collect_value_lines(
    fset: &guff::position::FileSet,
    prog: &Program,
    func: &Function,
    v: Value,
    seen: &mut HashMap<Value, ()>,
    out: &mut Vec<i64>,
) {
    if seen.contains_key(&v) {
        let pos = value_pos(prog, func, v);
        if pos.is_valid() {
            let p = fset.position(pos);
            if p.is_valid() {
                out.push(p.line);
            }
        }
        return;
    }
    seen.insert(v, ());

    if let Value::Instr(iid) = v {
        if let InstrData::Phi(phi) = func.instrs.get(iid) {
            for edge in &phi.edges {
                if let Some(edge) = edge {
                    collect_value_lines(fset, prog, func, *edge, seen, out);
                }
            }
            out.sort_unstable();
            out.dedup();
            return;
        }
    }

    let pos = value_pos(prog, func, v);
    if !pos.is_valid() {
        return;
    }
    let p = fset.position(pos);
    if p.is_valid() {
        out.push(p.line);
    }
}

fn err_line_hint(
    fset: &guff::position::FileSet,
    prog: &Program,
    func: &Function,
    err_val: Value,
) -> String {
    let mut lines = Vec::new();
    let mut seen = HashMap::new();
    collect_value_lines(fset, prog, func, err_val, &mut seen, &mut lines);
    match lines.as_slice() {
        // Upstream builds this with `fmt.Sprintf("lines %v", errLines)` on a
        // `[]int`, and Go renders a slice space-separated: `lines [39 41]`.
        // Rust's `{:?}` puts commas in, which no golangci-lint output has, so
        // every multi-line hint was a guaranteed mismatch.
        //
        // The empty case takes the same branch upstream — `%v` of a nil slice
        // is `[]` — so it prints `lines []`, not a word of our own choosing.
        [one] => format!("line {one}"),
        many => {
            let joined: Vec<String> = many.iter().map(|l| l.to_string()).collect();
            format!("lines [{}]", joined.join(" "))
        }
    }
}

fn run_func(
    prog: &Program,
    func: &Function,
    cache: &mut ErrTypeCache,
    out: &mut Vec<(u32, Value, bool)>,
) {
    for (bid, block) in func.live_blocks() {
        if let Some(v) = bin_op_err_nil(prog, func, cache, bid, Token::NEQ) {
            if block.succs.is_empty() {
                continue;
            }
            let then_b = block.succs[0];
            if let Some(pos) = is_return_nil(prog, func, cache, then_b) {
                if !uses_error_value(func, then_b, v) {
                    // true => "not nil"
                    out.push((pos, v, true));
                }
            }
        } else if let Some(v) = bin_op_err_nil(prog, func, cache, bid, Token::EQL) {
            if block.succs.is_empty() {
                continue;
            }
            let then_b = block.succs[0];
            if func.blocks.get(then_b).preds.len() == 1 {
                if let Some(pos) = is_return_error(func, then_b, v) {
                    // false => "is nil"
                    out.push((pos, v, false));
                }
            }
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut diagnostics = Vec::new();
    {
        let ir = pass
            .result_of::<buildir::BuildIrResult>(buildir::analyzer())
            .ok_or_else(|| "nilerr requires buildir analyzer".to_string())?;
        let Some(mut cache) = ErrTypeCache::new(&ir.prog) else {
            return Ok(None);
        };
        let fset = pass.fset().clone();
        for &fid in ir.src_funcs_with_methods() {
            let func = ir.prog.functions.get(fid);
            let mut reports = Vec::new();
            run_func(&ir.prog, func, &mut cache, &mut reports);
            for (pos, err_val, not_nil) in reports {
                if pos == 0 {
                    continue;
                }
                let hint = err_line_hint(&fset, &ir.prog, func, err_val);
                let message = if not_nil {
                    format!("error is not nil ({hint}) but it returns nil")
                } else {
                    format!("error is nil ({hint}) but it returns error")
                };
                diagnostics.push((pos, message));
            }
        }
    }
    for (pos, message) in diagnostics {
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
