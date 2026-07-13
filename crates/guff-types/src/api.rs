//! Port of the public API surface from `cmd/compile/internal/types2/api.go`.
//!
//! This chunk (18a) ports only the *scaffolding* structs the Checker needs to
//! exist before its engine can be built:
//!
//! - [`Config`] — type-checker configuration (minimal subset, §6.2 of the
//!   migration plan).
//! - [`Info`] — the result-recording struct (minimal subset, §6.3).
//! - [`TypeAndValue`] — the (mode, type, value) triple stored per expression.
//! - [`TypeCheckError`] — a collected diagnostic (§6.6). Go reports errors
//!   eagerly through `Config.Error`; we collect them in a `Vec` instead so
//!   tests can inspect them.
//!
//! ## Deferrals (chunk-18a)
//!
//! - `Config`: `Importer`, `Sizes`, the `Error func(error)` callback,
//!   `FakeImportC`, `EnableAlias`, `ErrorURL`, `IgnoreFuncBodies`, etc. are
//!   omitted until a consumer needs them (D14 in the deferral table).
//! - `Info`: `types`/`defs`/`uses`/`init_order`/`selections`/`instances` are
//!   present (chunks 49–53). `Implicits`, `Scopes`, `FileVersions` remain
//!   omitted — they key on statement/spec/file nodes, which carry no stamped
//!   id in our `Expr`/`Ident`-only AST. All maps are keyed by a `u32` AST-node
//!   id (the stamped node id; see [`guff::stamp`](guff::stamp)).
//! - `TypeAndValue`: Go's bit-flag predicate methods (`IsValue`, `IsType`,
//!   `Addressable`, `HasOk`, …) are DEFERRED — the `mode` field is retained
//!   so they can be added trivially later.

use std::collections::HashMap;

use guff_constant::Value;
use guff_types_errors::Code;

use crate::arena::TypeId;
use crate::operand::OperandMode;

/// Type-checker configuration.
///
/// Minimal subset of Go's `types2.Config` — see module docs for what's
/// deferred.
#[derive(Debug, Default, Clone)]
pub struct Config {
    /// Accepted Go language version, e.g. `"go1.26"`. Empty disables version
    /// checks. (Go: `Config.GoVersion`.)
    pub go_version: String,

    /// If set, packages are not checked for unused imports.
    /// (Go: `Config.DisableUnusedImportCheck`.)
    pub disable_unused_import_check: bool,

    /// If set, a debug trace is printed. (Go: `Config.Trace`.)
    pub trace: bool,

    /// Sizing functions for package `unsafe` (`Sizeof`/`Alignof`/`Offsetof`).
    /// `None` means the default (`gc`/`amd64`) sizes are used, matching Go's
    /// `stdSizes` fallback when `Config.Sizes == nil`. (Go: `Config.Sizes`.)
    pub sizes: Option<crate::sizes::Sizes>,

    /// If set, `import "C"` is allowed and treated as importing package `"C"`.
    /// (Go: `Config.FakeImportC`.)
    pub fake_import_c: bool,
    // IgnoreFuncBodies, IgnoreBranchErrors, ErrorURL, EnableAlias. Add from
    // api.go when a consumer needs them.
}

/// Result type information for a type-checked package.
///
/// Minimal subset of Go's `types2.Info`. Maps are keyed by a `u32` AST-node
/// id rather than by `*syntax.Expr`/`*syntax.Name` pointers. `defs`/`uses` are
/// recorded (chunk 49) and `types` (chunk 50) via
/// [`Checker::record`](crate::Checker::record) /
/// [`record_type_and_value`](crate::Checker::record_type_and_value), keyed on
/// the stable node id stamped onto every `Expr`
/// (see [`guff::stamp`](guff::stamp)).
#[derive(Debug, Default, Clone)]
pub struct Info {
    /// Maps expressions to their type and (for constants) value.
    /// (Go: `Info.Types`.)
    pub types: HashMap<u32, TypeAndValue>,

    /// Maps identifiers to the objects they define. `None` for identifiers
    /// that denote no object (e.g. blank `_`). (Go: `Info.Defs`.)
    pub defs: HashMap<u32, Option<crate::arena::ObjectId>>,

    /// Maps identifiers to the objects they denote. (Go: `Info.Uses`.)
    pub uses: HashMap<u32, crate::arena::ObjectId>,

    /// Lists the package-level initializers (variables with initialization
    /// expressions) in the order they must be evaluated. Computed by
    /// [`Checker::init_order`](crate::Checker::init_order). (Go: `Info.InitOrder`.)
    pub init_order: Vec<Initializer>,

    /// Maps selector expressions (excluding qualified identifiers) to their
    /// corresponding selections. Keyed on the `SelectorExpr`'s node id.
    /// Recorded by [`Checker::record_selection`](crate::Checker::record_selection).
    /// (Go: `Info.Selections`.)
    pub selections: HashMap<u32, crate::selection::Selection>,

