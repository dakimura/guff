// Port of Go's `go/doc/comment` package.
//
// Original: Copyright 2022 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
//! # STATUS
//!
//! Ported: the document model (`Doc`/`Block`/`Text`), [`Parser::parse`]
//! (`parse.go`) and [`Printer::comment`] (`print.go`). Together those are the
//! whole of what `go/printer`'s `formatDocComment` calls, which is why they
//! are here: gofmt rewrites every doc comment through this round trip, so
//! without it `guff fmt` leaves `//Foo` alone where gofmt writes `// Foo`.
//!
//! Not ported: `Printer::Text` / `HTML` / `Markdown` (`text.go`, `html.go`,
//! `markdown.go`), and with them the Hirschberg–Larmore line wrapper. Nothing
//! on the gofmt path reaches them — `formatDocComment` calls `Comment` and
//! only `Comment`, and unlike `Text` it does not wrap. That is a property of
//! `go/printer/comment.go`, not a judgement call, so it is stated here rather
//! than left as a to-do.

mod parse;
mod print;
mod std_pkgs;
mod unicode_tables;


pub use parse::{default_lookup_package, Parser};
pub use print::Printer;

/// A parsed Go doc comment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Doc {
    /// The sequence of content blocks in the comment.
    pub content: Vec<Block>,

    /// The link definitions in the comment.
    pub links: Vec<LinkDef>,
}

/// A single link definition (`[text]: url`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkDef {
    /// The link text.
    pub text: String,
    /// The link URL.
    pub url: String,
    /// Whether the comment uses the definition.
    pub used: bool,
}

/// Block-level content in a doc comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Paragraph),
    Heading(Heading),
    Code(Code),
    List(List),
}

/// A doc comment heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub text: Vec<Text>,
}

/// A paragraph of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paragraph {
    pub text: Vec<Text>,
}

/// A preformatted code block.
///
/// `text` ends with a newline, is never empty, and neither starts nor ends
/// with a blank line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    pub text: String,
}

/// A numbered or bullet list. Lists are always non-empty.
///
/// In a numbered list every item's `number` is a non-empty decimal string; in
/// a bullet list every item's `number` is empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {
    pub items: Vec<ListItem>,

    /// Forces a blank line before the list when reformatting, overriding the
    /// usual conditions. The parser sets it for any list preceded by a blank
    /// line, so that the blank line survives printing.
    pub force_blank_before: bool,

    /// Forces blank lines between items when reformatting. The parser sets it
    /// for any list that has a blank line between two of its items.
    pub force_blank_between: bool,
}

/// A single item in a numbered or bullet list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    /// `"1"`, `"2"`, … for a numbered list; empty for a bullet list.
    pub number: String,

    /// The item content. The parser and printer both require every element to
    /// be a [`Block::Paragraph`].
    pub content: Vec<Block>,
}

impl List {
    /// Reports whether a reformatting should include a blank line before the
    /// list. Same default rule as [`List::blank_between`].
    pub fn blank_before(&self) -> bool {
        self.force_blank_before || self.blank_between()
    }

    /// Reports whether a reformatting should include a blank line between
    /// each pair of list items: if any item has multiple paragraphs, then the
    /// items must themselves be separated by blank lines.
    pub fn blank_between(&self) -> bool {
        if self.force_blank_between {
            return true;
        }
        self.items.iter().any(|item| item.content.len() != 1)
    }
}

/// Text-level content in a doc comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Text {
    /// Rendered as plain text.
    Plain(String),
    /// Rendered as italicized text.
    Italic(String),
    /// A link to a specific URL.
    Link(Link),
    /// A link to documentation for a Go package or symbol.
    DocLink(DocLink),
}

/// A link to a specific URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Whether this is an automatic (implicit) link of a literal URL.
    pub auto: bool,
    pub text: Vec<Text>,
    pub url: String,
}

/// A link to documentation for a Go package or symbol.
///
/// The combinations of non-empty fields are:
/// - `import_path`: a link to another package
/// - `import_path`, `name`: a const, func, type or var in another package
/// - `import_path`, `recv`, `name`: a method in another package
/// - `name`: a const, func, type or var in this package
/// - `recv`, `name`: a method in this package
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocLink {
    pub text: Vec<Text>,
    pub import_path: String,
    /// Receiver type, without any pointer star, for methods.
    pub recv: String,
    pub name: String,
}
