//! SSA IR Disassembler.
//!
//! Port of go/ssa's `print.go` (and the `WriteFunction` helper of `func.go`).

use crate::program::Program;
use crate::function::Function;
use crate::value::Value;
use crate::instr::InstrData;
use crate::ids::{InstrId, BlockId};
use crate::arena::ArenaId;
use guff_types::TypeId;
use guff_types::typestring::type_string;

/// Column width used for right-aligning block annotations and value types,
/// matching go/ssa's `WriteFunction`.
const PUNCHCARD: usize = 80;
const TABWIDTH: usize = 8;

/// rel_type renders a type the way go/ssa's disassembler does. Package
/// qualification (RelativeTo) is deferred; we always print the fully qualified
/// form for now.
fn rel_type(prog: &Program, typ: TypeId) -> String {
    type_string(
        &prog.type_arena,
        &prog.object_arena,
        &prog.package_arena,
        typ,
        None,
    )
}

/// disassemble_value returns the name of an SSA value, i.e. how it is referred
/// to as an operand of another instruction. (Go: `Value.Name`)
pub fn disassemble_value(v: Value, prog: &Program, f: &Function) -> String {
    match v {
        Value::Instr(id) => {
            // Use the register number assigned by number_registers; fall back to
            // the arena index if numbering has not been run yet.
            match f.reg_nums.get(&id) {
                Some(n) => format!("t{}", n),
                None => format!("t{}", id.index()),
            }
        }
        Value::Param(id) => f.params.get(id).name.clone(),
        Value::FreeVar(id) => f.freevars.get(id).name.clone(),
        Value::Const(id) => {
            let c = prog.constants.get(id);
            if let Some(val) = &c.val {
                val.to_string()
            } else {
                let t = c.typ;
                zero_value_string(prog, t)
            }
        }
        Value::Global(id) => prog.globals.get(id).name.clone(),
        Value::Builtin(id) => prog.builtins.get(id).name.clone(),
        Value::Function(id) => prog.functions.get(id).name.clone(),
    }
}

/// zero_value_string renders the source-level zero value of `typ`, matching the
/// value part of go/ssa's disassembly of a `Const` whose `Value` is nil (an
/// aggregate/pointer/… zero). Basic zeros are `false`/`0`/`""`; pointer, slice,
/// chan, map, func, and interface zeros are `nil`; struct and array zeros are
/// `T{}` (named/alias types keep their own name). (Go:
/// `typesinternal.ZeroString`, used by `(*Const).RelString`.)
fn zero_value_string(prog: &Program, typ: TypeId) -> String {
    use guff_types::{TypeData, IS_BOOLEAN, IS_NUMERIC, IS_STRING};
    match prog.type_arena.get(typ) {
        TypeData::Basic(b) => {
            let info = b.info();
            if info.contains(IS_BOOLEAN) {
                "false".to_string()
            } else if info.contains(IS_NUMERIC) {
                "0".to_string()
            } else if info.contains(IS_STRING) {
                "\"\"".to_string()
            } else {
                // unsafe.Pointer / untyped nil.
                "nil".to_string()
            }
        }
        TypeData::Pointer(_)
        | TypeData::Slice(_)
        | TypeData::Chan(_)
        | TypeData::Map(_)
        | TypeData::Signature(_)
        | TypeData::Interface(_) => "nil".to_string(),
        TypeData::Array(_) | TypeData::Struct(_) => format!("{}{{}}", rel_type(prog, typ)),
        TypeData::Named(_) | TypeData::Alias(_) => {
            let u = typ.underlying(&prog.type_arena);
            match prog.type_arena.get(u) {
                TypeData::Struct(_) | TypeData::Array(_) => format!("{}{{}}", rel_type(prog, typ)),
                _ => zero_value_string(prog, u),
            }
        }
        _ => "nil".to_string(),
    }
}

