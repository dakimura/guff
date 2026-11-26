//! Port of `golang.org/x/tools/internal/refactor`'s import edits.
//!
//! A suggested fix that writes `slices.Contains(...)` into a file that does not
//! import `slices` produces code that does not compile. Upstream's fix for that
//! is one function — `refactor.AddImport` — called by every modernize checker
//! that names a package in its replacement text, and the reason every one of
//! them can hard-code the string `"slices."` is that `AddImport` hands back the
//! prefix to use, which is *not* always the package's own name:
//!
//! * the file may already import the package under an alias (`myatomic`), in
//!   which case the existing name is reused and no import is added;
//! * the name may be shadowed at the fix site, in which case a fresh one
//!   (`slices0`) is chosen and the new import is a renaming import;
//! * the file may dot-import it, in which case the prefix is empty.
//!
//! The `member` argument is the symbol the caller intends to reference. It is
//! consulted only for a dot import, where the question is not whether the
//! package is in scope but whether *that name* is.

use guff::ast::{Decl, File, ImportSpec};
use guff_types::arena::{ObjectArena, ObjectData, ObjectId, ScopeArena, ScopeId};
use guff_types::api::Info;
use guff_types::scope::{innermost, lookup_parent};
use guff::token::Token;

use crate::diagnostic::TextEdit;
use crate::pass::Pass;

/// The prefix to qualify `member` with, plus the edits that add the import.
///
/// Returns `None` when the file's scope information is unavailable, which is
/// the one case upstream cannot have: it panics on a missing enclosing block.
/// Here a caller that gets `None` must drop its suggested fix rather than write
/// an unqualified reference — the fix would not compile.
///
/// Equivalent to `refactor.AddImport`.
pub fn add_import(
    pass: &Pass<'_>,
    file: &File,
    preferred_name: &str,
    pkgpath: &str,
    member: &str,
    pos: u32,
) -> Option<(String, Vec<TextEdit>)> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let scopes = &artifacts.scopes;
    let objects = &artifacts.objects;

    let file_scope = *info.scopes.get(&file.id)?;
    // Upstream panics here ("no enclosing lexical block"). The fix site always
    // lies inside the file, so a miss means the scope tree and the AST
    // disagree; declining the fix is the safe reading of that.
    let scope = innermost(scopes, file_scope, pos)?;

    // Is there an existing import of this package, and are we in its scope?
    for spec in &file.imports {
        let Some((pkgname_obj, pkgname_local, imported_path)) = pkg_name_of(info, artifacts, spec)
        else {
            continue;
        };
        if imported_path != pkgpath {
            continue;
        }
        if preferred_name == "_" {
            // Request for a blank import; any existing import will do.
            return Some((String::new(), Vec::new()));
        }
        if pkgname_local == "." {
            // The scope of the member must be the file scope.
            if let Some((found, _)) = lookup_parent(scopes, objects, scope, member, pos) {
                if found == file_scope {
                    return Some((String::new(), Vec::new()));
                }
            }
        } else if let Some((_, obj)) = lookup_parent(scopes, objects, scope, &pkgname_local, pos) {
            if obj == pkgname_obj {
                return Some((format!("{pkgname_local}."), Vec::new()));
            }
        }
    }

    // We must add a new import.
    let mut prefix = String::new();
    let mut new_name = preferred_name.to_string();
    if preferred_name != "_" {
        new_name = fresh_name(scopes, objects, scope, pos, preferred_name);
        prefix = format!("{new_name}.");
    }

    // Use a renaming import whenever the preferred name is not available, or
    // the chosen name does not match the last segment of its path.
    if new_name == preferred_name && new_name == path_base(pkgpath) {
        new_name = String::new();
    }

    Some((
        prefix,
        add_import_edits(file, file_source(pass, file), &new_name, pkgpath),
    ))
}

