//! `gofmt -s` parity: upstream's own testdata, plus one case per rewrite.
//!
//! The `.input`/`.golden` pairs come from `$GOROOT/src/cmd/gofmt/testdata` —
//! every file there marked `//gofmt -s`. They are the corpus Go itself uses to
//! test this feature, so they cover the shapes upstream thought were
//! interesting; the hand-written cases below cover the branches they do not
//! separate, and each of those expectations was taken from running the real
//! `gofmt -s`.

use std::path::{Path, PathBuf};

fn testdata_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/gofmt_simplify")
}

fn simplified(src: &str) -> String {
    let out = guff::format::source_simplified(src.as_bytes()).expect("format");
    String::from_utf8(out).expect("utf-8")
}

#[test]
fn upstream_gofmt_s_testdata() {
    let dir = testdata_dir();
    let mut pairs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("testdata dir")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "input"))
        .collect();
    pairs.sort();
    assert_eq!(
        pairs.len(),
        4,
        "expected upstream's four `//gofmt -s` pairs, found {}",
        pairs.len()
    );

    for input in &pairs {
        let golden = input.with_extension("golden");
        let src = std::fs::read_to_string(input).expect("read input");
        let want = std::fs::read_to_string(&golden).expect("read golden");
        let got = simplified(&src);
        assert_eq!(
            got,
            want,
            "{} differs from upstream's golden",
            input.file_name().expect("name").to_string_lossy()
        );
    }
}

/// The composite-literal rewrite: an element whose own type repeats the outer
/// element type drops the repeat. Array, slice and map all reach it, and for a
/// map both the key and the value type do.
#[test]
fn composite_literal_types_are_elided() {
    assert_eq!(
        simplified("package p\n\nvar a = [][]int{[]int{1, 2}, []int{3}}\n"),
        "package p\n\nvar a = [][]int{{1, 2}, {3}}\n"
    );
    assert_eq!(
        simplified("package p\n\nvar a = [2][2]int{[2]int{1, 2}, [2]int{3, 4}}\n"),
        "package p\n\nvar a = [2][2]int{{1, 2}, {3, 4}}\n"
    );
    assert_eq!(
        simplified("package p\n\nvar m = map[string][]string{\"a\": []string{\"x\"}}\n"),
        "package p\n\nvar m = map[string][]string{\"a\": {\"x\"}}\n"
    );
    // The map *key* type is elided as well — a separate call in upstream, and
    // one a value-only implementation would pass every other test without.
    assert_eq!(
        simplified("package p\n\ntype K struct{ a int }\n\nvar m = map[K]int{K{1}: 2}\n"),
        "package p\n\ntype K struct{ a int }\n\nvar m = map[K]int{{1}: 2}\n"
    );
}

/// The second half of the same rewrite: an element type of `*T` lets `&T{…}`
/// lose both the `&` and the `T`. This is the arm that replaces the element
/// rather than just clearing a field.
#[test]
fn address_of_composite_literals_lose_the_ampersand() {
    assert_eq!(
        simplified("package p\n\ntype T struct{ a int }\n\nvar v = []*T{&T{1}, &T{2}}\n"),
        "package p\n\ntype T struct{ a int }\n\nvar v = []*T{{1}, {2}}\n"
    );
    assert_eq!(
        simplified("package p\n\ntype T struct{ a int }\n\nvar m = map[string]*T{\"x\": &T{1}}\n"),
        "package p\n\ntype T struct{ a int }\n\nvar m = map[string]*T{\"x\": {1}}\n"
    );
}

/// Nesting: the walk must reach literals inside literals. Upstream stops the
/// generic walk at a simplified composite literal precisely because
/// `simplifyLiteral` has already recursed, so a port that forgot either half
/// would leave the inner type behind.
#[test]
fn nested_literals_are_simplified_all_the_way_down() {
    assert_eq!(
        simplified("package p\n\nvar a = [][][]int{[][]int{[]int{1}}}\n"),
        "package p\n\nvar a = [][][]int{{{1}}}\n"
    );
}

/// `s[a:len(s)]` → `s[a:]`, and the two shapes that must be left alone.
#[test]
fn slice_expressions_drop_a_redundant_len() {
    assert_eq!(
        simplified("package p\n\nfunc f(s []int) []int { return s[1:len(s)] }\n"),
        "package p\n\nfunc f(s []int) []int { return s[1:] }\n"
    );
    // A 3-index slice always requires the 2nd and 3rd index.
    let three = "package p\n\nfunc f(s []int) []int { return s[1:len(s):cap(s)] }\n";
    assert_eq!(simplified(three), three);
    // Only a plain identifier qualifies, so a selector is untouched.
    let sel = "package p\n\ntype W struct{ s []int }\n\nfunc f(w W) []int { return w.s[1:len(w.s)] }\n";
    assert_eq!(simplified(sel), sel);
}

/// The range rewrites, including the ordering between them: the value is
/// dropped first, and only a range whose value is *then* absent may drop its
/// key as well.
#[test]
fn blank_range_variables_are_dropped() {
    assert_eq!(
        simplified("package p\n\nfunc f(m map[string]int) {\n\tvar k string\n\tfor k, _ = range m {\n\t\t_ = k\n\t}\n}\n"),
        "package p\n\nfunc f(m map[string]int) {\n\tvar k string\n\tfor k = range m {\n\t\t_ = k\n\t}\n}\n"
    );
    assert_eq!(
        simplified("package p\n\nfunc f(m map[string]int) {\n\tfor _ = range m {\n\t}\n}\n"),
        "package p\n\nfunc f(m map[string]int) {\n\tfor range m {\n\t}\n}\n"
    );
    // A blank *key* with a live value stays put — there is no `for , v :=`.
    let keep = "package p\n\nfunc f(m map[string]int) {\n\tvar v int\n\tfor _, v = range m {\n\t\t_ = v\n\t}\n}\n";
    assert_eq!(simplified(keep), keep);
}

/// `removeEmptyDeclGroups`, and the comment that stops it.
#[test]
fn empty_declaration_groups_are_removed() {
    assert_eq!(
        simplified("package p\n\nconst ()\n\nvar ()\n\ntype ()\n\nvar x = 1\n"),
        "package p\n\nvar x = 1\n"
    );
    // A comment inside the group means it is not empty.
    let commented = "package p\n\nconst (\n// nothing here\n)\n\nvar x = 1\n";
    assert_eq!(simplified(commented), commented);
}

/// Without `-s` none of it happens — the same entry point, the other branch.
#[test]
fn plain_source_does_not_simplify() {
    let src = "package p\n\nvar a = [][]int{[]int{1}}\n";
    let out = guff::format::source(src.as_bytes()).expect("format");
    assert_eq!(String::from_utf8(out).expect("utf-8"), src);
}
