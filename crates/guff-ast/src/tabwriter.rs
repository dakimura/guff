// Port of Go's text/tabwriter package to Rust.
//
// Original: Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// The package is frozen in Go and is not accepting new features. This
// port mirrors `$(go env GOROOT)/src/text/tabwriter/tabwriter.go`
// closely so go/printer alignment matches system gofmt.

use std::io::{self, Write};

/// A cell represents a segment of text terminated by tabs or line breaks.
#[derive(Clone, Copy, Debug, Default)]
struct Cell {
    size: usize,  // cell size in bytes
    width: usize, // cell width in runes
    htab: bool,   // true if the cell is terminated by an htab ('\t')
}

/// Filter flags controlling [`Writer`] formatting.
pub type Flags = u32;

/// Ignore HTML tags and treat entities (starting with `&` and ending in
/// `;`) as single characters (width = 1).
pub const FILTER_HTML: Flags = 1 << 0;
/// Strip Escape characters bracketing escaped text segments instead of
/// passing them through unchanged with the text.
pub const STRIP_ESCAPE: Flags = 1 << 1;
/// Force right-alignment of cell content. Default is left-alignment.
pub const ALIGN_RIGHT: Flags = 1 << 2;
/// Handle empty columns as if they were not present in the input.
pub const DISCARD_EMPTY_COLUMNS: Flags = 1 << 3;
/// Always use tabs for indentation columns (padding of leading empty
/// cells on the left) independent of padchar.
pub const TAB_INDENT: Flags = 1 << 4;
/// Print a vertical bar (`|`) between columns (after formatting).
pub const DEBUG: Flags = 1 << 5;

/// To escape a text segment, bracket it with Escape characters.
///
/// The value 0xff was chosen because it cannot appear in a valid UTF-8
/// sequence.
pub const ESCAPE: u8 = 0xff;

/// A [`Writer`] is a filter that inserts padding around tab-delimited
/// columns in its input to align them in the output.
///
/// See the Go `text/tabwriter` package docs for the full algorithm.
pub struct Writer<W: Write> {
    output: W,
    minwidth: usize,
    tabwidth: usize,
    padding: usize,
    padbytes: [u8; 8],
    flags: Flags,

    buf: Vec<u8>,
    pos: usize,
    cell: Cell,
    end_char: u8,
    lines: Vec<Vec<Cell>>,
    widths: Vec<usize>,
}

impl<W: Write> Writer<W> {
    /// Allocate and initialize a new [`Writer`].
    pub fn new(
        output: W,
        minwidth: usize,
        tabwidth: usize,
        padding: usize,
        padchar: u8,
        flags: Flags,
    ) -> Self {
        let mut w = Self {
            output,
            minwidth: 0,
            tabwidth: 0,
            padding: 0,
            padbytes: [0; 8],
            flags: 0,
            buf: Vec::new(),
            pos: 0,
            cell: Cell::default(),
            end_char: 0,
            lines: Vec::new(),
            widths: Vec::new(),
        };
        w.init(minwidth, tabwidth, padding, padchar, flags);
        w
    }

    /// Re-initialize formatting parameters (keeps the underlying writer).
    pub fn init(
        &mut self,
        minwidth: usize,
        tabwidth: usize,
        padding: usize,
        padchar: u8,
        mut flags: Flags,
    ) {
        self.minwidth = minwidth;
        self.tabwidth = tabwidth;
        self.padding = padding;
        self.padbytes = [padchar; 8];
        if padchar == b'\t' {
            flags &= !ALIGN_RIGHT;
        }
        self.flags = flags;
        self.reset();
    }

    fn add_line(&mut self, flushed: bool) {
        // Grow like Go: reuse capacity when available so we can clear an
        // existing []cell slot instead of always allocating.
        if self.lines.len() < self.lines.capacity() {
            self.lines.push(Vec::new());
            let last = self.lines.len() - 1;
            // If we pushed into reserved capacity that already held a Vec
            // from a previous reset cycle, clear it — but `push` on a
            // truncated Vec always creates a fresh empty Vec, so clear is
            // a no-op. Kept for parity with Go's `lines[n-1] = lines[n-1][:0]`.
            self.lines[last].clear();
        } else {
            self.lines.push(Vec::new());
        }

        if !flushed {
            let n = self.lines.len();
            if n >= 2 {
                let prev = self.lines[n - 2].len();
                if prev > self.lines[n - 1].capacity() {
                    self.lines[n - 1] = Vec::with_capacity(prev);
                }
            }
        }
    }