/// The edits that add an import of `pkgpath`, with no analysis of whether that
/// is necessary or safe. `name`, when non-empty, becomes the import's explicit
/// local name.
///
/// `src` is the file's source bytes, and upstream needs no equivalent: its AST
/// always carries comments, so it can read the first declaration's doc comment
/// straight off the node. guff parses without `PARSE_COMMENTS` in the analysis
/// pipeline, so `Decl.doc` is always `None` there and the doc comment has to be
/// located in the text. Pass `None` only where the file genuinely has no
/// comments to respect: without it a new import is inserted *between* the first
/// declaration and its doc comment.
///
/// Equivalent to `refactor.AddImportEdits`.
pub fn add_import_edits(
    file: &File,
    src: Option<&[u8]>,
    name: &str,
    pkgpath: &str,
) -> Vec<TextEdit> {
    let mut new_text = quote(pkgpath);
    if !name.is_empty() {
        new_text = format!("{name} {new_text}");
    }

    // Insert either before the first declaration (including its doc comment),
    // or at the end of the file when there are none, or inside the existing
    // import group.
    let decl0 = file.decls.first();
    let before = match decl0 {
        None => file.file_end.0 as u32,
        Some(d) => {
            let doc = match d {
                Decl::GenDecl(gd) => gd.doc.as_ref(),
                Decl::FuncDecl(fd) => fd.doc.as_ref(),
                Decl::BadDecl(_) => None,
            };
            doc.map(|c| c.pos().0 as u32)
                .or_else(|| src.and_then(|src| first_decl_doc_pos(file, src)))
                .unwrap_or(d.pos().0 as u32)
        }
    };

    let grouped = match decl0 {
        Some(Decl::GenDecl(gd)) => {
            (gd.tok == Some(Token::IMPORT) && gd.rparen.is_valid()).then_some(gd)
        }
        _ => None,
    };

    let pos = match grouped {
        Some(gd) if is_std_package(pkgpath) && !gd.specs.is_empty() => {
            // A std package goes before the first existing spec, followed by a
            // blank line when the one it displaces is not itself std.
            let first = first_import_spec(gd);
            match first {
                Some(first) => {
                    // Upstream passes the *quoted* path here, and the test —
                    // "no dot in the first segment" — is unaffected by the
                    // quotes for every real import path.
                    if !is_std_package(&first.path.value) {
                        new_text.push('\n');
                    }
                    new_text.push_str("\n\t");
                    import_spec_pos(first)
                }
                None => {
                    new_text = format!("\t{new_text}\n");
                    gd.rparen.0 as u32
                }
            }
        }
        Some(gd) => {
            // Add the spec at the end of the group.
            new_text = format!("\t{new_text}\n");
            gd.rparen.0 as u32
        }
        None => {
            // No import decl, or a non-grouped one: add a new import decl
            // before the first decl. gofmt merges the two afterwards.
            new_text = format!("import {new_text}\n\n");
            before
        }
    };

    vec![TextEdit {
        pos,
        end: pos,
        new_text,
    }]
}

/// The first name in `preferred`, `preferred0`, `preferred1`, ... not already
/// declared at `pos`.
///
/// Equivalent to `refactor.FreshName`.
pub fn fresh_name(
    scopes: &ScopeArena,
    objects: &ObjectArena,
    scope: ScopeId,
    pos: u32,
    preferred: &str,
) -> String {
    let mut new_name = preferred.to_string();
    let mut i = 0u32;
    loop {
        if lookup_parent(scopes, objects, scope, &new_name, pos).is_none() {
            return new_name; // fresh
        }
        new_name = format!("{preferred}{i}");
        i += 1;
    }
}

/// The `PkgName` an import spec binds: its object, its local name, and the path
/// of the package it names.
///
/// Equivalent to `types.Info.PkgNameOf`.
fn pkg_name_of(
    info: &Info,
    artifacts: &guff_packages::TypecheckArtifacts,
    spec: &ImportSpec,
) -> Option<(ObjectId, String, String)> {
    let obj = match &spec.name {
        Some(name) => info.defs.get(&name.id).copied().flatten()?,
        None => info.implicits.get(&spec.id).copied()?,
    };
    let ObjectData::PkgName(pn) = artifacts.objects.get(obj) else {
        return None;
    };
    Some((
        obj,
        pn.name().to_string(),
        artifacts.packages.get(pn.imported()).path().to_string(),
    ))
}

