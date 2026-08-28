//! Port of [`github.com/godoc-lint/godoc-lint`](https://github.com/godoc-lint/godoc-lint)
//! (golangci-lint wrapper in `pkg/golinters/godoclint`).
//!
//! Implements the **basic** default rule set — `pkg-doc`, `single-pkg-doc`,
//! `start-with-name`, `deprecated` — plus `no-unused-link` and
//! `require-pkg-doc`, which are not in it but which `default: all` and an
//! explicit `enable:` both reach.
//!
//! Comments are re-parsed with [`PARSE_COMMENTS`] because production package
//! load uses `Mode::NONE`, which drops lead comments after the package clause.
//!
//! Settings: `linters.settings.godoclint` (`default` / `enable` / `disable` /
//! `options`). The `*/include-tests` half of upstream's options is *not*
//! configuration: golangci-lint overwrites each one after reading the user's
//! config, so they are the [`INCLUDE_TESTS`] constants below.
//!
//! Block structure comes from [`guff::doc::comment`], the port of the parser
//! upstream itself feeds every doc comment through, so "paragraph" here means
//! what it means to godoc rather than "text between blank lines".
//!
//! DEFERRED, with the reason each is still deferred:
//!
//! - `require-doc` — needs a symbol model this file does not build:
//!   `TrailingDoc` (`const X = 1 // doc`) and a symbol list that keeps the
//!   symbols with *no* doc, which [`collect_symbol_docs`] drops.
//! - `max-len` — needs `go/doc/comment`'s printer and the pinned
//!   `ignore-patterns: ["^\+kubebuilder:"]`. `options.max-len.length` is
//!   already parsed.
//! - `require-stdlib-doclink` — needs upstream's generated index of standard
//!   library symbols (`stdlib_doclink/stdlib.json`).
//!
//! Also deferred: `//godoclint:disable` directives.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use guff::ast::{CommentGroup, Decl, Expr, Spec};
use guff::token::Token;
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

/// The `*/include-tests` values golangci-lint pins in its `PlainConfig`
/// literal, overwriting whatever the user wrote.
///
/// They are not uniform, and the two neighbouring rules that disagree are the
/// trap: `require-pkg-doc` is `false` while `require-doc` is `true`, so a
/// `_test.go` file can be reported by one and invisible to the other. Read the
/// row before reusing an adjacent rule's guard.
/// One constant per implemented rule; the rest arrive with their rule
/// (`max-len: true`, `require-doc: true`, `require-stdlib-doclink: true`).
mod include_tests {
    pub const PKG_DOC: bool = false;
    pub const SINGLE_PKG_DOC: bool = true;
    pub const REQUIRE_PKG_DOC: bool = false;
    pub const START_WITH_NAME: bool = false;
    pub const NO_UNUSED_LINK: bool = true;
    /// `deprecated` takes no option: upstream hard-codes `false` at the call
    /// site (`AnalysisApplicableFiles(actx, false, …)`).
    pub const DEPRECATED: bool = false;
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

/// `shared.HasDeprecatedParagraph`: a *paragraph* whose first text run is
/// plain and starts with `Deprecated: `.
///
/// Both halves matter and neither survives a blank-line split. A `Deprecated:`
/// line that is indented parses as a **code block**, not a paragraph, and a
/// paragraph opening with a link or doc link has a first run that is not
/// `Plain`. Upstream skips both.
fn has_deprecated_paragraph(text: &str) -> bool {
    first_plain_of_each_paragraph(text)
        .any(|t| t.starts_with(CORRECT_DEPRECATION_MARKER))
}

/// The `Text[0]`-if-`Plain` of every `Paragraph` block, which is the only
/// thing either deprecation check looks at.
fn first_plain_of_each_paragraph(text: &str) -> impl Iterator<Item = String> {
    use guff::doc::comment::{Block, Parser, Text};
    Parser::default()
        .parse(text)
        .content
        .into_iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => match p.text.into_iter().next() {
                Some(Text::Plain(s)) => Some(s),
                _ => None,
            },
            _ => None,
        })
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