    fn reset(&mut self) {
        self.buf.clear();
        self.pos = 0;
        self.cell = Cell::default();
        self.end_char = 0;
        self.lines.clear();
        self.widths.clear();
        self.add_line(true);
    }

    fn write0(&mut self, buf: &[u8]) -> io::Result<()> {
        self.output.write_all(buf)
    }

    fn write_n(&mut self, src: &[u8], mut n: usize) -> io::Result<()> {
        while n > src.len() {
            self.write0(src)?;
            n -= src.len();
        }
        self.write0(&src[..n])
    }

    fn write_padding(&mut self, textw: usize, mut cellw: usize, use_tabs: bool) -> io::Result<()> {
        if self.padbytes[0] == b'\t' || use_tabs {
            if self.tabwidth == 0 {
                return Ok(());
            }
            cellw = (cellw + self.tabwidth - 1) / self.tabwidth * self.tabwidth;
            let n = cellw.checked_sub(textw).expect("tabwriter: internal error");
            const TABS: &[u8] = b"\t\t\t\t\t\t\t\t";
            self.write_n(TABS, (n + self.tabwidth - 1) / self.tabwidth)
        } else {
            let pad = self.padbytes;
            self.write_n(&pad, cellw - textw)
        }
    }

    fn write_lines(&mut self, mut pos: usize, line0: usize, line1: usize) -> io::Result<usize> {
        for i in line0..line1 {
            let line_len = self.lines[i].len();
            let mut use_tabs = self.flags & TAB_INDENT != 0;

            for j in 0..line_len {
                let c = self.lines[i][j];
                if j > 0 && self.flags & DEBUG != 0 {
                    self.write0(b"|")?;
                }

                if c.size == 0 {
                    if j < self.widths.len() {
                        let w = self.widths[j];
                        self.write_padding(c.width, w, use_tabs)?;
                    }
                } else {
                    use_tabs = false;
                    if self.flags & ALIGN_RIGHT == 0 {
                        let end = pos + c.size;
                        let chunk = self.buf[pos..end].to_vec();
                        self.write0(&chunk)?;
                        pos = end;
                        if j < self.widths.len() {
                            let w = self.widths[j];
                            self.write_padding(c.width, w, false)?;
                        }
                    } else {
                        if j < self.widths.len() {
                            let w = self.widths[j];
                            self.write_padding(c.width, w, false)?;
                        }
                        let end = pos + c.size;
                        let chunk = self.buf[pos..end].to_vec();
                        self.write0(&chunk)?;
                        pos = end;
                    }
                }
            }

            if i + 1 == self.lines.len() {
                let end = pos + self.cell.size;
                let chunk = self.buf[pos..end].to_vec();
                self.write0(&chunk)?;
                pos = end;
            } else {
                self.write0(b"\n")?;
            }
        }
        Ok(pos)
    }

    fn format(&mut self, mut pos0: usize, mut line0: usize, line1: usize) -> io::Result<usize> {
        // Mirror Go's recursive column-block formatter exactly.
        let column = self.widths.len();
        let mut this = line0;
        while this < line1 {
            // cell exists in this column ⇒ line has more cells than the
            // previous line (last cell per line is ignored; see Go docs).
            if column >= self.lines[this].len().saturating_sub(1) {
                this += 1;
                continue;
            }

            pos0 = self.write_lines(pos0, line0, this)?;
            line0 = this;

            let mut width = self.minwidth;
            let mut discardable = true;
            while this < line1 {
                if column >= self.lines[this].len().saturating_sub(1) {
                    break;
                }
                let c = self.lines[this][column];
                let w = c.width + self.padding;
                if w > width {
                    width = w;
                }
                if c.width > 0 || c.htab {
                    discardable = false;
                }
                this += 1;
            }

            if discardable && self.flags & DISCARD_EMPTY_COLUMNS != 0 {
                width = 0;
            }

            self.widths.push(width);
            pos0 = self.format(pos0, line0, this)?;
            self.widths.pop();
            line0 = this;
        }
        self.write_lines(pos0, line0, line1)
    }

