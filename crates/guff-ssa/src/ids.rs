//! Typed arena handles for SSA objects (see docs/DEVELOPMENT.md §2.3).
//!
//! Each id is a `NonZeroU32` newtype so `Option<Id>` occupies 4 bytes. Ids are
//! 1-indexed internally; index 0 is reserved as the niche.
//!
//! Two ownership scopes exist, mirroring go/ssa:
//! - **program-level**: [`FuncId`], [`GlobalId`], [`ConstId`], [`BuiltinId`],
//!   [`PackageId`] — owned by the `Program`.
//! - **function-local**: [`BlockId`], [`InstrId`], [`ParamId`], [`FreeVarId`] —
//!   owned by a single `Function` (mirrors go/ssa's function-local numbering).

use crate::arena::ArenaId;
use std::num::NonZeroU32;

macro_rules! def_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
        pub struct $name(NonZeroU32);

        impl ArenaId for $name {
            #[inline]
            fn from_index(index: usize) -> Self {
                let raw = (index + 1) as u32;
                $name(NonZeroU32::new(raw).expect("arena index never 0"))
            }

            #[inline]
            fn index(self) -> usize {
                (self.0.get() - 1) as usize
            }
        }
    };
}

def_id!(
    /// Handle to a `Function` owned by a `Program`.
    FuncId
);
def_id!(
    /// Handle to a package-level `Global` variable.
    GlobalId
);
def_id!(
    /// Handle to a `Const` value.
    ConstId
);
def_id!(
    /// Handle to a `Builtin` function value.
    BuiltinId
);
def_id!(
    /// Handle to an SSA `Package`.
    PackageId
);
def_id!(
    /// Handle to a `BasicBlock` within a `Function`.
    BlockId
);
def_id!(
    /// Handle to an `Instruction` within a `Function`.
    InstrId
);
def_id!(
    /// Handle to a `Parameter` within a `Function`.
    ParamId
);
def_id!(
    /// Handle to a `FreeVar` (closure capture) within a `Function`.
    FreeVarId
);
