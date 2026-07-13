//! Port of `cmd/compile/internal/types2/chan.go`.

use crate::arena::{TypeArena, TypeData, TypeId};

/// Channel direction.
///
/// Equivalent to `types2.ChanDir`. Numeric values match Go's `iota` ordering
/// for cross-language tooling.
#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum ChanDir {
    SendRecv = 0,
    SendOnly = 1,
    RecvOnly = 2,
}

/// A channel type.
///
/// Equivalent to `types2.Chan`.
#[derive(Debug, Clone)]
pub struct Chan {
    dir: ChanDir,
    elem: TypeId,
}

impl Chan {
    pub fn dir(&self) -> ChanDir {
        self.dir
    }

    pub fn elem(&self) -> TypeId {
        self.elem
    }
}

/// Construct a new channel type.
///
/// Equivalent to `types2.NewChan`.
pub fn new_chan(arena: &mut TypeArena, dir: ChanDir, elem: TypeId) -> TypeId {
    arena.alloc(TypeData::Chan(Chan { dir, elem }))
}

/// Free-function accessor — panics if `id` is not a Chan.
pub fn chan_dir(arena: &TypeArena, id: TypeId) -> ChanDir {
    as_chan(arena, id).dir
}

pub fn chan_elem(arena: &TypeArena, id: TypeId) -> TypeId {
    as_chan(arena, id).elem
}

fn as_chan(arena: &TypeArena, id: TypeId) -> &Chan {
    match arena.get(id) {
        TypeData::Chan(c) => c,
        other => panic!("expected Chan, got {:?}", std::mem::discriminant(other)),
    }
}
