//! Port of `cmd/compile/internal/types2/util.go` (Step 39).
//!
//! Go factors differences between `go/types` (go/ast) and `types2`
//! (cmd/compile/internal/syntax) into this file. Our AST is go/ast-shaped
//! (`guff-ast`), so most helpers are thin wrappers around fields already
//! consulted at call sites. Centralising them keeps the Go↔Rust mapping
//! discoverable and lets call sites share one spelling.
//!
//! Helpers that already live elsewhere (and are not re-exported here):
//! - `makeFromLiteral` → [`guff_constant::make_from_literal`]
//! - `ExprString` → deferred (operand rendering uses type/value strings)

use guff::ast::{ArrayType, CallExpr, Expr};
use guff::Pos;

/// Reports whether the last argument in `call` is followed by `...`.
///
/// Equivalent to `hasDots`.
pub fn has_dots(call: &CallExpr) -> bool {
    call.ellipsis.0 != 0
}

/// Reports whether `atyp` is of the form `[...]E`.
///
/// Equivalent to go/types `isdddArray` (types2 uses `Len == nil` for the
/// compiler syntax; go/ast encodes it as an `Ellipsis` length expr).
pub fn is_ddd_array(atyp: &ArrayType) -> bool {
    matches!(atyp.len.as_deref(), Some(Expr::Ellipsis(e)) if e.elt.is_none())
}

/// Compare two positions. Negative / zero / positive means before / same /
/// after. Byte offsets in the same file are compared numerically; unknown
/// (`0`) sorts first.
///
/// Equivalent to `cmpPos` for our `u32`/`Pos` encoding (no multi-file
/// filename tie-break — callers that need file identity must layer it).
pub fn cmp_pos(p: Pos, q: Pos) -> i32 {
    (p.0 as i32).saturating_sub(q.0 as i32)
}

/// Start position of an expression.
///
/// Equivalent to `startPos`.
pub fn start_pos(e: &Expr) -> Pos {
    e.pos()
}

/// End position of an expression.
///
/// Equivalent to `endPos`.
pub fn end_pos(e: &Expr) -> Pos {
    e.end()
}
