//! `go/doc/comment` parity: upstream's own txtar corpus, run against the port.
//!
//! Mirrors `TestTestdata` in `$GOROOT/src/go/doc/comment/testdata_test.go`,
//! including the parser hooks that test installs (`Words`, `LookupPackage`,
//! `LookupSym`) — without them the `doclink*` and `link*` cases would exercise
//! only the fallback paths.
//!
//! Only the `gofmt` sections are compared: that is `Printer.Comment`, the one
//! printer `go/printer`'s `formatDocComment` calls and the one this crate
//! ports. Every fixture must have such a section, so none of them can quietly
//! assert nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use guff::doc::comment::{default_lookup_package, Parser, Printer};

fn testdata_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/doc_comment")
}

/// Splits a txtar archive into `(comment, [(name, data)])`.
fn txtar_parse(src: &str) -> (String, Vec<(String, String)>) {
    let mut comment = String::new();
    let mut files: Vec<(String, String)> = Vec::new();
    let mut current: Option<String> = None;

    for line in src.split_inclusive('\n') {
        if let Some(name) = txtar_marker(line) {
            files.push((name, String::new()));
            current = Some(String::new());
            continue;
        }
        match current {
            Some(_) => files.last_mut().expect("marker seen").1.push_str(line),
            None => comment.push_str(line),
        }
    }
    (comment, files)
}

/// Returns the file name if `line` is a `-- name --` marker line.
fn txtar_marker(line: &str) -> Option<String> {
    let t = line.strip_suffix('\n').unwrap_or(line);
    let t = t.strip_suffix('\r').unwrap_or(t);
    let inner = t.strip_prefix("-- ")?.strip_suffix(" --")?;
    Some(inner.to_string())
}

/// Removes the trailing `$` markers upstream uses to make lines with trailing
/// spaces visible (and to survive editors that strip them).
fn strip_dollars(s: &str) -> String {
    s.replace("$\n", "\n")
}

#[test]
fn upstream_testdata_gofmt_sections() {
    let dir = testdata_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("testdata dir")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .collect();
    files.sort();
    assert!(
        files.len() >= 50,
        "expected the full upstream corpus, found {} files",
        files.len()
    );

    // The exact hooks TestTestdata installs.
    let mut words = HashMap::new();
    words.insert("italicword".to_string(), String::new());
    words.insert(
        "linkedword".to_string(),
        "https://example.com/linkedword".to_string(),
    );
    let lookup_package = |name: &str| -> Option<String> {
        if name == "comment" {
            return Some("go/doc/comment".to_string());
        }
        default_lookup_package(name)
    };
    let lookup_sym = |recv: &str, name: &str| -> bool {
        matches!(
            (recv, name),
            ("Parser", "Parse") | ("", "Doc") | ("", "NoURL")
        )
    };
    let parser = Parser {
        words,
        lookup_package: Some(&lookup_package),
        lookup_sym: Some(&lookup_sym),
    };

    let mut checked = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for file in &files {
        let src = std::fs::read_to_string(file).expect("read fixture");
        let (_comment, sections) = txtar_parse(&src);
        let name = file.file_name().expect("file name").to_string_lossy();

        assert_eq!(
            sections.first().map(|(n, _)| n.as_str()),
            Some("input"),
            "{name}: first txtar section is not \"input\""
        );
        let input = strip_dollars(&sections[0].1);

        let Some((_, want_raw)) = sections.iter().find(|(n, _)| n == "gofmt") else {
            failures.push(format!("{name}: no \"gofmt\" section — fixture asserts nothing"));
            continue;
        };

        // Upstream collapses the trailing blank line txtar adds before the
        // next `-- name --` marker.
        let mut want = strip_dollars(want_raw);
        while want.len() >= 2 && want.ends_with("\n\n") {
            want.pop();
        }

        let got = Printer.comment(&parser.parse(&input));
        if got != want {
            failures.push(format!(
                "{name}:\n--- want ---\n{want}--- got ---\n{got}--- end ---"
            ));
        }
        checked += 1;
    }

    assert!(
        failures.is_empty(),
        "{} of {} fixtures diverge:\n\n{}",
        failures.len(),
        files.len(),
        failures.join("\n\n")
    );
    assert_eq!(
        checked,
        files.len(),
        "every fixture must contribute a gofmt assertion"
    );
}

/// The shape `go/printer` actually depends on: a doc comment whose `//` lines
/// have no space after the slashes comes back with one.
#[test]
fn round_trip_adds_the_missing_space() {
    let parser = Parser::default();
    assert_eq!(
        Printer.comment(&parser.parse("nolint\n")),
        "nolint\n",
        "the parser is fed comment text with markers already stripped"
    );
    assert_eq!(
        Printer.comment(&parser.parse("Foo does a thing.\n\nAnd another.\n")),
        "Foo does a thing.\n\nAnd another.\n"
    );
    // An empty doc comment prints as nothing at all, which is why
    // `formatDocComment` can delete a lone `//`.
    assert_eq!(Printer.comment(&parser.parse("")), "");
}
