//! Port of `internal/pkgbits/reloc.go`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RelocKind(pub i32);

impl RelocKind {
    pub const STRING: Self = Self(0);
    pub const META: Self = Self(1);
    pub const POS_BASE: Self = Self(2);
    pub const PKG: Self = Self(3);
    pub const NAME: Self = Self(4);
    pub const TYPE: Self = Self(5);
    pub const OBJ: Self = Self(6);
    pub const OBJ_EXT: Self = Self(7);
    pub const OBJ_DICT: Self = Self(8);
    pub const BODY: Self = Self(9);
    pub const NUM_RELOC: usize = 10;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Index(pub i32);

pub const PUBLIC_ROOT_IDX: Index = Index(0);

#[derive(Debug, Clone, Copy)]
pub struct RelocEnt {
    pub kind: RelocKind,
    pub idx: Index,
}
