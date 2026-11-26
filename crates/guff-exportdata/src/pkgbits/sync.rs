//! Port of `internal/pkgbits/sync.go` (SyncMarker constants).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncMarker(pub i32);

impl SyncMarker {
    pub const EOF: Self = Self(1);
    pub const BOOL: Self = Self(2);
    pub const INT64: Self = Self(3);
    pub const UINT64: Self = Self(4);
    pub const STRING: Self = Self(5);
    pub const VALUE: Self = Self(6);
    pub const VAL: Self = Self(7);
    pub const RELOCS: Self = Self(8);
    pub const RELOC: Self = Self(9);
    pub const USE_RELOC: Self = Self(10);
    pub const PUBLIC: Self = Self(11);
    pub const POS: Self = Self(12);
    pub const POS_BASE: Self = Self(13);
    pub const OBJECT: Self = Self(14);
    pub const OBJECT1: Self = Self(15);
    pub const PKG: Self = Self(16);
    pub const PKG_DEF: Self = Self(17);
    pub const METHOD: Self = Self(18);
    pub const TYPE: Self = Self(19);
    pub const TYPE_IDX: Self = Self(20);
    pub const TYPE_PARAM_NAMES: Self = Self(21);
    pub const SIGNATURE: Self = Self(22);
    pub const PARAMS: Self = Self(23);
    pub const PARAM: Self = Self(24);
    pub const CODE_OBJ: Self = Self(25);
    pub const SYM: Self = Self(26);
    pub const LOCAL_IDENT: Self = Self(27);
    pub const SELECTOR: Self = Self(28);
}
