//! Inferred switch discovery from SSA control flow — port of
//! go/ssa/ssautil/switch.go.

use crate::hash::HashSet;
use std::fmt;

use guff::token::Token;
use guff_types::TypeId;

use crate::function::Function;
use crate::ids::{BlockId, ConstId, InstrId};
use crate::instr::{BinOp, Extract, If, InstrData, TypeAssert};
use crate::print::{const_rel_string, disassemble_value, instruction_string};
use crate::program::Program;
use crate::value::Value;

/// A single constant comparison in a value switch. (Go: `ssautil.ConstCase`.)
#[derive(Debug, Clone)]
pub struct ConstCase {
    pub block: BlockId,
    pub body: BlockId,
    pub value: ConstId,
}

/// A single type assertion in a type switch. (Go: `ssautil.TypeCase`.)
#[derive(Debug, Clone)]
pub struct TypeCase {
    pub block: BlockId,
    pub body: BlockId,
    pub typ: TypeId,
    pub binding: Value,
}

/// A logical multiway branch recovered from an if/else chain. (Go:
/// `ssautil.Switch`.)
#[derive(Debug, Clone)]
pub struct Switch {
    pub start: BlockId,
    pub x: Value,
    pub const_cases: Vec<ConstCase>,
    pub type_cases: Vec<TypeCase>,
    pub default: Option<BlockId>,
}

impl Switch {
    /// Renders the switch the way go/ssa/ssautil does for tests and debugging.
    pub fn to_string(&self, prog: &Program, f: &Function) -> String {
        let mut buf = String::new();
        if !self.const_cases.is_empty() {
            use std::fmt::Write;
            writeln!(
                &mut buf,
                "switch {} {{",
                disassemble_value(self.x, prog, f)
            )
            .unwrap();
            for c in &self.const_cases {
                let body_instr = f.blocks.get(c.body).instrs.first().copied();
                let body_str = body_instr
                    .map(|id| instruction_string(id, c.body, f, prog))
                    .unwrap_or_default();
                writeln!(
                    &mut buf,
                    "case {}: {}",
                    const_rel_string(prog, c.value),
                    body_str
                )
                .unwrap();
            }
        } else {
            use std::fmt::Write;
            writeln!(
                &mut buf,
                "switch {}.(type) {{",
                disassemble_value(self.x, prog, f)
            )
            .unwrap();
            for c in &self.type_cases {
                let body_instr = f.blocks.get(c.body).instrs.first().copied();
                let body_str = body_instr
                    .map(|id| instruction_string(id, c.body, f, prog))
                    .unwrap_or_default();
                writeln!(
                    &mut buf,
                    "case {} {}: {}",
                    disassemble_value(c.binding, prog, f),
                    guff_types::typestring::type_string(
                        &prog.type_arena,
                        &prog.object_arena,
                        &prog.package_arena,
                        c.typ,
                        None,
                    ),
                    body_str
                )
                .unwrap();
            }
        }
        if let Some(def) = self.default {
            use std::fmt::Write;
            let body_instr = f.blocks.get(def).instrs.first().copied();
            let body_str = body_instr
                .map(|id| instruction_string(id, def, f, prog))
                .unwrap_or_default();
            writeln!(&mut buf, "default: {}", body_str).unwrap();
        }
        buf.push('}');
        buf
    }
}

impl fmt::Display for Switch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Switch(start={:?}, const={}, type={})",
            self.start,
            self.const_cases.len(),
            self.type_cases.len()
        )
    }
}

/// Examines `f`'s CFG and returns inferred value and type switches in dominance
/// order. (Go: `ssautil.Switches`.)
pub fn switches(_prog: &Program, f: &Function) -> Vec<Switch> {
    let mut out = Vec::new();
    let mut seen = HashSet::default();
    for b in f.dom_preorder() {
        if let Some((x, k)) = is_comparison_block(f, b) {
            let mut sw = Switch {
                start: b,
                x,
                const_cases: Vec::new(),
                type_cases: Vec::new(),
                default: None,
            };
            value_switch(f, &mut sw, k, &mut seen);
            if sw.const_cases.len() > 1 {
                out.push(sw);
            }
        }

        if let Some((y, x, t)) = is_type_assert_block(f, b) {
            let mut sw = Switch {
                start: b,
                x,
                const_cases: Vec::new(),
                type_cases: Vec::new(),
                default: None,
            };
            type_switch(f, &mut sw, y, t, &mut seen);
            if sw.type_cases.len() > 1 {
                out.push(sw);
            }
        }
    }
    out
}

