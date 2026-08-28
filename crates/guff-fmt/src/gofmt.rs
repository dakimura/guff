//! `gofmt` formatter — native Rust port by default (PERF_TASKS Task 1b).
//!
//! Matches golangci-lint `pkg/goformatters/gofmt` settings:
//! - `simplify` → `-s` (still subprocess / unimplemented natively)
//! - `rewrite-rules` → repeated `-r 'pattern -> replacement'` (subprocess)
//!
//! Native path (no subprocess) is used when there are no rewrite rules and
//! either simplify is off, or `GUFF_NATIVE_FMT=0` is not forcing subprocess.
//! Set `GUFF_NATIVE_FMT=0` to force the system `gofmt` binary for all cases.
//! Set `GUFF_NATIVE_FMT=1` to prefer native even when simplify is on (simplify
//! is then ignored until Task 1b `-s` lands).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::native::{self, NativeOptions};
use crate::runner::FormatError;
use crate::Formatter;

pub const NAME: &str = "gofmt";

/// One gofmt rewrite rule (`-r`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RewriteRule {
    pub pattern: String,
    pub replacement: String,
}

/// Options for [`Gofmt`] (`formatters.settings.gofmt`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GofmtOptions {
    /// Pass `-s` (simplify code). **Defaults to true**, see [`Default`].
    pub simplify: bool,
    /// Pass `-r` for each rule.
    pub rewrite_rules: Vec<RewriteRule>,
}

impl Default for GofmtOptions {
    /// golangci-lint's `defaultFormatterSettings`, which seeds
    /// `formatters.settings.gofmt` with `Simplify: true` — so a config that
    /// merely enables the formatter gets `gofmt -s`.
    ///
    /// This is spelled out rather than derived because the derived value is
    /// `false`, and a `false` here is *silent*: guff formats the file, writes
    /// it, exits 0, and simply leaves `[]int{[]int{1}}` alone where
    /// `golangci-lint fmt` writes `[][]int{{1}}`. `GciOptions` and
    /// `GolinesOptions` already spell theirs out for the same reason; gofmt
    /// was the one that did not.
    ///
    /// Not to be confused with the *no formatter configured* fallback, which
    /// is plain `go/format.Source` with no `-s` — see
    /// [`GofmtOptions::plain`].
    fn default() -> Self {
        Self {
            simplify: true,
            rewrite_rules: Vec::new(),
        }
    }
}

impl GofmtOptions {
    /// Plain `gofmt`: no `-s`, no rewrite rules.
    ///
    /// This is what golangci-lint's `MetaFormatter.Format` does when no
    /// formatter is enabled — it calls `go/format.Source` directly rather
    /// than building a gofmt formatter, so the `simplify: true` config
    /// default never reaches it. Every `--fix` run with an empty
    /// `formatters.enable` takes that path (compat/fix's 193 cases are all of
    /// them), which is why the distinction is worth a named constructor.
    pub fn plain() -> Self {
        Self {
            simplify: false,
            rewrite_rules: Vec::new(),
        }
    }
}

/// Formatter: native `go/format` port, with subprocess fallback.
#[derive(Debug, Clone, Default)]
pub struct Gofmt {
    options: GofmtOptions,
    /// Override binary path (tests / non-standard installs / subprocess path).
    binary: Option<String>,
}

impl Gofmt {
    pub fn new(options: GofmtOptions) -> Self {
        Self {
            options,
            binary: None,
        }
    }

    pub fn with_binary(mut self, path: impl Into<String>) -> Self {
        self.binary = Some(path.into());
        self
    }

    /// Whether to use the in-process native implementation.
    fn use_native(&self) -> bool {
        // Explicit force-off.
        if std::env::var_os("GUFF_NATIVE_FMT").is_some_and(|v| v == "0") {
            return false;
        }
        // Rewrite rules have no native port yet.
        if !self.options.rewrite_rules.is_empty() {
            return false;
        }
        // simplify (-s) not ported yet — keep subprocess unless forced on.
        if self.options.simplify {
            return std::env::var_os("GUFF_NATIVE_FMT").is_some_and(|v| v == "1");
        }
        // Default: native (harness-proven on prometheus + GOROOT).
        true
    }

    /// One `gofmt` invocation over `src`, returning its stdout.
    fn run_gofmt(
        &self,
        bin: &str,
        args: &[String],
        filename: &str,
        src: &[u8],
    ) -> Result<Vec<u8>, FormatError> {
        let mut cmd = Command::new(bin);
        cmd.args(args);
        // gofmt reads stdin when no path args are given.
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| FormatError::Io {
            formatter: NAME.to_string(),
            path: filename.to_string(),
            source: e,
        })?;

