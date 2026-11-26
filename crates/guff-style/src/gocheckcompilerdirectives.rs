//! Port of [`github.com/leighmcculloch/gocheckcompilerdirectives`](https://github.com/leighmcculloch/gocheckcompilerdirectives)
//! (`4d63.com/gocheckcompilerdirectives`).
//!
//! Reports invalid `//go:` compiler directives:
//! 1. Space after `//` (e.g. `// go:embed`) — silently ignored by the compiler.
//! 2. Unknown directive names (e.g. `//go:genrate`) — also silently ignored.
//!
//! Re-parses with `PARSE_COMMENTS` because load uses `Mode::NONE` and drops
//! `file.Comments`. Upstream only extracts a directive name when a space
//! follows it (args / trailing text); bare `//go:name` with no trailing space
//! is skipped — match that behavior for parity.

use std::fs;
use std::sync::{Arc, OnceLock};

use guff::ast::File;
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::FileSet;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

/// Known `//go:` directive names from upstream (go1.24 compiler directives).
const KNOWN: &[&str] = &[
    "build",
    "cgo_dynamic_linker",
    "cgo_export_dynamic",
    "cgo_export_static",
    "cgo_import_dynamic",
    "cgo_import_static",
    "cgo_ldflag",
    "cgo_unsafe_args",
    "debug",
    "embed",
    "fix",
    "generate",
    "linkname",
    "nocheckptr",
    "noescape",
    "noinline",
    "nointerface",
    "norace",
    "nosplit",
    "notinheap",
    "nowritebarrier",
    "nowritebarrierrec",
    "systemstack",
    "uintptrescapes",
    "uintptrkeepalive",
    "wasmimport",
    "wasmexport",
    "yeswritebarrierrec",
];

fn is_known(directive: &str) -> bool {
    KNOWN.iter().any(|&k| k == directive)
}

fn reparse(path: &std::path::Path) -> Option<(Arc<FileSet>, File)> {
    let src = fs::read(path).ok()?;
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, &src, PARSE_COMMENTS).ok()?;
    Some((fset, file))
}

/// Upstream `run` body for a single `//…` comment text.
fn check_comment_text(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    if !text.starts_with("//") {
        return out;
    }
    let mut start = 2usize;
    let mut spaces = 0usize;
    for c in text[start..].chars() {
        if c == ' ' {
            spaces += 1;
            continue;
        }
        break;
    }
    start += spaces;
    if !text[start..].starts_with("go:") {
        return out;
    }
    start += 3;
    let Some(end) = text[start..].find(' ') else {
        // Upstream: no trailing space → skip (directive name not extracted).
        return out;
    };
    let directive = &text[start..start + end];
    if directive.is_empty() {
        return out;
    }
    let prefix = &text[..start + end];
    if spaces > 0 {
        out.push(format!("compiler directive contains space: {prefix}"));
    }
    if !is_known(directive) {
        out.push(format!("compiler directive unrecognized: {prefix}"));
    }
    out
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "gocheckcompilerdirectives requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    let paths = pass.pkg().compiled_go_files.clone();
    let n = pass.files().len();

    for i in 0..n {
        let Some(path) = paths.get(i) else {
            continue;
        };
        let Some((_fset, parsed)) = reparse(path) else {
            continue;
        };
        for cg in &parsed.comments {
            for c in &cg.list {
                for message in check_comment_text(&c.text) {
                    pending.push((c.slash.0 as u32, message));
                }
            }
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "gocheckcompilerdirectives",
        doc: "Checks that go compiler directive comments (//go:) are valid.",
        url: "https://github.com/leighmcculloch/gocheckcompilerdirectives",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_space_and_unknown() {
        let msgs = check_comment_text("// go:embed file.txt");
        assert_eq!(
            msgs,
            vec!["compiler directive contains space: // go:embed".to_string()]
        );

        let msgs = check_comment_text("//go:genrate echo hi");
        assert_eq!(
            msgs,
            vec!["compiler directive unrecognized: //go:genrate".to_string()]
        );
    }

    #[test]
    fn allows_known_and_skips_bare() {
        assert!(check_comment_text("//go:generate echo hi").is_empty());
        assert!(check_comment_text("//go:noinline").is_empty());
        assert!(check_comment_text("//go:genrate").is_empty()); // bare → skipped
        assert!(check_comment_text("// regular comment").is_empty());
        assert!(check_comment_text("/* go:embed */").is_empty());
    }
}