fn value_switch(f: &Function, sw: &mut Switch, mut k: ConstId, seen: &mut HashSet<BlockId>) {
    let mut b = sw.start;
    let x = sw.x;
    loop {
        if seen.contains(&b) {
            break;
        }
        seen.insert(b);

        let block = f.blocks.get(b);
        if block.succs.len() < 2 {
            break;
        }
        sw.const_cases.push(ConstCase {
            block: b,
            body: block.succs[0],
            value: k,
        });
        b = block.succs[1];
        if f.blocks.get(b).instrs.len() > 2 {
            break;
        }
        if f.blocks.get(b).preds.len() != 1 {
            break;
        }
        let Some((nx, nk)) = is_comparison_block(f, b) else {
            break;
        };
        if nx != x {
            break;
        }
        k = nk;
    }
    sw.default = Some(b);
}

fn type_switch(f: &Function, sw: &mut Switch, mut y: Value, mut t: TypeId, seen: &mut HashSet<BlockId>) {
    let mut b = sw.start;
    let x = sw.x;
    loop {
        if seen.contains(&b) {
            break;
        }
        seen.insert(b);

        let block = f.blocks.get(b);
        if block.succs.len() < 2 {
            break;
        }
        sw.type_cases.push(TypeCase {
            block: b,
            body: block.succs[0],
            typ: t,
            binding: y,
        });
        b = block.succs[1];
        if f.blocks.get(b).instrs.len() > 4 {
            break;
        }
        if f.blocks.get(b).preds.len() != 1 {
            break;
        }
        let Some((ny, nx, nt)) = is_type_assert_block(f, b) else {
            break;
        };
        if nx != x {
            break;
        }
        y = ny;
        t = nt;
    }
    sw.default = Some(b);
}

fn instr_in_block(f: &Function, b: BlockId, id: InstrId) -> bool {
    f.blocks.get(b).instrs.contains(&id)
}

/// Returns `(v, k)` when `b` ends with `if v == k` and `k` is a constant.
fn is_comparison_block(f: &Function, b: BlockId) -> Option<(Value, ConstId)> {
    let block = f.blocks.get(b);
    let n = block.instrs.len();
    if n < 2 {
        return None;
    }
    let if_id = *block.instrs.last()?;
    let InstrData::If(If { cond }) = f.instrs.get(if_id) else {
        return None;
    };
    let bin_id = match cond {
        Value::Instr(id) => *id,
        _ => return None,
    };
    if !instr_in_block(f, b, bin_id) {
        return None;
    }
    let InstrData::BinOp(BinOp { op, x, y, .. }) = f.instrs.get(bin_id) else {
        return None;
    };
    if *op != Token::EQL {
        return None;
    }
    if let Value::Const(k) = *y {
        return Some((*x, k));
    }
    if let Value::Const(k) = *x {
        return Some((*y, k));
    }
    None
}

/// Returns `(y, x, T)` when `b` ends with `if y, ok := x.(T); ok`.
fn is_type_assert_block(f: &Function, b: BlockId) -> Option<(Value, Value, TypeId)> {
    let block = f.blocks.get(b);
    let n = block.instrs.len();
    if n < 4 {
        return None;
    }
    let if_id = *block.instrs.last()?;
    let InstrData::If(If { cond }) = f.instrs.get(if_id) else {
        return None;
    };
    let ext1_id = match cond {
        Value::Instr(id) => *id,
        _ => return None,
    };
    if !instr_in_block(f, b, ext1_id) {
        return None;
    }
    let InstrData::Extract(Extract {
        tuple,
        index: 1,
        ..
    }) = f.instrs.get(ext1_id)
    else {
        return None;
    };
    let ta_id = match tuple {
        Value::Instr(id) => *id,
        _ => return None,
    };
    if !instr_in_block(f, b, ta_id) {
        return None;
    }
    let InstrData::TypeAssert(TypeAssert { x, assert_type, .. }) = f.instrs.get(ta_id) else {
        return None;
    };
    let ext0_id = *block.instrs.get(n - 3)?;
    let InstrData::Extract(Extract {
        tuple: tuple0,
        index: 0,
        ..
    }) = f.instrs.get(ext0_id)
    else {
        return None;
    };
    if tuple0 != &Value::Instr(ta_id) {
        return None;
    }
    Some((Value::Instr(ext0_id), *x, *assert_type))
}