        {
            let mut stdin = child.stdin.take().ok_or_else(|| FormatError::Message {
                formatter: NAME.to_string(),
                path: filename.to_string(),
                message: "failed to open gofmt stdin".into(),
            })?;
            stdin.write_all(src).map_err(|e| FormatError::Io {
                formatter: NAME.to_string(),
                path: filename.to_string(),
                source: e,
            })?;
        }

        let output = child.wait_with_output().map_err(|e| FormatError::Io {
            formatter: NAME.to_string(),
            path: filename.to_string(),
            source: e,
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(FormatError::Message {
                formatter: NAME.to_string(),
                path: filename.to_string(),
                message: format!("gofmt failed: {}", stderr.trim()),
            });
        }

        Ok(output.stdout)
    }
}

impl Formatter for Gofmt {
    fn name(&self) -> &str {
        NAME
    }

    fn options_fingerprint(&self) -> String {
        let rules: String = self
            .options
            .rewrite_rules
            .iter()
            .map(|r| format!("{}->{}", r.pattern, r.replacement))
            .collect::<Vec<_>>()
            .join(";");
        crate::fingerprint_parts(&[
            ("simplify", if self.options.simplify { "1" } else { "0" }),
            ("rules", &rules),
            ("native", if self.use_native() { "1" } else { "0" }),
        ])
    }

    fn format(&self, filename: &str, src: &[u8]) -> Result<Vec<u8>, FormatError> {
        if self.use_native() {
            let opts = NativeOptions {
                simplify: self.options.simplify,
                filename: filename.to_string(),
                ..Default::default()
            };
            return native::gofmt::format(src, &opts).map_err(|e| match e {
                FormatError::Message {
                    formatter: _,
                    path,
                    message,
                } => FormatError::Message {
                    formatter: NAME.to_string(),
                    path,
                    message,
                },
                other => other,
            });
        }

        // `gofmt -r` is a *single-valued* string flag, so `-r A -r B` silently
        // keeps only B. golangci-lint's fork does not shell out at all: its
        // `rewriteFileContent` loops the rules and applies each to the AST in
        // order. One invocation per rule reproduces that byte for byte —
        // gofmt's print∘parse is identity on its own output, so chaining is the
        // same as chaining the rewrites inside a single parse.
        //
        // `-s` rides on the last invocation because that is where cmd/gofmt
        // puts it: `processFile` rewrites first, then simplifies, then prints.
        let bin = self.binary.as_deref().unwrap_or("gofmt");
        let mut data = src.to_vec();
        if self.options.rewrite_rules.is_empty() {
            let mut args: Vec<String> = Vec::new();
            if self.options.simplify {
                args.push("-s".into());
            }
            data = self.run_gofmt(bin, &args, filename, &data)?;
        } else {
            let last = self.options.rewrite_rules.len() - 1;
            for (i, rule) in self.options.rewrite_rules.iter().enumerate() {
                let mut args: Vec<String> = Vec::new();
                if self.options.simplify && i == last {
                    args.push("-s".into());
                }
                args.push("-r".into());
                args.push(format!("{} -> {}", rule.pattern, rule.replacement));
                data = self.run_gofmt(bin, &args, filename, &data)?;
            }
        }
        Ok(data)
    }

