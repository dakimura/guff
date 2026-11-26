//! Port of loggercheck v0.13.0 `internal/nilness`: whether a pointer passed as
//! a variadic logging argument is provably non-nil at the call site, over an
//! SSA form of the package built on first use.
//!
//! Upstream builds its own `ssa.Program` (not `buildssa`) with `SetNoReturn`
//! for the calls that end a block — `os.Exit`, `runtime.Goexit`, `log.Fatal*`
//! / `Panic*`, the testing `Fatal` / `Skip` family, klog's `Fatal*` / `Exit*`
//! — and answers per query from the dominating `!= nil` branches, a
//! dereference that dominates the call, and the values that are non-nil by
//! construction; a phi needs every edge proved.
//!
//! guff-ssa differs from go/ssa in one place this reads: it does not lower a
//! variadic call to `new [n]any` + stores + a slice, but keeps one operand
//! per argument. The argument at AST index `i` is operand `offset + i`
//! (`offset` is the receiver, when there is one), and like upstream's store
//! of a `MakeInterface` it is only followed when it is one.

use std::collections::{HashMap, HashSet};

use guff::ast::CallExpr;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;
use guff_ssa::ids::{BlockId, FuncId, InstrId};
use guff_ssa::instr::InstrData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::{value_type_of, Program};
use guff_ssa::ssautil::build_package_for_analysis;
use guff_ssa::value::Value;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::ObjectId;

/// `nilness.Index`: SSA call instructions by the position of their `(`.
#[derive(Default)]
pub(crate) struct NilIndex {
    built: bool,
    data: Option<Built>,
}

struct Built {
    prog: Program,
    /// `None` when two instructions share the position: never trusted.
    calls: HashMap<u32, Option<(FuncId, InstrId)>>,
    /// go/ssa's `Instruction.Block()`, per function.
    blocks_of: HashMap<FuncId, HashMap<InstrId, BlockId>>,
}

impl NilIndex {
    /// `NonNilVariadicArg`: whether `call.args[arg_index]`, in the variadic
    /// part of a call without `...`, is provably non-nil.
    pub(crate) fn non_nil_variadic_arg(&mut self, pass: &Pass<'_>, call: &CallExpr, arg_index: usize) -> bool {
        if !self.built {
            self.built = true;
            self.data = build(pass);
        }
        let Some(data) = self.data.as_mut() else {
            return false;
        };
        let Some(&Some((fid, iid))) = data.calls.get(&(call.lparen.0 as u32)) else {
            return false;
        };
        let Some(v) = variadic_value(&data.prog, fid, iid, call.args.len(), arg_index) else {
            return false;
        };
        let blocks_of = data
            .blocks_of
            .entry(fid)
            .or_insert_with(|| block_of_instr(&data.prog, fid));
        let Some(&b) = blocks_of.get(&iid) else {
            return false;
        };
        let mut q = Query {
            prog: &data.prog,
            fid,
            blocks_of,
            visiting: HashSet::new(),
        };
        q.non_nil_at(v, b, Some(iid), None)
    }
}

/// `buildCalls`: `None` when the package's SSA cannot be built, and then
/// nothing is known.
fn build(pass: &Pass<'_>) -> Option<Built> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let files = pass.files();
    if files.is_empty() {
        return None;
    }
    let no_return = no_return_objects(pass);
    let mut built = build_package_for_analysis(
        artifacts.snapshot_for_ssa(),
        files,
        pass.fset().clone(),
        BuilderMode::default(),
        no_return,
    )
    .ok()?;
    let prog = &mut built.prog;

    // The declared functions and methods, the package initializer (package
    // variable initializers live there), and the closures inside them.
    let mut fns: Vec<FuncId> = Vec::new();
    for (fid, f) in prog.functions.iter() {
        if f.pkg != Some(built.pkg) || f.blocks.is_empty() {
            continue;
        }
        let declared = f.object.is_some() && f.synthetic.is_none();
        let init = f.synthetic.as_deref() == Some("package initializer");
        if declared || init {
            fns.push(fid);
        }
    }
    let mut all: Vec<FuncId> = Vec::new();
    let mut seen: HashSet<FuncId> = HashSet::new();
    while let Some(fid) = fns.pop() {
        if !seen.insert(fid) {
            continue;
        }
        all.push(fid);
        fns.extend(prog.functions.get(fid).anon_funcs.iter().copied());
    }

    // `Common().Pos()` is the call's `(` for every call instruction; guff-ssa
    // stamps `go` / `defer` with their keyword instead, and its `CallCommon`
    // carries no position, so the syntax maps one to the other.
    let mut keyword_to_lparen: HashMap<u32, u32> = HashMap::new();
    for file in files {
        walk::preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::DeferStmt(d) => {
                    keyword_to_lparen.insert(d.defer_.0 as u32, d.call.lparen.0 as u32);
                }
                NodeRef::GoStmt(g) => {
                    keyword_to_lparen.insert(g.go_.0 as u32, g.call.lparen.0 as u32);
                }
                _ => {}
            }
            true
        });
    }

    let mut calls: HashMap<u32, Option<(FuncId, InstrId)>> = HashMap::new();
    for &fid in &all {
        prog.functions.get_mut(fid).compute_referrers();
        let f = prog.functions.get(fid);
        for (_, b) in f.live_blocks() {
            for &iid in &b.instrs {
                let pos = f.pos(iid).0 as u32;
                let pos = match f.instrs.get(iid) {
                    InstrData::Call(_) => pos,
                    InstrData::Go(_) | InstrData::Defer(_) => {
                        keyword_to_lparen.get(&pos).copied().unwrap_or(0)
                    }
                    _ => continue,
                };
                if pos == 0 {
                    continue;
                }
                match calls.get(&pos) {
                    Some(Some(prev)) if *prev != (fid, iid) => {
                        calls.insert(pos, None); // ambiguous: never trust it
                    }
                    Some(_) => {}
                    None => {
                        calls.insert(pos, Some((fid, iid)));
                    }
                }
            }
        }
    }
    Some(Built {
        prog: built.prog,
        calls,
        blocks_of: HashMap::new(),
    })
}

