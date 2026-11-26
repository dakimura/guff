//! Port of error-reporting from `cmd/compile/internal/types2/errors.go`
//! (plus the type-rendering slice of `format.go`).
//!
//! Go reports errors eagerly: `Checker.error`/`errorf` build an `error_`, then
//! `report()` calls `conf.Error` (or bails on the first one). Our checker has
//! no callback — instead [`Checker::error`] *collects* diagnostics into
//! `self.errors` (and remembers the first message in `self.first_err`) so
//! tests can inspect the whole set. See §6.6 of the migration plan.
//!
//! ## `errorf` convention (chunk-19)
//!
//! Rust has no variadic `printf`, so we do **not** port `errorf`. Every Go
//! `check.errorf(at, Code, "x %s", y)` becomes
//! `self.error(at, Code, format!("x {}", y))` at the call site. Type names in
//! a `%s` position are rendered with [`Checker::type_str`].
//!
//! ## Deferrals (chunk-19, see §8)
//!
//! - The follow-on suppression "cheap trick" (dropping later messages that
//!   contain "invalid operand"/"invalid type"), `soft` errors, multi-line
//!   sub-errors (`desc []errorDesc`), continuation errors, `runtime.Caller`
//!   source locations, `ErrorURL`, and duplicate-report suppression are all
//!   omitted. One error = one message, for now.
//! - The rich `format.go` `sprintf` (operands, objects, positions, `this`)
//!   is deferred to chunk-40; only type rendering is wired here.

use guff_types_errors::Code;

use crate::arena::TypeId;
use crate::check::Checker;

impl Checker {
    /// Record a type-checking error at `pos` with code `code` and message
    /// `msg`. The first message also becomes [`Checker::first_err`].
    ///
    /// Equivalent to `Checker.error` followed by `report`/`handleError`, in
    /// the collecting model (no eager callback, no bailout).
    pub fn error(&mut self, pos: u32, code: Code, msg: impl Into<String>) {
        let mut msg = msg.into();

        // Report invalid syntax trees explicitly (mirrors handleError).
        if code == Code::InvalidSyntaxTree {
            msg = format!("invalid syntax tree: {}", msg);
        }

        if self.first_err.is_none() {
            self.first_err = Some(msg.clone());
        }

        self.errors
            .push(crate::api::TypeCheckError { pos, code, msg });
    }

    /// Render a type as a string for embedding in an error message, qualifying
    /// foreign packages by name and leaving same-package types bare.
    ///
    /// This is the type-name slice of Go's `Checker.sprintf` `%s` handling; the
    /// package qualifier matches `(*Checker).qualifier` (see `format.rs`,
    /// chunk-59). The remaining `sprintf` argument renderers (operands,
    /// type/operand lists) also live in `format.rs`.
    pub fn type_str(&self, t: TypeId) -> String {
        let cur = self.pkg;
        let qf = move |pkg, parena: &crate::arena::PackageArena| {
            crate::format::qualifier(cur, pkg, parena)
        };
        crate::typestring::type_string(&self.types, &self.objects, &self.packages, t, Some(&qf))
    }
}
