// Port of Go's go/doc/comment/print.go — the `Comment` printer only.
//
// Original: Copyright 2022 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.

use super::{Block, Doc, Text};

/// A doc comment printer.
///
/// Upstream's `Printer` carries configuration for the HTML, Markdown and text
/// renderers; [`Printer::comment`] reads none of it, so the ported struct is
/// empty. It exists so the call shape matches `go/printer`'s
/// `var pr comment.Printer; pr.Comment(d)`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Printer;

impl Printer {
    /// The standard Go formatting of the [`Doc`], without any comment markers.
    pub fn comment(&self, d: &Doc) -> String {
        let mut out = String::new();
        for (i, x) in d.content.iter().enumerate() {
            if i > 0 && blank_before(x) {
                out.push('\n');
            }
            block(&mut out, x);
        }

        // Print one block containing all the link definitions that were used,
        // and then a second block containing all the unused ones. This makes
        // it easy to clean up the unused ones: gofmt and delete the final
        // block. And it's a nice visual signal without affecting the way the
        // comment formats for users.
        for i in 0..2 {
            let used = i == 0;
            let mut first = true;
            for def in &d.links {
                if def.used == used {
                    if first {
                        out.push('\n');
                        first = false;
                    }
                    out.push('[');
                    out.push_str(&def.text);
                    out.push_str("]: ");
                    out.push_str(&def.url);
                    out.push('\n');
                }
            }
        }

        out
    }
}

/// Reports whether the block `x` requires a blank line before it. All blocks
/// do, except for lists that return false from [`List::blank_before`].
///
/// [`List::blank_before`]: super::List::blank_before
fn blank_before(x: &Block) -> bool {
    match x {
        Block::List(l) => l.blank_before(),
        _ => true,
    }
}

/// Prints the block `x` to `out`.
fn block(out: &mut String, x: &Block) {
    match x {
        Block::Paragraph(p) => {
            text(out, "", &p.text);
            out.push('\n');
        }

        Block::Heading(h) => {
            out.push_str("# ");
            text(out, "", &h.text);
            out.push('\n');
        }

        Block::Code(c) => {
            let mut md: &str = &c.text;
            while !md.is_empty() {
                let (line, rest) = match md.split_once('\n') {
                    Some((line, rest)) => (line, rest),
                    None => (md, ""),
                };
                if !line.is_empty() {
                    out.push('\t');
                    out.push_str(line);
                }
                out.push('\n');
                md = rest;
            }
        }

        Block::List(l) => {
            let loose = l.blank_between();
            for (i, item) in l.items.iter().enumerate() {
                if i > 0 && loose {
                    out.push('\n');
                }
                out.push(' ');
                if item.number.is_empty() {
                    out.push_str(" - ");
                } else {
                    out.push_str(&item.number);
                    out.push_str(". ");
                }
                for (i, blk) in item.content.iter().enumerate() {
                    const FOUR_SPACE: &str = "    ";
                    if i > 0 {
                        out.push('\n');
                        out.push_str(FOUR_SPACE);
                    }
                    let Block::Paragraph(p) = blk else {
                        unreachable!("list item content is always a paragraph")
                    };
                    text(out, FOUR_SPACE, &p.text);
                    out.push('\n');
                }
            }
        }
    }
}

/// Prints the text sequence `x` to `out`.
fn text(out: &mut String, ind: &str, x: &[Text]) {
    for t in x {
        match t {
            Text::Plain(s) => indent(out, ind, s),
            Text::Italic(s) => indent(out, ind, s),
            Text::Link(l) => {
                if l.auto {
                    text(out, ind, &l.text);
                } else {
                    out.push('[');
                    text(out, ind, &l.text);
                    out.push(']');
                }
            }
            Text::DocLink(l) => {
                out.push('[');
                text(out, ind, &l.text);
                out.push(']');
            }
        }
    }
}

/// Prints `s` to `out`, indenting with `ind` after each newline in `s`.
fn indent(out: &mut String, ind: &str, s: &str) {
    let mut s = s;
    while !s.is_empty() {
        match s.split_once('\n') {
            Some((line, rest)) => {
                out.push_str(line);
                out.push('\n');
                out.push_str(ind);
                s = rest;
            }
            None => {
                out.push_str(s);
                s = "";
            }
        }
    }
}