    fn append(&mut self, text: &[u8]) {
        self.buf.extend_from_slice(text);
        self.cell.size += text.len();
    }

    fn update_width(&mut self) {
        self.cell.width += rune_count(&self.buf[self.pos..]);
        self.pos = self.buf.len();
    }

    fn start_escape(&mut self, ch: u8) {
        match ch {
            ESCAPE => self.end_char = ESCAPE,
            b'<' => self.end_char = b'>',
            b'&' => self.end_char = b';',
            _ => {}
        }
    }

    fn end_escape(&mut self) {
        match self.end_char {
            ESCAPE => {
                self.update_width();
                if self.flags & STRIP_ESCAPE == 0 {
                    self.cell.width = self.cell.width.saturating_sub(2);
                }
            }
            b'>' => {}
            b';' => {
                self.cell.width += 1;
            }
            _ => {}
        }
        self.pos = self.buf.len();
        self.end_char = 0;
    }

    fn terminate_cell(&mut self, htab: bool) -> usize {
        self.cell.htab = htab;
        let cell = self.cell;
        let last = self.lines.len() - 1;
        self.lines[last].push(cell);
        self.cell = Cell::default();
        self.lines[last].len()
    }

    /// Flush buffered data to the underlying writer.
    pub fn flush(&mut self) -> io::Result<()> {
        self.flush_no_defers()
    }

    fn flush_no_defers(&mut self) -> io::Result<()> {
        if self.cell.size > 0 {
            if self.end_char != 0 {
                self.end_escape();
            }
            self.terminate_cell(false);
        }
        let nlines = self.lines.len();
        self.format(0, 0, nlines)?;
        self.reset();
        Ok(())
    }

    /// Write `buf` through the tabwriter filter.
    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut n = 0usize;
        for (i, &ch) in buf.iter().enumerate() {
            if self.end_char == 0 {
                match ch {
                    b'\t' | b'\x0b' | b'\n' | b'\x0c' => {
                        self.append(&buf[n..i]);
                        self.update_width();
                        n = i + 1;
                        let ncells = self.terminate_cell(ch == b'\t');
                        if ch == b'\n' || ch == b'\x0c' {
                            self.add_line(ch == b'\x0c');
                            if ch == b'\x0c' || ncells == 1 {
                                self.flush_no_defers()?;
                                if ch == b'\x0c' && self.flags & DEBUG != 0 {
                                    self.write0(b"---\n")?;
                                }
                            }
                        }
                    }
                    ESCAPE => {
                        self.append(&buf[n..i]);
                        self.update_width();
                        n = i;
                        if self.flags & STRIP_ESCAPE != 0 {
                            n += 1;
                        }
                        self.start_escape(ESCAPE);
                    }
                    b'<' | b'&' => {
                        if self.flags & FILTER_HTML != 0 {
                            self.append(&buf[n..i]);
                            self.update_width();
                            n = i;
                            self.start_escape(ch);
                        }
                    }
                    _ => {}
                }
            } else if ch == self.end_char {
                let mut j = i + 1;
                if ch == ESCAPE && self.flags & STRIP_ESCAPE != 0 {
                    j = i;
                }
                self.append(&buf[n..j]);
                n = i + 1;
                self.end_escape();
            }
        }
        self.append(&buf[n..]);
        Ok(buf.len())
    }

    /// Consume the writer, flushing first, and return the inner `W`.
    pub fn into_inner(mut self) -> io::Result<W> {
        self.flush()?;
        Ok(self.output)
    }
}

impl<W: Write> Write for Writer<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Writer::write(self, buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Writer::flush(self)
    }
}

