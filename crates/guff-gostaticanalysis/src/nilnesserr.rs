//! Port of [`github.com/alingse/nilnesserr`](https://github.com/alingse/nilnesserr).
//!
//! Reports returning / passing a nil-valued error after a different error was
//! checked non-nil. Combines control-flow nilness facts (from x/tools nilness)
//! with nilerr-style error-value tracking.
//!
//! DEFERRED: full `ChangeInterface` / `MakeInterface` modeling (guff-ssa stubs),
//! `SliceToArrayPointer`, and type-parameter CoreType nillability.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::token::Token;
use guff_analysis::callcheck::{flatten_ssa_value, is_nil_const, static_callee, SsaValue};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, Diagnostic, RunError, RunFn, Pass};
use guff_ssa::function::Function;
use guff_ssa::ids::{BlockId, InstrId};
use guff_ssa::instr::{BinOp, Call, If, InstrData, Return, Slice, UnOp};
use guff_ssa::program::{value_type_of, Program};
use guff_ssa::value::Value;
use guff_types::api_predicates::api_implements;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::BasicKind;
use guff_types::signature::signature_variadic;
use guff_types::TypeId;

const MSG_RETURN: &str = "return a nil value error after check error";
const MSG_CALL: &str = "call function with a nil value error after check error";
const MSG_VARIADIC: &str = "call variadic function with a nil value error after check error";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Nilness {
    NonNil = -1,
    Unknown = 0,
    Nil = 1,
}

impl Nilness {
    fn negate(self) -> Self {
        match self {
            Nilness::NonNil => Nilness::Nil,
            Nilness::Nil => Nilness::NonNil,
            Nilness::Unknown => Nilness::Unknown,
        }
    }
}

#[derive(Clone, Copy)]
struct Fact {
    value: Value,
    nilness: Nilness,
}

#[derive(Clone, Copy)]
struct ErrFact {
    value: Value,
    nilness: Nilness,
}

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

fn is_const_nil(prog: &Program, func: &Function, v: Value) -> bool {
    is_nil_const(prog, func, SsaValue::new(v))
}

fn extract_checked_error_value(
    prog: &Program,
    func: &Function,
    cache: &mut ErrTypeCache,
    bin: &BinOp,
) -> Option<Value> {
    let xt = value_type_of(prog, func, bin.x);
    let yt = value_type_of(prog, func, bin.y);
    if cache.is_err_type(prog, xt) && is_const_nil(prog, func, bin.y) {
        return Some(bin.x);
    }
    if cache.is_err_type(prog, yt) && is_const_nil(prog, func, bin.x) {
        return Some(bin.y);
    }
    None
}

fn find_last_nonnil_value(errors: &[ErrFact], res: Value) -> Option<Value> {
    for last in errors.iter().rev() {
        if last.value == res {
            return None;
        }
        if last.nilness == Nilness::NonNil {
            return Some(last.value);
        }
    }
    None
}

fn check_ssa_value(
    prog: &Program,
    func: &Function,
    cache: &mut ErrTypeCache,
    res: Value,
    errors: &[ErrFact],
    is_nilness: &dyn Fn(Value) -> bool,
) -> bool {
    let typ = value_type_of(prog, func, res);
    if !cache.is_err_type(prog, typ) || is_const_nil(prog, func, res) || !is_nilness(res) {
        return false;
    }
    find_last_nonnil_value(errors, res).is_some()
}

