//! SSA Package members.

use crate::ids::{ConstId, FuncId, GlobalId};
use guff_types::TypeId;

/// MemberData represents a member of a Go package.
/// (Go: `Member` interface implementations)
#[derive(Debug, Clone, Copy)]
pub enum MemberData {
    Global(GlobalId),
    Function(FuncId),
    NamedConst(ConstId),
    Type(TypeId),
}
