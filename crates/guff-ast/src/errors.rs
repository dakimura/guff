// Port of Go's go/scanner/errors.go to Rust.
//
// Original: Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.

use std::cmp::Ordering;
use std::fmt;
use std::io::{self, Write};

use crate::position::Position;

/// In an [`ErrorList`], an error is represented by an `Error`.
/// `pos` (if valid) points to the beginning of the offending token; `msg`
/// describes the error condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub pos: Position,
    pub msg: String,
}

impl Error {
    pub fn new(pos: Position, msg: impl Into<String>) -> Self {
        Error {
            pos,
            msg: msg.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.pos.filename.is_empty() || self.pos.is_valid() {
            write!(f, "{}: {}", self.pos, self.msg)
        } else {
            f.write_str(&self.msg)
        }
    }
}

impl std::error::Error for Error {}

/// `ErrorList` is a list of [`Error`]s. The empty list is ready to use.
#[derive(Debug, Default, Clone)]
pub struct ErrorList(Vec<Error>);

impl ErrorList {
    pub fn new() -> Self {
        ErrorList(Vec::new())
    }

    /// Append an error with the given position and message.
    pub fn add(&mut self, pos: Position, msg: impl Into<String>) {
        self.0.push(Error {
            pos,
            msg: msg.into(),
        });
    }

    /// Drop all errors.
    pub fn reset(&mut self) {
        self.0.clear();
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Error> {
        self.0.iter()
    }

    pub fn as_slice(&self) -> &[Error] {
        &self.0
    }

    /// Sort the list by (filename, line, column, message).
    pub fn sort(&mut self) {
        self.0.sort_by(Self::less);
    }

    fn less(a: &Error, b: &Error) -> Ordering {
        // It is not sufficient to simply compare file offsets because the
        // offsets do not reflect modified line information (through
        // `//line` comments).
        a.pos
            .filename
            .cmp(&b.pos.filename)
            .then_with(|| a.pos.line.cmp(&b.pos.line))
            .then_with(|| a.pos.column.cmp(&b.pos.column))
            .then_with(|| a.msg.cmp(&b.msg))
    }

    /// Sort the list, then keep only the first error per (filename, line).
    pub fn remove_multiples(&mut self) {
        self.sort();
        let mut out: Vec<Error> = Vec::with_capacity(self.0.len());
        let mut last_filename = String::new();
        let mut last_line: i64 = -1;
        let mut have_last = false;
        for e in self.0.drain(..) {
            if !have_last || e.pos.filename != last_filename || e.pos.line != last_line {
                last_filename = e.pos.filename.clone();
                last_line = e.pos.line;
                have_last = true;
                out.push(e);
            }
        }
        self.0 = out;
    }

    /// Convert to a `Result`-friendly error: `None` when empty.
    pub fn err(self) -> Option<Self> {
        if self.0.is_empty() {
            None
        } else {
            Some(self)
        }
    }
}

impl fmt::Display for ErrorList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.len() {
            0 => f.write_str("no errors"),
            1 => write!(f, "{}", self.0[0]),
            n => write!(f, "{} (and {} more errors)", self.0[0], n - 1),
        }
    }
}

impl std::error::Error for ErrorList {}

impl std::ops::Index<usize> for ErrorList {
    type Output = Error;
    fn index(&self, i: usize) -> &Error {
        &self.0[i]
    }
}

impl<'a> IntoIterator for &'a ErrorList {
    type Item = &'a Error;
    type IntoIter = std::slice::Iter<'a, Error>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Print one error per line to `w`. If `err` is an [`ErrorList`] each entry
/// is printed; otherwise the error's `Display` form is printed once.
pub fn print_error<W: Write>(w: &mut W, err: &(dyn std::error::Error + 'static)) -> io::Result<()> {
    // Try to downcast to ErrorList for the bulk-print case.
    if let Some(list) = err.downcast_ref::<ErrorList>() {
        for e in list.iter() {
            writeln!(w, "{}", e)?;
        }
    } else {
        writeln!(w, "{}", err)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(filename: &str, line: i64, column: i64) -> Position {
        Position {
            filename: filename.to_string(),
            offset: 0,
            line,
            column,
        }
    }

    #[test]
    fn error_display_with_position() {
        let e = Error::new(pos("a.go", 2, 3), "bad");
        assert_eq!(e.to_string(), "a.go:2:3: bad");
    }

    #[test]
    fn error_display_without_position_or_filename() {
        let e = Error::new(Position::default(), "lonely");
        assert_eq!(e.to_string(), "lonely");
    }

    #[test]
    fn errorlist_empty() {
        let list = ErrorList::new();
        assert_eq!(list.to_string(), "no errors");
        assert!(list.is_empty());
    }

    #[test]
    fn errorlist_single() {
        let mut list = ErrorList::new();
        list.add(pos("f.go", 1, 1), "boom");
        assert_eq!(list.to_string(), "f.go:1:1: boom");
    }

    #[test]
    fn errorlist_multiple() {
        let mut list = ErrorList::new();
        list.add(pos("f.go", 1, 1), "a");
        list.add(pos("f.go", 2, 1), "b");
        list.add(pos("f.go", 3, 1), "c");
        assert_eq!(list.to_string(), "f.go:1:1: a (and 2 more errors)");
    }

    #[test]
    fn errorlist_sort() {
        let mut list = ErrorList::new();
        list.add(pos("b.go", 1, 1), "y");
        list.add(pos("a.go", 2, 1), "z");
        list.add(pos("a.go", 1, 5), "x");
        list.add(pos("a.go", 1, 1), "w");
        list.sort();
        let got: Vec<&str> = list.iter().map(|e| e.msg.as_str()).collect();
        assert_eq!(got, vec!["w", "x", "z", "y"]);
    }

    #[test]
    fn errorlist_remove_multiples() {
        let mut list = ErrorList::new();
        list.add(pos("a.go", 1, 1), "first");
        list.add(pos("a.go", 1, 5), "second"); // same file+line -> drop
        list.add(pos("a.go", 2, 1), "third");
        list.add(pos("b.go", 1, 1), "fourth");
        list.add(pos("b.go", 1, 9), "fifth"); // same file+line -> drop
        list.remove_multiples();
        let got: Vec<&str> = list.iter().map(|e| e.msg.as_str()).collect();
        assert_eq!(got, vec!["first", "third", "fourth"]);
    }

    #[test]
    fn err_returns_none_when_empty() {
        let list = ErrorList::new();
        assert!(list.err().is_none());
    }

    #[test]
    fn err_returns_some_when_nonempty() {
        let mut list = ErrorList::new();
        list.add(Position::default(), "x");
        assert!(list.err().is_some());
    }

    #[test]
    fn print_error_writes_errorlist() {
        let mut list = ErrorList::new();
        list.add(pos("a.go", 1, 1), "one");
        list.add(pos("a.go", 2, 1), "two");
        let mut buf: Vec<u8> = Vec::new();
        print_error(&mut buf, &list).unwrap();
        let got = String::from_utf8(buf).unwrap();
        assert_eq!(got, "a.go:1:1: one\na.go:2:1: two\n");
    }

    #[test]
    fn print_error_writes_plain_error() {
        let e = Error::new(pos("a.go", 1, 1), "boom");
        let mut buf: Vec<u8> = Vec::new();
        print_error(&mut buf, &e).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "a.go:1:1: boom\n");
    }
}