/// object_descr renders a type-checker object the way go's `types.ObjectString`
/// does for a DebugRef description, e.g. `var x int` or `func f func()`. It is
/// a pragmatic subset (kind keyword + name + type); full package-qualified
/// object formatting is deferred.
fn object_descr(prog: &Program, obj: guff_types::ObjectId) -> String {
    use guff_types::ObjectData;
    let kind = match prog.object_arena.get(obj) {
        ObjectData::Var(_) => "var",
        ObjectData::Func(_) => "func",
        ObjectData::Const(_) => "const",
        ObjectData::TypeName(_) => "type",
        ObjectData::PkgName(_) => "package",
        ObjectData::Nil(_) => "nil",
        ObjectData::Builtin(_) => "builtin",
    };
    let name = obj.name(&prog.object_arena);
    match obj.typ(&prog.object_arena) {
        Some(t) => format!("{} {} {}", kind, name, rel_type(prog, t)),
        None => format!("{} {}", kind, name),
    }
}

/// block_index returns the semantic index (`b.index`) of a block, used when an
/// instruction refers to another block (jump/if targets, phi predecessors).
fn block_index(f: &Function, id: BlockId) -> i32 {
    f.blocks.get(id).index
}

/// struct_field_name returns the name of the `index`th field of the struct that
/// `struct_ty`'s underlying type is, for rendering `Field`/`FieldAddr`.
/// (Go: `fieldOf(t, i).Name()`.)
fn struct_field_name(prog: &Program, struct_ty: TypeId, index: usize) -> String {
    let u = struct_ty.underlying(&prog.type_arena);
    let fld = guff_types::struct_field(&prog.type_arena, u, index);
    fld.name(&prog.object_arena).to_string()
}

/// instr_result_type returns the result type of a value-producing instruction,
/// if we currently track it. Instructions whose type is not yet recorded return
/// None (their type is simply omitted from the disassembly).
fn instr_result_type(data: &InstrData) -> Option<TypeId> {
    data.result_type()
}

