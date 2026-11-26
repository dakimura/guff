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
    /// DEFERRED: go/ssa's `NewConst` *normalizes* a `None` value through
    /// `soleTypeKind` — `0` for any numeric type, `false` for a boolean, `""`
    /// for a string — so a zero constant of `int` reads back as the integer `0`
    /// rather than as "no value" (x/tools `go/ssa/const.go`).
    ///
    /// guff keeps the `None`, which makes every consumer responsible for the
    /// normalization. It is not academic: `var acc int` has no initializing
    /// store, so the lifter feeds the loop phi one of these, and a range
    /// analysis that reads it as unknown loses the bound. gosec G115 hit exactly
    /// that and normalizes locally (`gosec_g115::const_id_int64`); fixing it
    /// here would be the real repair, and touches every SSA consumer — see
    /// docs/DEVELOPMENT.md §8 R27.1.
    pub fn new(val: Option<ConstantValue>, typ: TypeId) -> Self {
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
