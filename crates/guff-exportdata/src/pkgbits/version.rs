//! Port of `internal/pkgbits/version.go`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version(u32);

impl Version {
    pub const V0: Self = Self(0);
    pub const V1: Self = Self(1);
    pub const V2: Self = Self(2);
    pub const NUM_VERSIONS: u32 = 3;

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
}

fn introduced(f: Field) -> u32 {
    match f {
        Field::Flags => 1,
        Field::AliasTypeParamNames => 2,
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
