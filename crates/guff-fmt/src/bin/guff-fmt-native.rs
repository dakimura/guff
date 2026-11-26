//! `guff-fmt-native` — stdin→stdout candidate for `regress/fmt_diff.py`.
//!
//! Exit codes:
//! - `0` — formatted bytes on stdout (byte-identical to the reference tool
//!   once the corresponding Task 1 sub-task is done)
//! - `1` — format / parse / I/O error (message on stderr)
//! - `2` — native formatter not implemented yet ([`EXIT_NOT_IMPLEMENTED`])

use std::env;
use std::io::{self, Read, Write};
use std::process::ExitCode;

use guff_fmt::native::{
    format, is_not_implemented, NativeKind, NativeOptions, EXIT_NOT_IMPLEMENTED,
};

fn usage() -> ! {
    eprintln!(
        "usage: guff-fmt-native <gofmt|gofumpt|goimports|gci> [options...]
reads Go source from stdin, writes formatted source to stdout

options:
  --simplify                 gofmt -s
  --extra                    gofumpt -extra
  --lang <ver>               gofumpt -lang
  --modpath <path>           gofumpt -modpath
  --local <a,b,...>          goimports -local
  --section <name>           gci -s (repeatable)
  --custom-order             gci --custom-order
  --no-lex-order             gci --no-lex-order
  --filename <path>          path hint (goimports -srcdir / gci localmodule)

exit 2 = native formatter not implemented yet (PERF_TASKS Task 1)"
    );
    std::process::exit(2);
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(kind_s) = args.next() else {
        usage();
    };
    let Some(kind) = NativeKind::parse(&kind_s) else {
        eprintln!("unknown formatter {kind_s:?}");
        usage();
    };

    let mut opts = NativeOptions::default();
    let mut rest = args.peekable();
    while let Some(a) = rest.next() {
        match a.as_str() {
            "--simplify" => opts.simplify = true,
            "--extra" => opts.extra_rules = true,
            "--custom-order" => opts.gci_custom_order = true,
            "--no-lex-order" => opts.gci_no_lex_order = true,
            "--lang" => {
                opts.lang = Some(rest.next().unwrap_or_else(|| usage()));
            }
            "--modpath" => {
                opts.module_path = Some(rest.next().unwrap_or_else(|| usage()));
            }
            "--local" => {
                let raw = rest.next().unwrap_or_else(|| usage());
                opts.local_prefixes = raw
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            "--section" => {
                opts.gci_sections
                    .push(rest.next().unwrap_or_else(|| usage()));
            }
            "--filename" => {
                opts.filename = rest.next().unwrap_or_else(|| usage());
            }
            "-h" | "--help" => usage(),
            other => {
                eprintln!("unknown option {other}");
                usage();
            }
        }
    }

    let mut src = Vec::new();
    if let Err(e) = io::stdin().read_to_end(&mut src) {
        eprintln!("stdin: {e}");
        return ExitCode::from(1);
    }

    match format(kind, &src, &opts) {
        Ok(out) => {
            if let Err(e) = io::stdout().write_all(&out) {
                eprintln!("stdout: {e}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        Err(e) if is_not_implemented(&e) => {
            eprintln!("{e}");
            ExitCode::from(EXIT_NOT_IMPLEMENTED as u8)
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}
