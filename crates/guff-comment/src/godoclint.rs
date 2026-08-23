//! Port of [`github.com/godoc-lint/godoc-lint`](https://github.com/godoc-lint/godoc-lint)
//! (golangci-lint wrapper in `pkg/golinters/godoclint`).
//!
//! Implements the **basic** default rule set:
//! `pkg-doc`, `single-pkg-doc`, `start-with-name`, `deprecated`.
//!
//! Comments are re-parsed with [`PARSE_COMMENTS`] because production package
//! load uses `Mode::NONE`, which drops lead comments after the package clause.
//!
//! Settings: `linters.settings.godoclint` (`default` / `enable` / `disable`).
//!
//! DEFERRED: `require-doc` / `require-pkg-doc` / `max-len` / `no-unused-link` /
//! `require-stdlib-doclink`; per-rule `options.*`; `//godoclint:disable`
//! directives; full `go/doc/comment` paragraph parsing (deprecated marker
//! detection uses blank-line paragraphs as an approximation).

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use guff::ast::{CommentGroup, Decl, Expr, Spec};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use regex::Regex;

use crate::options::GodoclintOptions;
use crate::util::{reparse_with_comments, reparsed_pos};

fn is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

fn is_test_path(path: &Path) -> bool {
    path.to_string_lossy().ends_with("_test.go")
}

fn doc_text(doc: &CommentGroup) -> Option<String> {
    let text = doc.text();
    let trimmed = text.trim_end_matches('\n');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Approximate `shared.HasDeprecatedParagraph`: any blank-line-separated
/// paragraph whose first line starts with `Deprecated: `.
fn has_deprecated_paragraph(text: &str) -> bool {
    for para in text.split("\n\n") {
        let first = para.lines().next().unwrap_or("").trim_start();
        if first.starts_with("Deprecated: ") {
            return true;
        }
    }
    false
}

fn check_pkg_doc_prefix(text: &str, package_name: &str) -> Option<String> {
    let expected = format!("Package {package_name}");
    if !text.starts_with(&expected) {
        return Some(expected);
    }
    let rest = &text[expected.len()..];
    if rest.is_empty()
        || rest.starts_with(' ')
        || rest.starts_with('\t')
        || rest.starts_with('\r')
        || rest.starts_with('\n')
    {
        None
    } else {
        Some(expected)
    }
}

fn start_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:(A|a|AN|An|an|THE|The|the) )?(?P<symbol_name>.+?)\b").unwrap()
    })
}

fn match_symbol_name(text: &str, symbol: &str) -> bool {
    let head_line = text.split('\n').next().unwrap_or(text);
    let head_line = head_line.strip_prefix('\r').unwrap_or(head_line);
    let head = head_line
        .split([' ', '\t'])
        .next()
        .unwrap_or(head_line);
    if head == symbol {
        return true;
    }
    if let Some(caps) = start_pattern().captures(text) {
        if let Some(m) = caps.name("symbol_name") {
            return m.as_str() == symbol;
        }
    }
    false
}

fn probable_deprecation_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^deprecated:.?").unwrap())
}

const CORRECT_DEPRECATION_MARKER: &str = "Deprecated: ";

fn check_deprecations(text: &str) -> bool {
    for para in text.split("\n\n") {
        let first = para.lines().next().unwrap_or("").trim_start();
        if first.is_empty() {
            continue;
        }
        if let Some(m) = probable_deprecation_re().find(first) {
            let matched = m.as_str();
            if matched != CORRECT_DEPRECATION_MARKER {
                return true;
            }
        }
    }
    false
}