    /// Maps identifiers denoting generic types or functions to their type
    /// arguments and instantiated type. Keyed on the *instantiated identifier*
    /// node id (the `T` in `T[int]`, the `Sel` of `pkg.T[int]`). Recorded by
    /// [`Checker::record_instance`](crate::Checker::record_instance).
    /// (Go: `Info.Instances`.)
    pub instances: HashMap<u32, Instance>,

    /// Maps the node that opens a lexical scope to that [`ScopeId`]. Keyed on
    /// the node id stamped onto scope-bearing nodes: `File`, `FuncType` (the
    /// function scope holding params/results/body; not the body `BlockStmt`),
    /// `TypeSpec` (the type-parameter scope of a generic type), `BlockStmt`,
    /// `IfStmt`, `SwitchStmt`, `TypeSwitchStmt`, `CaseClause`, `CommClause`,
    /// `ForStmt`, `RangeStmt`. Recorded by
    /// [`Checker::record_scope`](crate::Checker::record_scope).
    /// (Go: `Info.Scopes`.)
    pub scopes: HashMap<u32, crate::arena::ScopeId>,

    /// Maps nodes to their implicitly declared objects (objects with no explicit
    /// identifier in the source). Keyed on:
    /// - the `CaseClause` of a type switch with a binding (`switch v := x.(type)`)
    ///   → the case-specific narrowed `Var`;
    /// - a `Field` of an anonymous parameter/result (`func(int)`) → its `Var`;
    /// - a `Field` of an unnamed method receiver (`func (T) M()`) → its recv `Var`;
    /// - an `ImportSpec` with no explicit name (`import "unsafe"`) → its `PkgName`
    ///   (only `unsafe` is resolvable without an importer, D16).
    ///
    /// Recorded by [`Checker::record_implicit`](crate::Checker::record_implicit).
    /// (Go: `Info.Implicits`.)
    pub implicits: HashMap<u32, crate::arena::ObjectId>,

    /// Maps each checked file's AST id to its `//go:build` language version.
    /// (Go: `Info.FileVersions`.)
    pub file_versions: HashMap<u32, String>,
    // DEFERRED (D14): StoreTypesInSyntax. Non-`unsafe` imports
    // can't be resolved without an importer (D16), so their PkgName implicits
    // are naturally absent.
}

/// The type arguments and instantiated type of a generic instantiation.
///
/// Equivalent to `types2.Instance`. Go stores the type arguments as a
/// `*TypeList`; we keep a plain `Vec<TypeId>` (the same information).
///
/// Invariant (Go): instantiating `Uses[id].typ()` with `TypeArgs` yields an
/// equivalent of `typ`.
#[derive(Debug, Clone)]
pub struct Instance {
    /// The type arguments, in source order.
    pub type_args: Vec<TypeId>,
    /// The resulting instantiated type (a `Named` or `Signature`).
    pub typ: TypeId,
}

/// An initialization of one or more package-level variables from a single
/// initialization expression.
///
/// Equivalent to `types2.Initializer`. For an n:1 declaration such as
/// `a, b = f()`, `lhs` lists every assigned variable in source order and `rhs`
/// is the shared init expression. For a simple `var x = e`, `lhs` is `[x]`.
#[derive(Debug, Clone)]
pub struct Initializer {
    /// The variable(s) being initialized, in source order.
    pub lhs: Vec<crate::arena::ObjectId>,
    /// The initialization expression.
    pub rhs: guff::ast::Expr,
}

/// The type and (for constants) value of an expression.
///
/// Equivalent to `types2.TypeAndValue`. Go's bit-flag predicate methods are
/// DEFERRED (the `mode` field carries the information needed to add them).
#[derive(Debug, Clone)]
pub struct TypeAndValue {
    /// Addressing mode of the expression (Go's unexported `mode` field).
    pub mode: OperandMode,
    /// Type of the expression.
    pub typ: TypeId,
    /// Constant value, if the expression is a constant.
    pub val: Option<Value>,
}

// DEFERRED (chunk-18a): TypeAndValue::{is_void, is_type, is_builtin, is_value,
// is_nil, addressable, assignable, has_ok} — Go's bit-flag predicate methods.
// They are pure functions of `mode`; add when a consumer needs them.

/// A collected type-checking diagnostic.
///
/// Go reports errors eagerly via `Config.Error` (or stops at the first one).
/// We collect them in `Checker.errors` instead, so tests can assert on the
/// set produced. See §6.6 of the migration plan.
#[derive(Debug, Clone)]
pub struct TypeCheckError {
    /// Source position. `0` = unknown (full `syntax.Pos` integration is
    /// deferred, D07).
    pub pos: u32,
    /// The error code (`internal/types/errors.Code`).
    pub code: Code,
    /// Rendered message.
    pub msg: String,
}