fn validate_variadic_alloc(
    prog: &Program,
    func: &Function,
    call: &Call,
) -> Option<InstrId> {
    let callee = static_callee(&call.call)?;
    let sig = prog.functions.get(callee).signature?;
    if !signature_variadic(&prog.type_arena, sig) {
        return None;
    }
    let last = call.call.args.last().copied()?;
    let Value::Instr(slice_id) = last else {
        return None;
    };
    let InstrData::Slice(Slice {
        x,
        low: None,
        high: None,
        max: None,
        ..
    }) = func.instrs.get(slice_id)
    else {
        return None;
    };
    let Value::Instr(alloc_id) = *x else {
        return None;
    };
    let InstrData::Alloc(alloc) = func.instrs.get(alloc_id) else {
        return None;
    };
    // Alloc.typ is *T; T should be an array (varargs buffer).
    let TypeData::Pointer(p) = prog.type_arena.get(alloc.typ) else {
        return None;
    };
    let elem = p.elem();
    if !matches!(
        prog.type_arena.get(elem.underlying(&prog.type_arena)),
        TypeData::Array(_)
    ) {
        return None;
    }
    Some(alloc_id)
}

/// `extractVariadicErrors`' unwrap: the value stored into the varargs array is
/// the error *widened to `any`*, and upstream reads through that
/// `ChangeInterface` before asking whether it is an error at all.
///
/// guff widens the same way (`error` is already an interface, so `emit_conv`
/// answers `ChangeInterface`, not `MakeInterface`), but this peeled only
/// `ChangeType`. Everything an `%v` argument goes through was therefore opaque:
/// `log.Printf("failed: %v", err)` handed the check an `any`, `isErrType` said
/// no, and the finding disappeared — while `sink(err)`, which needs no
/// widening, reported. syncthing `cmd/strelaysrv/pool.go` is the first shape.
fn peel_iface_wrap(func: &Function, v: Value) -> Value {
    let mut cur = flatten_ssa_value(func, v);
    loop {
        let Value::Instr(i) = cur else {
            return cur;
        };
        match func.instrs.get(i) {
            InstrData::ChangeInterface(ci) => cur = flatten_ssa_value(func, ci.x),
            _ => return cur,
        }
    }
}

fn extract_variadic_errors(func: &Function, alloc_id: InstrId) -> Vec<Value> {
    let Some(referrers) = func.referrers.as_ref() else {
        return Vec::new();
    };
    let Some(refs) = referrers.get(&Value::Instr(alloc_id)) else {
        return Vec::new();
    };
    let mut values = Vec::new();
    for &instr_id in refs {
        if !matches!(func.instrs.get(instr_id), InstrData::IndexAddr(_)) {
            continue;
        }
        let Some(ia_refs) = referrers.get(&Value::Instr(instr_id)) else {
            continue;
        };
        for &instr2 in ia_refs {
            let InstrData::Store(store) = func.instrs.get(instr2) else {
                continue;
            };
            values.push(peel_iface_wrap(func, store.val));
        }
    }
    values
}

fn check_variadic_call(prog: &Program, func: &Function, call: &Call) -> Vec<Value> {
    let Some(alloc_id) = validate_variadic_alloc(prog, func, call) else {
        return Vec::new();
    };
    extract_variadic_errors(func, alloc_id)
}

