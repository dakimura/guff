//! Framework for validating arguments in statically-known function calls.
//!
//! Port of `honnef.co/go/tools/analysis/callcheck`.

use std::collections::HashMap;

use guff::ast::{CallExpr, File};
use guff::token::Token;
use guff::walk::{NodeRef, preorder};
use guff::Pos;
use guff_ssa::ids::GlobalId;
use guff_constant::{string_val, Kind};
use guff_ssa::const_val::Const;
use guff_ssa::function::Function;
use guff_ssa::ids::{FuncId, InstrId};
use guff_ssa::instr::{CallCommon, FieldAddr, InstrData, Slice};
use guff_ssa::program::{value_type_of, Program};
use guff_ssa::value::Value;
use guff_types::alias::unalias_readonly;
use guff_types::arena::{ObjectArena, PackageArena, TypeArena, TypeData};
use guff_types::basic::BasicKind;
use guff_types::typestring::type_string;
use guff_types::TypeId;
use guff_types::signature::signature_recv;
use guff_types::ObjectId;
use guff_types::Sizes;

use crate::code;
use crate::pass::Pass;
use crate::passes::buildir::BuildIrResult;

/// Kind of SSA call-site instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallSiteKind {
    Call,
    Defer,
    Go,
}

/// A statically-known function call site under analysis.
pub struct Call<'a> {
    pub common: &'a CallCommon,
    pub args: Vec<Argument>,
    invalids: Vec<String>,
    _private: (),
}

/// One argument passed to a checked call.
pub struct Argument {
    pub value: SsaValue,
    invalids: Vec<String>,
}

/// Wrapper around an SSA operand for constant extraction.
#[derive(Clone, Copy)]
pub struct SsaValue {
    inner: Value,
}

impl Argument {
    /// Records an error on this argument.
    pub fn invalid(&mut self, msg: impl Into<String>) {
        self.invalids.push(msg.into());
    }
}

impl Call<'_> {
    /// Records an error on the call itself (not a specific argument).
    pub fn invalid(&mut self, msg: impl Into<String>) {
        self.invalids.push(msg.into());
    }

    fn into_reports(self) -> (Vec<Vec<String>>, Vec<String>) {
        let arg_msgs = self.args.into_iter().map(|a| a.invalids).collect();
        (arg_msgs, self.invalids)
    }
}

impl SsaValue {
    pub fn new(v: Value) -> Self {
        Self { inner: v }
    }

    pub fn value(&self) -> Value {
        self.inner
    }
}

/// Context passed to call-check rules.
pub struct CallContext<'a> {
    pub prog: &'a Program,
    pub caller: &'a Function,
    pub callee: Option<FuncId>,
    /// Import path of the package under analysis (e.g. `"example.com/foo"`).
    pub pkg_path: &'a str,
    /// Target platform sizes (`pass.types_sizes()`).
    pub sizes: Sizes,
}

/// User-defined validation for a single function name (`"time.Parse"`, …).
pub type CheckFn = fn(&mut Call<'_>, &CallContext<'_>);

/// Runs `rules` over every static call in the package's SSA IR.
pub fn run(pass: &mut Pass<'_>, rules: &HashMap<&str, CheckFn>) {
    let pending = {
        let Some(ir) = pass.result_of::<BuildIrResult>(crate::passes::buildir::analyzer()) else {
            return;
        };
        let artifacts = match pass.pkg().type_artifacts.as_ref() {
            Some(a) => a,
            None => return,
        };

        let mut pending = Vec::new();

        for &fid in ir.src_funcs_with_methods() {
            let caller = ir.prog.functions.get(fid);
            for (_, block) in caller.live_blocks() {
                for &iid in &block.instrs {
                    let Some((kind, common)) = call_common(caller, iid) else {
                        continue;
                    };
                    let Some(target) = resolve_call_target(common, &ir.prog) else {
                        continue;
                    };
                    let name = code::type_func_name(
                        &ir.prog.type_arena,
                        &artifacts.objects,
                        &artifacts.packages,
                        target,
                    );
                    let Some(&check) = rules.get(name.as_str()) else {
                        continue;
                    };

                    let pos = caller.pos(iid);
                    let mut call = build_call(&ir.prog, caller, common, target);
                    let ctx = CallContext {
                        prog: &ir.prog,
                        caller,
                        callee: static_callee(common),
                        pkg_path: pass.pkg().pkg_path.as_str(),
                        sizes: pass.types_sizes(),
                    };
                    check(&mut call, &ctx);
                    let (arg_msgs, call_msgs) = call.into_reports();
                    pending.push(PendingReport {
                        kind,
                        pos,
                        arg_msgs,
                        call_msgs,
                    });
                }
            }
        }
        pending
    };

    for report in pending {
        emit_report(pass, report);
    }
}

