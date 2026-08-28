//! Port of [`github.com/godoc-lint/godoc-lint`](https://github.com/godoc-lint/godoc-lint)
//! (golangci-lint wrapper in `pkg/golinters/godoclint`).
//!
//! Implements the **basic** default rule set — `pkg-doc`, `single-pkg-doc`,
//! `start-with-name`, `deprecated` — plus `no-unused-link`, `require-pkg-doc`
//! and `require-doc`, which are not in it but which `default: all` and an
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
/// (`max-len: true`, `require-stdlib-doclink: true`).
mod include_tests {
    pub const PKG_DOC: bool = false;
    pub const SINGLE_PKG_DOC: bool = true;
    pub const REQUIRE_PKG_DOC: bool = false;
    pub const REQUIRE_DOC: bool = true;
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

/// One entry of upstream's `FileInspection.SymbolDecl`.
///
/// This is the whole list, including symbols that carry no doc of their own.
/// The collector this replaces dropped those at three `continue`s, which is
/// exactly the set `deprecated`'s parent-doc branch — and, when it lands,
/// `require-doc` — is made of.
///
/// Upstream's `MultiSpecDecl` / `MultiSpecIndex` / `MultiNameIndex` /
/// `IsTypeAlias` are omitted: no implemented rule reads them.
struct SymbolDecl<'a> {
    kind: SymbolKind,
    name: &'a str,
    /// The *identifier*, which is where `require-doc` reports. Every other
    /// rule reports at a doc comment, so this is the one rule that has an
    /// answer for a symbol with no comment anywhere near it.
    ident: &'a guff::ast::Ident,
    doc: Option<&'a CommentGroup>,
    /// `const X = 1 // doc`, from `spec.Comment`. Only `require-doc` reads it,
    /// and only for const/var/type — a trailing comment on a `func` is not in
    /// upstream's model at all.
    trailing_doc: Option<&'a CommentGroup>,
    /// The enclosing `GenDecl`'s doc, and **only** for a parenthesized group:
    /// upstream treats the comment above a single-line `const`/`var`/`type` as
    /// the symbol's own `Doc` and leaves `ParentDoc` nil.
    parent_doc: Option<&'a CommentGroup>,
    multi_name: bool,
    is_method: bool,
    method_recv_base: Option<&'a str>,
}

impl SymbolDecl<'_> {
    /// `ast.IsExported(Name)`, with the receiver adjustment upstream applies in
    /// `require-doc` and `start-with-name`.
    ///
    /// `deprecated` deliberately does **not** apply it — see [`run`].
    fn exported_for_godoc(&self) -> bool {
        let mut exported = is_exported(self.name);
        if self.is_method {
            if let Some(base) = self.method_recv_base {
                exported = exported && is_exported(base);
            }
        }
        exported
    }
}

/// `model.SymbolDeclKind`, collapsed.
///
/// Upstream keeps `Const` / `Var` / `Type` apart but no rule distinguishes
/// them: `require-doc` branches only on `Func` vs everything else, because a
/// `func` has neither a trailing comment nor a parent doc to fall back on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SymbolKind {
    Func,
    Value,
}