/// `ast.ImportSpec.Pos`: the local name when the import is renamed, else the
/// path literal.
fn import_spec_pos(spec: &ImportSpec) -> u32 {
    match &spec.name {
        Some(name) => name.pos().0 as u32,
        None => spec.path.pos().0 as u32,
    }
}

fn first_import_spec(gd: &guff::ast::GenDecl) -> Option<&ImportSpec> {
    gd.specs.iter().find_map(|s| match s {
        guff::ast::Spec::ImportSpec(is) => Some(is),
        _ => None,
    })
}

/// The edits to remove `pos..end`, taking the whole line when the range has the
/// line to itself.
///
/// Scoped port of `refactor.DeleteStmt`. Upstream spends ~150 lines deciding
/// how much of the surrounding line goes with a statement, because it has to
/// separate three cases that a `token.Pos` alone cannot: a sibling statement on
/// the same line (`a(); b()`), a comment that belongs to the statement, and a
/// comment that belongs to its neighbour. It does that with the parent node and
/// the file's comment list.
///
/// This asks two questions of the text instead. The line goes when nothing but
/// whitespace precedes the range on it, and nothing but whitespace or a `//`
/// comment follows. Upstream is explicit that a trailing comment goes with its
/// statement — "it removes whole lines like `stmt // comment`" — and declining
/// there is not the safe choice it looks like: it strands the comment on a line
/// whose code is gone.
///
/// A sibling statement (`a(); b()`) or an enclosing brace on the line is what
/// makes upstream keep it, and those are what the whitespace test rejects.
///
/// DEFERRED: block comments around the range, and upstream's finer runs that
/// trim *part* of a line's comments. Both need comment positions, which the
/// analysis pipeline does not parse (see [`add_import_edits`]); declining is
/// conservative for them because a `/* */` neighbour may not be the statement's.
pub fn delete_with_line(file: &File, src: Option<&[u8]>, pos: u32, end: u32) -> Vec<TextEdit> {
    let narrow = || {
        vec![TextEdit {
            pos,
            end,
            new_text: String::new(),
        }]
    };
    let Some(src) = src else {
        return narrow();
    };
    let base = file.file_start.0;
    let (Ok(start_off), Ok(end_off)) = (
        usize::try_from(i64::from(pos) - base),
        usize::try_from(i64::from(end) - base),
    ) else {
        return narrow();
    };
    if end_off > src.len() || start_off > end_off {
        return narrow();
    }

    // Back to the start of the line holding `pos`.
    let line_start = src[..start_off]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    // Forward to just past the newline ending the line holding `end`.
    let rest = &src[end_off..];
    let line_end = match rest.iter().position(|&b| b == b'\n') {
        Some(i) => end_off + i + 1,
        None => src.len(),
    };
    let trailing_end = if line_end > end_off && src[line_end - 1] == b'\n' {
        line_end - 1
    } else {
        line_end
    };

    let before = &src[line_start..start_off];
    let after = &src[end_off..trailing_end];
    // A `//` comment after the statement belongs to it and goes with the line;
    // anything else after it is code that must not.
    let after_ok = match after.iter().position(|b| !b.is_ascii_whitespace()) {
        None => true,
        Some(i) => after[i..].starts_with(b"//"),
    };
    if !before.iter().all(u8::is_ascii_whitespace) || !after_ok {
        return narrow();
    }
    vec![TextEdit {
        pos: u32::try_from(line_start as i64 + base).unwrap_or(pos),
        end: u32::try_from(line_end as i64 + base).unwrap_or(end),
        new_text: String::new(),
    }]
}

