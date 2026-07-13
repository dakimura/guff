//! SSA Constants.

use guff_constant::Value as ConstantValue;
use guff_types::TypeId;

/// Const represents a Go constant.
/// (Go: `Const`)
pub struct Const {
    pub typ: TypeId,
    pub val: Option<ConstantValue>,
}

impl Const {
    pub fn new(val: Option<ConstantValue>, typ: TypeId) -> Self {
        // TODO: soleTypeKind logic if val is None
        Self { typ, val }
    }

    /// Returns a new "zero" constant of the specified type.
    pub fn zero(typ: TypeId) -> Self {
        Self::new(None, typ)
    }

    /// IsNil returns true if this constant is a nil value.
    pub fn is_nil(&self) -> bool {
        self.val.is_none()
        // DEFERRED: nillable(self.typ) check
    }
}