/// instr_body returns the operation string of an instruction (go's
/// `Instruction.String`), without the `tN = ` register prefix or trailing type.
fn instr_body(id: InstrId, block: BlockId, f: &Function, prog: &Program) -> String {
    let data = f.instrs.get(id);
    match data {
        InstrData::Alloc(a) => {
            // `a.typ` is the Alloc's value type `*T`; the body prints the
            // pointee `T` (Go: `mustDeref(v.Type())`).
            let op = if a.heap { "new" } else { "local" };
            let pointee = guff_types::pointer_elem(&prog.type_arena, a.typ);
            format!("{} {} ({})", op, rel_type(prog, pointee), a.comment)
        }
        InstrData::BinOp(b) => format!(
            "{} {} {}",
            disassemble_value(b.x, prog, f),
            b.op.as_str(),
            disassemble_value(b.y, prog, f)
        ),
        InstrData::UnOp(u) => {
            let comma_ok = if u.comma_ok { ",ok" } else { "" };
            format!("{}{}{}", u.op.as_str(), disassemble_value(u.x, prog, f), comma_ok)
        }
        InstrData::Call(c) => print_call(&c.call, "", prog, f),
        InstrData::Go(g) => print_call(&g.call, "go ", prog, f),
        InstrData::Defer(d) => print_call(&d.call, "defer ", prog, f),
        InstrData::MakeClosure(m) => {
            let mut s = format!("make closure {}", prog.functions.get(m.fn_).name);
            if !m.bindings.is_empty() {
                s.push_str(" [");
                for (i, b) in m.bindings.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push_str(&disassemble_value(*b, prog, f));
                }
                s.push(']');
            }
            s
        }
        InstrData::Store(s) => format!(
            "*{} = {}",
            disassemble_value(s.addr, prog, f),
            disassemble_value(s.val, prog, f)
        ),
        InstrData::Index(i) => format!(
            "{}[{}]",
            disassemble_value(i.x, prog, f),
            disassemble_value(i.index, prog, f)
        ),
        InstrData::Lookup(l) => {
            // Go: "<x>[<index>]" with an optional ",ok" suffix for the
            // comma-ok (2-tuple) form.
            let comma_ok = if l.comma_ok { ",ok" } else { "" };
            format!(
                "{}[{}]{}",
                disassemble_value(l.x, prog, f),
                disassemble_value(l.index, prog, f),
                comma_ok
            )
        }
        InstrData::Range(r) => format!("range {}", disassemble_value(r.x, prog, f)),
        InstrData::Next(n) => format!("next {}", disassemble_value(n.iter, prog, f)),
        InstrData::IndexAddr(i) => format!(
            "&{}[{}]",
            disassemble_value(i.x, prog, f),
            disassemble_value(i.index, prog, f)
        ),
        InstrData::MapUpdate(u) => format!(
            "{}[{}] = {}",
            disassemble_value(u.map, prog, f),
            disassemble_value(u.key, prog, f),
            disassemble_value(u.value, prog, f)
        ),
        InstrData::Slice(s) => {
            let part = |v: &Option<Value>| match v {
                Some(v) => disassemble_value(*v, prog, f),
                None => String::new(),
            };
            let mut out = format!(
                "slice {}[{}:{}]",
                disassemble_value(s.x, prog, f),
                part(&s.low),
                part(&s.high)
            );
            if let Some(m) = &s.max {
                out.push_str(&format!(":{}", disassemble_value(*m, prog, f)));
            }
            out
        }
        InstrData::MakeMap(m) => {
            // Go: "make <type> <reserve>" (the reserve name, empty if absent).
            let res = match &m.reserve {
                Some(v) => disassemble_value(*v, prog, f),
                None => String::new(),
            };
            format!("make {} {}", rel_type(prog, m.typ), res)
        }
        InstrData::MakeChan(c) => {
            let size = match &c.size {
                Some(v) => disassemble_value(*v, prog, f),
                None => "0".to_string(),
            };
            format!("make {} {}", rel_type(prog, c.typ), size)
        }
        InstrData::MakeSlice(s) => {
            let len = s
                .len
                .map(|v| disassemble_value(v, prog, f))
                .unwrap_or_default();
            let cap = s
                .cap
                .map(|v| disassemble_value(v, prog, f))
                .unwrap_or_default();
            format!("make {} {} {}", rel_type(prog, s.typ), len, cap)
        }
        InstrData::TypeAssert(t) => {
            let comma_ok = if t.comma_ok { ",ok" } else { "" };
            format!(
                "typeassert{} {}.({})",
                comma_ok,
                disassemble_value(t.x, prog, f),
                rel_type(prog, t.assert_type)
            )
        }
        InstrData::Select(sel) => {
            let mut s = String::from(if sel.blocking {
                "select blocking ["
            } else {
                "select nonblocking ["
            });
            for (i, st) in sel.states.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                match st.dir {
                    guff_types::ChanDir::RecvOnly => {
                        s.push_str("<-");
                        s.push_str(&disassemble_value(st.chan, prog, f));
                    }
                    guff_types::ChanDir::SendOnly => {
                        s.push_str(&disassemble_value(st.chan, prog, f));
                        s.push_str("<-");
                        if let Some(v) = st.send {
                            s.push_str(&disassemble_value(v, prog, f));
                        }
                    }
                    guff_types::ChanDir::SendRecv => {
                        s.push_str(&disassemble_value(st.chan, prog, f));
                    }
                }
            }
            s.push(']');
            s
        }
        InstrData::Send(send) => format!(
            "send {} <- {}",
            disassemble_value(send.chan, prog, f),
            disassemble_value(send.x, prog, f)
        ),
        InstrData::Panic(p) => format!("panic {}", disassemble_value(p.x, prog, f)),
        InstrData::Phi(p) => {
            let block = f.blocks.get(block);
            let mut s = String::from("phi [");
            for (i, edge) in p.edges.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                let pred = if i < block.preds.len() {
                    block_index(f, block.preds[i])
                } else {
                    -1
                };
                let val = match edge {
                    Some(v) => disassemble_value(*v, prog, f),
                    None => "<nil>".to_string(),
                };
                s.push_str(&format!("{}: {}", pred, val));
            }
            s.push(']');
            if !p.comment.is_empty() {
                s.push_str(&format!(" #{}", p.comment));
            }
            s
        }
        InstrData::Jump(_) => {
            let succs = &f.blocks.get(block).succs;
            let target = if succs.len() == 1 { block_index(f, succs[0]) } else { -1 };
            format!("jump {}", target)
        }
        InstrData::If(if_) => {
            let succs = &f.blocks.get(block).succs;
            let (t, e) = if succs.len() == 2 {
                (block_index(f, succs[0]), block_index(f, succs[1]))
            } else {
                (-1, -1)
            };
            format!("if {} goto {} else {}", disassemble_value(if_.cond, prog, f), t, e)
        }
        InstrData::Extract(e) => {
            format!("extract {} #{}", disassemble_value(e.tuple, prog, f), e.index)
        }
        InstrData::Field(fld) => {
            // Go: "<x>.<fieldname> [#<index>]". The name comes from x's struct type.
            let x_ty = crate::program::value_type_of(prog, f, fld.x);
            let name = struct_field_name(prog, x_ty, fld.field);
            format!("{}.{} [#{}]", disassemble_value(fld.x, prog, f), name, fld.field)
        }
        InstrData::FieldAddr(fld) => {
            // Go: "&<x>.<fieldname> [#<index>]". x is a pointer, so deref first.
            let x_ty = crate::program::value_type_of(prog, f, fld.x);
            let pointee = guff_types::pointer_elem(&prog.type_arena, x_ty);
            let name = struct_field_name(prog, pointee, fld.field);
            format!("&{}.{} [#{}]", disassemble_value(fld.x, prog, f), name, fld.field)
        }
        InstrData::ChangeType(c) => {
            // Go: "changetype <resultType> <- <xType> (<x>)".
            let x_ty = crate::program::value_type_of(prog, f, c.x);
            format!(
                "changetype {} <- {} ({})",
                rel_type(prog, c.typ),
                rel_type(prog, x_ty),
                disassemble_value(c.x, prog, f)
            )
        }
        InstrData::Convert(c) => {
            // Go: "convert <resultType> <- <xType> (<x>)".
            let x_ty = crate::program::value_type_of(prog, f, c.x);
            format!(
                "convert {} <- {} ({})",
                rel_type(prog, c.typ),
                rel_type(prog, x_ty),
                disassemble_value(c.x, prog, f)
            )
        }
        InstrData::Return(ret) => {
            let mut s = String::from("return");
            for (i, v) in ret.results.iter().enumerate() {
                s.push_str(if i == 0 { " " } else { ", " });
                s.push_str(&disassemble_value(*v, prog, f));
            }
            s
        }
        InstrData::RunDefers(_) => "rundefers".to_string(),
        InstrData::DebugRef(d) => {
            // "; [address of ]<descr> @ line:col is <name>"
            // descr is the object's declaration string ("var x int") when the
            // expression is an identifier, else the AST node name.
            let descr = match d.object {
                Some(obj) => object_descr(prog, obj),
                None => d.expr_descr.clone(),
            };
            let (line, col) = match &prog.fset {
                Some(fset) => {
                    let p = fset.position(f.pos(id));
                    (p.line, p.column)
                }
                None => (0, 0),
            };
            let addr = if d.is_addr { "address of " } else { "" };
            format!(
                "; {}{} @ {}:{} is {}",
                addr,
                descr,
                line,
                col,
                disassemble_value(d.x, prog, f)
            )
        }
        _ => "<unimplemented instruction>".to_string(),
    }
}