/// Where the first declaration's doc comment starts, read from the source text.
///
/// Everything between the package clause and the first declaration is comments
/// and whitespace — the grammar allows nothing else — so the doc comment is
/// simply whatever follows the last blank line in that span. A blank line
/// detaches a comment from the declaration below it, which is the same rule
/// `go/ast`'s comment attachment applies, and the reason
/// `// unrelated\n\nfunc f()` must not push the new import above the comment.
///
/// Returns `None` when the span holds no comment at all.
fn first_decl_doc_pos(file: &File, src: &[u8]) -> Option<u32> {
    let base = file.file_start.0;
    let decl_off = usize::try_from(file.decls.first()?.pos().0 - base).ok()?;
    // Start after the package clause's own line, so a trailing
    // `package p // import "x"` comment is never mistaken for a doc comment.
    let after_pkg = usize::try_from(file.name.end().0 - base).ok()?;
    if decl_off > src.len() || after_pkg > decl_off {
        return None;
    }
    let span = &src[after_pkg..decl_off];

    // The last blank line in the span; the doc comment begins after it.
    let mut start = 0usize;
    let mut line_start = 0usize;
    let mut saw_first_newline = false;
    for (i, &b) in span.iter().enumerate() {
        if b != b'\n' {
            continue;
        }
        if !saw_first_newline {
            // Rest of the package clause's line.
            saw_first_newline = true;
            start = i + 1;
            line_start = i + 1;
            continue;
        }
        if span[line_start..i].iter().all(|c| c.is_ascii_whitespace()) {
            start = i + 1;
        }
        line_start = i + 1;
    }

    let off = start + span[start..].iter().position(|c| !c.is_ascii_whitespace())?;
    // Only a comment can precede the declaration here; if the first thing we
    // find is the declaration itself, there is no doc comment.
    if !span[off..].starts_with(b"//") && !span[off..].starts_with(b"/*") {
        return None;
    }
    u32::try_from(after_pkg + off).ok().map(|o| o + base as u32)
}

/// The file whose extent covers `pos`.
///
/// Equivalent to `astutil.EnclosingFile`, which upstream reaches for whenever a
/// fix is computed from an object or a statement rather than from a syntax walk
/// that still has the file in hand.
pub fn enclosing_file<'p>(pass: &'p Pass<'_>, pos: u32) -> Option<&'p File> {
    pass.files()
        .iter()
        .find(|f| f.file_start.0 as u32 <= pos && pos < f.file_end.0 as u32)
}

/// The source bytes of `file`, when the package retained them.
pub fn file_source<'p>(pass: &'p Pass<'_>, file: &File) -> Option<&'p [u8]> {
    let idx = pass.files().iter().position(|f| f.id == file.id)?;
    pass.pkg().source_bytes(idx)
}

/// Reports whether `path` names a standard-library package: no dot in the first
/// segment. Vendored std (`vendor/golang.org/x/net/...`) counts; `testdata`
/// does not.
///
/// Equivalent to `packagepath.IsStdPackage`.
fn is_std_package(path: &str) -> bool {
    let first = path.split('/').next().unwrap_or(path);
    !first.contains('.') && path != "testdata"
}

/// `path.Base`, for the import-path grammar only: no Windows separators, no
/// cleaning beyond a trailing slash.
fn path_base(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".into();
    }
    match trimmed.rfind('/') {
        Some(i) => trimmed[i + 1..].to_string(),
        None => trimmed.to_string(),
    }
}

