//! Port of `internal/pkgbits/version.go`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version(u32);

impl Version {
    pub const V0: Self = Self(0);
    pub const V1: Self = Self(1);
    pub const V2: Self = Self(2);
    /// Compact composite-literal element lists (function bodies only).
    pub const V3: Self = Self(3);
    /// Generic methods as standalone function objects (go1.27.0).
    pub const V4: Self = Self(4);
    /// Method index, to keep generic and ordinary methods in source order
    /// (go1.27.x, go.dev/issue/81188).
    pub const V5: Self = Self(5);
    pub const NUM_VERSIONS: u32 = 6;

    pub fn raw(self) -> u32 {
        self.0
    }

    pub fn from_raw(v: u32) -> Self {
        Self(v)
    }

    pub fn has(self, field: Field) -> bool {
        introduced(field) <= self.0 && (self.0 < removed(field) || removed(field) == 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Field {
    Flags = 0,
    HasInit = 1,
    DerivedFuncInstance = 2,
    AliasTypeParamNames = 3,
    DerivedInfoNeeded = 4,
    CompactCompLiterals = 5,
    GenericMethods = 6,
    PreserveMethodOrder = 7,
}

fn introduced(f: Field) -> u32 {
    match f {
        Field::Flags => 1,
        Field::AliasTypeParamNames => 2,
        Field::CompactCompLiterals => 3,
        Field::GenericMethods => 4,
        Field::PreserveMethodOrder => 5,
        _ => 0,
    }
}

fn removed(f: Field) -> u32 {
    match f {
        Field::HasInit | Field::DerivedFuncInstance | Field::DerivedInfoNeeded => 2,
        _ => 0,
    }
}

pub const FLAG_SYNC_MARKERS: u32 = 1 << 0;
