//! Native `gofumpt` — PERF_TASKS Task 1c.
//!
//! Pipeline: parse → simplify → gofumpt AST/line rules → `guff::format` print.
//! Extra rules (`-extra`) enable GroupParams + ClotheReturns.

mod fumpter;
mod simplify;

use std::io::Write;
use std::sync::Arc;

use guff::format::FormatError as AstFormatError;
use guff::import::sort_imports;
use guff::parser::{Mode as ParserMode, PARSE_COMMENTS, SKIP_OBJECT_RESOLUTION};
use guff::parser_interface;
use guff::printer::{
    Config, Mode as PrinterMode, PrintNode, NORMALIZE_NUMBERS, TAB_INDENT, USE_SPACES,
};
use guff::FileSet;

use crate::native::NativeOptions;
use crate::runner::FormatError;

use fumpter::{apply_file, Extra, Options as FumptOptions};

const PARSER_MODE: ParserMode = ParserMode(PARSE_COMMENTS.0 | SKIP_OBJECT_RESOLUTION.0);
const TAB_WIDTH: i32 = 8;
const PRINTER_MODE: PrinterMode = USE_SPACES | TAB_INDENT | NORMALIZE_NUMBERS;

/// Format `src` like `gofumpt` (`-extra` / `-lang` / `-modpath` from `opts`).
pub fn format(src: &[u8], opts: &NativeOptions) -> Result<Vec<u8>, FormatError> {
    match format_inner(src, opts) {
        Ok(out) => Ok(out),
        Err(AstFormatError::Parse(e)) => Err(FormatError::Message {
            formatter: "native-gofumpt".into(),
            path: path_label(opts),
            message: e.to_string(),
        }),
        Err(AstFormatError::Io(e)) => Err(FormatError::Io {
            formatter: "native-gofumpt".into(),
            path: path_label(opts),
            source: e,
        }),
    }
}

fn path_label(opts: &NativeOptions) -> String {
    if opts.filename.is_empty() {
        "<standard input>".into()
    } else {
        opts.filename.clone()
    }
}

fn format_inner(src: &[u8], opts: &NativeOptions) -> Result<Vec<u8>, AstFormatError> {
    let fset = Arc::new(FileSet::new());
    // Mirror gofumpt: dummy file so NoPos+1 is never a real file offset.
    let _ = fset.add_file("gofumpt_base.go", 1, 10);

    let mut file = parser_interface::parse_file(&fset, "", Some(src), PARSER_MODE)?;
    sort_imports(&fset, &mut file);

    let fumpt_opts = FumptOptions {
        lang_version: opts.lang.clone().unwrap_or_default(),
        module_path: opts.module_path.clone().unwrap_or_default(),
        extra: if opts.extra_rules {
            Extra {
                group_params: true,
                clothe_returns: true,
            }
        } else {
            Extra::default()
        },
    };
    apply_file(&fset, &mut file, fumpt_opts);
    // Ensure imports are sorted so we never take format.Node's re-parse path.
    // (gofumpt may emit `if T{}.M()` without parens; re-parsing that fails.)
    sort_imports(&fset, &mut file);

    let cfg = Config {
        mode: PRINTER_MODE,
        tabwidth: TAB_WIDTH,
        indent: 0,
    };
    let mut buf = Vec::new();
    cfg.fprint(&mut buf, &fset, PrintNode::File(&file))?;
    if !buf.ends_with(b"\n") {
        buf.push(b'\n');
    }
    Ok(buf)
}

/// Byte-counter Write used by print-length heuristics.
pub(crate) struct ByteCounter(pub(crate) usize);

impl Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