/// `noReturn`, as the set of function objects it answers true for.
fn no_return_objects(pass: &Pass<'_>) -> std::collections::HashSet<ObjectId> {
    let mut out = std::collections::HashSet::new();
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return out;
    };
    for oid in artifacts.objects.ids() {
        let ObjectData::Func(func) = artifacts.objects.get(oid) else {
            continue;
        };
        let Some(pkg) = oid.pkg(&artifacts.objects) else {
            continue;
        };
        let full = artifacts.packages.get(pkg).path();
        // GOPATH-mode vendoring prefixes the import path.
        let path = match full.rfind("/vendor/") {
            Some(i) => &full[i + "/vendor/".len()..],
            None => full,
        };
        if !matches!(path, "os" | "runtime" | "log" | "testing" | "k8s.io/klog/v2") {
            continue;
        }
        let name = oid.name(&artifacts.objects);
        let is_method = func
            .typ()
            .map(|t| t.underlying(&artifacts.types))
            .is_some_and(|sig| {
                matches!(artifacts.types.get(sig), TypeData::Signature(_))
                    && guff_types::signature::signature_recv(&artifacts.types, sig).is_some()
            });
        let hit = match path {
            "os" => !is_method && name == "Exit",
            "runtime" => !is_method && name == "Goexit",
            "log" => name.starts_with("Fatal") || name.starts_with("Panic"),
            "testing" => {
                is_method && matches!(name, "Fatal" | "Fatalf" | "FailNow" | "Skip" | "Skipf" | "SkipNow")
            }
            "k8s.io/klog/v2" => {
                !is_method && (name.starts_with("Fatal") || name.starts_with("Exit") || name == "FlushAndExit")
            }
            _ => false,
        };
        if hit {
            out.insert(oid);
        }
    }
    out
}

/// `variadicValue`: the operand behind the `MakeInterface` that boxed the
/// argument, or `None` when it was not boxed (it already is an interface) or
/// the operands do not line up with the syntax.
fn variadic_value(prog: &Program, fid: FuncId, iid: InstrId, nargs: usize, arg_index: usize) -> Option<Value> {
    let f = prog.functions.get(fid);
    let common = match f.instrs.get(iid) {
        InstrData::Call(c) => &c.call,
        InstrData::Go(g) => &g.call,
        InstrData::Defer(d) => &d.call,
        _ => return None,
    };
    if common.ellipsis {
        return None;
    }
    let offset = common.args.len().checked_sub(nargs)?;
    let Value::Instr(arg) = *common.args.get(offset + arg_index)? else {
        return None;
    };
    match f.instrs.get(arg) {
        InstrData::MakeInterface(mi) => Some(mi.x),
        _ => None,
    }
}

fn block_of_instr(prog: &Program, fid: FuncId) -> HashMap<InstrId, BlockId> {
    let f = prog.functions.get(fid);
    let mut out = HashMap::new();
    for (bid, b) in f.live_blocks() {
        for &iid in &b.instrs {
            out.insert(iid, bid);
        }
    }
    out
}

struct Query<'a> {
    prog: &'a Program,
    fid: FuncId,
    blocks_of: &'a HashMap<InstrId, BlockId>,
    visiting: HashSet<InstrId>,
}