struct PendingReport {
    kind: CallSiteKind,
    pos: Pos,
    arg_msgs: Vec<Vec<String>>,
    call_msgs: Vec<String>,
}

fn call_common(func: &Function, iid: InstrId) -> Option<(CallSiteKind, &CallCommon)> {
    match func.instrs.get(iid) {
        InstrData::Call(c) => Some((CallSiteKind::Call, &c.call)),
        InstrData::Defer(d) => Some((CallSiteKind::Defer, &d.call)),
        InstrData::Go(g) => Some((CallSiteKind::Go, &g.call)),
        _ => None,
    }
}

pub fn static_callee(common: &CallCommon) -> Option<FuncId> {
    if common.method.is_some() {
        return None;
    }
    match common.value {
        Value::Function(fid) => Some(fid),
        _ => None,
    }
}

pub fn resolve_call_target(common: &CallCommon, prog: &Program) -> Option<ObjectId> {
    if let Some(method) = common.method {
        return Some(method);
    }
    let callee = static_callee(common)?;
    prog.functions.get(callee).object
}

/// Renders the fully-qualified name of a static call target.
pub fn call_target_name(ctx: &CallContext<'_>, common: &CallCommon) -> Option<String> {
    let target = resolve_call_target(common, ctx.prog)?;
    Some(code::type_func_name(
        &ctx.prog.type_arena,
        &ctx.prog.object_arena,
        &ctx.prog.package_arena,
        target,
    ))
}

fn build_call<'a>(
    prog: &'a Program,
    caller: &Function,
    common: &'a CallCommon,
    target: ObjectId,
) -> Call<'a> {
    let mut ir_args = common.args.clone();
    if common.method.is_none() {
        if let Some(sig) = target.typ(&prog.object_arena) {
            if signature_recv(&prog.type_arena, sig).is_some() && !ir_args.is_empty() {
                ir_args.remove(0);
            }
        }
    }

    // Upstream unwraps interface boxing before a rule ever sees the argument
    // (`if iarg, ok := arg.(*ir.MakeInterface); ok { arg = iarg.X }`), so a rule
    // that asks for the argument's type gets the *boxed* type, not `any`. SA1014
    // depends on this: `json.Unmarshal(data, m)` with a map `m` is a finding
    // even though the parameter is `any`.
    let args = ir_args
        .into_iter()
        .map(|v| Argument {
            value: SsaValue {
                inner: unwrap_make_interface(caller, v),
            },
            invalids: Vec::new(),
        })
        .collect();

    Call {
        common,
        args,
        invalids: Vec::new(),
        _private: (),
    }
}

/// Returns the value a [`MakeInterface`](InstrData::MakeInterface) boxes, or `v`
/// unchanged. (Go: the `arg = iarg.X` line in `callcheck.checkCalls`.)
fn unwrap_make_interface(caller: &Function, v: Value) -> Value {
    let Value::Instr(iid) = v else { return v };
    match caller.instrs.get(iid) {
        InstrData::MakeInterface(mi) => mi.x,
        _ => v,
    }
}

fn emit_report(pass: &mut Pass<'_>, report: PendingReport) {
    let files = pass.files();
    let PendingReport {
        kind,
        pos,
        arg_msgs,
        call_msgs,
    } = report;
    let ast_call = find_ast_call(files, kind, pos);

    let mut diags = Vec::new();
    for (idx, msgs) in arg_msgs.into_iter().enumerate() {
        for msg in msgs {
            let arg_pos = if let Some(ce) = ast_call {
                ce.args
                    .get(idx)
                    .map(|e| e.pos().0 as u32)
                    .or_else(|| ce.args.first().map(|e| e.pos().0 as u32))
                    .unwrap_or(pos.0 as u32)
            } else {
                pos.0 as u32
            };
            diags.push((arg_pos, msg));
        }
    }
    // Upstream reports the call *instruction*, and honnef's IR gives every
    // instruction the AST node it came from: `Instruction.Pos()` is
    // `Source().Pos()`. For a plain call that node is the `ast.CallExpr`, whose
    // position is the start of the callee expression — not the `(` that
    // guff-ssa stamps (go/ssa's convention, which honnef deliberately dropped).
    // `defer` / `go` already agree, because there the source node is the
    // statement and both spellings start at the keyword.
    let call_pos = match (kind, ast_call) {
        (CallSiteKind::Call, Some(ce)) => ce.pos().0 as u32,
        _ => pos.0 as u32,
    };
    for msg in call_msgs {
        diags.push((call_pos, msg));
    }
    for (diag_pos, msg) in diags {
        pass.reportf(diag_pos, msg);
    }
}