fn print_call(call: &crate::instr::CallCommon, prefix: &str, prog: &Program, f: &Function) -> String {
    let mut s = String::from(prefix);
    if let Some(method) = call.method {
        s.push_str("invoke ");
        s.push_str(&disassemble_value(call.value, prog, f));
        s.push('.');
        s.push_str(method.name(&prog.object_arena));
    } else {
        s.push_str(&disassemble_value(call.value, prog, f));
    }
    s.push('(');
    for (i, arg) in call.args.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&disassemble_value(*arg, prog, f));
    }
    s.push(')');
    s
}

/// instruction_string returns the go/ssa `Instruction.String()` form (operation
/// only, no `tN = ` register prefix or trailing type column). Used by
/// [`crate::ssautil::switch::Switch::to_string`].
pub fn instruction_string(id: InstrId, block: BlockId, f: &Function, prog: &Program) -> String {
    instr_body(id, block, f, prog)
}

/// const_rel_string renders a constant the way go/ssa prints switch case
/// comparands (`42:int`, `"foo":string`, …).
pub fn const_rel_string(prog: &Program, id: crate::ids::ConstId) -> String {
    let c = prog.constants.get(id);
    let val = match &c.val {
        Some(v) => v.to_string(),
        None => zero_value_string(prog, c.typ),
    };
    format!("{}:{}", val, rel_type(prog, c.typ))
}

