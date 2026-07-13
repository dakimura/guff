//! Port of `version.go` — Go language-version handling.
//!
//! A [`GoVersion`] is a Go language version of the form `"go1.N"` (release
//! numbers are stripped: `"go1.20.1"` normalises to `"go1.20"`). The empty
//! string is the invalid version. The named constants ([`go1_18`], …) mark the
//! versions that introduced language changes, and [`Checker::allow_version`]
//! reports whether the effective version permits a given feature.
//!
//! Go keeps `goVersion` as a bare string type; we wrap it in a newtype so the
//! API (`is_valid` / `cmp`) reads the same. Version algebra (`Lang` / `Compare`)
//! is delegated to the already-ported `guff-version` crate.

use guff_goversion::VERSION;
use guff_types_errors::Code;
use guff_version::{compare as version_compare, lang as version_lang};

use crate::check::Checker;

/// A Go language version string of the form `"go1.N"`. The empty string is the
/// invalid version. Equivalent to Go's `goVersion`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct GoVersion(String);

/// Returns `v` as a [`GoVersion`] (e.g. `"go1.20.1"` becomes `"go1.20"`). If
/// `v` is not a valid Go version, the result is the empty (invalid) version.
///
/// Equivalent to Go's `asGoVersion`.
pub fn as_go_version(v: &str) -> GoVersion {
    GoVersion(version_lang(v))
}

impl GoVersion {
    /// Reports whether this is a valid Go version. Equivalent to
    /// `goVersion.isValid`.
    pub fn is_valid(&self) -> bool {
        !self.0.is_empty()
    }

    /// Returns -1, 0, or +1 depending on whether `self < other`, `self == other`,
    /// or `self > other`, interpreted as Go versions. Equivalent to
    /// `goVersion.cmp`.
    pub fn cmp(&self, other: &GoVersion) -> i32 {
        version_compare(&self.0, &other.0)
    }

    /// The underlying `"go1.N"` string (empty when invalid).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for GoVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// Go versions that introduced language changes. Go declares these as
// package-level `var`s initialised by `asGoVersion`; since that isn't `const`
// in Rust, we expose them as functions with the same names.
macro_rules! go_version_const {
    ($name:ident, $lit:literal, $doc:literal) => {
        #[doc = $doc]
        pub fn $name() -> GoVersion {
            as_go_version($lit)
        }
    };
}

go_version_const!(
    go1_9,
    "go1.9",
    "Go 1.9 — 3-index slices of arrays/pointers."
);
go_version_const!(
    go1_13,
    "go1.13",
    "Go 1.13 — signed shift counts, binary/octal literals."
);
go_version_const!(
    go1_14,
    "go1.14",
    "Go 1.14 — overlapping embedded interface method sets."
);
go_version_const!(
    go1_17,
    "go1.17",
    "Go 1.17 — slice-to-array-pointer conversions, unsafe.Add/Slice."
);
go_version_const!(go1_18, "go1.18", "Go 1.18 — generics.");
go_version_const!(
    go1_20,
    "go1.20",
    "Go 1.20 — slice-to-array conversions, comparable satisfied by all comparable types."
);
go_version_const!(go1_21, "go1.21", "Go 1.21 — min/max/clear builtins.");
go_version_const!(
    go1_22,
    "go1.22",
    "Go 1.22 — per-iteration loop variables, range-over-int."
);
go_version_const!(go1_23, "go1.23", "Go 1.23 — range-over-func iterators.");
go_version_const!(go1_26, "go1.26", "Go 1.26 — new(expr) value form.");

/// The current (deployed) Go version — `go1.<goversion::VERSION>`. Equivalent
/// to Go's `go_current`.
pub fn go_current() -> GoVersion {
    as_go_version(&format!("go1.{}", VERSION))
}

impl Checker {
    /// Reports whether the current effective Go version (which may vary from
    /// one file to another) is allowed to use the feature version `want`.
    ///
    /// Equivalent to `Checker.allowVersion`. An invalid effective version
    /// (the empty string — version checks disabled) allows every feature.
    pub fn allow_version(&self, want: &GoVersion) -> bool {
        let v = as_go_version(&self.env.version);
        !v.is_valid() || v.cmp(want) >= 0
    }

    /// Like [`allow_version`](Self::allow_version) but also reports a version
    /// error at `pos` when the feature is not allowed. `msg` is the (already
    /// formatted) description of the feature. Returns whether the feature is
    /// allowed.
    ///
    /// Equivalent to `Checker.verifyVersionf` (errors collected via
    /// `versionErrorf`).
    pub fn verify_versionf(&mut self, pos: u32, v: &GoVersion, msg: impl Into<String>) -> bool {
        if !self.allow_version(v) {
            self.version_errorf(pos, v, msg);
            return false;
        }
        true
    }

    /// Reports a `UnsupportedFeature` error: "<msg> requires <v> or later".
    /// Equivalent to `Checker.versionErrorf` (`errors.go`).
    pub fn version_errorf(&mut self, pos: u32, v: &GoVersion, msg: impl Into<String>) {
        let full = format!("{} requires {} or later", msg.into(), v);
        self.error(pos, Code::UnsupportedFeature, full);
    }
}
