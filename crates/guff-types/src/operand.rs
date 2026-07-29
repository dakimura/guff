//! Port of `cmd/compile/internal/types2/operand.go`.
//!
//! An [`Operand`] is the abstract value carried by the type checker as it
//! walks expressions: it captures the addressing mode, the source `Expr`,
//! the resolved type, and (for constants) the constant value.
//!
//! ## Wiring
//!
//! - `expr` borrows [`guff::ast::Expr`] from the AST (Go's `*syntax.Expr`;
//!   no deep clone on the typecheck hot path).
//! - `val` uses [`guff_constant::Value`] from `guff-constant`.
//! - `id` uses our existing [`crate::BuiltinId`] for the `Builtin` mode.
//!
//! All three are `Option`s — synthetic operands constructed inside the
//! Checker may have `expr = None`; non-constant modes have `val = None`;
//! non-Builtin modes have `id = None`.
//!
//! ## Chunk-14 deferrals
//!
//! - **`assignableTo`** — the heavyweight assignability decision is in
//!   Go's `operand.assignableTo` and pulls in `Checker.implements`,
//!   `assertableTo`, `convertibleTo`, etc. None of those are ported yet
//!   (chunk-11 / chunk-12 deferrals cascade). Will land with
//!   `assignments.go` / `conversions.go` in chunks 14b/14c.
//! - **Full `operandString` rendering** — uses Go's `ExprString`,
//!   `TypeString`, `WriteType`, all part of `typestring.go` (Tier 5).
//!   For now [`Operand::to_string`] produces a minimal, parseable
//!   placeholder.

use std::fmt;

use guff::ast::Expr;
use guff::token::Token;
use guff_constant::Value;

use crate::arena::{TypeArena, TypeData, TypeId};
use crate::basic::{BasicKind, IS_UNTYPED};
use crate::object::builtin::BuiltinId;
use crate::predicates::is_valid;

/// Addressing mode of an [`Operand`]. The discriminants match Go's `iota`
/// order so cross-tool dumps line up.
///
/// Equivalent to `types2.operandMode`.
#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub enum OperandMode {
    /// Operand is invalid (default).
    #[default]
    Invalid = 0,
    /// Operand represents no value (result of a function call w/o result).
    NoValue = 1,
    /// Operand is a built-in function.
    Builtin = 2,
    /// Operand is a type.
    TypeExpr = 3,
    /// Operand is a constant; `typ` is a Basic type.
    Constant = 4,
    /// Operand is an addressable variable.
    Variable = 5,
    /// Operand is a map index expression (variable-like on LHS,
    /// commaok on RHS).
    MapIndex = 6,
    /// Operand is a computed value.
    Value = 7,
    /// Operand is the nil value (only used by types2).
    NilValue = 8,
    /// Like `Value`, but the operand may be used in a `comma, ok`
    /// expression.
    CommaOk = 9,
    /// Like `CommaOk`, but the second value is `error`, not `bool`.
    CommaErr = 10,
    /// Operand is a cgo function.
    CgoFunc = 11,
}

impl OperandMode {
    /// Short, human-readable name for the mode. Matches Go's
    /// `operandModeString` table.
    pub fn as_str(self) -> &'static str {
        match self {
            OperandMode::Invalid => "invalid operand",
            OperandMode::NoValue => "no value",
            OperandMode::Builtin => "built-in",
            OperandMode::TypeExpr => "type",
            OperandMode::Constant => "constant",
            OperandMode::Variable => "variable",
            OperandMode::MapIndex => "map index expression",
            OperandMode::Value => "value",
            OperandMode::NilValue => "nil",
            OperandMode::CommaOk => "comma, ok expression",
            OperandMode::CommaErr => "comma, error expression",
            OperandMode::CgoFunc => "cgo function",
        }
    }
}

/// An intermediate value during type checking.
///
/// The zero value (via [`Operand::invalid`]) is a ready-to-use invalid
/// operand — matching Go's zero-value contract.
///
/// `'a` is the lifetime of the AST expression this operand refers to
/// (package syntax for real nodes; a stack-local synthetic `Expr` for
/// compound assignments and similar).
///
/// Equivalent to `types2.operand`.
#[derive(Debug, Clone, Default)]
pub struct Operand<'a> {
    pub mode: OperandMode,
    pub expr: Option<&'a Expr>,
    pub typ: Option<TypeId>,
    pub val: Option<Value>,
    pub id: Option<BuiltinId>,
}

impl<'a> Operand<'a> {
    /// A fresh invalid operand. Equivalent to Go's `operand{}` zero value.
    pub fn invalid() -> Self {
        Self::default()
    }

    /// Position of the expression corresponding to this operand. Returns
    /// `0` (= nopos) if `expr` is `None`.
    ///
    /// Equivalent to `operand.Pos`.
    pub fn pos(&self) -> i64 {
        match self.expr {
            Some(e) => e.pos().0,
            None => 0,
        }
    }

    /// Reports whether this operand is the untyped `nil` value.
    ///
    /// Equivalent to `operand.isNil` (types2 branch — `go/types` uses a
    /// different check but we don't expose that here).
    pub fn is_nil(&self) -> bool {
        self.mode == OperandMode::NilValue
    }

