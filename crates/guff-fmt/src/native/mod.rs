//! Native (in-process) Go formatters — Task 1 of `docs/PERF_TASKS.md`.
//!
//! Subprocess formatters remain available via `GUFF_NATIVE_FMT=0`. Flip each
//! formatter's default only after [`regress/fmt_diff.py`] is byte-identical
//! on prometheus (and ideally GOROOT).
//!
//! Sub-tasks:
//! - **1b** [`gofmt`] — `go/printer` + format ✅ (default ON)
//! - **1c** [`gofumpt`] — gofmt + gofumpt rules ✅ (default ON; prometheus `--extra`)
//! - **1d** [`goimports`] — format-only group/sort (harness green; Formatter stays subprocess)
//! - **1e** [`gci`] — import section sorting ✅ (default ON)

use crate::runner::FormatError;

pub mod gci;
pub mod gofmt;
pub mod gofumpt;
pub mod goimports;

/// Which native formatter the CLI / harness is targeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeKind {
    Gofmt,
    Gofumpt,
    Goimports,
    Gci,
}

impl NativeKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "gofmt" => Some(Self::Gofmt),
            "gofumpt" => Some(Self::Gofumpt),
            "goimports" => Some(Self::Goimports),
            "gci" => Some(Self::Gci),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gofmt => "gofmt",
            Self::Gofumpt => "gofumpt",
            Self::Goimports => "goimports",
            Self::Gci => "gci",
        }
    }
}

/// Process-exit code used by `guff-fmt-native` when a formatter is not
/// implemented yet. The diff harness treats this as "skip / not ready",
/// distinct from a byte mismatch (exit 1) or I/O failure.
pub const EXIT_NOT_IMPLEMENTED: i32 = 2;

/// Error returned while a native formatter is still a stub.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // kept for harness exit-2 path if a formatter is stubbed again
pub struct NotImplemented {
    pub kind: NativeKind,
}

impl std::fmt::Display for NotImplemented {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "native {} is not implemented yet (PERF_TASKS Task 1)",
            self.kind.as_str()
        )
    }
}

impl std::error::Error for NotImplemented {}

/// Options shared by the native CLI / harness invocation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeOptions {
    /// `gofmt -s` / simplify.
    pub simplify: bool,
    /// `gofumpt -extra`.
    pub extra_rules: bool,
    /// `gofumpt -lang` (e.g. `go1.22`).
    pub lang: Option<String>,
    /// `gofumpt -modpath`.
    pub module_path: Option<String>,
    /// `goimports -local` (comma-joined prefixes).
    pub local_prefixes: Vec<String>,
    /// `gci -s` section list.
    pub gci_sections: Vec<String>,
    /// `gci --custom-order`.
    pub gci_custom_order: bool,
    /// `gci --no-lex-order`.
    pub gci_no_lex_order: bool,
    /// Real path hint (goimports `-srcdir`, gci localmodule).
    pub filename: String,
}

/// Format `src` with the requested native formatter.
///
/// Returns [`Err`] with a [`FormatError::Message`] wrapping
/// [`NotImplemented`] until the corresponding Task 1 sub-task lands.
pub fn format(kind: NativeKind, src: &[u8], opts: &NativeOptions) -> Result<Vec<u8>, FormatError> {
    match kind {
        NativeKind::Gofmt => gofmt::format(src, opts),
        NativeKind::Gofumpt => gofumpt::format(src, opts),
        NativeKind::Goimports => goimports::format(src, opts),
        NativeKind::Gci => gci::format(src, opts),
    }
}

#[allow(dead_code)]
pub(crate) fn not_implemented(kind: NativeKind, filename: &str) -> Result<Vec<u8>, FormatError> {
    Err(FormatError::Message {
        formatter: format!("native-{}", kind.as_str()),
        path: if filename.is_empty() {
            "<standard input>".into()
        } else {
            filename.to_string()
        },
        message: NotImplemented { kind }.to_string(),
    })
}

/// True when `err` is the stub [`NotImplemented`] sentinel (vs a real
/// format/parse failure). Used by the CLI to choose exit code 2.
pub fn is_not_implemented(err: &FormatError) -> bool {
    match err {
        FormatError::Message { message, .. } => message.contains("not implemented yet"),
        _ => false,
    }
}
