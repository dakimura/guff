//! Port of `internal/pkgbits/codes.go`.

use super::sync::SyncMarker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CodeVal {
    Bool = 0,
    String = 1,
    Int64 = 2,
    BigInt = 3,
    BigRat = 4,
    BigFloat = 5,
}

impl CodeVal {
    pub fn marker() -> SyncMarker {
        SyncMarker::VAL
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CodeType {
    Basic = 0,
    Named = 1,
    Pointer = 2,
    Slice = 3,
    Array = 4,
    Chan = 5,
    Map = 6,
    Signature = 7,
    Struct = 8,
    Interface = 9,
    Union = 10,
    TypeParam = 11,
}

impl CodeType {
    pub fn marker() -> SyncMarker {
        SyncMarker::TYPE
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CodeObj {
    Alias = 0,
    Const = 1,
    Type = 2,
    Func = 3,
    Var = 4,
    Stub = 5,
}

impl CodeObj {
    pub fn marker() -> SyncMarker {
        SyncMarker::CODE_OBJ
    }
}
