//! The `Value` handle — an SSA expression that yields a value.
//!
//! Port of go/ssa's `Value` interface. In Go, an instruction's operands are
//! `*Value` pointers into a heap graph; here an operand is a `Value` handle
//! stored inline in the referencing instruction. Rewriting an operand (as the
//! lifter does) simply mutates the field.
//!
//! `Value` is `Copy` and 8 bytes (a tag plus a `NonZeroU32` id), so passing
//! and storing operands is cheap.

use crate::ids::{BuiltinId, ConstId, FreeVarId, FuncId, GlobalId, InstrId, ParamId};

/// An SSA value: a `Copy` handle into a `Function`'s or `Program`'s arenas.
///
/// The variant determines which arena the payload id addresses. Function-local
/// variants ([`Value::Instr`], [`Value::Param`], [`Value::FreeVar`]) are only
/// meaningful within their parent function; the rest are program-level.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum Value {
    /// The result of a value-defining instruction (a virtual register):
    /// `BinOp`, `Call`, `Phi`, `Alloc`, ... (function-local).
    Instr(InstrId),
    /// A function parameter (function-local).
    Param(ParamId),
    /// A free variable captured by a closure (function-local).
    FreeVar(FreeVarId),
    /// A constant value (program-level; `Parent()` is nil in go/ssa).
    Const(ConstId),
    /// The address of a package-level variable (program-level).
    Global(GlobalId),
    /// A built-in function such as `len` or `append` (program-level).
    Builtin(BuiltinId),
    /// A function used as a value: package-level, method, or anonymous
    /// (program-level).
    Function(FuncId),
}