    fn list_unformatted(&self, files: &[&Path]) -> Option<Vec<PathBuf>> {
        // Native path: return None so the runner's per-file `check_file` path
        // formats each file once. A prior `native_list` pre-pass would format
        // the whole tree and then re-format every flagged file in `check_file`.
        if self.use_native() {
            return None;
        }
        // `gofmt -l` takes the same single-valued `-r` as `gofmt` does, so it
        // cannot express more than one rewrite rule — and unlike `format`
        // above there is no chaining trick: `-l` prints file names, not
        // content. Decline the prefilter instead, which costs an optimization
        // and nothing else: the caller then diffs every file through the
        // per-file path, which does apply all the rules.
        if self.options.rewrite_rules.len() > 1 {
            return None;
        }
        // Subprocess prefilter (`GUFF_NATIVE_FMT=0`): system `gofmt -l`.
        let bin = self.binary.as_deref().unwrap_or("gofmt");
        crate::runner::batch_list(files, || {
            let mut c = Command::new(bin);
            c.arg("-l");
            if self.options.simplify {
                c.arg("-s");
            }
            for rule in &self.options.rewrite_rules {
                c.arg("-r")
                    .arg(format!("{} -> {}", rule.pattern, rule.replacement));
            }
            c
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_badly_spaced_source() {
        // `plain()`, so this exercises the native path rather than the
        // subprocess `default()` now routes to.
        let fmt = Gofmt::new(GofmtOptions::plain());
        let src = b"package main\nfunc main(  ) {\nx:=1\n}\n";
        let out = fmt.format("main.go", src).expect("gofmt");
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("func main() {"));
        assert!(s.contains("x := 1"));
    }

    /// golangci-lint's `defaultFormatterSettings` sets `Simplify: true`, so a
    /// config that only says `enable: [gofmt]` means `gofmt -s`. A derived
    /// `false` here is silent — guff formats, writes, exits 0, and leaves the
    /// literal alone.
    #[test]
    fn config_default_simplifies_and_plain_does_not() {
        assert!(
            GofmtOptions::default().simplify,
            "formatters.settings.gofmt.simplify defaults to true upstream"
        );
        assert!(
            !GofmtOptions::plain().simplify,
            "the no-formatter-configured fallback is go/format.Source, not gofmt -s"
        );
        assert!(GofmtOptions::default().rewrite_rules.is_empty());
    }

    #[test]
    fn simplify_collapses_slice() {
        // -s still uses the system binary (native simplify not ported).
        let fmt = Gofmt::new(GofmtOptions::default());
        // gofmt -s rewrites s[a:len(s)] → s[a:]
        let src = b"package p\n\nfunc f(s []int) []int {\n\treturn s[1:len(s)]\n}\n";
        let out = fmt.format("p.go", src).expect("gofmt -s");
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("s[1:]"), "expected simplify rewrite, got:\n{s}");
    }

    #[test]
    fn plain_does_not_simplify() {
        let fmt = Gofmt::new(GofmtOptions::plain());
        let src = b"package p\n\nfunc f(s []int) []int {\n\treturn s[1:len(s)]\n}\n";
        let out = fmt.format("p.go", src).expect("gofmt");
        let s = String::from_utf8(out).unwrap();
        assert!(
            s.contains("s[1:len(s)]"),
            "plain gofmt must leave the slice alone, got:\n{s}"
        );
    }

    /// `gofmt -r` is a single-valued string flag, so `-r A -r B` keeps only B.
    /// Upstream's fork loops the rules and applies each to the AST, so all of
    /// them must land — one invocation per rule.
    #[test]
    fn every_rewrite_rule_is_applied() {
        let fmt = Gofmt::new(GofmtOptions {
            simplify: false,
            rewrite_rules: vec![
                RewriteRule {
                    pattern: "interface{}".into(),
                    replacement: "any".into(),
                },
                RewriteRule {
                    pattern: "a[b:len(a)]".into(),
                    replacement: "a[b:]".into(),
                },
            ],
        });
        let src = b"package p\n\nfunc f(v interface{}, s []int) []int {\n\t_ = v\n\treturn s[1:len(s)]\n}\n";
        let out = fmt.format("p.go", src).expect("gofmt -r");
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("v any"), "first rule was dropped, got:\n{s}");
        assert!(s.contains("s[1:]"), "second rule was dropped, got:\n{s}");
    }

    /// `gofmt -l -r A -r B` would silently list files against B alone, and a
    /// prefilter that under-reports drops findings. There is no chaining fix
    /// for `-l`, so the prefilter declines and the per-file path answers.
    #[test]
    fn the_prefilter_declines_more_than_one_rewrite_rule() {
        let rule = |p: &str| RewriteRule {
            pattern: p.into(),
            replacement: "any".into(),
        };
        let one = Gofmt::new(GofmtOptions {
            simplify: false,
            rewrite_rules: vec![rule("interface{}")],
        });
        assert!(
            one.list_unformatted(&[]).is_some(),
            "one rule is expressible as a single -r"
        );
        let two = Gofmt::new(GofmtOptions {
            simplify: false,
            rewrite_rules: vec![rule("interface{}"), rule("any")],
        });
        assert!(two.list_unformatted(&[]).is_none());
    }

    #[test]
    fn native_is_the_default_when_simplify_is_off() {
        // Ensure GUFF_NATIVE_FMT=0 is not set for this assertion.
        std::env::remove_var("GUFF_NATIVE_FMT");
        assert!(Gofmt::new(GofmtOptions::plain()).use_native());
        // With simplify on there is no native path yet, so the subprocess is
        // used — the config default therefore does *not* run native today.
        assert!(!Gofmt::new(GofmtOptions::default()).use_native());
    }
}