fn find_ast_call<'a>(files: &'a [File], kind: CallSiteKind, pos: Pos) -> Option<&'a CallExpr> {
    let want = pos.0 as u32;
    let mut found = None;
    for file in files {
        preorder(NodeRef::File(file), |n| {
            if found.is_some() {
                return false;
            }
            match (kind, n) {
                (CallSiteKind::Call, NodeRef::CallExpr(c)) if c.lparen.0 as u32 == want => {
                    found = Some(c);
                    return false;
                }
                (CallSiteKind::Defer, NodeRef::DeferStmt(d)) if d.defer_.0 as u32 == want => {
                    found = Some(&d.call);
                    return false;
                }
                (CallSiteKind::Go, NodeRef::GoStmt(g)) if g.go_.0 as u32 == want => {
                    found = Some(&g.call);
                    return false;
                }
                _ => {}
            }
            true
        });
        if found.is_some() {
            break;
        }
    }
    found
}

/// Flattens `MakeInterface` wrappers and returns the underlying SSA constant.
pub fn extract_const<'a>(
    prog: &'a Program,
    caller: &Function,
    value: SsaValue,
) -> Option<&'a Const> {
    let v = flatten_value(caller, value.inner);
    match v {
        Value::Const(id) => Some(prog.constants.get(id)),
        _ => None,
    }
}

/// Like [`extract_const`] but requires a string constant value, returned as
/// the **bytes** Go would see: `"\xff"` is one byte, not U+00FF.
///
/// Use this whenever the bytes are handed to a parser — `regexp.Compile`,
/// `url.Parse`, `time.Parse` — because the error those return depends on the
/// exact bytes, and an ill-formed one is precisely the interesting case.
pub fn extract_const_bytes(
    prog: &Program,
    caller: &Function,
    value: SsaValue,
) -> Option<Vec<u8>> {
    let c = extract_const(prog, caller, value)?;
    let val = c.val.as_ref()?;
    if val.kind() != Kind::String {
        return None;
    }
    Some(string_val(val))
}

/// [`extract_const_bytes`] decoded as UTF-8, with ill-formed bytes replaced by
/// U+FFFD — which is what Go itself yields when a check ranges over the
/// string. Callers that inspect the bytes want [`extract_const_bytes`].
pub fn extract_const_string(
    prog: &Program,
    caller: &Function,
    value: SsaValue,
) -> Option<String> {
    extract_const_bytes(prog, caller, value)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Like [`extract_const`] but requires an integer constant value.
pub fn extract_const_int(
    prog: &Program,
    caller: &Function,
    value: SsaValue,
) -> Option<i64> {
    let c = extract_const(prog, caller, value)?;
    let val = c.val.as_ref()?;
    if val.kind() != Kind::Int {
        return None;
    }
    let (n, _) = guff_constant::int64_val(val);
    Some(n)
}

/// Flattens `ChangeType` wrappers on an SSA operand.
pub fn flatten_ssa_value(caller: &Function, v: Value) -> Value {
    let mut cur = v;
    loop {
        let Value::Instr(i) = cur else {
            return cur;
        };
        match caller.instrs.get(i) {
            InstrData::ChangeType(ct) => cur = ct.x,
            _ => return cur,
        }
    }
}

fn flatten_value(caller: &Function, v: Value) -> Value {
    flatten_ssa_value(caller, flatten_ir_value(caller, v).unwrap_or(v))
}

/// Recursively flattens `Phi` nodes when all edges agree (Go `irutil.Flatten`).
pub fn flatten_ir_value(caller: &Function, v: Value) -> Option<Value> {
    let mut failed = false;
    let mut seen = std::collections::HashSet::new();
    let mut out: Option<Value> = None;

    fn dfs(
        caller: &Function,
        v: Value,
        seen: &mut std::collections::HashSet<Value>,
        failed: &mut bool,
        out: &mut Option<Value>,
    ) {
        if *failed {
            return;
        }
        if !seen.insert(v) {
            return;
        }
        if let Value::Instr(i) = v {
            if let InstrData::Phi(phi) = caller.instrs.get(i) {
                for e in &phi.edges {
                    if let Some(ev) = e {
                        dfs(caller, *ev, seen, failed, out);
                    }
                }
                return;
            }
        }
        if out.is_none() {
            *out = Some(v);
        } else if *out != Some(v) {
            *failed = true;
        }
    }

    dfs(caller, v, &mut seen, &mut failed, &mut out);
    if failed { None } else { out }
}

/// If `value` is (after flattening) a `FieldAddr`, returns its struct operand and field index.
pub fn field_addr_from_value(caller: &Function, value: SsaValue) -> Option<(Value, usize)> {
    let v = flatten_ssa_value(
        caller,
        flatten_ir_value(caller, value.inner).unwrap_or(value.inner),
    );
    let Value::Instr(i) = v else {
        return None;
    };
    let InstrData::FieldAddr(FieldAddr { x, field, .. }) = caller.instrs.get(i) else {
        return None;
    };
    Some((*x, *field))
}

/// If `value` is (after flattening) a `Slice`, returns a reference to it.
pub fn slice_from_value<'a>(
    caller: &'a Function,
    value: SsaValue,
) -> Option<&'a Slice> {
    let v = flatten_ssa_value(
        caller,
        flatten_ir_value(caller, value.inner).unwrap_or(value.inner),
    );
    let Value::Instr(i) = v else {
        return None;
    };
    match caller.instrs.get(i) {
        InstrData::Slice(s) => Some(s),
        _ => None,
    }
}

