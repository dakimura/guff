//! Port of `internal/gcimporter/bimport.go` (`fakeFileSet`).

use rustc_hash::FxHashMap as HashMap;
use std::sync::Arc;

use guff::position::{FileSet, Pos, NO_POS};

const MAX_LINES: i64 = 64 * 1024;

/// Stride between per-file provisional position blocks. One greater than
/// `MAX_LINES` so a clamped line (`1..=MAX_LINES`) never collides with the next
/// file's block and `prov % STRIDE` decodes the line exactly.
const STRIDE: i64 = MAX_LINES + 1;

struct FileRec {
    name: String,
    last_line: i64,
}

/// Synthesizes [`Pos`] values from filename/line/column without reading sources.
///
/// Positions are handed out during decode as compact *provisional* handles
/// (`file_index * STRIDE + line`) rather than real [`FileSet`] offsets: a
/// file's true size (its maximum line) is unknown until the whole package has
/// been decoded. [`FakeFileSet::finalize`] then registers each file in the
/// shared `FileSet` sized to its actual line count and returns the per-file
/// base offsets; [`FakeFileSet::translate`] maps a provisional handle to the
/// real offset.
///
/// Sizing files exactly — instead of reserving a fixed 64Ki-line block each —
/// keeps the shared `FileSet`'s offset space small enough that the `u32`
/// positions stored on type objects (`ObjectMeta::pos`) don't overflow on
/// large multi-package runs. Reserving 64Ki lines per imported dependency file
/// pushed the shared fset's base past `u32::MAX` on Prometheus, truncating the
/// positions of source files parsed afterwards so their diagnostics mapped onto
/// unrelated dependency files (R25.2 — see docs/DEVELOPMENT.md §8 R25).
pub struct FakeFileSet {
    fset: Arc<FileSet>,
    index: HashMap<String, usize>,
    files: Vec<FileRec>,
}

impl FakeFileSet {
    pub fn new(fset: Arc<FileSet>) -> Self {
        Self {
            fset,
            index: HashMap::default(),
            files: Vec::new(),
        }
    }

    /// Returns a provisional position handle for `file:line`, or `0` (`nopos`)
    /// for an empty filename. The handle is only meaningful until [`finalize`]
    /// and [`translate`] convert it into a real `FileSet` offset.
    ///
    /// [`finalize`]: FakeFileSet::finalize
    /// [`translate`]: FakeFileSet::translate
    pub fn pos(&mut self, file: &str, line: i32, _column: i32) -> u32 {
        if file.is_empty() {
            return 0;
        }
        let mut line = i64::from(line);
        if line < 1 || line > MAX_LINES {
            line = 1;
        }

        let idx = match self.index.get(file) {
            Some(&i) => i,
            None => {
                let i = self.files.len();
                self.index.insert(file.to_string(), i);
                self.files.push(FileRec {
                    name: file.to_string(),
                    last_line: 0,
                });
                i
            }
        };
        let rec = &mut self.files[idx];
        if line > rec.last_line {
            rec.last_line = line;
        }
        (idx as i64 * STRIDE + line) as u32
    }

    /// Register every seen file in the shared `FileSet`, each sized to its
    /// actual maximum line, and return the per-file base offsets indexed by the
    /// file index encoded into provisional handles.
    pub fn finalize(&self) -> Vec<i64> {
        let mut bases = Vec::with_capacity(self.files.len());
        for rec in &self.files {
            let n = rec.last_line.max(1);
            let file = self.fset.add_file(&rec.name, -1, n);
            // Each synthetic line occupies one byte, so line `k` starts at
            // offset `k - 1`; `set_lines` requires every entry `< size` (`n`).
            file.set_lines((0..n).collect());
            bases.push(file.base());
        }
        bases
    }

    /// Convert a provisional handle from [`pos`] into a real `FileSet` offset,
    /// using the `bases` returned by [`finalize`].
    ///
    /// [`pos`]: FakeFileSet::pos
    /// [`finalize`]: FakeFileSet::finalize
    pub fn translate(&self, bases: &[i64], prov: u32) -> u32 {
        if prov == 0 {
            return 0;
        }
        let prov = i64::from(prov);
        let idx = (prov / STRIDE) as usize;
        let line = prov % STRIDE;
        (bases[idx] + line - 1) as u32
    }

    pub fn pos_from_u32(&self, p: u32) -> Pos {
        if p == 0 {
            NO_POS
        } else {
            Pos(i64::from(p))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// After finalize+translate, a provisional handle resolves to the exact
    /// file:line it was created for. Column is always 1 (imports don't preserve
    /// columns). `0` (nopos) round-trips as `0`.
    #[test]
    fn translate_resolves_to_correct_file_line() {
        let fset = FileSet::new();
        let mut fake = FakeFileSet::new(fset.clone());

        let a1 = fake.pos("a.go", 3, 0);
        let b1 = fake.pos("b.go", 100, 0);
        let a2 = fake.pos("a.go", 42, 0);
        let none = fake.pos("", 5, 0);
        assert_eq!(none, 0);

        let bases = fake.finalize();
        for (prov, name, line) in [(a1, "a.go", 3), (b1, "b.go", 100), (a2, "a.go", 42)] {
            let real = fake.translate(&bases, prov);
            let pos = fset.position(Pos(i64::from(real)));
            assert_eq!(pos.filename, name);
            assert_eq!(pos.line, line, "line for {name}");
            assert_eq!(pos.column, 1, "column for {name}");
        }
        assert_eq!(fake.translate(&bases, 0), 0);
    }

    /// R25.2: reserving a fixed 64Ki-line block per file pushed the shared
    /// `FileSet` base past `u32::MAX`, truncating later positions. Sizing files
    /// to their actual line count keeps the base compact even with many files,
    /// so every real position fits in the `u32` slot type objects use.
    #[test]
    fn many_small_files_stay_within_u32() {
        let fset = FileSet::new();
        let mut fake = FakeFileSet::new(fset.clone());

        let n_files = 100_000;
        let mut handles = Vec::with_capacity(n_files);
        for i in 0..n_files {
            // Each file uses only a handful of lines — the common case that the
            // old fixed 64Ki reservation wasted ~99% of.
            handles.push((format!("f{i}.go"), fake.pos(&format!("f{i}.go"), 7, 0)));
        }

        let bases = fake.finalize();
        // Sized-to-actual: base stays far below u32::MAX (fixed 64Ki blocks
        // would have reached ~6.5G here).
        assert!(fset.base() < u32::MAX as i64, "fset base overflowed u32");

        for (name, prov) in handles.iter().take(1000) {
            let real = fake.translate(&bases, *prov);
            // The real offset must fit in u32 (the width of `ObjectMeta::pos`).
            let pos = fset.position(Pos(i64::from(real)));
            assert_eq!(&pos.filename, name);
            assert_eq!(pos.line, 7);
        }
    }
}
