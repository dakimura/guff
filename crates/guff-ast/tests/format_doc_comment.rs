//! `go/printer`'s `formatDocComment`, end to end through [`guff::format::source`].
//!
//! [`crate::doc_comment`] gates the parse/print round trip against upstream's
//! own corpus; this file gates the layer above it — which comment groups
//! `go/printer` sends through that round trip, and how it puts the `//` back.
//!
//! One case per branch of `formatDocComment`, because the function is mostly
//! branches and a single "the space came back" fixture would hide all of them:
//! the two `/* */` bail-outs, the `/* */` rewrite, directive extraction, the
//! empty-text bail-out, tab-indented code lines, blank lines, and the
//! doc-comment-position test that keeps comments inside a function body alone.
//!
//! Every expectation here is `gofmt`'s, verified against go1.26.5.

fn fmt(src: &str) -> String {
    let out = guff::format::source(src.as_bytes()).expect("format");
    String::from_utf8(out).expect("utf-8")
}

#[test]
fn missing_space_is_restored_in_doc_position() {
    assert_eq!(
        fmt("package p\n\n//Foo does a thing.\nfunc Foo() {}\n"),
        "package p\n\n// Foo does a thing.\nfunc Foo() {}\n"
    );
}

#[test]
fn extra_space_and_tabs_are_squeezed() {
    assert_eq!(
        fmt("package p\n\n//  Foo does a thing.\nfunc Foo() {}\n"),
        "package p\n\n// Foo does a thing.\nfunc Foo() {}\n"
    );
    assert_eq!(
        fmt("package p\n\n//\tFoo does a thing.\nfunc Foo() {}\n"),
        "package p\n\n// Foo does a thing.\nfunc Foo() {}\n"
    );
}

#[test]
fn comment_inside_a_function_body_is_left_alone() {
    // Not in doc-comment position, so `formatDocComment` never sees it. This
    // is the half of the behaviour a "the space came back" test cannot show.
    assert_eq!(
        fmt("package p\n\nfunc Foo() {\n\t//bar\n\t_ = 0\n}\n"),
        "package p\n\nfunc Foo() {\n\t//bar\n\t_ = 0\n}\n"
    );
}

#[test]
fn a_directive_keeps_its_missing_space() {
    // `//nolint:gocritic` matches `//[a-z0-9]+:[a-z0-9]`, so it is a directive
    // and is passed through untouched — while the bare `//nolint` above it is
    // not, and gets the space. This pair is the compat/fix gocritic case.
    assert_eq!(
        fmt("package p\n\n//nolint\nfunc A() {}\n\n//nolint:gocritic // why\nfunc B() {}\n"),
        "package p\n\n// nolint\nfunc A() {}\n\n//nolint:gocritic // why\nfunc B() {}\n"
    );
}

#[test]
fn directives_are_moved_below_the_prose() {
    // A directive mixed into a doc comment is pulled out and re-emitted after
    // the text, separated by a bare `//`.
    assert_eq!(
        fmt("package p\n\n//go:noinline\n// Foo does a thing.\nfunc Foo() {}\n"),
        "package p\n\n// Foo does a thing.\n//\n//go:noinline\nfunc Foo() {}\n"
    );
}

#[test]
fn a_doc_comment_of_only_a_directive_is_untouched() {
    // Every line is a directive, so the extracted text is empty and
    // `formatDocComment` bails out before parsing.
    assert_eq!(
        fmt("package p\n\n//go:noinline\nfunc Foo() {}\n"),
        "package p\n\n//go:noinline\nfunc Foo() {}\n"
    );
}

#[test]
fn a_bare_slash_slash_doc_comment_is_deleted() {
    // The extracted text is "\n", which parses to an empty Doc and prints as
    // nothing — so the comment group disappears entirely. This is why a
    // partial "just add the space" rule is not a smaller version of this
    // change: it would have kept the comment.
    assert_eq!(
        fmt("package p\n\n//\nfunc Foo() {}\n"),
        "package p\n\nfunc Foo() {}\n"
    );
}

#[test]
fn blank_lines_and_indented_code_survive_the_round_trip() {
    let src = "package p\n\n\
        //Foo does a thing.\n\
        //\n\
        //\tfoo := Foo()\n\
        //\t_ = foo\n\
        //\n\
        //And then some.\n\
        func Foo() {}\n";
    let want = "package p\n\n\
        // Foo does a thing.\n\
        //\n\
        //\tfoo := Foo()\n\
        //\t_ = foo\n\
        //\n\
        // And then some.\n\
        func Foo() {}\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn unindented_code_block_is_reindented() {
    // The parser's "pasted in without indenting" heuristic: a line ending in
    // `{` starts a code block, and the printer emits it with a tab.
    let src = "package p\n\n\
        // Foo does a thing:\n\
        //\n\
        //\tif x {\n\
        //\t\treturn\n\
        //\t}\n\
        func Foo() {}\n";
    assert_eq!(fmt(src), src);
}

#[test]
fn a_list_is_renumbered_to_canonical_spacing() {
    let src = "package p\n\n\
        // Foo does a thing:\n\
        //  - one\n\
        //  - two\n\
        func Foo() {}\n";
    let want = "package p\n\n\
        // Foo does a thing:\n\
        //   - one\n\
        //   - two\n\
        func Foo() {}\n";
    assert_eq!(fmt(src), want);
}

#[test]
fn a_multiline_block_doc_comment_is_rewritten() {
    assert_eq!(
        fmt("package p\n\n/*\nFoo does a thing.\n\nAnd another.\n*/\nfunc Foo() {}\n"),
        "package p\n\n/*\nFoo does a thing.\n\nAnd another.\n*/\nfunc Foo() {}\n"
    );
    // Leading indentation inside the block is stripped by `unindent`.
    assert_eq!(
        fmt("package p\n\n/*\n   Foo does a thing.\n*/\nfunc Foo() {}\n"),
        "package p\n\n/*\nFoo does a thing.\n*/\nfunc Foo() {}\n"
    );
}

#[test]
fn a_single_line_block_doc_comment_is_left_alone() {
    // No newline inside, so `formatDocComment` returns early: reformatting it
    // would only make things worse.
    assert_eq!(
        fmt("package p\n\n/* Foo does a thing. */\nfunc Foo() {}\n"),
        "package p\n\n/* Foo does a thing. */\nfunc Foo() {}\n"
    );
}

#[test]
fn an_old_style_starred_block_comment_is_left_alone() {
    // `allStars`: every line begins with `*`, so this is a pre-Go-1.19 banner
    // rather than a doc comment, and is passed through.
    let src = "package p\n\n/*\n * Foo does a thing.\n * And another.\n */\nfunc Foo() {}\n";
    assert_eq!(fmt(src), src);
}

#[test]
fn a_package_doc_comment_is_reformatted_too() {
    assert_eq!(
        fmt("//Package p does a thing.\npackage p\n"),
        "// Package p does a thing.\npackage p\n"
    );
}

#[test]
fn import_doc_comments_are_not_reformatted() {
    // `go/printer` skips the doc-comment path when the previous token was
    // `import`, so a comment inside an import block keeps its own spacing.
    let src = "package p\n\nimport (\n\t//fmt is used below.\n\t\"fmt\"\n)\n\nvar _ = fmt.Sprint\n";
    assert_eq!(fmt(src), src);
}