    /// Set this operand to the untyped constant produced by parsing
    /// `lit` as a literal of kind `tok`. On parse failure the operand
    /// becomes [`OperandMode::Invalid`] with type `Typ[Invalid]`.
    ///
    /// `typ_table` is the predeclared types lookup (from
    /// [`crate::init_universe`] or [`crate::init_universe_full`]).
    ///
    /// `tok` must be one of [`Token::INT`], [`Token::FLOAT`],
    /// [`Token::IMAG`], [`Token::CHAR`], [`Token::STRING`] (matching
    /// Go's `syntax.LitKind`).
    ///
    /// Equivalent to `operand.setConst`.
    pub fn set_const(&mut self, typ_table: &[TypeId], tok: Token, lit: &str) {
        let basic_kind = match tok {
            Token::INT => BasicKind::UntypedInt,
            Token::FLOAT => BasicKind::UntypedFloat,
            Token::IMAG => BasicKind::UntypedComplex,
            Token::CHAR => BasicKind::UntypedRune,
            Token::STRING => BasicKind::UntypedString,
            other => panic!("set_const: not a literal token: {:?}", other),
        };
        let val = guff_constant::make_from_literal(lit, tok, 0);
        if matches!(val, Value::Unknown) {
            self.mode = OperandMode::Invalid;
            self.typ = Some(typ_table[BasicKind::Invalid as usize]);
            return;
        }
        self.mode = OperandMode::Constant;
        self.typ = Some(typ_table[basic_kind as usize]);
        self.val = Some(val);
    }
}

/// Kind of composite type, used in error messages ("array", "slice", …).
/// Returns `""` for basic types (matching Go's empty-string sentinel).
///
/// Equivalent to `compositeKind`.
pub fn composite_kind(arena: &TypeArena, typ: TypeId) -> &'static str {
    let u = typ.underlying(arena);
    match arena.get(u) {
        TypeData::Basic(_) => "",
        TypeData::Array(_) => "array",
        TypeData::Slice(_) => "slice",
        TypeData::Struct(_) => "struct",
        TypeData::Pointer(_) => "pointer",
        TypeData::Signature(_) => "func",
        TypeData::Interface(_) => "interface",
        TypeData::Map(_) => "map",
        TypeData::Chan(_) => "chan",
        TypeData::Tuple(_) => "tuple",
        TypeData::Union(_) => "union",
        // Named/Alias/TypeParam shouldn't appear under `Underlying()` —
        // Named/Alias return their underlying, TypeParam returns iface.
        // Treat defensively as basic-ish.
        _ => "",
    }
}

/// Renders an operand the way Go's `operandString` does (mode-first layout),
/// using [`crate::typestring::type_string`] for type names (chunk 17 wired
/// this in, replacing the earlier `type#{id}` placeholders). Always uses the
/// default (import-path) qualifier.
///
/// Equivalent to `operandString`.
pub fn operand_string(
    arena: &TypeArena,
    oarena: &crate::arena::ObjectArena,
    parena: &crate::arena::PackageArena,
    x: &Operand<'_>,
) -> String {
    // Special-case nilvalue first.
    if x.mode == OperandMode::NilValue {
        return match x.typ {
            None => "nil (with invalid type)".to_string(),
            Some(t) => match arena.get(t) {
                TypeData::Basic(b) if b.kind() == BasicKind::Invalid => {
                    "nil (with invalid type)".to_string()
                }
                TypeData::Basic(b) if b.kind() == BasicKind::UntypedNil => "nil".to_string(),
                _ => format!(
                    "nil (of type {})",
                    crate::typestring::type_string(arena, oarena, parena, t, None)
                ),
            },
        };
    }

    let mut out = String::new();
    // mode header
    out.push_str(x.mode.as_str());

    // <untyped kind> for non-no-type modes
    let has_type = !matches!(
        x.mode,
        OperandMode::Invalid | OperandMode::NoValue | OperandMode::Builtin | OperandMode::TypeExpr
    ) && x.typ.is_some();

    if let Some(t) = x.typ {
        if has_type {
            if let TypeData::Basic(b) = arena.get(t) {
                if (b.info().0 & IS_UNTYPED.0) != 0 {
                    out.push_str(" ");
                    out.push_str(b.name());
                }
            }
        }
    }

    if let Some(v) = &x.val {
        if x.mode == OperandMode::Constant {
            out.push_str(" ");
            out.push_str(&v.to_string());
        }
    }

    if has_type {
        if let Some(t) = x.typ {
            if is_valid(arena, t) {
                let what = composite_kind(arena, t);
                let name = crate::typestring::type_string(arena, oarena, parena, t, None);
                if !what.is_empty() {
                    out.push_str(&format!(" of {} type {}", what, name));
                } else {
                    out.push_str(&format!(" of type {}", name));
                }
            } else {
                out.push_str(" with invalid type");
            }
        }
    }

    out
}

impl fmt::Display for Operand<'_> {
    /// `Display` requires a [`TypeArena`] for full rendering, so this
    /// fallback uses a placeholder when none is available. Prefer
    /// [`operand_string`] in test/debug code.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<operand mode={}>", self.mode.as_str())
    }
}

// ---------------------------------------------------------------------------
// Forward pointer for chunk-14b/14c (`conversions.go` / `assignments.go`)
//
// - `Operand::assignable_to(arena, T, cause)` — currently DEFERRED. The Go
//   implementation in `operand.assignableTo` uses Checker.implements,
//   newAssertableTo, and other Checker-dependent helpers. When lifting,
//   the signature will roughly be:
//
//       pub fn assignable_to(
//           type_arena: &mut TypeArena,
//           object_arena: &ObjectArena,
//           package_arena: &PackageArena,
//           x: &Operand,
//           t: TypeId,
//       ) -> AssignableResult { ... }
//
//   where `AssignableResult` carries the boolean plus an
//   `internal/types/errors.Code` (we have these via `guff-types-errors`).