/// disassemble_instr returns the disassembly of a single instruction line
/// (without the leading tab), mirroring the per-instruction formatting of
/// go/ssa's `WriteFunction`: an optional `tN = ` register prefix, the operation
/// string, and a right-aligned result type.
pub fn disassemble_instr(id: InstrId, block: BlockId, f: &Function, prog: &Program) -> String {
    let data = f.instrs.get(id);
    let result_type = instr_result_type(data);
    let is_value = data.is_value();

    let mut line = String::new();
    let mut l = PUNCHCARD as isize - TABWIDTH as isize;

    if is_value {
        let num = match f.reg_nums.get(&id) {
            Some(n) => *n,
            None => id.index() as u32,
        };
        let name = format!("t{} = ", num);
        l -= name.len() as isize;
        line.push_str(&name);
    }

    let body = instr_body(id, block, f, prog);
    l -= body.len() as isize;
    line.push_str(&body);

    if let Some(t) = result_type {
        line.push(' ');
        let ts = rel_type(prog, t);
        l -= (ts.len() + 2) as isize;
        if l > 0 {
            line.push_str(&" ".repeat(l as usize));
        }
        line.push_str(&ts);
    }

    line
}

/// disassemble_block returns a string representation of an SSA basic block.
pub fn disassemble_block(id: BlockId, f: &Function, prog: &Program) -> String {
    let block = f.blocks.get(id);
    let mut s = String::new();

    // Block header: "<index>:" followed by a right-aligned annotation of the
    // form "<comment> P:<preds> S:<succs> [idom:<i>]".
    let head = format!("{}:", block.index);
    let mut bmsg = format!("{} P:{} S:{}", block.comment, block.preds.len(), block.succs.len());
    if let Some(idom) = block.idom() {
        bmsg.push_str(&format!(" idom:{}", block_index(f, idom)));
    }
    let pad = PUNCHCARD.saturating_sub(1 + head.len() + bmsg.len());
    s.push_str(&head);
    s.push_str(&" ".repeat(pad));
    s.push_str(&bmsg);
    s.push('\n');

    for &instr_id in &block.instrs {
        s.push('\t');
        s.push_str(&disassemble_instr(instr_id, id, f, prog));
        s.push('\n');
    }
    s
}

/// disassemble_function returns a string representation of an SSA function.
pub fn disassemble_function(f: &Function, prog: &Program) -> String {
    // Header. When the function's signature is recorded we render it the way
    // go/ssa's writeSignature does ("func f(x int) int:"); otherwise we fall
    // back to the bare name form. Receiver rendering for methods is deferred.
    let mut s = match f.signature {
        Some(sig) => format!("func {}{}:\n", f.name, guff_types::typestring::signature_string(
            &prog.type_arena,
            &prog.object_arena,
            &prog.package_arena,
            sig,
            None,
        )),
        None => format!("func {}():\n", f.name),
    };
    if f.blocks.is_empty() {
        s.push_str("\t(external)\n");
        return s;
    }
    for (id, block) in f.blocks.iter() {
        if block.deleted {
            continue;
        }
        s.push_str(&disassemble_block(id, f, prog));
    }
    s
}
