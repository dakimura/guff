//! Port of [`github.com/julz/importas`](https://github.com/julz/importas)
//! (golangci-lint wrapper in `pkg/golinters/importas`).
//!
//! Enforces consistent import aliases from `linters.settings.importas`.
//! SuggestedFix rewrites the import line only; use-site renames via
//! `types.Info.Uses` are DEFERRED (see DEVELOPMENT.md R13).

use std::sync::OnceLock;

use guff::ast::ImportSpec;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use regex::Regex;

use crate::options::{ImportasAlias, ImportasOptions};

fn unquote_import(path: &str) -> &str {
    path.trim_matches('"').trim_matches('`')
}

struct CompiledRule {
    re: Regex,
    alias: String,
}

/// Convert Go `regexp.ReplaceAllString` `$1foo` style to Rust regex `${1}foo`.
///
/// Rust's `regex` treats `$1pkg` as a named group `1pkg` (empty); Go treats it
/// as capture 1 + literal `pkg`.
fn go_style_replacement(alias: &str) -> String {
    let bytes = alias.as_bytes();
    let mut out = String::with_capacity(alias.len() + 4);
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next.is_ascii_digit() {
                out.push('$');
                out.push('{');
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    out.push(bytes[i] as char);
                    i += 1;
                }
                out.push('}');
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

impl CompiledRule {
    fn compile(a: &ImportasAlias) -> Option<Self> {
        let re = Regex::new(&format!("^{}$", a.pkg)).ok()?;
        Some(Self {
            re,
            alias: go_style_replacement(&a.alias),
        })
    }

    fn alias_for(&self, path: &str) -> Option<String> {
        if !self.re.is_match(path) {
            return None;
        }
        Some(self.re.replace(path, self.alias.as_str()).into_owned())
    }
}

fn compile_rules(opts: &ImportasOptions) -> Vec<CompiledRule> {
    opts.alias.iter().filter_map(CompiledRule::compile).collect()
}

fn find_required(rules: &[CompiledRule], path: &str) -> Option<String> {
    for rule in rules {
        if let Some(alias) = rule.alias_for(path) {
            return Some(alias);
        }
    }
    None
}

fn import_pos(imp: &ImportSpec) -> u32 {
    if let Some(n) = &imp.name {
        n.pos().0 as u32
    } else {
        imp.path.value_pos.0 as u32
    }
}

fn import_end(imp: &ImportSpec) -> u32 {
    if imp.end_pos.0 != 0 {
        imp.end_pos.0 as u32
    } else {
        imp.path.end().0 as u32
    }
}

fn import_line_text(path: &str, required: &str) -> String {
    if required.is_empty() {
        format!("\"{path}\"")
    } else {
        format!("{required} \"{path}\"")
    }
}

fn visit_import(
    opts: &ImportasOptions,
    rules: &[CompiledRule],
    imp: &ImportSpec,
    pending: &mut Vec<Diagnostic>,
) {
    let alias = imp.name.as_ref().map(|n| n.name.as_str()).unwrap_or("");

    if !opts.no_unaliased && alias.is_empty() {
        return;
    }

    if alias == "." {
        return;
    }
    if alias.starts_with('_') {
        return;
    }

    let path = unquote_import(&imp.path.value);
    let pos = import_pos(imp);
    let end = import_end(imp);

    if let Some(required) = find_required(rules, path) {
        if required != alias {
            let message = if alias.is_empty() {
                format!(
                    "import {path:?} imported without alias but must be with alias {required:?} according to config"
                )
            } else {
                format!(
                    "import {path:?} imported as {alias:?} but must be {required:?} according to config"
                )
            };
            pending.push(Diagnostic {
                pos,
                end,
                message,
                suggested_fixes: vec![SuggestedFix {
                    message: "Use correct alias".into(),
                    text_edits: vec![TextEdit {
                        pos,
                        end,
                        new_text: import_line_text(path, &required),
                    }],
                }],
                ..Diagnostic::default()
            });
        }
    } else if opts.no_extra_aliases && !alias.is_empty() {
        pending.push(Diagnostic {
            pos,
            end,
            message: format!("import {path:?} has alias {alias:?} which is not part of config"),
            suggested_fixes: vec![SuggestedFix {
                message: "remove alias".into(),
                text_edits: vec![TextEdit {
                    pos,
                    end,
                    new_text: import_line_text(path, ""),
                }],
            }],
            ..Diagnostic::default()
        });
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "importas requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<ImportasOptions>("importas")
        .cloned()
        .unwrap_or_default();
    let rules = compile_rules(&opts);

    // With no alias rules and both flags off, there is nothing to report
    // (golangci logs a hint but does not fail the run).
    if rules.is_empty() && !opts.no_unaliased && !opts.no_extra_aliases {
        return Ok(None);
    }

    let mut pending = Vec::new();
    for file in pass.files() {
        for imp in &file.imports {
            visit_import(&opts, &rules, imp, &mut pending);
        }
    }

    for diag in pending {
        pass.report(diag);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "importas",
        doc: "Enforces consistent import aliases",
        url: "https://github.com/julz/importas",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::ImportasAlias;

    #[test]
    fn go_style_replacement_inserts_braces() {
        assert_eq!(go_style_replacement("$1pkg"), "${1}pkg");
        assert_eq!(go_style_replacement("${1}pkg"), "${1}pkg");
        assert_eq!(go_style_replacement("plain"), "plain");
    }

    #[test]
    fn regex_capture_alias() {
        let rules = compile_rules(&ImportasOptions {
            alias: vec![ImportasAlias {
                pkg: r"github\.com/foo/(\w+)".into(),
                alias: "$1pkg".into(),
            }],
            ..ImportasOptions::default()
        });
        assert_eq!(
            find_required(&rules, "github.com/foo/bar").as_deref(),
            Some("barpkg")
        );
        assert_eq!(find_required(&rules, "fmt"), None);
    }

    #[test]
    fn literal_alias() {
        let rules = compile_rules(&ImportasOptions {
            alias: vec![ImportasAlias {
                pkg: "fmt".into(),
                alias: "fmtpkg".into(),
            }],
            ..ImportasOptions::default()
        });
        assert_eq!(
            find_required(&rules, "fmt").as_deref(),
            Some("fmtpkg")
        );
    }
}