/// `strconv.Quote` for an import path. Package paths are printable ASCII
/// without quotes or backslashes in practice, so this is the escape set Go
/// would produce for them.
fn quote(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 2);
    out.push('"');
    for c in path.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff::parser::{parse_file, PARSE_COMMENTS};
    use guff::position::FileSet;

    /// Apply `add_import_edits` to `src` and return the rewritten source, so a
    /// test reads as the transformation it is pinning rather than as a pair of
    /// offsets. The offset itself is asserted separately where it is the point.
    fn parse(src: &str) -> guff::ast::File {
        let fset = FileSet::new();
        parse_file(&fset, "t.go", src.as_bytes(), PARSE_COMMENTS).expect("parse")
    }

    fn apply(src: &str, file: &guff::ast::File, edits: &[TextEdit]) -> String {
        assert_eq!(edits.len(), 1, "one edit: {edits:?}");
        let base = file.file_start.0 as usize;
        let (a, b) = (edits[0].pos as usize - base, edits[0].end as usize - base);
        format!("{}{}{}", &src[..a], edits[0].new_text, &src[b..])
    }

    fn add_with(src: &str, name: &str, pkgpath: &str, mode: guff::parser::Mode) -> String {
        let fset = FileSet::new();
        let file = parse_file(&fset, "t.go", src.as_bytes(), mode).expect("parse");
        let edits = add_import_edits(&file, Some(src.as_bytes()), name, pkgpath);
        assert_eq!(edits.len(), 1, "upstream always returns exactly one edit");
        let base = file.file_start.0 as usize;
        let at = edits[0].pos as usize - base;
        assert_eq!(edits[0].pos, edits[0].end, "an insertion, not a replacement");
        format!("{}{}{}", &src[..at], edits[0].new_text, &src[at..])
    }

    /// The analysis pipeline parses without `PARSE_COMMENTS`, so every case
    /// here is checked both ways: the two must agree, or `--fix` writes one
    /// thing under the linter and another under a test.
    fn add(src: &str, name: &str, pkgpath: &str) -> String {
        let with = add_with(src, name, pkgpath, PARSE_COMMENTS);
        let without = add_with(src, name, pkgpath, guff::parser::Mode::NONE);
        assert_eq!(
            with, without,
            "comment-less parse must place the import identically"
        );
        with
    }

    /// No import declaration: a new one goes before the first declaration,
    /// *above its doc comment*, and gofmt merges it with anything below later.
    #[test]
    fn adds_a_new_import_decl_above_the_first_decls_doc_comment() {
        assert_eq!(
            add("package p\n\n// doc\nfunc f() {}\n", "", "strings"),
            "package p\n\nimport \"strings\"\n\n// doc\nfunc f() {}\n"
        );
    }

    /// A statement alone on its line takes the line with it.
    #[test]
    fn delete_with_line_takes_the_whole_line() {
        let src = "package p\n\nfunc f() {\n\tone()\n\ttwo()\n}\n";
        let file = parse(src);
        let at = src.find("one()").expect("in source");
        let base = file.file_start.0 as usize;
        let edits = delete_with_line(
            &file,
            Some(src.as_bytes()),
            (at + base) as u32,
            (at + base + "one()".len()) as u32,
        );
        assert_eq!(apply(src, &file, &edits), "package p\n\nfunc f() {\n\ttwo()\n}\n");
    }

    /// A sibling statement on the same line means the line stays: deleting it
    /// would take the neighbour too.
    #[test]
    fn delete_with_line_keeps_a_shared_line() {
        let src = "package p\n\nfunc f() {\n\tone(); two()\n}\n";
        let file = parse(src);
        let at = src.find("one()").expect("in source");
        let base = file.file_start.0 as usize;
        let edits = delete_with_line(
            &file,
            Some(src.as_bytes()),
            (at + base) as u32,
            (at + base + "one()".len()) as u32,
        );
        assert_eq!(apply(src, &file, &edits), "package p\n\nfunc f() {\n\t; two()\n}\n");
    }

    /// A trailing comment goes with the statement it annotates.
    ///
    /// Upstream says so outright — "it removes whole lines like `stmt //
    /// comment`" — and keeping the line is not the cautious choice it appears
    /// to be: it leaves the comment behind explaining code that is gone.
    /// `compat/fix` caught this as govet writing one line more than upstream,
    /// after this test had been asserting the opposite.
    #[test]
    fn delete_with_line_takes_a_trailing_comment_with_the_line() {
        let src = "package p\n\nfunc f() {\n\tone() // why\n\ttwo()\n}\n";
        let file = parse(src);
        let at = src.find("one()").expect("in source");
        let base = file.file_start.0 as usize;
        let edits = delete_with_line(
            &file,
            Some(src.as_bytes()),
            (at + base) as u32,
            (at + base + "one()".len()) as u32,
        );
        assert_eq!(
            apply(src, &file, &edits),
            "package p\n\nfunc f() {\n\ttwo()\n}\n"
        );
    }

    /// A blank line detaches a comment from the declaration below it, and a
    /// detached comment must stay *above* the new import — it is not the
    /// declaration's doc.
    #[test]
    fn a_detached_comment_stays_above_the_new_import() {
        assert_eq!(
            add("package p\n\n// detached\n\nfunc f() {}\n", "", "strings"),
            "package p\n\n// detached\n\nimport \"strings\"\n\nfunc f() {}\n"
        );
    }

    /// A comment on the package clause's own line is not a doc comment for
    /// anything below it.
    #[test]
    fn a_trailing_package_comment_is_not_the_first_decls_doc() {
        assert_eq!(
            add("package p // import \"x\"\n\nfunc f() {}\n", "", "strings"),
            "package p // import \"x\"\n\nimport \"strings\"\n\nfunc f() {}\n"
        );
    }

    /// With no declarations at all there is nothing to insert before, so the
    /// insertion point is the end of the file.
    #[test]
    fn adds_a_new_import_decl_at_end_of_a_file_with_no_decls() {
        assert_eq!(
            add("package p\n", "", "strings"),
            "package p\nimport \"strings\"\n\n"
        );
    }

    /// A single un-parenthesised `import "x"` is *not* a group: upstream adds a
    /// second import decl rather than rewriting the first into a group.
    #[test]
    fn a_single_ungrouped_import_gets_a_second_import_decl() {
        assert_eq!(
            add("package p\n\nimport \"fmt\"\n", "", "strings"),
            "package p\n\nimport \"strings\"\n\nimport \"fmt\"\n"
        );
    }

    /// A std package joins an existing group *before* the first spec, and a
    /// blank line separates it from a non-std neighbour.
    #[test]
    fn a_std_package_goes_first_in_a_group_and_splits_from_non_std() {
        assert_eq!(
            add(
                "package p\n\nimport (\n\t\"example.com/x\"\n)\n",
                "",
                "strings"
            ),
            "package p\n\nimport (\n\t\"strings\"\n\n\t\"example.com/x\"\n)\n"
        );
        // std neighbour: no blank line.
        assert_eq!(
            add("package p\n\nimport (\n\t\"fmt\"\n)\n", "", "strings"),
            "package p\n\nimport (\n\t\"strings\"\n\t\"fmt\"\n)\n"
        );
    }

    /// A non-std package goes at the end of the group instead, at the rparen.
    #[test]
    fn a_non_std_package_goes_at_the_end_of_the_group() {
        assert_eq!(
            add("package p\n\nimport (\n\t\"fmt\"\n)\n", "", "example.com/x"),
            "package p\n\nimport (\n\t\"fmt\"\n\t\"example.com/x\"\n)\n"
        );
    }

    /// A non-empty `name` becomes a renaming import.
    #[test]
    fn a_name_becomes_a_renaming_import() {
        assert_eq!(
            add("package p\n\nimport (\n\t\"fmt\"\n)\n", "s", "strings"),
            "package p\n\nimport (\n\ts \"strings\"\n\t\"fmt\"\n)\n"
        );
    }

    /// The first-segment test, quotes and all: upstream passes the *quoted*
    /// path when it inspects the spec it is displacing, and that is harmless
    /// because a quote is not a dot.
    #[test]
    fn is_std_package_looks_only_at_the_first_segment() {
        assert!(is_std_package("os"));
        assert!(is_std_package("net/http"));
        assert!(is_std_package("vendor/golang.org/x/net/dns/dnsmessage"));
        assert!(!is_std_package("golang.org/x/net/dns/dnsmessage"));
        assert!(!is_std_package("testdata"));
        assert!(is_std_package("\"net/http\""));
        assert!(!is_std_package("\"example.com/x\""));
    }

    #[test]
    fn path_base_names_the_last_segment() {
        assert_eq!(path_base("net/http"), "http");
        assert_eq!(path_base("slices"), "slices");
        assert_eq!(path_base("sync/atomic"), "atomic");
        assert_eq!(path_base("a/b/"), "b");
        assert_eq!(path_base(""), ".");
    }
}