/// Port of `inspect.Inspector`'s top-level symbol walk.
///
/// The branch that matters is `GenDecl.Lparen`: a single-line
/// `// doc\nconst foo = 0` puts the comment in `Doc` with a nil `ParentDoc`,
/// while `// doc\nconst ( foo = 0 )` puts it in `ParentDoc` with a nil `Doc`.
/// guff's parser, unlike `go/parser`, copies a single-line `GenDecl`'s lead
/// comment onto the spec as well, so reading `spec.doc` alone would make the
/// two shapes indistinguishable. Branching on `lparen` the way upstream does
/// keeps that quirk out of the answer.
fn collect_symbol_decls(file: &guff::ast::File) -> Vec<SymbolDecl<'_>> {
    let mut out = Vec::new();
    for decl in &file.decls {
        match decl {
            Decl::FuncDecl(fd) => {
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
                out.push(SymbolDecl {
                    kind: SymbolKind::Func,
                    name: fd.name.name.as_str(),
                    ident: &fd.name,
                    doc: fd.doc.as_ref(),
                    trailing_doc: None,
                    parent_doc: None,
                    multi_name: false,
                    is_method,
                    method_recv_base,
                });
            }
            Decl::GenDecl(gen) => {
                // `import (…)` declares no symbols; upstream's `switch dt.Tok`
                // falls through to `continue` for anything but const/var/type.
                if !matches!(
                    gen.tok,
                    Some(Token::CONST) | Some(Token::VAR) | Some(Token::TYPE)
                ) {
                    continue;
                }
                let grouped = gen.lparen != guff::NO_POS;
                let parent_doc = if grouped { gen.doc.as_ref() } else { None };
                for spec in &gen.specs {
                    match spec {
                        Spec::TypeSpec(ts) => {
                            let doc = if grouped {
                                ts.doc.as_ref()
                            } else {
                                gen.doc.as_ref()
                            };
                            out.push(SymbolDecl {
                                kind: SymbolKind::Value,
                                name: ts.name.name.as_str(),
                                ident: &ts.name,
                                doc,
                                trailing_doc: ts.comment.as_ref(),
                                parent_doc,
                                multi_name: false,
                                is_method: false,
                                method_recv_base: None,
                            });
                        }
                        Spec::ValueSpec(vs) => {
                            let doc = if grouped {
                                vs.doc.as_ref()
                            } else {
                                gen.doc.as_ref()
                            };
                            let multi_name = vs.names.len() > 1;
                            for name in &vs.names {
                                out.push(SymbolDecl {
                                    kind: SymbolKind::Value,
                                    name: name.name.as_str(),
                                    ident: name,
                                    doc,
                                    trailing_doc: vs.comment.as_ref(),
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
            // `SymbolDeclKindBad`. Upstream records it and every rule then
            // skips it, so recording it here would change nothing.
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
    // Upstream returns before touching a single file when both halves are
    // ignored, so a config that says "require nothing" is not a rule that
    // reports nothing — it is a rule that never runs.
    let require_public = !options.require_doc_ignore_exported;
    let require_private = !options.require_doc_ignore_unexported;
    let check_require_doc =
        rules.contains("require-doc") && (require_public || require_private);

    if !check_pkg_doc
        && !check_single
        && !check_start
        && !check_deprecated
        && !check_unused_link
        && !check_require_pkg_doc
        && !check_require_doc
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
    let mut deprecated_seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
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
        // The two rules reading this comment group do *not* share a test-file
        // guard: `single-pkg-doc` is pinned `include-tests: true` while
        // `pkg-doc` is `false`. A `_test.go` file's package doc therefore
        // counts toward "more than one godoc" and is simultaneously invisible
        // to the prefix check. (`deprecated` also reads it, from its own set
        // below, with a third answer of the same shape.)
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
                }
            }
        }

        let symbols = collect_symbol_decls(&parsed);

        // --- no-unused-link ---
        //
        // Upstream collects a *set* of comment groups — every symbol's own doc
        // and its parent doc — then checks each once. Two consequences the
        // rules around it do not share: there is no `exported` filter, and a
        // parent doc must be reached even when *no* spec under it has a doc of
        // its own, which is the common `const (…)` shape.
        //
        // Pinned `NoUnusedLinkIncludeTests: true`, so a `_test.go` file's
        // package doc is in upstream's set while `pkg-doc` and
        // `start-with-name` (pinned `false`) skip the file entirely.
        if check_unused_link && !(is_test && !include_tests::NO_UNUSED_LINK) {
            let mut docs: Vec<&CommentGroup> = Vec::new();
            if let Some(d) = &parsed.doc {
                docs.push(d);
            }
            for sym in &symbols {
                if let Some(p) = sym.parent_doc {
                    docs.push(p);
                }
                if let Some(d) = sym.doc {
                    docs.push(d);
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

        // --- deprecated ---
        //
        // Same set-of-groups shape as `no-unused-link`, and for the same
        // reason: `sd.ParentDoc` is collected *before* the `sd.Doc == nil`
        // continue, so a `// deprecated:` above a `const (…)` whose specs are
        // all undocumented is upstream's to report. Threading it through the
        // symbol's own doc — which is what this used to do — both missed that
        // shape and reported the parent's defect at the *child's* position.
        //
        // The export filter here is `ast.IsExported(sd.Name)` with **no**
        // receiver adjustment, unlike `require-doc` and `start-with-name`. An
        // exported method on an unexported receiver is checked by this rule
        // and skipped by those two. Sharing one `exported` local across the
        // loop is what hid that.
        if check_deprecated && !(is_test && !include_tests::DEPRECATED) {
            let mut docs: Vec<&CommentGroup> = Vec::new();
            if let Some(d) = &parsed.doc {
                docs.push(d);
            }
            for sym in &symbols {
                if !is_exported(sym.name) {
                    continue;
                }
                if let Some(p) = sym.parent_doc {
                    docs.push(p);
                }
                if let Some(d) = sym.doc {
                    docs.push(d);
                }
            }
            for d in docs {
                let Some(dpos) = reparsed_pos(&fset, file.pos(), &re_fset, d.pos()) else {
                    continue;
                };
                if !deprecated_seen.insert(dpos) {
                    continue;
                }
                let Some(dtext) = doc_text(d) else {
                    continue;
                };
                if check_deprecations(&dtext) {
                    pending.push((
                        dpos,
                        format!(
                            "deprecation note should be formatted as \"{CORRECT_DEPRECATION_MARKER}\""
                        ),
                    ));
                }
            }
        }

        // --- start-with-name ---
        if check_start && !(is_test && !include_tests::START_WITH_NAME) {
            for sym in &symbols {
                // Upstream's guard is `!isExported && !includePrivate → skip`,
                // not `isExported → check`. Reading it as the latter silently
                // dropped every diagnostic `include-unexported` exists to
                // produce — and the rule is in the *basic* default set, so
                // this was not opt-in territory.
                if !sym.exported_for_godoc() && !start_with_name_include_unexported {
                    continue;
                }
                if sym.name == "_" || sym.multi_name {
                    continue;
                }
                let Some(doc) = sym.doc else {
                    continue;
                };
                let Some(text) = doc_text(doc) else {
                    continue;
                };
                if has_deprecated_paragraph(&text) || match_symbol_name(&text, sym.name) {
                    continue;
                }
                let Some(pos) = reparsed_pos(&fset, file.pos(), &re_fset, doc.pos()) else {
                    continue;
                };
                pending.push((
                    pos,
                    format!("godoc should start with symbol name (\"{}\")", sym.name),
                ));
            }
        }

        // --- require-doc ---
        //
        // Pinned `include-tests: true`, the opposite of `require-pkg-doc`'s
        // `false`. The two rules read the same tree and disagree about which
        // files are in it.
        //
        // The const/var/type branch accepts a godoc from any of three places,
        // in upstream's order: the symbol's own doc, a trailing comment on the
        // spec, then the enclosing group's doc. A `func` has only the first —
        // upstream's model gives it no `TrailingDoc` and no `ParentDoc`.
        if check_require_doc && !(is_test && !include_tests::REQUIRE_DOC) {
            for sym in &symbols {
                let exported = sym.exported_for_godoc();
                if (exported && !require_public) || (!exported && !require_private) {
                    continue;
                }
                // `var _ = 0` names nothing to document.
                if sym.name == "_" {
                    continue;
                }
                let documented = match sym.kind {
                    SymbolKind::Func => sym.doc.and_then(doc_text).is_some(),
                    SymbolKind::Value => {
                        sym.doc.and_then(doc_text).is_some()
                            || sym.trailing_doc.and_then(doc_text).is_some()
                            || sym.parent_doc.and_then(doc_text).is_some()
                    }
                };
                if documented {
                    continue;
                }
                let Some(pos) = reparsed_pos(&fset, file.pos(), &re_fset, sym.ident.pos()) else {
                    continue;
                };
                pending.push((pos, format!("symbol should have a godoc (\"{}\")", sym.name)));
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

    fn parse(src: &str) -> (std::sync::Arc<guff::FileSet>, guff::ast::File) {
        reparse_with_comments(Path::new("t.go"), Some(src.as_bytes())).expect("parse")
    }

    fn decls(src: &str) -> Vec<(String, Option<String>, Option<String>)> {
        let (_fset, file) = parse(src);
        collect_symbol_decls(&file)
            .iter()
            .map(|d| {
                (
                    d.name.to_string(),
                    d.doc.and_then(doc_text),
                    d.parent_doc.and_then(doc_text),
                )
            })
            .collect()
    }

    /// The `Lparen` branch is the whole point of porting the inspector rather
    /// than reading `spec.doc`: guff's parser copies a single-line `GenDecl`'s
    /// lead comment onto the spec as well (go/parser passes `nil`), so
    /// `spec.doc` alone cannot tell `// d\nconst a = 1` from
    /// `// d\nconst ( a = 1 )`. Upstream calls the first one `Doc` and the
    /// second one `ParentDoc`, and `deprecated` reports at different lines for
    /// the two.
    #[test]
    fn single_line_decl_doc_is_doc_not_parent_doc() {
        assert_eq!(
            decls("package p\n\n// d\nconst a = 1\n"),
            vec![("a".into(), Some("d".into()), None)]
        );
        assert_eq!(
            decls("package p\n\n// d\nconst (\n\ta = 1\n)\n"),
            vec![("a".into(), None, Some("d".into()))]
        );
        assert_eq!(
            decls("package p\n\n// d\ntype a int\n"),
            vec![("a".into(), Some("d".into()), None)]
        );
        assert_eq!(
            decls("package p\n\n// d\ntype (\n\ta int\n)\n"),
            vec![("a".into(), None, Some("d".into()))]
        );
    }

    /// Symbols with no doc at all are in the list. That is the difference from
    /// the collector this replaced, and the reason `deprecated` could not see
    /// a parent doc above a group of undocumented specs.
    #[test]
    fn undocumented_symbols_are_kept() {
        assert_eq!(
            decls("package p\n\n// d\nconst (\n\ta = 1\n\tb = 2\n)\n"),
            vec![
                ("a".into(), None, Some("d".into())),
                ("b".into(), None, Some("d".into())),
            ]
        );
        assert_eq!(
            decls("package p\n\nfunc f() {}\n"),
            vec![("f".into(), None, None)]
        );
    }

    /// One entry per name, sharing the spec's doc — and `import (…)` declares
    /// no symbols, so its doc is in no rule's set.
    #[test]
    fn multi_name_specs_and_imports() {
        assert_eq!(
            decls("package p\n\n// d\nvar a, b = 1, 2\n"),
            vec![
                ("a".into(), Some("d".into()), None),
                ("b".into(), Some("d".into()), None),
            ]
        );
        assert!(decls("package p\n\n// d\nimport \"fmt\"\n").is_empty());
    }

    /// `deprecated` uses the bare name; `require-doc` and `start-with-name`
    /// fold in the receiver's base type. An exported method on an unexported
    /// receiver is where the two answers part.
    #[test]
    fn receiver_base_type_only_narrows_the_godoc_export_rule() {
        let (_fset, file) = parse("package p\n\ntype hidden int\n\nfunc (h hidden) M() {}\n");
        let all = collect_symbol_decls(&file);
        let m = all.iter().find(|d| d.name == "M").expect("method");
        assert!(is_exported(m.name), "deprecated's filter sees it as exported");
        assert!(
            !m.exported_for_godoc(),
            "require-doc / start-with-name fold in the unexported receiver"
        );
    }

    /// `require-doc`'s const/var/type branch accepts a godoc from three
    /// places. The trailing one is the reason `spec.Comment` is in the model
    /// at all, and it has a hole a fixture will not show: a *directive*
    /// trailing comment (`//go:generate`, `//foo:bar`) is dropped by
    /// `CommentGroup::text()`, so the group exists and documents nothing.
    #[test]
    fn trailing_directive_comment_documents_nothing() {
        let (_fset, file) = parse("package p\n\nconst A = 0 // godoc\n");
        let d = &collect_symbol_decls(&file)[0];
        assert!(d.trailing_doc.and_then(doc_text).is_some());

        let (_fset, file) = parse("package p\n\nconst A = 0 //foo:bar\n");
        let d = &collect_symbol_decls(&file)[0];
        assert!(
            d.trailing_doc.is_some(),
            "the group is there — go/parser attaches it"
        );
        assert!(
            d.trailing_doc.and_then(doc_text).is_none(),
            "but Text() drops directives, so the symbol is undocumented"
        );
    }

    /// A `func` has no trailing comment and no parent doc in upstream's model,
    /// so the three-way fallback must not apply to it.
    #[test]
    fn funcs_have_only_their_own_doc() {
        let (_fset, file) = parse("package p\n\nfunc F() {} // not a godoc\n");
        let d = &collect_symbol_decls(&file)[0];
        assert_eq!(d.name, "F");
        assert!(d.kind == SymbolKind::Func);
        assert!(d.doc.is_none());
        assert!(d.trailing_doc.is_none());
        assert!(d.parent_doc.is_none());
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