fn receiver_base_type_name(ty: &Expr) -> Option<&str> {
    let mut t = ty;
    if let Expr::StarExpr(star) = t {
        t = &star.x;
    }
    match t {
        Expr::Ident(id) => Some(id.name.as_str()),
        Expr::IndexExpr(idx) => match &*idx.x {
            Expr::Ident(id) => Some(id.name.as_str()),
            _ => None,
        },
        Expr::IndexListExpr(idx) => match &*idx.x {
            Expr::Ident(id) => Some(id.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

struct SymbolDoc<'a> {
    name: &'a str,
    doc: &'a CommentGroup,
    /// Parent GenDecl doc (for grouped const/var/type).
    parent_doc: Option<&'a CommentGroup>,
    multi_name: bool,
    is_method: bool,
    method_recv_base: Option<&'a str>,
}

fn collect_symbol_docs(file: &guff::ast::File) -> Vec<SymbolDoc<'_>> {
    let mut out = Vec::new();
    for decl in &file.decls {
        match decl {
            Decl::FuncDecl(fd) => {
                let Some(doc) = &fd.doc else {
                    continue;
                };
                let (is_method, method_recv_base) = if let Some(recv) = &fd.recv {
                    let base = recv
                        .list
                        .first()
                        .and_then(|f| f.ty.as_ref())
                        .and_then(receiver_base_type_name);
                    (true, base)
                } else {
                    (false, None)
                };
                out.push(SymbolDoc {
                    name: fd.name.name.as_str(),
                    doc,
                    parent_doc: None,
                    multi_name: false,
                    is_method,
                    method_recv_base,
                });
            }
            Decl::GenDecl(gen) => {
                let parent_doc = gen.doc.as_ref();
                for spec in &gen.specs {
                    match spec {
                        Spec::TypeSpec(ts) => {
                            // Upstream: only the per-spec doc is a symbol Doc;
                            // GenDecl parent docs live in ParentDoc and are
                            // skipped by start-with-name when Doc is empty.
                            let Some(doc) = ts.doc.as_ref() else {
                                continue;
                            };
                            out.push(SymbolDoc {
                                name: ts.name.name.as_str(),
                                doc,
                                parent_doc,
                                multi_name: false,
                                is_method: false,
                                method_recv_base: None,
                            });
                        }
                        Spec::ValueSpec(vs) => {
                            let multi_name = vs.names.len() > 1;
                            let Some(doc) = vs.doc.as_ref() else {
                                continue;
                            };
                            for name in &vs.names {
                                out.push(SymbolDoc {
                                    name: name.name.as_str(),
                                    doc,
                                    parent_doc,
                                    multi_name,
                                    is_method: false,
                                    method_recv_base: None,
                                });
                            }
                        }
                        Spec::ImportSpec(_) => {}
                    }
                }
            }
            Decl::BadDecl(_) => {}
        }
    }
    out
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "godoclint requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<GodoclintOptions>("godoclint")
        .cloned()
        .unwrap_or_default();
    let rules = options.effective_rules();

    let check_pkg_doc = rules.contains("pkg-doc");
    let check_single = rules.contains("single-pkg-doc");
    let check_start = rules.contains("start-with-name");
    let check_deprecated = rules.contains("deprecated");

    if !check_pkg_doc && !check_single && !check_start && !check_deprecated {
        return Ok(None);
    }

    let paths: Vec<_> = pass.pkg().compiled_go_files.clone();
    let fset = pass.fset().clone();
    let n = pass.files().len();

    let mut pending: Vec<(u32, String)> = Vec::new();
    // package name → list of (pos, has_nonempty_doc) for single-pkg-doc
    let mut pkg_docs: HashMap<String, Vec<(u32, bool)>> = HashMap::new();

    for i in 0..n {
        let file = &pass.files()[i];
        let Some(path) = paths.get(i) else {
            continue;
        };
        let skip_tests = is_test_path(path);
        let Some((re_fset, parsed)) = reparse_with_comments(path, pass.pkg().source_bytes(i))
        else {
            continue;
        };
        let pkg_name = parsed.name.name.as_str();

        // --- package docs ---
        if !skip_tests {
            if let Some(pkg_doc) = &parsed.doc {
                if let Some(text) = doc_text(pkg_doc) {
                    if let Some(pos) = reparsed_pos(&fset, file.pos(), &re_fset, pkg_doc.pos()) {
                        if check_single {
                            pkg_docs
                                .entry(pkg_name.to_string())
                                .or_default()
                                .push((pos, true));
                        }
                        if check_pkg_doc
                            && pkg_name != "main"
                            && pkg_name != "main_test"
                            && !has_deprecated_paragraph(&text)
                        {
                            if let Some(expected) = check_pkg_doc_prefix(&text, pkg_name) {
                                pending.push((
                                    pos,
                                    format!("package godoc should start with \"{expected} \""),
                                ));
                            }
                        }
                        if check_deprecated && check_deprecations(&text) {
                            pending.push((
                                pos,
                                format!(
                                    "deprecation note should be formatted as \"{CORRECT_DEPRECATION_MARKER}\""
                                ),
                            ));
                        }
                    }
                } else if check_single {
                    // empty package doc — not counted for single-pkg-doc
                }
            }
        }

        // --- symbol docs ---
        for sym in collect_symbol_docs(&parsed) {
            let Some(text) = doc_text(sym.doc) else {
                continue;
            };
            let Some(pos) = reparsed_pos(&fset, file.pos(), &re_fset, sym.doc.pos()) else {
                continue;
            };

            let mut exported = is_exported(sym.name);
            if sym.is_method {
                if let Some(base) = sym.method_recv_base {
                    exported = exported && is_exported(base);
                }
            }

            if check_start && !skip_tests && exported && sym.name != "_" && !sym.multi_name {
                if !has_deprecated_paragraph(&text) && !match_symbol_name(&text, sym.name) {
                    pending.push((
                        pos,
                        format!("godoc should start with symbol name (\"{}\")", sym.name),
                    ));
                }
            }

            if check_deprecated && exported {
                // Upstream also checks parent docs for grouped decls.
                let bad = check_deprecations(&text)
                    || sym
                        .parent_doc
                        .filter(|p| !std::ptr::eq(*p as *const _, sym.doc as *const _))
                        .and_then(doc_text)
                        .is_some_and(|pt| check_deprecations(&pt));
                if bad {
                    pending.push((
                        pos,
                        format!(
                            "deprecation note should be formatted as \"{CORRECT_DEPRECATION_MARKER}\""
                        ),
                    ));
                }
            }
        }
    }

    if check_single {
        for (pkg, entries) in pkg_docs {
            if entries.len() < 2 {
                continue;
            }
            for (pos, _) in entries {
                pending.push((pos, format!("package has more than one godoc (\"{pkg}\")")));
            }
        }
    }

    pending.sort_by_key(|(pos, _)| *pos);
    // Dedup identical (pos, msg) from parent+child double checks.
    pending.dedup();

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "godoclint",
        doc: "Checks Golang's documentation practice (godoc)",
        url: "https://github.com/godoc-lint/godoc-lint",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::GodoclintOptions;
    use std::collections::HashSet;

    #[test]
    fn default_enables_basic_rules() {
        let rules = GodoclintOptions::default().effective_rules();
        for r in ["pkg-doc", "single-pkg-doc", "start-with-name", "deprecated"] {
            assert!(rules.contains(r), "missing {r}");
        }
        assert!(!rules.contains("require-doc"));
        assert!(!rules.contains("max-len"));
    }

    #[test]
    fn all_default_lists_known_rules() {
        let opts = GodoclintOptions {
            default: "all".into(),
            enable: Vec::new(),
            disable: Vec::new(),
        };
        let rules = opts.effective_rules();
        assert!(rules.contains("pkg-doc"));
        assert!(rules.contains("require-doc"));
        assert!(rules.contains("max-len"));
        assert!(rules.contains("require-stdlib-doclink"));
    }

    #[test]
    fn none_plus_enable() {
        let opts = GodoclintOptions {
            default: "none".into(),
            enable: vec!["pkg-doc".into(), "deprecated".into()],
            disable: Vec::new(),
        };
        let rules = opts.effective_rules();
        assert_eq!(
            rules,
            HashSet::from(["pkg-doc".into(), "deprecated".into()])
        );
    }

    #[test]
    fn disable_removes_from_basic() {
        let opts = GodoclintOptions {
            default: "basic".into(),
            enable: Vec::new(),
            disable: vec!["deprecated".into()],
        };
        let rules = opts.effective_rules();
        assert!(!rules.contains("deprecated"));
        assert!(rules.contains("pkg-doc"));
    }

    #[test]
    fn pkg_doc_prefix() {
        assert!(check_pkg_doc_prefix("Package foo does X.", "foo").is_none());
        assert!(check_pkg_doc_prefix("Package foo", "foo").is_none());
        assert_eq!(
            check_pkg_doc_prefix("This is foo.", "foo"),
            Some("Package foo".into())
        );
        assert_eq!(
            check_pkg_doc_prefix("Package foobar", "foo"),
            Some("Package foo".into())
        );
    }

    #[test]
    fn symbol_name_match_allows_articles() {
        assert!(match_symbol_name("Foo is a thing.", "Foo"));
        assert!(match_symbol_name("A Foo is a thing.", "Foo"));
        assert!(match_symbol_name("An Foo is odd.", "Foo"));
        assert!(match_symbol_name("The Foo does X.", "Foo"));
        assert!(!match_symbol_name("This is Foo.", "Foo"));
    }

    #[test]
    fn deprecation_marker_detection() {
        assert!(check_deprecations("DEPRECATED: do not use"));
        assert!(check_deprecations("deprecated: do not use"));
        assert!(!check_deprecations("Deprecated: do not use"));
        assert!(has_deprecated_paragraph("Foo is X.\n\nDeprecated: use Bar."));
        assert!(!has_deprecated_paragraph("Foo is X.\n\nNot deprecated."));
    }
}