/// Reports whether `value` is an SSA constant (including nil).
pub fn is_ssa_const(caller: &Function, value: SsaValue) -> bool {
    matches!(
        flatten_ssa_value(
            caller,
            flatten_ir_value(caller, value.inner).unwrap_or(value.inner),
        ),
        Value::Const(_)
    )
}

/// Reports whether two optional slice bound operands denote the same index.
pub fn slice_bounds_equal(
    prog: &Program,
    caller: &Function,
    a: Option<Value>,
    b: Option<Value>,
) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            let fx = flatten_ssa_value(
                caller,
                flatten_ir_value(caller, x).unwrap_or(x),
            );
            let fy = flatten_ssa_value(
                caller,
                flatten_ir_value(caller, y).unwrap_or(y),
            );
            if fx == fy {
                return true;
            }
            let nx = extract_const_int(prog, caller, SsaValue { inner: fx });
            let ny = extract_const_int(prog, caller, SsaValue { inner: fy });
            nx.is_some() && nx == ny
        }
        _ => false,
    }
}

/// If `value` is a load of a package-level global (`*g` where `g` is `Global`),
/// returns that global's id.
pub fn loaded_global(
    _prog: &Program,
    caller: &Function,
    value: SsaValue,
) -> Option<GlobalId> {
    let v = flatten_ssa_value(caller, value.inner);
    match v {
        Value::Global(g) => Some(g),
        Value::Instr(i) => {
            let InstrData::UnOp(unop) = caller.instrs.get(i) else {
                return None;
            };
            if unop.op != Token::MUL {
                return None;
            }
            match flatten_ssa_value(caller, unop.x) {
                Value::Global(g) => Some(g),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Returns the import path of the package that owns `gid`.
pub fn global_import_path(prog: &Program, gid: GlobalId) -> Option<String> {
    let global = prog.globals.get(gid);
    let ssa_pkg = prog.packages.get(global.pkg);
    let type_pkg = ssa_pkg.type_pkg();
    Some(prog.package_arena.get(type_pkg).path().to_string())
}

/// Returns the SSA type of an operand after peeling `ChangeType` wrappers.
pub fn ssa_value_type(prog: &Program, caller: &Function, value: SsaValue) -> TypeId {
    value_type_of(prog, caller, flatten_value(caller, value.inner))
}

/// Reports whether `typ`'s underlying type is a pointer or interface.
pub fn is_pointer_or_interface_type(arena: &TypeArena, typ: TypeId) -> bool {
    let u = typ.underlying(arena);
    matches!(
        arena.get(u),
        TypeData::Pointer(_) | TypeData::Interface(_)
    )
}

/// Reports whether `typ`'s underlying type is a slice.
pub fn is_slice_type(arena: &TypeArena, typ: TypeId) -> bool {
    matches!(
        arena.get(typ.underlying(arena)),
        TypeData::Slice(_)
    )
}

/// Renders a type as a string (Go `types.TypeString` with no qualifier).
pub fn render_type(
    arena: &TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    typ: TypeId,
) -> String {
    type_string(arena, objects, packages, typ, None)
}

/// Reports whether `value` is a `ChangeType` from the named type (e.g. `"net.IP"`).
pub fn is_converted_from_type(
    prog: &Program,
    caller: &Function,
    value: SsaValue,
    want: &str,
) -> bool {
    let Value::Instr(i) = value.inner else {
        return false;
    };
    let InstrData::ChangeType(ct) = caller.instrs.get(i) else {
        return false;
    };
    let src = value_type_of(prog, caller, ct.x);
    let name = render_type(
        &prog.type_arena,
        &prog.object_arena,
        &prog.package_arena,
        unalias_readonly(&prog.type_arena, src),
    );
    name == want
}

/// Reports whether a type renders as the given name (e.g. `"context.Context"`).
pub fn is_named_type(
    arena: &TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    typ: TypeId,
    name: &str,
) -> bool {
    render_type(arena, objects, packages, typ) == name
}

/// Reports whether `value` is an unbuffered `make(chan T)` (or `make(chan T, 0)`).
pub fn is_unbuffered_make_chan(
    prog: &Program,
    caller: &Function,
    value: SsaValue,
) -> bool {
    let v = flatten_make_chan(caller, value.inner);
    let Value::Instr(i) = v else {
        return false;
    };
    let InstrData::MakeChan(mc) = caller.instrs.get(i) else {
        return false;
    };
    let Some(size) = mc.size else {
        return true;
    };
    extract_const_int(prog, caller, SsaValue { inner: size })
        .is_some_and(|n| n == 0)
}

fn flatten_make_chan(caller: &Function, v: Value) -> Value {
    let mut cur = v;
    loop {
        let Value::Instr(i) = cur else {
            return cur;
        };
        match caller.instrs.get(i) {
            InstrData::ChangeType(ct) => cur = ct.x,
            InstrData::MakeChan(_) => return cur,
            _ => return cur,
        }
    }
}

/// Reports whether `value` is an untyped or typed nil constant.
pub fn is_nil_const(prog: &Program, caller: &Function, value: SsaValue) -> bool {
    let v = flatten_value(caller, value.inner);
    let Value::Const(id) = v else {
        return false;
    };
    prog.constants.get(id).is_nil()
}

/// If `typ` is a built-in (possibly via alias), returns `(basic name, alias name?)`.
pub fn builtin_key_type(
    arena: &TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    typ: TypeId,
) -> Option<(String, Option<String>)> {
    if let TypeData::Alias(a) = arena.get(typ) {
        let rhs = a.rhs()?;
        if let TypeData::Basic(b) = arena.get(unalias_readonly(arena, rhs)) {
            let alias = a.obj().name(objects).to_string();
            return Some((b.name().to_string(), Some(alias)));
        }
    }
    let u = unalias_readonly(arena, typ);
    if let TypeData::Basic(b) = arena.get(u) {
        if b.kind() != BasicKind::UntypedNil {
            return Some((b.name().to_string(), None));
        }
    }
    None
}

/// Reports whether `typ` is an empty *anonymous* struct type.
///
/// Upstream SA1029 checks `T.(*types.Struct)` without calling `Underlying()`,
/// so a named `type pathParam struct{}` is allowed as a context key.
pub fn is_empty_struct_type(arena: &TypeArena, typ: TypeId) -> bool {
    let typ = unalias_readonly(arena, typ);
    match arena.get(typ) {
        TypeData::Struct(s) => s.num_fields() == 0,
        _ => false,
    }
}

/// Read-only comparability check (dynamic mode: interfaces are comparable).
pub fn is_comparable_type(
    arena: &TypeArena,
    objects: &ObjectArena,
    typ: TypeId,
    seen: &mut std::collections::HashSet<TypeId>,
) -> bool {
    if !seen.insert(typ) {
        return true;
    }
    let u = typ.underlying(arena);
    match arena.get(u) {
        TypeData::Basic(b) => b.kind() != BasicKind::UntypedNil,
        TypeData::Pointer(_) | TypeData::Chan(_) | TypeData::Interface(_) => true,
        TypeData::Slice(_) | TypeData::Map(_) | TypeData::Signature(_) => false,
        TypeData::Array(a) => is_comparable_type(arena, objects, a.elem(), seen),
        TypeData::Struct(s) => {
            for i in 0..s.num_fields() {
                let f = s.field(i);
                let ftyp = f.typ(objects).expect("field type");
                if !is_comparable_type(arena, objects, ftyp, seen) {
                    return false;
                }
            }
            true
        }
        TypeData::Tuple(t) => {
            for i in 0..t.len() {
                let elem = t.at(i).typ(objects).expect("tuple element type");
                if !is_comparable_type(arena, objects, elem, seen) {
                    return false;
                }
            }
            true
        }
        _ => true,
    }
}