/// `no_unused_link.checkNoUnusedLink`: every link definition the comment
/// declares but never references.
///
/// `Doc.links` carries the `used` flag the parser sets while resolving `[text]`
/// spans, so this reads it directly rather than re-scanning the text — which
/// is also why the rule was deferred until `go/doc/comment` was ported.
///
/// Returns the link texts in declaration order; upstream reports one
/// diagnostic per unused definition.
fn unused_links(text: &str) -> Vec<String> {
    guff::doc::comment::Parser::default()
        .parse(text)
        .links
        .into_iter()
        .filter(|d| !d.used)
        .map(|d| d.text)
        .collect()
}

/// `deprecated.checkDeprecations`.
///
/// The correct usage of a deprecation marker is at the beginning of a
/// *paragraph* — not a heading, code block or list — with the exact spelling
/// `Deprecated: `. Anything else the regexp matches is reported.
fn check_deprecations(text: &str) -> bool {
    first_plain_of_each_paragraph(text).any(|t| {
        probable_deprecation_re()
            .find(&t)
            .is_some_and(|m| m.as_str() != CORRECT_DEPRECATION_MARKER)
    })
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
    let check_unused_link = rules.contains("no-unused-link");
    let check_require_pkg_doc = rules.contains("require-pkg-doc");
    let start_with_name_include_unexported = options.start_with_name_include_unexported;

    if !check_pkg_doc
        && !check_single
        && !check_start
        && !check_deprecated
        && !check_unused_link
        && !check_require_pkg_doc
    {
        return Ok(None);
    }

    let paths: Vec<_> = pass.pkg().compiled_go_files.clone();
    let fset = pass.fset().clone();
    let n = pass.files().len();

    let mut pending: Vec<(u32, String)> = Vec::new();
    // `no-unused-link` collects comment groups into a *set* upstream, so a
    // parent doc shared by several specs of one `const (…)` block is checked
    // once rather than once per spec. Positions stand in for identity: two
    // distinct groups cannot start at the same offset.
    let mut unused_link_seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
    // package name → list of (pos, has_nonempty_doc) for single-pkg-doc
    let mut pkg_docs: HashMap<String, Vec<(u32, bool)>> = HashMap::new();
    // package name → (position of the *first* file's package identifier, whether
    // any file in the package has a non-empty doc) for require-pkg-doc.
    //
    // Separate from `pkg_docs`, which only records files that *have* a doc:
    // this rule has to report at a file it found nothing in, so it needs the
    // package clause of every applicable file, documented or not.
    let mut pkg_any_doc: HashMap<String, (u32, bool)> = HashMap::new();

    for i in 0..n {
        let file = &pass.files()[i];
        let Some(path) = paths.get(i) else {
            continue;
        };
        let is_test = is_test_path(path);
        let Some((re_fset, parsed)) = reparse_with_comments(path, pass.pkg().source_bytes(i))
        else {
            continue;
        };
        let pkg_name = parsed.name.name.as_str();

        // --- require-pkg-doc bookkeeping ---
        //
        // golangci-lint pins `RequirePkgDocIncludeTests: false`, so `_test.go`
        // files neither satisfy the requirement nor get reported.
        if check_require_pkg_doc && !(is_test && !include_tests::REQUIRE_PKG_DOC) {
            if let Some(name_pos) = reparsed_pos(&fset, file.pos(), &re_fset, parsed.name.pos()) {
                let has_doc = parsed.doc.as_ref().and_then(doc_text).is_some();
                let e = pkg_any_doc
                    .entry(pkg_name.to_string())
                    .or_insert((name_pos, false));
                // `or_insert` keeps the first file's position, which is where
                // upstream reports (`fs[0].Name.Pos()`).
                e.1 |= has_doc;
            }
        }

        // --- package docs ---
        //
        // The three rules reading this comment group do *not* share a
        // test-file guard: `single-pkg-doc` is pinned `include-tests: true`
        // while `pkg-doc` and `deprecated` are `false`. A `_test.go` file's
        // package doc therefore counts toward "more than one godoc" and is
        // simultaneously invisible to the prefix and deprecation checks.
        if let Some(pkg_doc) = &parsed.doc {
            if let Some(text) = doc_text(pkg_doc) {
                if let Some(pos) = reparsed_pos(&fset, file.pos(), &re_fset, pkg_doc.pos()) {
                    if check_single && !(is_test && !include_tests::SINGLE_PKG_DOC) {
                        pkg_docs
                            .entry(pkg_name.to_string())
                            .or_default()
                            .push((pos, true));
                    }
                    if check_pkg_doc
                        && !(is_test && !include_tests::PKG_DOC)
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
                    if check_deprecated
                        && !(is_test && !include_tests::DEPRECATED)
                        && check_deprecations(&text)
                    {
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

        // --- no-unused-link ---
        //
        // Upstream collects a *set* of comment groups — every symbol's own doc
        // and its parent doc — then checks each once. Two consequences the
        // rules around it do not share: there is no `exported` filter, and a
        // parent doc must be reached even when *no* spec under it has a doc of
        // its own, which is the common `const (…)` shape. `collect_symbol_docs`
        // yields nothing for that case, so the decl docs are gathered here
        // rather than through it.
        if check_unused_link && !(is_test && !include_tests::NO_UNUSED_LINK) {
            let mut docs: Vec<&CommentGroup> = Vec::new();
            // Pinned `NoUnusedLinkIncludeTests: true`, so a `_test.go` file's
            // package doc is in upstream's set while `pkg-doc` and
            // `start-with-name` (pinned `false`) skip the file entirely.
            if let Some(d) = &parsed.doc {
                docs.push(d);
            }
            for decl in &parsed.decls {
                if let Decl::GenDecl(g) = decl {
                    // Imports are not symbol declarations, so their doc is not
                    // in upstream's set.
                    let is_symbol_decl = matches!(
                        g.tok,
                        Some(Token::CONST) | Some(Token::VAR) | Some(Token::TYPE)
                    );
                    if is_symbol_decl && !g.specs.is_empty() {
                        if let Some(d) = &g.doc {
                            docs.push(d);
                        }
                    }
                }
            }
            for sym in collect_symbol_docs(&parsed) {
                docs.push(sym.doc);
                if let Some(p) = sym.parent_doc {
                    docs.push(p);
                }
            }
            for d in docs {
                let Some(dpos) = reparsed_pos(&fset, file.pos(), &re_fset, d.pos()) else {
                    continue;
                };
                if !unused_link_seen.insert(dpos) {
                    continue;
                }
                let Some(dtext) = doc_text(d) else {
                    continue;
                };
                for link in unused_links(&dtext) {
                    pending.push((dpos, format!("godoc has unused link (\"{link}\")")));
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

            // `start-with-name` reaches unexported symbols when
            // `options.start-with-name.include-unexported` is set; upstream's
            // guard is `!isExported && !includePrivate → skip`, not
            // `isExported → check`. Reading it as the latter silently dropped
            // every diagnostic the option exists to produce — and the rule is
            // in the *basic* default set, so this was not opt-in territory.
            if check_start
                && !(is_test && !include_tests::START_WITH_NAME)
                && (exported || start_with_name_include_unexported)
                && sym.name != "_"
                && !sym.multi_name
            {
                if !has_deprecated_paragraph(&text) && !match_symbol_name(&text, sym.name) {
                    pending.push((
                        pos,
                        format!("godoc should start with symbol name (\"{}\")", sym.name),
                    ));
                }
            }

            if check_deprecated && !(is_test && !include_tests::DEPRECATED) && exported {
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

    if check_require_pkg_doc {
        for (pkg, (first_pos, any_doc)) in pkg_any_doc {
            if any_doc {
                continue;
            }
            pending.push((first_pos, format!("package should have a godoc (\"{pkg}\")")));
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        };
        let rules = opts.effective_rules();
        assert!(!rules.contains("deprecated"));
        assert!(rules.contains("pkg-doc"));
    }

    /// godoc-lint's `config/default.yaml` is the floor golangci-lint's plain
    /// config is layered over, so an option the user did not write keeps the
    /// upstream value — not the Rust zero value. `ignore-unexported` is the
    /// entry where those two differ, and reading it as `false` would turn
    /// `require-doc` into a check on every unexported symbol in the tree.
    #[test]
    fn option_defaults_come_from_upstream_default_yaml() {
        let o = GodoclintOptions::default();
        assert_eq!(o.max_len_length, 77);
        assert!(!o.require_doc_ignore_exported);
        assert!(o.require_doc_ignore_unexported);
        assert!(!o.start_with_name_include_unexported);
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

    /// `no-unused-link` reads the `used` flag the doc-comment parser sets while
    /// resolving `[text]` spans, so a definition referenced anywhere in the
    /// comment counts as used.
    #[test]
    fn unused_links_are_the_ones_never_referenced() {
        assert_eq!(
            unused_links("Foo does a thing.\n\n[a]: https://example.com/a"),
            vec!["a".to_string()]
        );
        assert!(unused_links("Foo, see [a].\n\n[a]: https://example.com/a").is_empty());
        assert_eq!(
            unused_links("Foo.\n\n[a]: https://example.com/a\n[b]: https://example.com/b"),
            vec!["a".to_string(), "b".to_string()],
            "reported in declaration order, one per definition"
        );
        // A reference anywhere in the comment counts, not just the paragraph
        // holding the definition.
        assert!(unused_links("See [a] below.\n\nMore prose.\n\n[a]: https://x").is_empty());
        assert!(unused_links("Foo has no links at all.").is_empty());
    }

    /// Only a *paragraph* carries a deprecation marker. Every other block kind
    /// upstream's parser can produce is skipped, and a blank-line split cannot
    /// tell them apart — an indented marker used to be reported here and is
    /// not by `golangci-lint`.
    ///
    /// One case per block kind the parser emits, plus the false negative
    /// upstream documents and deliberately keeps.
    #[test]
    fn only_paragraphs_carry_a_deprecation_marker() {
        // Paragraph: both branches of the marker spelling.
        assert!(check_deprecations("Foo does a thing.\n\ndeprecated: use Bar."));
        assert!(!check_deprecations("Foo does a thing.\n\nDeprecated: use Bar."));

        // Code block — indented, so not a paragraph.
        assert!(!check_deprecations(
            "Foo does a thing.\n\n\tdeprecated: use Bar."
        ));
        // Still a code block when it follows other prose and a blank line.
        assert!(!check_deprecations(
            "Foo does a thing:\n\n\tif x {\n\t}\n\n\tdeprecated: use Bar."
        ));

        // Heading.
        assert!(!check_deprecations(
            "Foo does a thing.\n\n# deprecated: nope\n\nmore text"
        ));

        // List item.
        assert!(!check_deprecations(
            "Foo does a thing.\n\n  - deprecated: nope"
        ));

        // Upstream's documented false negative: a marker that begins a *line*
        // but not a paragraph is in the middle of the preceding one, and is
        // left alone rather than risk flagging prose that merely ends in the
        // word "deprecated:".
        assert!(!check_deprecations(
            "Foo is a symbol.\ndeprecated: use Bar."
        ));

        // `has_deprecated_paragraph` reads the same block structure.
        assert!(!has_deprecated_paragraph(
            "Foo is X.\n\n\tDeprecated: use Bar."
        ));
        assert!(has_deprecated_paragraph("Foo is X.\n\nDeprecated: use Bar."));
    }
}