fn report_if(
    prog: &Program,
    func: &Function,
    cache: &mut ErrTypeCache,
    errors: &[ErrFact],
    is_nilness: &dyn Fn(Value) -> bool,
    value: Value,
    pos: u32,
    message: &'static str,
    out: &mut Vec<(u32, &'static str)>,
) {
    if check_ssa_value(prog, func, cache, value, errors, is_nilness) {
        out.push((pos, message));
    }
}

fn fixed_param_count(prog: &Program, call: &Call) -> Option<usize> {
    let callee = static_callee(&call.call)?;
    let sig = prog.functions.get(callee).signature?;
    if !signature_variadic(&prog.type_arena, sig) {
        return None;
    }
    let params = guff_types::signature_params(&prog.type_arena, sig)?;
    let n = guff_types::tuple_len(&prog.type_arena, Some(params));
    // Last parameter is the `...T` slice; fixed args are the preceding ones.
    Some(n.saturating_sub(1))
}

fn check_nilnesserr_block(
    prog: &Program,
    func: &Function,
    cache: &mut ErrTypeCache,
    block: &guff_ssa::block::BasicBlock,
    errors: &[ErrFact],
    is_nilness: &dyn Fn(Value) -> bool,
    out: &mut Vec<(u32, &'static str)>,
) {
    for &iid in &block.instrs {
        let pos = func.pos(iid);
        if !pos.is_valid() {
            continue;
        }
        let pos_u = pos.0 as u32;
        match func.instrs.get(iid) {
            InstrData::Return(Return { results }) => {
                for &value in results {
                    report_if(
                        prog,
                        func,
                        cache,
                        errors,
                        is_nilness,
                        value,
                        pos_u,
                        MSG_RETURN,
                        out,
                    );
                }
            }
            InstrData::Call(call) => {
                // guff-ssa currently passes variadic operands as flat Call args
                // (no Alloc+Slice packing like go/ssa). Classify by signature:
                // args at/after the `...` parameter use the variadic message.
                let variadic_from = fixed_param_count(prog, call);
                for (i, &value) in call.call.args.iter().enumerate() {
                    let (msg, value) = match variadic_from {
                        // An argument in the `...` tail stands where upstream
                        // reads a value out of the varargs array, so it is
                        // unwrapped the same way. A fixed parameter is not:
                        // upstream's plain `Call.Args` loop peels nothing.
                        Some(fixed) if i >= fixed => (MSG_VARIADIC, peel_iface_wrap(func, value)),
                        _ => (MSG_CALL, value),
                    };
                    report_if(
                        prog,
                        func,
                        cache,
                        errors,
                        is_nilness,
                        value,
                        pos_u,
                        msg,
                        out,
                    );
                }
                // When/if go/ssa-style varargs packing lands, also walk the
                // Alloc→IndexAddr→Store chain (upstream extractVariadicErrors).
                for value in check_variadic_call(prog, func, call) {
                    report_if(
                        prog,
                        func,
                        cache,
                        errors,
                        is_nilness,
                        value,
                        pos_u,
                        MSG_VARIADIC,
                        out,
                    );
                }
            }
            _ => {}
        }
    }
}

fn expand_facts(f: Fact) -> Vec<Fact> {
    // Upstream also expands ChangeInterface; stubbed in guff-ssa.
    vec![f]
}

fn nilness_of(prog: &Program, func: &Function, stack: &[Fact], v: Value) -> Nilness {
    // Intrinsic unwraps (ChangeInterface / Slice / SliceToArrayPointer /
    // MakeInterface) partially deferred when SSA modeling is incomplete.
    if let Value::Instr(iid) = v {
        match func.instrs.get(iid) {
            InstrData::ChangeType(ct) => {
                let underlying = nilness_of(prog, func, stack, ct.x);
                if underlying != Nilness::Unknown {
                    return underlying;
                }
            }
            InstrData::Slice(s) => {
                let underlying = nilness_of(prog, func, stack, s.x);
                if underlying != Nilness::Unknown {
                    return underlying;
                }
            }
            _ => {}
        }
    }

    match v {
        Value::Instr(iid) => match func.instrs.get(iid) {
            InstrData::Alloc(_)
            | InstrData::FieldAddr(_)
            | InstrData::IndexAddr(_)
            | InstrData::MakeChan(_)
            | InstrData::MakeClosure(_)
            | InstrData::MakeMap(_)
            | InstrData::MakeSlice(_) => return Nilness::NonNil,
            _ => {}
        },
        Value::FreeVar(_) | Value::Function(_) | Value::Global(_) => return Nilness::NonNil,
        Value::Const(_) => {
            return if is_const_nil(prog, func, v) {
                Nilness::Nil
            } else {
                Nilness::Unknown
            };
        }
        _ => {}
    }

    for f in stack {
        if f.value == v {
            return f.nilness;
        }
    }
    Nilness::Unknown
}

fn eq_bin_op(
    func: &Function,
    bid: BlockId,
) -> Option<(InstrId, Value, Value, Token, BlockId, BlockId)> {
    let block = func.blocks.get(bid);
    let &last = block.instrs.last()?;
    let InstrData::If(If { cond }) = func.instrs.get(last) else {
        return None;
    };
    let Value::Instr(binop_id) = *cond else {
        return None;
    };
    let InstrData::BinOp(BinOp { op, x, y, .. }) = func.instrs.get(binop_id) else {
        return None;
    };
    if block.succs.len() < 2 {
        return None;
    }
    let (tsucc, fsucc) = match *op {
        Token::EQL => (block.succs[0], block.succs[1]),
        Token::NEQ => (block.succs[1], block.succs[0]),
        _ => return None,
    };
    Some((binop_id, *x, *y, *op, tsucc, fsucc))
}

fn is_nillable(prog: &Program, t: TypeId) -> bool {
    let u = t.underlying(&prog.type_arena);
    match prog.type_arena.get(u) {
        TypeData::Pointer(_)
        | TypeData::Map(_)
        | TypeData::Signature(_)
        | TypeData::Chan(_)
        | TypeData::Interface(_)
        | TypeData::Slice(_) => true,
        TypeData::Basic(b) => b.kind() == BasicKind::UnsafePointer,
        _ => false,
    }
}

fn run_func(
    prog: &Program,
    func: &Function,
    cache: &mut ErrTypeCache,
    out: &mut Vec<(u32, &'static str)>,
) {
    if func.blocks.is_empty() {
        return;
    }
    let Some(entry) = func.live_blocks().find(|(_, b)| b.index == 0).map(|(id, _)| id) else {
        return;
    };

    let max_index = func
        .live_blocks()
        .map(|(_, b)| b.index)
        .max()
        .unwrap_or(-1);
    if max_index < 0 {
        return;
    }
    let mut seen = vec![false; (max_index as usize) + 1];

    fn visit(
        prog: &Program,
        func: &Function,
        cache: &mut ErrTypeCache,
        seen: &mut [bool],
        bid: BlockId,
        stack: Vec<Fact>,
        errors: Vec<ErrFact>,
        out: &mut Vec<(u32, &'static str)>,
    ) {
        let block = func.blocks.get(bid);
        let idx = block.index as usize;
        if idx >= seen.len() || seen[idx] {
            return;
        }
        seen[idx] = true;

        {
            let is_nilness = |v: Value| nilness_of(prog, func, &stack, v) == Nilness::Nil;
            check_nilnesserr_block(prog, func, cache, block, &errors, &is_nilness, out);
        }

        if let Some((binop_id, x, y, _op, tsucc, fsucc)) = eq_bin_op(func, bid) {
            let InstrData::BinOp(bin) = func.instrs.get(binop_id) else {
                unreachable!("eq_bin_op guarantees BinOp");
            };
            let err_value = extract_checked_error_value(prog, func, cache, bin);

            let xnil = nilness_of(prog, func, &stack, x);
            let ynil = nilness_of(prog, func, &stack, y);

            if xnil != Nilness::Unknown
                && ynil != Nilness::Unknown
                && (xnil == Nilness::Nil || ynil == Nilness::Nil)
            {
                let skip = if xnil == ynil { fsucc } else { tsucc };
                for &d in func.blocks.get(bid).dominees() {
                    if d == skip && func.blocks.get(d).preds.len() == 1 {
                        continue;
                    }
                    visit(
                        prog,
                        func,
                        cache,
                        seen,
                        d,
                        stack.clone(),
                        errors.clone(),
                        out,
                    );
                }
                return;
            }

            if xnil == Nilness::Nil || ynil == Nilness::Nil {
                let new_facts = if xnil == Nilness::Nil {
                    expand_facts(Fact {
                        value: y,
                        nilness: Nilness::Nil,
                    })
                } else {
                    expand_facts(Fact {
                        value: x,
                        nilness: Nilness::Nil,
                    })
                };
                let negated: Vec<Fact> = new_facts
                    .iter()
                    .map(|f| Fact {
                        value: f.value,
                        nilness: f.nilness.negate(),
                    })
                    .collect();

                for &d in func.blocks.get(bid).dominees() {
                    let mut s = stack.clone();
                    let mut errs = errors.clone();
                    if func.blocks.get(d).preds.len() == 1 {
                        if d == tsucc {
                            s.extend(new_facts.iter().copied());
                            if let Some(ev) = err_value {
                                errs.push(ErrFact {
                                    value: ev,
                                    nilness: Nilness::Nil,
                                });
                            }
                        } else if d == fsucc {
                            s.extend(negated.iter().copied());
                            if let Some(ev) = err_value {
                                errs.push(ErrFact {
                                    value: ev,
                                    nilness: Nilness::NonNil,
                                });
                            }
                        }
                    }
                    visit(prog, func, cache, seen, d, s, errs, out);
                }
                return;
            }
        }

        // Type-assert comma-ok: else branch learns ptr is nil.
        if let Some(&last) = func.blocks.get(bid).instrs.last() {
            if let InstrData::If(If { cond }) = func.instrs.get(last) {
                let block = func.blocks.get(bid);
                if block.succs.len() >= 2 {
                    let mut cond_v = *cond;
                    let mut fsucc = block.succs[1];
                    if let Value::Instr(uid) = cond_v {
                        if let InstrData::UnOp(UnOp {
                            op: Token::NOT, x, ..
                        }) = func.instrs.get(uid)
                        {
                            cond_v = *x;
                            fsucc = block.succs[0];
                        }
                    }
                    if let Value::Instr(eid) = cond_v {
                        if let InstrData::Extract(ex) = func.instrs.get(eid) {
                            if ex.index == 1 {
                                if let Value::Instr(assert_id) = ex.tuple {
                                    if let InstrData::TypeAssert(ta) = func.instrs.get(assert_id) {
                                        if is_nillable(prog, ta.assert_type) {
                                            if let Some(refs) = func
                                                .referrers
                                                .as_ref()
                                                .and_then(|r| r.get(&Value::Instr(assert_id)))
                                            {
                                                for &pinstr in refs {
                                                    if let InstrData::Extract(extract0) =
                                                        func.instrs.get(pinstr)
                                                    {
                                                        if extract0.index == 0
                                                            && extract0.tuple == ex.tuple
                                                        {
                                                            for &d in block.dominees() {
                                                                if func.blocks.get(d).preds.len()
                                                                    == 1
                                                                    && d == fsucc
                                                                {
                                                                    let mut s = stack.clone();
                                                                    s.push(Fact {
                                                                        value: Value::Instr(pinstr),
                                                                        nilness: Nilness::Nil,
                                                                    });
                                                                    visit(
                                                                        prog,
                                                                        func,
                                                                        cache,
                                                                        seen,
                                                                        d,
                                                                        s,
                                                                        errors.clone(),
                                                                        out,
                                                                    );
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        for &d in func.blocks.get(bid).dominees() {
            visit(
                prog,
                func,
                cache,
                seen,
                d,
                stack.clone(),
                errors.clone(),
                out,
            );
        }
    }

    visit(
        prog,
        func,
        cache,
        &mut seen,
        entry,
        Vec::with_capacity(20),
        Vec::new(),
        out,
    );
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut reports = Vec::new();
    {
        let ir = pass
            .result_of::<buildir::BuildIrResult>(buildir::analyzer())
            .ok_or_else(|| "nilnesserr requires buildir analyzer".to_string())?;
        let Some(mut cache) = ErrTypeCache::new(&ir.prog) else {
            return Ok(None);
        };
        for &fid in ir.src_funcs_with_methods() {
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
            message: message.into(),
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

/// The `nilnesserr` analyzer.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "nilnesserr",
        doc: "Reports constructs that checks for err != nil, but returns a different nil value error.",
        url: "https://github.com/alingse/nilnesserr",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    })
}