impl Query<'_> {
    fn func(&self) -> &guff_ssa::function::Function {
        self.prog.functions.get(self.fid)
    }

    /// `nonNilAt`: `v` is provably non-nil just before `at` in `b` (at the end
    /// of `b` when `at` is `None`; on the edge `b -> edge_to` when given).
    fn non_nil_at(&mut self, v: Value, b: BlockId, at: Option<InstrId>, edge_to: Option<BlockId>) -> bool {
        match v {
            Value::Global(_) | Value::Function(_) => return true,
            Value::Const(_) => return false,
            Value::Instr(iid) => match self.func().instrs.get(iid) {
                InstrData::Alloc(_)
                | InstrData::FieldAddr(_)
                | InstrData::IndexAddr(_)
                | InstrData::MakeClosure(_) => return true,
                InstrData::Phi(_) => {
                    if self.phi(iid) {
                        return true;
                    }
                }
                _ => {}
            },
            _ => {}
        }
        if self.condition_fact(v, b, edge_to) {
            return true;
        }
        if self.dereferenced_before(v, b, at) {
            return true;
        }
        if let Value::Instr(iid) = v {
            if let InstrData::ChangeType(ct) = self.func().instrs.get(iid) {
                let x = ct.x;
                return self.non_nil_at(x, b, at, edge_to);
            }
        }
        false
    }

    fn phi(&mut self, p: InstrId) -> bool {
        if self.visiting.contains(&p) {
            // A phi cycle only copies values that entered through its other
            // edges, so assuming non-nil here is sound once those are checked.
            return true;
        }
        let Some(&pb) = self.blocks_of.get(&p) else {
            return false;
        };
        let InstrData::Phi(phi) = self.func().instrs.get(p) else {
            return false;
        };
        let edges = phi.edges.clone();
        let preds = self.func().blocks.get(pb).preds.clone();
        self.visiting.insert(p);
        let mut ok = true;
        for (i, edge) in edges.into_iter().enumerate() {
            let (Some(edge), Some(&pred)) = (edge, preds.get(i)) else {
                ok = false;
                break;
            };
            if !self.non_nil_at(edge, pred, None, Some(pb)) {
                ok = false;
                break;
            }
        }
        self.visiting.remove(&p);
        ok
    }

    /// `conditionFact`: a dominating `v != nil` / `v == nil` branch proves
    /// `v` non-nil; a branch only speaks for a successor whose sole
    /// predecessor is the branching block.
    fn condition_fact(&self, v: Value, b: BlockId, edge_to: Option<BlockId>) -> bool {
        if let Some(to) = edge_to {
            if self.branch_proves_non_nil(v, b, to) {
                return true;
            }
        }
        let mut child = Some(b);
        while let Some(c) = child {
            let blk = self.func().blocks.get(c);
            if blk.preds.len() == 1 && self.branch_proves_non_nil(v, blk.preds[0], c) {
                return true;
            }
            child = blk.idom();
        }
        false
    }

    fn branch_proves_non_nil(&self, v: Value, from: BlockId, to: BlockId) -> bool {
        let f = self.func();
        let fb = f.blocks.get(from);
        if fb.succs.len() != 2 || fb.succs[0] == fb.succs[1] {
            return false;
        }
        let Some(&last) = fb.instrs.last() else {
            return false;
        };
        let InstrData::If(iff) = f.instrs.get(last) else {
            return false;
        };
        let Value::Instr(cid) = iff.cond else {
            return false;
        };
        let InstrData::BinOp(binop) = f.instrs.get(cid) else {
            return false;
        };
        if binop.op != Token::EQL && binop.op != Token::NEQ {
            return false;
        }
        let is_nil_check =
            (binop.x == v && self.is_nil_const(binop.y)) || (binop.y == v && self.is_nil_const(binop.x));
        if !is_nil_check {
            return false;
        }
        let non_nil_succ = if binop.op == Token::NEQ { fb.succs[0] } else { fb.succs[1] };
        to == non_nil_succ
    }

    fn is_nil_const(&self, v: Value) -> bool {
        match v {
            Value::Const(c) => self.prog.constants.get(c).is_nil(),
            _ => false,
        }
    }

    /// `dereferencedBefore`: an instruction that panics on a nil `v`
    /// dominates the point.
    fn dereferenced_before(&self, v: Value, b: BlockId, at: Option<InstrId>) -> bool {
        let f = self.func();
        let Some(refs) = f.referrers.as_ref().and_then(|r| r.get(&v)) else {
            return false;
        };
        for &r in refs {
            if !self.derefs(r, v) {
                continue;
            }
            let Some(&rb) = self.blocks_of.get(&r) else {
                continue;
            };
            if rb == b {
                for &i in &f.blocks.get(b).instrs {
                    if Some(i) == at {
                        break;
                    }
                    if i == r {
                        return true;
                    }
                }
                continue;
            }
            if f.blocks.get(rb).dominates(f.blocks.get(b)) {
                return true;
            }
        }
        false
    }

    fn derefs(&self, instr: InstrId, v: Value) -> bool {
        let f = self.func();
        match f.instrs.get(instr) {
            InstrData::UnOp(u) => u.op == Token::MUL && u.x == v,
            InstrData::FieldAddr(fa) => fa.x == v,
            InstrData::Store(s) => s.addr == v,
            InstrData::IndexAddr(ia) => {
                if ia.x != v {
                    return false;
                }
                let t = value_type_of(self.prog, f, v);
                let ta = &self.prog.type_arena;
                matches!(ta.get(t.underlying(ta)), TypeData::Pointer(_))
            }
            _ => false,
        }
    }
}