fn rune_count(buf: &[u8]) -> usize {
    std::str::from_utf8(buf)
        .map(|s| s.chars().count())
        .unwrap_or_else(|_| {
            // Match Go's utf8.RuneCount on invalid UTF-8: count runes with
            // replacement for bad sequences. Go counts each bad byte as one
            // rune via utf8.DecodeRune.
            let mut i = 0;
            let mut n = 0;
            while i < buf.len() {
                match std::str::from_utf8(&buf[i..]) {
                    Ok(s) => {
                        n += s.chars().count();
                        break;
                    }
                    Err(e) => {
                        if e.valid_up_to() > 0 {
                            n += std::str::from_utf8(&buf[i..i + e.valid_up_to()])
                                .unwrap()
                                .chars()
                                .count();
                            i += e.valid_up_to();
                        }
                        // one bad byte (or incomplete sequence) → one rune
                        let skip = e.error_len().unwrap_or(1);
                        n += 1;
                        i += skip;
                    }
                }
            }
            n
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(
        minwidth: usize,
        tabwidth: usize,
        padding: usize,
        padchar: u8,
        flags: Flags,
        src: &[u8],
        expected: &[u8],
    ) {
        // all at once
        let mut buf = Vec::new();
        {
            let mut w = Writer::new(&mut buf, minwidth, tabwidth, padding, padchar, flags);
            w.write(src).unwrap();
            w.flush().unwrap();
        }
        assert_eq!(
            buf, expected,
            "all-at-once\nsrc={src:?}\ngot={buf:?}\nwant={expected:?}"
        );

        // byte-by-byte
        buf.clear();
        {
            let mut w = Writer::new(&mut buf, minwidth, tabwidth, padding, padchar, flags);
            for i in 0..src.len() {
                w.write(&src[i..i + 1]).unwrap();
            }
            w.flush().unwrap();
        }
        assert_eq!(buf, expected, "byte-by-byte");
    }

    #[test]
    fn empty() {
        check(8, 0, 1, b'.', 0, b"", b"");
    }

    #[test]
    fn escape_passthrough() {
        check(8, 0, 1, b'.', 0, b"\xff\t\xff", b"\xff\t\xff");
    }

    #[test]
    fn escape_stripped() {
        check(8, 0, 1, b'.', STRIP_ESCAPE, b"\xff\t\xff", b"\t");
    }

    #[test]
    fn newlines() {
        check(8, 0, 1, b'.', 0, b"\n\n\n", b"\n\n\n");
    }

    #[test]
    fn plain_lines() {
        check(8, 0, 1, b'.', 0, b"a\nb\nc", b"a\nb\nc");
    }

    #[test]
    fn columns() {
        check(8, 0, 1, b'.', 0, b"a\tb\nc\td\n", b"a.......b\nc.......d\n");
    }

    #[test]
    fn align_right() {
        check(
            8,
            0,
            1,
            b'.',
            ALIGN_RIGHT,
            b"a\tb\nc\td\n",
            b".......ab\n.......cd\n",
        );
    }

    #[test]
    fn formfeed() {
        check(8, 8, 1, b'.', 0, b"a\tb\x0cc\td\n", b"a.......b\nc.......d\n");
    }

    #[test]
    fn vtab_discard() {
        check(
            8,
            8,
            1,
            b'.',
            DISCARD_EMPTY_COLUMNS,
            b"a\x0bb\nc\x0bd\n",
            b"a.......b\nc.......d\n",
        );
    }

    #[test]
    fn gofmt_like() {
        check(
            0,
            8,
            1,
            b' ',
            TAB_INDENT | DISCARD_EMPTY_COLUMNS,
            b"\tx\ty\n\txx\tyy\n",
            b"\tx  y\n\txx yy\n",
        );
    }

    #[test]
    fn gofmt_like2() {
        check(
            0,
            8,
            1,
            b' ',
            TAB_INDENT | DISCARD_EMPTY_COLUMNS,
            b"a\tb\tc\naa\tbb\tcc\n",
            b"a  b  c\naa bb cc\n",
        );
    }

    #[test]
    fn debug_bars() {
        check(
            8,
            0,
            1,
            b'.',
            DEBUG,
            b"a\tb\nc\td\n",
            b"a.......|b\nc.......|d\n",
        );
    }
}
