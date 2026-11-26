// Port of Go's go/printer/comment.go.
//
// Original: Copyright 2022 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.

use crate::ast::Comment;
use crate::doc::comment::{Parser, Printer};

/// Reformats a top-level doc comment list to canonical form, returning the
/// canonical formatting.
///
/// This is the round trip gofmt performs on every doc comment: strip the
/// markers, parse the text with [`crate::doc::comment`], print it back, and
/// re-attach the markers. It is what turns `//Foo` into `// Foo`, collapses
/// `//  two spaces`, retabs indented code blocks, and drops a doc comment that
/// holds nothing but a bare `//`.
pub(crate) fn format_doc_comment(list: &[Comment]) -> Vec<Comment> {
    // Extract comment text (removing comment markers).
    let kind: &str;
    let mut text: String;
    let mut directives: Vec<&Comment> = Vec::new();

    let Some(first) = list.first() else {
        return list.to_vec();
    };

    if list.len() == 1 && first.text.starts_with("/*") {
        kind = "/*";
        text = first.text.clone();
        if !text.contains('\n') || all_stars(&text) {
            // Single-line /* .. */ comment in doc comment position, or
            // multiline old-style comment like
            //	/*
            //	 * Comment
            //	 * text here.
            //	 */
            // Should not happen, since it will not work well as a doc comment,
            // but if it does, just ignore: reformatting it will only make the
            // situation worse.
            return list.to_vec();
        }
        text = text[2..text.len() - 2].to_string(); // cut /* and */
    } else if first.text.starts_with("//") {
        kind = "//";
        let mut b = String::new();
        for c in list {
            let Some(after) = c.text.strip_prefix("//") else {
                return list.to_vec();
            };
            // Accumulate //go:build etc lines separately.
            if is_directive(after) {
                directives.push(c);
                continue;
            }
            b.push_str(after.strip_prefix(' ').unwrap_or(after));
            b.push('\n');
        }
        text = b;
    } else {
        // Not sure what this is, so leave alone.
        return list.to_vec();
    }

    if text.is_empty() {
        return list.to_vec();
    }

    // Parse comment and reformat as text.
    let text = Printer.comment(&Parser::default().parse(&text));

    // For /* */ comment, return one big comment with text inside.
    let slash = first.slash;
    if kind == "/*" {
        return vec![Comment {
            slash,
            text: format!("/*\n{text}*/"),
        }];
    }

    // For // comment, return sequence of // lines.
    let mut out: Vec<Comment> = Vec::new();
    let mut rest: &str = &text;
    while !rest.is_empty() {
        let (line, tail) = match rest.split_once('\n') {
            Some((line, tail)) => (line, tail),
            None => (rest, ""),
        };
        rest = tail;
        let line = if line.is_empty() {
            "//".to_string()
        } else if line.starts_with('\t') {
            format!("//{line}")
        } else {
            format!("// {line}")
        };
        out.push(Comment { slash, text: line });
    }
    if !directives.is_empty() {
        out.push(Comment {
            slash,
            text: "//".to_string(),
        });
        for c in directives {
            out.push(Comment {
                slash,
                text: c.text.clone(),
            });
        }
    }
    out
}

/// `is_directive` reports whether `c` (comment body with `//` stripped)
/// is a Go comment directive. Same rules as [`crate::ast::is_directive`].
pub(crate) fn is_directive(c: &str) -> bool {
    crate::ast::is_directive(c)
}

/// Reports whether `text` is the interior of an old-style `/* */` comment with
/// a star at the start of each line.
pub(crate) fn all_stars(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] != b'*' {
                return false;
            }
        }
        i += 1;
    }
    true
}
