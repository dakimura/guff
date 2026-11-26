//! Common traits for SSA nodes.

use crate::ids::FuncId;

/// A Node is an SSA node that can report its source position.
/// (Go: `Node`)
pub trait Node {
    // DEFERRED: pos()
}

/// A Member is a member of a Go package.
/// (Go: `Member`)
pub trait Member: Node {
    // DEFERRED: methods
}

/// A Value is an SSA value that can be referenced by an instruction.
/// (Go: `Value`)
pub trait Value: Node {
    // DEFERRED: methods
}

/// An Instruction is an SSA instruction that can be executed.
/// (Go: `Instruction`)
pub trait Instruction: Node {
    /// Returns the function that contains this instruction.
    fn parent(&self) -> FuncId;
    // DEFERRED: other methods
}
