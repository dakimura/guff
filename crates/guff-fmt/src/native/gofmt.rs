//! Native `gofmt` — PERF_TASKS Task 1b ✅.
//!
//! Parse with `PARSE_COMMENTS | SKIP_OBJECT_RESOLUTION`, then
//! [`guff::format::source`] (`go/format` + `go/printer` + tabwriter).
//! Byte-identical to system `gofmt` on prometheus + GOROOT (`regress/fmt_diff.py`).
//!
//! `opts.simplify` (`gofmt -s`) goes through [`guff::simplify`].

use guff::format::{self, FormatError as AstFormatError};

use crate::native::NativeOptions;
use crate::runner::FormatError;

/// Format `src` like `gofmt`, honouring `opts.simplify` (`-s`).
pub fn format(src: &[u8], opts: &NativeOptions) -> Result<Vec<u8>, FormatError> {
    let formatted = if opts.simplify {
        format::source_simplified(src)
    } else {
        format::source(src)
    };
    match formatted {
        Ok(out) => Ok(out),
        Err(AstFormatError::Parse(e)) => Err(FormatError::Message {
            formatter: "native-gofmt".into(),
            path: if opts.filename.is_empty() {
                "<standard input>".into()
            } else {
                opts.filename.clone()
            },
            message: e.to_string(),
        }),
        Err(AstFormatError::Io(e)) => Err(FormatError::Io {
            formatter: "native-gofmt".into(),
            path: if opts.filename.is_empty() {
                "<standard input>".into()
            } else {
                opts.filename.clone()
            },
            source: e,
        }),
    }
}
