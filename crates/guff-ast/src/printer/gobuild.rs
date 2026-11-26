// Port of Go's go/printer/gobuild.go to Rust.
//
// Original: Copyright 2020 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.

use crate::constraint;
use crate::tabwriter::ESCAPE;

use super::printer::Printer;

impl Printer<'_> {
    pub(crate) fn fix_go_build_lines(&mut self) {
        if self.go_build.is_empty() && self.plus_build.is_empty() {
            return;
        }

        // Find latest possible placement of //go:build and // +build comments.
        let mut insert = 0usize;
        let mut pos = 0usize;
        loop {
            let mut blank = true;
            while pos < self.output.len()
                && (self.output[pos] == b' ' || self.output[pos] == b'\t')
            {
                pos += 1;
            }
            if pos + 3 < self.output.len()
                && self.output[pos] == ESCAPE
                && self.output[pos + 1] == b'/'
                && self.output[pos + 2] == b'/'
            {
                blank = false;
                while pos < self.output.len() && !is_nl(self.output[pos]) {
                    pos += 1;
                }
            }
            if pos >= self.output.len() || !is_nl(self.output[pos]) {
                break;
            }
            pos += 1;
            if blank {
                insert = pos;
            }
        }

        if !self.go_build.is_empty() && self.go_build[0] < insert {
            insert = self.go_build[0];
        } else if !self.plus_build.is_empty() && self.plus_build[0] < insert {
            insert = self.plus_build[0];
        }

        let x: Option<constraint::Expr> = match self.go_build.len() {
            0 => {
                let mut acc: Option<constraint::Expr> = None;
                for &p in &self.plus_build {
                    let text = self.comment_text_at(p);
                    match constraint::parse(&text) {
                        Ok(y) => {
                            acc = Some(match acc {
                                None => y,
                                Some(prev) => constraint::Expr::and(prev, y),
                            });
                        }
                        Err(_) => {
                            acc = None;
                            break;
                        }
                    }
                }
                acc
            }
            1 => constraint::parse(&self.comment_text_at(self.go_build[0])).ok(),
            _ => None,
        };

        let mut block: Vec<u8> = Vec::new();
        match &x {
            None => {
                for &p in &self.go_build {
                    block.extend_from_slice(&self.line_at(p));
                }
                for &p in &self.plus_build {
                    block.extend_from_slice(&self.line_at(p));
                }
            }
            Some(expr) => {
                block.push(ESCAPE);
                block.extend_from_slice(b"//go:build ");
                block.extend_from_slice(expr.to_string().as_bytes());
                block.push(ESCAPE);
                block.push(b'\n');
                if !self.plus_build.is_empty() {
                    let lines = match constraint::plus_build_lines(expr) {
                        Ok(ls) => ls,
                        Err(e) => vec![format!("// +build error: {e}")],
                    };
                    for line in lines {
                        block.push(ESCAPE);
                        block.extend_from_slice(line.as_bytes());
                        block.push(ESCAPE);
                        block.push(b'\n');
                    }
                }
            }
        }
        block.push(b'\n');

        let mut to_delete = self.go_build.clone();
        to_delete.extend_from_slice(&self.plus_build);
        to_delete.sort_unstable();

        let mut after: Vec<u8> = Vec::new();
        let mut start = insert;
        for &end in &to_delete {
            if end < start {
                continue;
            }
            after = append_lines(after, &self.output[start..end]);
            start = end + self.line_at(end).len();
        }
        after = append_lines(after, &self.output[start..]);
        if after.len() >= 2 && is_nl(after[after.len() - 1]) && is_nl(after[after.len() - 2]) {
            after.pop();
        }

        self.output.truncate(insert);
        self.output.extend_from_slice(&block);
        self.output.extend_from_slice(&after);
    }

    fn line_at(&self, start: usize) -> Vec<u8> {
        let mut pos = start;
        while pos < self.output.len() && !is_nl(self.output[pos]) {
            pos += 1;
        }
        if pos < self.output.len() {
            pos += 1;
        }
        self.output[start..pos].to_vec()
    }

    fn comment_text_at(&self, mut start: usize) -> String {
        if start < self.output.len() && self.output[start] == ESCAPE {
            start += 1;
        }
        let mut pos = start;
        while pos < self.output.len() && self.output[pos] != ESCAPE && !is_nl(self.output[pos]) {
            pos += 1;
        }
        String::from_utf8_lossy(&self.output[start..pos]).into_owned()
    }
}

fn append_lines(mut x: Vec<u8>, y: &[u8]) -> Vec<u8> {
    let y = if !y.is_empty()
        && is_nl(y[0])
        && (x.is_empty()
            || (x.len() >= 2 && is_nl(x[x.len() - 1]) && is_nl(x[x.len() - 2])))
    {
        &y[1..]
    } else {
        y
    };
    x.extend_from_slice(y);
    x
}

fn is_nl(b: u8) -> bool {
    b == b'\n' || b == b'\x0c'
}
