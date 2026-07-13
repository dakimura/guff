//! Port of (error and trace) message-formatting support from
//! `cmd/compile/internal/types2/format.go`.
//!
//! ## What maps, and what doesn't
//!
//! Go's `sprintf(qf, tpSubscripts, format, args...)` is a `fmt.Sprintf`
//! wrapper that pre-renders each `any` argument (an `*operand`, `Type`,
//! `Object`, `syntax.Pos`, `[]Type`, `[]*TypeParam`, …) to a string before the
//! `%s` substitution. Rust has no variadic `printf`, so callers already build
//! messages with `format!` and render type names with
//! [`Checker::type_str`](crate::check::Checker::type_str) (see the chunk-19
//! note in `errors.rs`). We therefore do **not** port `sprintf` verbatim;
//! instead we provide the per-argument renderers it relied on so call sites can
//! drop them straight into `format!`:
//!
//! - [`Checker::qualifier`] — the package qualifier (`format.go`'s
//!   `(*Checker).qualifier`), now also threaded into `type_str` so foreign
//!   packages render as `pkg.T` while same-package types stay bare `T`.
//! - [`Checker::type_list_str`] / [`Checker::operand_list_str`] — the bracketed
//!   `[]Type` / `[]*operand` cases.
//! - [`strip_annotations`] — `stripAnnotations`, for cleaning internal
//!   subscript annotations out of a finished message.
//! - [`ndigits`] — the `trace` helper.
//!
//! ## Deferrals (chunk-59, see §8)
//!
//! - `qualifier`'s `pkgPathMap`/`markImports` de-duplication (display the full
//!   quoted import path when two imported packages share a name) needs the
//!   importer (D16); until then a foreign package renders with its bare name.
//! - `trace`/`dump` (debug tracing to stdout) need real `syntax.Pos` (D07) and
//!   the `check.indent` counter; omitted. `ndigits` is ported because it is a
//!   pure helper and trivially testable.
//! - `tpSubscripts` type-parameter subscripts are already dropped by
//!   `typestring.rs` (its non-hashing path), so the flag has no analogue here.

use crate::arena::{PackageArena, PackageId};
use crate::check::Checker;
use crate::operand::{operand_string, Operand};
use crate::typestring::{type_string, Qualifier};

/// The lowest subscript digit, `'₀'` (U+2080). `stripAnnotations` strips the
/// ten code points `'₀'..='₉'`.
const SUBSCRIPT_ZERO: char = '\u{2080}';

/// Free-function form of [`Checker::qualifier`]: returns the qualifier string
/// for `pkg` when the package being checked is `cur`.
///
/// Mirrors `(*Checker).qualifier`: the empty string for the package under
/// check (so its own objects render unqualified), otherwise the package name.
/// The `pkgPathMap` path-disambiguation branch is deferred (D16).
pub fn qualifier(cur: PackageId, pkg: PackageId, parena: &PackageArena) -> String {
    if pkg != cur {
        parena.get(pkg).name().to_string()
    } else {
        String::new()
    }
}

/// `stripAnnotations` — remove internal subscript-digit annotations from `s`.
///
/// Faithful to the Go source: a rune is kept unless it is a subscript digit
/// `'₀'..='₉'` (U+2080..U+2089). (The Go comment also mentions `#`, but the
/// guard `r < '₀' || '₀'+10 <= r` keeps `#`, so we do too.) Returns `s`
/// unchanged when nothing was stripped.
pub fn strip_annotations(s: &str) -> String {
    let mut buf = String::with_capacity(s.len());
    for r in s.chars() {
        if r < SUBSCRIPT_ZERO || (SUBSCRIPT_ZERO as u32 + 10) <= r as u32 {
            buf.push(r);
        }
    }
    // Go returns the original when the length is unchanged; for our purposes
    // the rebuilt string is equal in that case, so just return it.
    buf
}

/// `ndigits` — the number of decimal digits in `x`, capped at 3.
///
/// Used by `trace` to align the `:` after a position. Ported as a pure helper
/// even though `trace` itself is deferred.
pub fn ndigits(x: u32) -> usize {
    match x {
        0..=9 => 1,
        10..=99 => 2,
        _ => 3,
    }
}

impl Checker {
    /// The package qualifier for `pkg` (`format.go`'s `(*Checker).qualifier`).
    ///
    /// Empty for the package under check, otherwise the package name. See the
    /// free [`qualifier`] for the deferred `pkgPathMap` branch.
    pub fn qualifier(&self, pkg: PackageId) -> String {
        qualifier(self.pkg, pkg, &self.packages)
    }

    /// Build a [`Qualifier`] closure bound to the package under check, and run
    /// `f` with it. Centralises the closure so the lifetime juggling lives in
    /// one place.
    fn with_qualifier<R>(&self, f: impl FnOnce(Qualifier<'_>) -> R) -> R {
        let cur = self.pkg;
        let qf = move |pkg: PackageId, parena: &PackageArena| qualifier(cur, pkg, parena);
        f(Some(&qf))
    }

    /// Render `[T1, T2, …]` for a slice of types, qualified for the package
    /// under check. The `[]Type` case of `sprintf`.
    pub fn type_list_str(&self, ts: &[crate::arena::TypeId]) -> String {
        self.with_qualifier(|qf| {
            let mut out = String::from("[");
            for (i, &t) in ts.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&type_string(
                    &self.types,
                    &self.objects,
                    &self.packages,
                    t,
                    qf,
                ));
            }
            out.push(']');
            out
        })
    }

    /// Render `[op1, op2, …]` for a slice of operands. The `[]*operand` case of
    /// `sprintf`.
    pub fn operand_list_str(&self, xs: &[Operand]) -> String {
        let mut out = String::from("[");
        for (i, x) in xs.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&operand_string(
                &self.types,
                &self.objects,
                &self.packages,
                x,
            ));
        }
        out.push(']');
        out
    }
}
