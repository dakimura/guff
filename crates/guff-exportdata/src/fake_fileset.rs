//! Port of `internal/gcimporter/bimport.go` (`fakeFileSet`).

use std::collections::HashMap;
use std::sync::Arc;

use guff::position::{FileSet, Pos, NO_POS};

const MAX_LINES: i64 = 64 * 1024;

struct FileInfo {
    file: Arc<guff::position::File>,
    last_line: i64,
}

/// Synthesizes [`Pos`] values from filename/line/column without reading sources.
pub struct FakeFileSet {
    fset: Arc<FileSet>,
    files: HashMap<String, FileInfo>,
}

impl FakeFileSet {
    pub fn new(fset: Arc<FileSet>) -> Self {
        Self {
            fset,
            files: HashMap::new(),
        }
    }

    pub fn pos(&mut self, file: &str, line: i32, _column: i32) -> u32 {
        if file.is_empty() {
            return 0;
        }
        let mut line = i64::from(line);
        if line > MAX_LINES {
            line = 1;
        }

        if !self.files.contains_key(file) {
            let arc_file = self.fset.add_file(file, -1, MAX_LINES);
            self.files.insert(
                file.to_string(),
                FileInfo {
                    file: arc_file,
                    last_line: 0,
                },
            );
        }
        let info = self.files.get_mut(file).expect("file inserted");
        if line > info.last_line {
            info.last_line = line;
        }
        let base = info.file.base();
        (base + line - 1) as u32
    }

    pub fn set_lines(&self) {
        let lines: Vec<i64> = (0..MAX_LINES).collect();
        for info in self.files.values() {
            let n = info.last_line as usize;
            if n > 0 {
                info.file.set_lines(lines[..n].to_vec());
            }
        }
    }

    pub fn pos_from_u32(&self, p: u32) -> Pos {
        if p == 0 {
            NO_POS
        } else {
            Pos(i64::from(p))
        }
    }
}
