// Port of Go's go/printer/comment.go (partial).
//
// Original: Copyright 2022 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
//! STATUS: `format_doc_comment` is currently a no-op (returns the input
//! list). Full parity needs a port of `go/doc/comment` Parser/Printer.
//! On already-gofmt'd corpora this is usually idempotent.

use crate::ast::Comment;

/// Reformats a top-level doc comment list to canonical form.
///
/// Currently returns `list` unchanged — see module STATUS.
pub(crate) fn format_doc_comment(list: &[Comment]) -> Vec<Comment> {
    list.to_vec()
}

/// `is_directive` reports whether `c` (comment body with `//` stripped)
/// is a Go comment directive. Same rules as [`crate::ast::is_directive`].
#[allow(dead_code)]
pub(crate) fn is_directive(c: &str) -> bool {
    crate::ast::is_directive(c)
}

/// Reports whether `text` is an old-style `/* */` comment with a star
/// at the start of each line.
#[allow(dead_code)]
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
