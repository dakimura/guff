// Port of Go's go/token/position.go to Rust.
//
// Original: Copyright 2010 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// `Pos`, `Position`, `File`, and `FileSet` mirror the Go API, with a
// few Rust-idiomatic adjustments:
//
// * `*File` becomes `Arc<File>`; identity comparisons use
//   `Arc::ptr_eq`.
// * The per-file mutex and the FileSet-wide locks are real
//   `std::sync::Mutex` / `RwLock`s.
// * The "last looked-up file" cache is **per thread** rather than Go's
//   process-wide `atomic.Pointer[File]` (PERF_TASKS_V3 V1-7). It used to be a
//   `Mutex<Option<Arc<File>>>` on the `FileSet`, which every `position()` call
//   locked — and `position()` is called from every analyzer on every
//   diagnostic and every source lookup. On prometheus `./...` that single
//   mutex cost **2.9s of the 20.3s profile** in `__psynch_mutexwait` +
//   `__psynch_mutexdrop`: ten rayon workers queueing on one lock. It was also
//   a *single* slot shared by all of them, so workers on different files
//   evicted each other's entry and the "fast path" mostly missed. Per thread
//   it is both uncontended and actually warm, since a worker runs one action
//   at a time.

use std::cell::RefCell;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use crate::tree::{file_key, Key, Tree};

/// `Position` describes an arbitrary source position including the
/// file, line, and column. Valid when `line > 0`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Position {
    pub filename: String,
    pub offset: i64,
    pub line: i64,
    pub column: i64,
}

impl Position {
    pub fn is_valid(&self) -> bool {
        self.line > 0
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = self.filename.clone();
        if self.is_valid() {
            if !s.is_empty() {
                s.push(':');
            }
            s.push_str(&self.line.to_string());
            if self.column != 0 {
                s.push(':');
                s.push_str(&self.column.to_string());
            }
        }
        if s.is_empty() {
            s.push('-');
        }
        f.write_str(&s)
    }
}

/// `Pos` is a compact source position handle within a `FileSet`.
///
/// `NO_POS` (the zero value) represents "no position" and is always
/// less than any real position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Pos(pub i64);

/// The zero `Pos` value. `NO_POS.is_valid()` is false.
pub const NO_POS: Pos = Pos(0);

impl Pos {
    pub fn is_valid(self) -> bool {
        self != NO_POS
    }
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Alternative file/line/column information for a given offset (set by
/// `//line` directives or `AddLineColumnInfo`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineInfo {
    pub offset: i64,
    pub filename: String,
    pub line: i64,
    pub column: i64,
}

/// Per-file mutable state. Guarded by `File.mutable`.
#[derive(Debug, Default)]
struct FileMutable {
    lines: Vec<i64>,
    infos: Vec<LineInfo>,
}

/// A `File` is a handle for a file belonging to a `FileSet`.
///
/// `File` values are typically shared as `Arc<File>`; internal mutable state is
/// protected by an `RwLock`. `name`, `base`, and `size` are immutable once the
/// file is added to a `FileSet`.
///
/// The lock is an `RwLock` rather than a `Mutex` because the access pattern is
/// write-once-then-read-forever: `add_line` fills the table while the file is
/// being parsed, and every `position()` / `line_for()` afterwards only reads it.
/// Under a `Mutex` those reads serialized across rayon workers sharing a
/// dependency's `File` (PERF_TASKS_V3 V1-7).
#[derive(Debug)]
pub struct File {
    name: String,
    base: i64,
    size: i64,
    mutable: RwLock<FileMutable>,
}

impl File {
    /// Internal constructor. `FileSet::add_file` is the normal entry point.
    pub(crate) fn new(name: String, base: i64, size: i64) -> Arc<Self> {
        Arc::new(File {
            name,
            base,
            size,
            mutable: RwLock::new(FileMutable {
                lines: vec![0],
                infos: Vec::new(),
            }),
        })
    }

    /// Build a File whose mutable state matches a deserialized record.
    pub(crate) fn from_serialized(
        name: String,
        base: i64,
        size: i64,
        lines: Vec<i64>,
        infos: Vec<LineInfo>,
    ) -> Arc<Self> {
        Arc::new(File {
            name,
            base,
            size,
            mutable: RwLock::new(FileMutable { lines, infos }),
        })
    }

    /// Construct a bare File with arbitrary base/size, for use in tree
    /// unit tests that need to bypass FileSet bookkeeping.
    #[cfg(test)]
    pub(crate) fn new_for_test(name: String, base: i64, size: i64) -> Arc<Self> {
        File::new(name, base, size)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn base(&self) -> i64 {
        self.base
    }

    pub fn size(&self) -> i64 {
        self.size
    }

    /// End returns the end position of the file (= `Pos(base+size)`).
    pub fn end(&self) -> Pos {
        Pos(self.base + self.size)
    }

    /// Plain-int form of [`File::end`], handy for diagnostics.
    pub(crate) fn end_pos(&self) -> i64 {
        self.base + self.size
    }

    pub fn line_count(&self) -> usize {
        self.mutable.read().unwrap().lines.len()
    }

    /// Add a line offset for a new line. Ignored if the offset is not
    /// strictly greater than the previous line offset or `>= size`.
    pub fn add_line(&self, offset: i64) {
        let mut m = self.mutable.write().unwrap();
        let i = m.lines.len();
        if (i == 0 || m.lines[i - 1] < offset) && offset < self.size {
            m.lines.push(offset);
        }
    }

    /// Merge `line` with the following line. Panics on invalid input.
    pub fn merge_line(&self, line: usize) {
        assert!(line >= 1, "invalid line number {} (should be >= 1)", line);
        let mut m = self.mutable.write().unwrap();
        assert!(
            line < m.lines.len(),
            "invalid line number {} (should be < {})",
            line,
            m.lines.len()
        );
        // Remove the entry at index `line` (which is the start of line `line+1`).
        m.lines.remove(line);
    }

    /// Clone of the current line-offset table.
    pub fn lines(&self) -> Vec<i64> {
        self.mutable.read().unwrap().lines.clone()
    }

    /// Replace the line-offset table. Returns false (and leaves the
    /// table unchanged) if `lines` is not strictly increasing or has
    /// any entry `>= size`.
    pub fn set_lines(&self, lines: Vec<i64>) -> bool {
        for (i, &offset) in lines.iter().enumerate() {
            if (i > 0 && offset <= lines[i - 1]) || self.size <= offset {
                return false;
            }
        }
        self.mutable.write().unwrap().lines = lines;
        true
    }

    /// Compute the line-offset table from raw file content.
    pub fn set_lines_for_content(&self, content: &[u8]) {
        let mut lines = Vec::new();
        let mut line: i64 = 0;
        for (offset, &b) in content.iter().enumerate() {
            if line >= 0 {
                lines.push(line);
            }
            line = -1;
            if b == b'\n' {
                line = offset as i64 + 1;
            }
        }
        self.mutable.write().unwrap().lines = lines;
    }

    /// Pos of the start of `line` (1-based). Panics on out-of-range.
    pub fn line_start(&self, line: usize) -> Pos {
        assert!(line >= 1, "invalid line number {} (should be >= 1)", line);
        let m = self.mutable.read().unwrap();
        assert!(
            line <= m.lines.len(),
            "invalid line number {} (should be < {})",
            line,
            m.lines.len()
        );
        Pos(self.base + m.lines[line - 1])
    }

    /// Add alternative line info with column = 1.
    pub fn add_line_info(&self, offset: i64, filename: &str, line: i64) {
        self.add_line_column_info(offset, filename, line, 1);
    }

    /// Add alternative file/line/column info for `offset`. Ignored
    /// unless the offset strictly increases relative to the previous
    /// info and is strictly less than `size`.
    pub fn add_line_column_info(&self, offset: i64, filename: &str, line: i64, column: i64) {
        let mut m = self.mutable.write().unwrap();
        let i = m.infos.len();
        if (i == 0 || m.infos[i - 1].offset < offset) && offset < self.size {
            m.infos.push(LineInfo {
                offset,
                filename: filename.to_string(),
                line,
                column,
            });
        }
    }

    fn fix_offset(&self, offset: i64) -> i64 {
        if offset < 0 {
            0
        } else if offset > self.size {
            self.size
        } else {
            offset
        }
    }

    /// Pos for `offset`. Out-of-bounds offsets are clamped to the file's
    /// start/end positions (matching Go issue 57490).
    pub fn pos(&self, offset: i64) -> Pos {
        Pos(self.base + self.fix_offset(offset))
    }

    /// Inverse of [`File::pos`]. Out-of-bounds `Pos` values clamp to 0
    /// / file size.
    pub fn offset(&self, p: Pos) -> i64 {
        self.fix_offset(p.0 - self.base)
    }

    pub fn line(&self, p: Pos) -> i64 {
        self.line_for(p, true)
    }

    fn unpack(&self, offset: i64, adjusted: bool) -> (String, i64, i64) {
        let m = self.mutable.read().unwrap();
        let mut filename = self.name.clone();
        let mut line: i64 = 0;
        let mut column: i64 = 0;
        let i = search_ints(&m.lines, offset);
        if i >= 0 {
            line = (i + 1) as i64;
            column = offset - m.lines[i as usize] + 1;
        }
        if adjusted && !m.infos.is_empty() {
            let j = search_line_infos(&m.infos, offset);
            if j >= 0 {
                let alt = &m.infos[j as usize];
                filename = alt.filename.clone();
                let k = search_ints(&m.lines, alt.offset);
                if k >= 0 {
                    let d = line - (k + 1) as i64;
                    line = alt.line + d;
                    if alt.column == 0 {
                        column = 0;
                    } else if d == 0 {
                        column = alt.column + (offset - alt.offset);
                    }
                }
            }
        }
        (filename, line, column)
    }

    /// Line of `p`, without materializing a [`Position`].
    ///
    /// [`File::position_for`] clones the file name into the `Position` it
    /// returns. The printer asks for a line per node it lays out, so on a run
    /// with a formatter enabled that clone is a heap allocation per query and
    /// nothing reads it. Same answer as `position_for(p, adjusted).line`.
    pub fn line_for(&self, p: Pos, adjusted: bool) -> i64 {
        if p == NO_POS {
            return 0;
        }
        let offset = self.fix_offset(p.0 - self.base);
        let m = self.mutable.read().unwrap();
        let i = search_ints(&m.lines, offset);
        let mut line = if i >= 0 { (i + 1) as i64 } else { 0 };
        if adjusted && !m.infos.is_empty() {
            let j = search_line_infos(&m.infos, offset);
            if j >= 0 {
                let alt = &m.infos[j as usize];
                let k = search_ints(&m.lines, alt.offset);
                if k >= 0 {
                    line = alt.line + (line - (k + 1) as i64);
                }
            }
        }
        line
    }

    fn position_internal(&self, p: Pos, adjusted: bool) -> Position {
        let offset = self.fix_offset(p.0 - self.base);
        let (filename, line, column) = self.unpack(offset, adjusted);
        Position {
            filename,
            offset,
            line,
            column,
        }
    }

    /// Position of `p`, optionally honoring `//line`-style overrides.
    pub fn position_for(&self, p: Pos, adjusted: bool) -> Position {
        if p != NO_POS {
            self.position_internal(p, adjusted)
        } else {
            Position::default()
        }
    }

    /// Position of `p` with line-directive adjustments applied.
    pub fn position(&self, p: Pos) -> Position {
        self.position_for(p, true)
    }

    /// Test-only access to the raw line offsets.
    #[cfg(test)]
    pub(crate) fn raw_lines(&self) -> Vec<i64> {
        self.mutable.read().unwrap().lines.clone()
    }

    /// Test-only access to the raw line-info table.
    #[cfg(test)]
    pub(crate) fn raw_infos(&self) -> Vec<LineInfo> {
        self.mutable.read().unwrap().infos.clone()
    }

    /// Internal accessor used by `serialize`.
    pub(crate) fn snapshot_for_serialize(&self) -> (String, i64, i64, Vec<i64>, Vec<LineInfo>) {
        let m = self.mutable.read().unwrap();
        (
            self.name.clone(),
            self.base,
            self.size,
            m.lines.clone(),
            m.infos.clone(),
        )
    }
}

/// Internal state of a FileSet, guarded by `FileSet::inner`.
struct FileSetInner {
    base: i64,
    tree: Tree,
}

/// Identity for the per-thread lookup cache. Handed out by [`FileSet::new`].
static NEXT_FILESET_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// This thread's most recent [`FileSet::file_internal`] hit.
    ///
    /// Two slots, not one: a worker typically alternates between the shared
    /// package `FileSet` and a private one (comment reparses and formatters
    /// each build their own), and a single slot would thrash between them.
    ///
    /// Entries are `(fileset id, generation, file)`. The generation guards
    /// against [`FileSet::remove_file`] retiring a file that this thread still
    /// has cached — the tree is behind a lock, but this cache is not.
    static LAST_FILE: RefCell<[Option<(u64, u64, Arc<File>)>; 2]> =
        const { RefCell::new([None, None]) };
}

/// A `FileSet` represents a set of source files. Methods are safe to
/// call concurrently from multiple threads.
pub struct FileSet {
    inner: RwLock<FileSetInner>,
    /// Distinguishes this set's entries in the per-thread [`LAST_FILE`] cache.
    id: u64,
    /// Bumped by [`FileSet::remove_file`]; invalidates cached entries.
    generation: AtomicU64,
}

impl FileSet {
    /// Create a new, empty FileSet.
    pub fn new() -> Arc<Self> {
        Arc::new(FileSet {
            inner: RwLock::new(FileSetInner {
                base: 1, // 0 == NO_POS
                tree: Tree::new(),
            }),
            id: NEXT_FILESET_ID.fetch_add(1, Ordering::Relaxed),
            generation: AtomicU64::new(0),
        })
    }

    /// Minimum base offset for the next `add_file` call.
    pub fn base(&self) -> i64 {
        self.inner.read().unwrap().base
    }

    /// Add a new file with the given name, base, and size.
    ///
    /// If `base < 0`, the current `FileSet::base()` is used. Otherwise
    /// `base` must be `>= FileSet::base()` and `size >= 0`.
    pub fn add_file(&self, filename: &str, base: i64, size: i64) -> Arc<File> {
        let mut inner = self.inner.write().unwrap();
        let base = if base < 0 { inner.base } else { base };
        assert!(
            base >= inner.base,
            "invalid base {} (should be >= {})",
            base,
            inner.base
        );
        assert!(size >= 0, "invalid size {} (should be >= 0)", size);
        let next_base = base
            .checked_add(size)
            .and_then(|v| v.checked_add(1))
            .expect("token.Pos offset overflow (> 2G of source code in file set)");
        let file = File::new(filename.to_string(), base, size);
        inner.base = next_base;
        inner.tree.add(file.clone());
        // No cache seeding: the per-thread cache is filled on the first lookup
        // that wants this file, and pre-seeding it from `add_file` would only
        // warm the *adding* thread — which in a parallel load is rarely the one
        // that goes on to ask for positions in it.
        file
    }

    /// Add already-constructed files to this FileSet. Files with the
    /// same `Arc<File>` identity that are already present are silently
    /// skipped; files whose ranges overlap with a *different* file
    /// panic via the underlying tree.
    pub fn add_existing_files(&self, files: &[Arc<File>]) {
        let mut inner = self.inner.write().unwrap();
        for f in files {
            inner.tree.add(f.clone());
            let new_base = f.base() + f.size() + 1;
            if new_base > inner.base {
                inner.base = new_base;
            }
        }
    }

    /// Remove `file` from the set. Removing a file that isn't a member
    /// is a no-op.
    pub fn remove_file(&self, file: &Arc<File>) {
        let mut inner = self.inner.write().unwrap();
        // Retire every thread's cached entry for this set. Threads cannot be
        // reached directly, so the generation does it for them: a cached entry
        // stamped with the old value stops matching.
        self.generation.fetch_add(1, Ordering::AcqRel);
        let (found, _) = inner.tree.locate(file_key(file));
        if let Some(idx) = found {
            if Arc::ptr_eq(inner.tree.file_at(idx), file) {
                inner.tree.delete(idx);
            }
        }
    }

    /// Snapshot of all files in ascending base order.
    pub fn files(&self) -> Vec<Arc<File>> {
        self.inner.read().unwrap().tree.all()
    }

    /// Visit each file in ascending base order until `yield_fn` returns
    /// false. Snapshot-based: concurrent additions/removals don't
    /// affect the iteration.
    pub fn iterate<F: FnMut(&Arc<File>) -> bool>(&self, mut yield_fn: F) {
        for f in self.files() {
            if !yield_fn(&f) {
                break;
            }
        }
    }

    fn file_internal(&self, p: Pos) -> Option<Arc<File>> {
        // Fast path: this thread's last looked-up file, no lock at all.
        let gen = self.generation.load(Ordering::Acquire);
        let hit = LAST_FILE.with(|c| {
            for slot in c.borrow().iter().flatten() {
                let (id, g, f) = slot;
                if *id == self.id && *g == gen && f.base <= p.0 && p.0 <= f.base + f.size {
                    return Some(f.clone());
                }
            }
            None
        });
        if hit.is_some() {
            return hit;
        }
        let inner = self.inner.read().unwrap();
        let (found, _) = inner.tree.locate(Key::point(p.0));
        if let Some(idx) = found {
            let f = inner.tree.file_at(idx).clone();
            // Re-read the generation while still holding the read lock: a
            // concurrent `remove_file` needs the write lock, so a bump seen
            // here means the removal is already ordered before this lookup and
            // caching `f` under the *old* generation would keep a retired file
            // reachable. Caching under the fresh value is safe — `f` came out
            // of the tree as it stands now.
            let gen = self.generation.load(Ordering::Acquire);
            let entry = (self.id, gen, f.clone());
            drop(inner);
            LAST_FILE.with(|c| {
                let mut slots = c.borrow_mut();
                slots.swap(0, 1);
                slots[0] = Some(entry);
            });
            return Some(f);
        }
        None
    }

    /// Lookup the file containing `p`, if any.
    pub fn file(&self, p: Pos) -> Option<Arc<File>> {
        if p == NO_POS {
            None
        } else {
            self.file_internal(p)
        }
    }

    /// Position of `p`, optionally honoring `//line` overrides.
    pub fn position_for(&self, p: Pos, adjusted: bool) -> Position {
        if p == NO_POS {
            return Position::default();
        }
        match self.file_internal(p) {
            Some(f) => f.position_internal(p, adjusted),
            None => Position::default(),
        }
    }

    /// Position of `p` with line-directive adjustments applied.
    pub fn position(&self, p: Pos) -> Position {
        self.position_for(p, true)
    }

    /// Line of `p`, without materializing a [`Position`] — see
    /// [`File::line_for`].
    pub fn line_for(&self, p: Pos, adjusted: bool) -> i64 {
        if p == NO_POS {
            return 0;
        }
        match self.file_internal(p) {
            Some(f) => f.line_for(p, adjusted),
            None => 0,
        }
    }

    /// Internal: read-locked access to (base, tree-snapshot) for serialize.
    pub(crate) fn snapshot_for_serialize(&self) -> (i64, Vec<Arc<File>>) {
        let inner = self.inner.read().unwrap();
        (inner.base, inner.tree.all())
    }

    /// Internal: replace the contents of this FileSet from a
    /// deserialized record. Used by `serialize::FileSet::read`.
    pub(crate) fn restore_from(&self, base: i64, files: Vec<Arc<File>>) {
        let mut inner = self.inner.write().unwrap();
        inner.base = base;
        inner.tree = Tree::new();
        for f in files {
            inner.tree.add(f);
        }
        drop(inner);
        // The tree was replaced wholesale, so every thread's cached entry for
        // this set is now suspect.
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
}

/// Convenience trait so callers can compare two `FileSet`s using
/// `Option`-style `None` for the "equal" case to mirror Go's
/// `equal(p, q) error`. Returns a description of the first difference.
impl FileSet {
    #[cfg(test)]
    pub(crate) fn diff(&self, other: &FileSet) -> Option<String> {
        let (pb, pfiles) = self.snapshot_for_serialize();
        let (qb, qfiles) = other.snapshot_for_serialize();
        if pb != qb {
            return Some(format!("different bases: {} != {}", pb, qb));
        }
        if pfiles.len() != qfiles.len() {
            return Some(format!(
                "different number of files: {} != {}",
                pfiles.len(),
                qfiles.len()
            ));
        }
        for (f, g) in pfiles.iter().zip(qfiles.iter()) {
            if f.name() != g.name() {
                return Some(format!(
                    "different filenames: {:?} != {:?}",
                    f.name(),
                    g.name()
                ));
            }
            if f.base() != g.base() {
                return Some(format!(
                    "different base for {:?}: {} != {}",
                    f.name(),
                    f.base(),
                    g.base()
                ));
            }
            if f.size() != g.size() {
                return Some(format!(
                    "different size for {:?}: {} != {}",
                    f.name(),
                    f.size(),
                    g.size()
                ));
            }
            let fl = f.raw_lines();
            let gl = g.raw_lines();
            if fl != gl {
                return Some(format!("different offsets for {:?}", f.name()));
            }
            let fi = f.raw_infos();
            let gi = g.raw_infos();
            if fi.len() != gi.len() {
                return Some(format!("different infos for {:?}", f.name()));
            }
            for (a, b) in fi.iter().zip(gi.iter()) {
                if a.offset != b.offset || a.filename != b.filename || a.line != b.line {
                    return Some(format!("different infos for {:?}", f.name()));
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------------
// Helper functions

/// Manually-inlined version of `sort.Search`-1: returns the largest `i`
/// with `a[i] <= x`, or -1 if no such element exists.
pub(crate) fn search_ints(a: &[i64], x: i64) -> isize {
    let (mut i, mut j) = (0usize, a.len());
    while i < j {
        let h = (i + j) >> 1;
        if a[h] <= x {
            i = h + 1;
        } else {
            j = h;
        }
    }
    i as isize - 1
}

fn search_line_infos(a: &[LineInfo], x: i64) -> isize {
    // BinarySearchFunc-style: locate by Offset, falling back to the
    // predecessor when x is not exactly an entry's offset.
    let (mut lo, mut hi) = (0usize, a.len());
    let mut found_at: Option<usize> = None;
    while lo < hi {
        let mid = (lo + hi) >> 1;
        match a[mid].offset.cmp(&x) {
            std::cmp::Ordering::Less => lo = mid + 1,
            std::cmp::Ordering::Greater => hi = mid,
            std::cmp::Ordering::Equal => {
                found_at = Some(mid);
                break;
            }
        }
    }
    if let Some(i) = found_at {
        i as isize
    } else {
        // lo is the insertion index; the "containing" entry is lo-1.
        lo as isize - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn check_pos(msg: &str, got: &Position, want: &Position) {
        assert_eq!(
            got.filename, want.filename,
            "{}: got filename = {:?}; want {:?}",
            msg, got.filename, want.filename
        );
        assert_eq!(
            got.offset, want.offset,
            "{}: got offset = {}; want {}",
            msg, got.offset, want.offset
        );
        assert_eq!(
            got.line, want.line,
            "{}: got line = {}; want {}",
            msg, got.line, want.line
        );
        assert_eq!(
            got.column, want.column,
            "{}: got column = {}; want {}",
            msg, got.column, want.column
        );
    }

    #[test]
    fn test_no_pos() {
        assert!(!NO_POS.is_valid(), "NO_POS should not be valid");
        let fset = FileSet::new();
        check_pos("fset NoPos", &fset.position(NO_POS), &Position::default());
    }

    struct Case {
        filename: &'static str,
        source: Option<&'static [u8]>,
        size: i64,
        lines: Vec<i64>,
    }

    fn cases() -> Vec<Case> {
        vec![
            Case {
                filename: "a",
                source: Some(b""),
                size: 0,
                lines: vec![],
            },
            Case {
                filename: "b",
                source: Some(b"01234"),
                size: 5,
                lines: vec![0],
            },
            Case {
                filename: "c",
                source: Some(b"\n\n\n\n\n\n\n\n\n"),
                size: 9,
                lines: vec![0, 1, 2, 3, 4, 5, 6, 7, 8],
            },
            Case {
                filename: "d",
                source: None,
                size: 100,
                lines: vec![0, 5, 10, 20, 30, 70, 71, 72, 80, 85, 90, 99],
            },
            Case {
                filename: "e",
                source: None,
                size: 777,
                lines: vec![0, 80, 100, 120, 130, 180, 267, 455, 500, 567, 620],
            },
            Case {
                filename: "f",
                source: Some(b"package p\n\nimport \"fmt\""),
                size: 23,
                lines: vec![0, 10, 11],
            },
            Case {
                filename: "g",
                source: Some(b"package p\n\nimport \"fmt\"\n"),
                size: 24,
                lines: vec![0, 10, 11],
            },
            Case {
                filename: "h",
                source: Some(b"package p\n\nimport \"fmt\"\n "),
                size: 25,
                lines: vec![0, 10, 11, 24],
            },
        ]
    }

    fn linecol(lines: &[i64], offs: i64) -> (i64, i64) {
        let mut prev = 0i64;
        for (line, &lo) in lines.iter().enumerate() {
            if offs < lo {
                return (line as i64, offs - prev + 1);
            }
            prev = lo;
        }
        (lines.len() as i64, offs - prev + 1)
    }

    fn verify_positions(fset: &FileSet, f: &Arc<File>, lines: &[i64]) {
        for offs in 0..f.size() {
            let p = f.pos(offs);
            let offs2 = f.offset(p);
            assert_eq!(
                offs2,
                offs,
                "{}, Offset: got offset {}; want {}",
                f.name(),
                offs2,
                offs
            );
            let (line, col) = linecol(lines, offs);
            let msg = format!("{} (offs = {}, p = {})", f.name(), offs, p);
            let want = Position {
                filename: f.name().to_string(),
                offset: offs,
                line,
                column: col,
            };
            check_pos(&msg, &f.position(f.pos(offs)), &want);
            check_pos(&msg, &fset.position(p), &want);
        }
    }

    fn make_test_source(size: i64, lines: &[i64]) -> Vec<u8> {
        let mut src = vec![0u8; size as usize];
        for &offs in lines {
            if offs > 0 {
                src[(offs - 1) as usize] = b'\n';
            }
        }
        src
    }

    #[test]
    fn test_positions() {
        const DELTA: i64 = 7;
        let fset = FileSet::new();
        for test in cases() {
            if let Some(src) = test.source {
                assert_eq!(
                    src.len() as i64,
                    test.size,
                    "{}: inconsistent test case: got file size {}; want {}",
                    test.filename,
                    src.len(),
                    test.size
                );
            }
            let f = fset.add_file(test.filename, fset.base() + DELTA, test.size);
            assert_eq!(f.name(), test.filename);
            assert_eq!(f.size(), test.size);
            assert!(
                Arc::ptr_eq(&fset.file(f.pos(0)).unwrap(), &f),
                "{}: f.Pos(0) was not found in f",
                f.name()
            );

            for (i, &offset) in test.lines.iter().enumerate() {
                f.add_line(offset);
                assert_eq!(
                    f.line_count(),
                    i + 1,
                    "{}, add_line: got line count {}; want {}",
                    f.name(),
                    f.line_count(),
                    i + 1
                );
                f.add_line(offset); // dup, ignored
                assert_eq!(f.line_count(), i + 1);
                verify_positions(&fset, &f, &test.lines[0..=i]);
            }

            assert!(
                f.set_lines(test.lines.clone()),
                "{}: set_lines failed",
                f.name()
            );
            assert_eq!(f.line_count(), test.lines.len());
            assert_eq!(f.lines(), test.lines);
            verify_positions(&fset, &f, &test.lines);

            let src = test
                .source
                .map(|s| s.to_vec())
                .unwrap_or_else(|| make_test_source(test.size, &test.lines));
            f.set_lines_for_content(&src);
            assert_eq!(f.line_count(), test.lines.len());
            verify_positions(&fset, &f, &test.lines);
        }
    }

    #[test]
    fn test_line_info() {
        let fset = FileSet::new();
        let f = fset.add_file("foo", fset.base(), 500);
        let lines = [0, 42, 77, 100, 210, 220, 277, 300, 333, 401];
        for &offs in &lines {
            f.add_line(offs);
            f.add_line_info(offs, "bar", 42);
        }
        for offs in 0..=f.size() {
            let p = f.pos(offs);
            let (_, col) = linecol(&lines, offs);
            let msg = format!("{} (offs = {}, p = {})", f.name(), offs, p);
            let want = Position {
                filename: "bar".to_string(),
                offset: offs,
                line: 42,
                column: col,
            };
            check_pos(&msg, &f.position(f.pos(offs)), &want);
            check_pos(&msg, &fset.position(p), &want);
        }
    }

    #[test]
    fn test_files() {
        let fset = FileSet::new();
        let tests = cases();
        for (i, test) in tests.iter().enumerate() {
            let base = if i % 2 == 1 { -1 } else { fset.base() };
            fset.add_file(test.filename, base, test.size);
            let mut j = 0;
            fset.iterate(|f| {
                assert_eq!(
                    f.name(),
                    tests[j].filename,
                    "got filename = {}; want {}",
                    f.name(),
                    tests[j].filename
                );
                j += 1;
                true
            });
            assert_eq!(j, i + 1, "got {} files; want {}", j, i + 1);
        }
    }

    #[test]
    fn test_file_set_past_end() {
        let fset = FileSet::new();
        for test in cases() {
            fset.add_file(test.filename, fset.base(), test.size);
        }
        assert!(fset.file(Pos(fset.base())).is_none());
    }

    #[test]
    fn test_file_set_cache_unlikely() {
        let fset = FileSet::new();
        let mut offsets: Vec<(String, i64)> = Vec::new();
        for test in cases() {
            offsets.push((test.filename.to_string(), fset.base()));
            fset.add_file(test.filename, fset.base(), test.size);
        }
        for (file, pos) in offsets {
            let f = fset.file(Pos(pos)).expect("file must be present");
            assert_eq!(f.name(), file);
        }
    }

    #[test]
    fn test_position_for() {
        let src: &[u8] = b"\nfoo\nb\nar\n//line :100\nfoobar\n//line bar:3\ndone\n";

        let filename = "foo";
        let fset = FileSet::new();
        let f = fset.add_file(filename, fset.base(), src.len() as i64);
        f.set_lines_for_content(src);

        let lines = f.raw_lines();
        for (i, &offs) in lines.iter().enumerate() {
            let got1 = f.position_for(f.pos(offs), false);
            let got2 = f.position_for(f.pos(offs), true);
            let got3 = f.position(f.pos(offs));
            let want = Position {
                filename: filename.to_string(),
                offset: offs,
                line: (i + 1) as i64,
                column: 1,
            };
            check_pos("1. PositionFor unadjusted", &got1, &want);
            check_pos("1. PositionFor adjusted", &got2, &want);
            check_pos("1. Position", &got3, &want);
        }

        const L1: usize = 5;
        const L2: usize = 7;
        f.add_line_info(lines[L1 - 1], "", 100);
        f.add_line_info(lines[L2 - 1], "bar", 3);

        // Unadjusted positions must not change.
        for (i, &offs) in lines.iter().enumerate() {
            let got1 = f.position_for(f.pos(offs), false);
            let want = Position {
                filename: filename.to_string(),
                offset: offs,
                line: (i + 1) as i64,
                column: 1,
            };
            check_pos("2. PositionFor unadjusted", &got1, &want);
        }

        // Adjusted positions reflect the new line directives.
        for (i, &offs) in lines.iter().enumerate() {
            let got2 = f.position_for(f.pos(offs), true);
            let got3 = f.position(f.pos(offs));
            let mut want = Position {
                filename: filename.to_string(),
                offset: offs,
                line: (i + 1) as i64,
                column: 1,
            };
            let line = want.line;
            if i + 1 >= L1 {
                want.filename = String::new();
                want.line = line - L1 as i64 + 100;
            }
            if i + 1 >= L2 {
                want.filename = "bar".to_string();
                want.line = line - L2 as i64 + 3;
            }
            check_pos("3. PositionFor adjusted", &got2, &want);
            check_pos("3. Position", &got3, &want);
        }
    }

    #[test]
    fn test_line_start() {
        let src = b"one\ntwo\nthree\n";
        let fset = FileSet::new();
        let f = fset.add_file("input", -1, src.len() as i64);
        f.set_lines_for_content(src);

        for line in 1..=3 {
            let pos = f.line_start(line);
            let position = fset.position(pos);
            assert_eq!(position.line, line as i64);
            assert_eq!(position.column, 1);
        }
    }

    #[test]
    fn test_remove_file() {
        let content_a: &[u8] = b"this\nis\nfileA";
        let content_b: &[u8] = b"this\nis\nfileB";
        let fset = FileSet::new();
        let a = fset.add_file("fileA", -1, content_a.len() as i64);
        a.set_lines_for_content(content_a);
        let b = fset.add_file("fileB", -1, content_b.len() as i64);
        b.set_lines_for_content(content_b);

        let check_pos_str = |pos: Pos, want: &str| {
            let got = fset.position(pos).to_string();
            assert_eq!(got, want, "Position({}) = {}, want {}", pos, got, want);
        };
        let check_num_files = |want: usize| {
            let mut got = 0;
            fset.iterate(|_f| {
                got += 1;
                true
            });
            assert_eq!(got, want);
        };

        let apos3 = a.pos(3);
        let bpos3 = b.pos(3);
        check_pos_str(apos3, "fileA:1:4");
        check_pos_str(bpos3, "fileB:1:4");
        check_num_files(2);

        fset.remove_file(&a);
        check_pos_str(apos3, "-");
        check_pos_str(bpos3, "fileB:1:4");
        check_num_files(1);

        // idempotent
        fset.remove_file(&a);
        check_pos_str(apos3, "-");
        check_pos_str(bpos3, "fileB:1:4");
        check_num_files(1);
    }

    #[test]
    fn test_file_add_line_column_info() {
        const FILENAME: &str = "test.go";
        const FILESIZE: i64 = 100;

        struct Case {
            name: &'static str,
            infos: Vec<LineInfo>,
            want: Vec<LineInfo>,
        }
        let mkinfo = |offset, filename: &str, line, column| LineInfo {
            offset,
            filename: filename.to_string(),
            line,
            column,
        };
        let tests = vec![
            Case {
                name: "normal",
                infos: vec![
                    mkinfo(10, FILENAME, 2, 1),
                    mkinfo(50, FILENAME, 3, 1),
                    mkinfo(80, FILENAME, 4, 2),
                ],
                want: vec![
                    mkinfo(10, FILENAME, 2, 1),
                    mkinfo(50, FILENAME, 3, 1),
                    mkinfo(80, FILENAME, 4, 2),
                ],
            },
            Case {
                name: "offset1 == file size",
                infos: vec![mkinfo(FILESIZE, FILENAME, 2, 1)],
                want: vec![],
            },
            Case {
                name: "offset1 > file size",
                infos: vec![mkinfo(FILESIZE + 1, FILENAME, 2, 1)],
                want: vec![],
            },
            Case {
                name: "offset2 == file size",
                infos: vec![mkinfo(10, FILENAME, 2, 1), mkinfo(FILESIZE, FILENAME, 3, 1)],
                want: vec![mkinfo(10, FILENAME, 2, 1)],
            },
            Case {
                name: "offset2 > file size",
                infos: vec![
                    mkinfo(10, FILENAME, 2, 1),
                    mkinfo(FILESIZE + 1, FILENAME, 3, 1),
                ],
                want: vec![mkinfo(10, FILENAME, 2, 1)],
            },
            Case {
                name: "offset2 == offset1",
                infos: vec![mkinfo(10, FILENAME, 2, 1), mkinfo(10, FILENAME, 3, 1)],
                want: vec![mkinfo(10, FILENAME, 2, 1)],
            },
            Case {
                name: "offset2 < offset1",
                infos: vec![mkinfo(10, FILENAME, 2, 1), mkinfo(9, FILENAME, 3, 1)],
                want: vec![mkinfo(10, FILENAME, 2, 1)],
            },
        ];

        for test in tests {
            let fs = FileSet::new();
            let f = fs.add_file(FILENAME, -1, FILESIZE);
            for info in &test.infos {
                f.add_line_column_info(info.offset, &info.filename, info.line, info.column);
            }
            assert_eq!(
                f.raw_infos(),
                test.want,
                "case {}: got {:?}, want {:?}",
                test.name,
                f.raw_infos(),
                test.want
            );
        }
    }

    #[test]
    fn test_issue_57490() {
        const FSIZE: i64 = 5;
        let fset = FileSet::new();
        let base = fset.base();
        let f = fset.add_file("f", base, FSIZE);

        assert_eq!(f.offset(NO_POS), 0);
        assert_eq!(f.offset(Pos(-1)), 0);
        assert_eq!(f.offset(Pos(base + FSIZE + 1)), FSIZE);

        assert_eq!(f.pos(-1), Pos(base));
        assert_eq!(f.pos(FSIZE + 1), Pos(base + FSIZE));

        let want = format!("{}:1:1", f.name());
        assert_eq!(f.position(Pos(-1)).to_string(), want);
        let want = format!("{}:1:{}", f.name(), FSIZE + 1);
        assert_eq!(f.position(Pos(FSIZE + 1)).to_string(), want);

        const XSIZE: i64 = FSIZE + 5;
        for offset in -XSIZE..XSIZE {
            let want1 = f.offset(Pos(f.base() + offset));
            assert_eq!(f.offset(f.pos(offset)), want1);

            let want2 = f.pos(offset);
            assert_eq!(f.pos(f.offset(want2)), want2);
        }
    }

    #[test]
    fn test_file_set_add_existing_files() {
        let fset = FileSet::new();
        let _ = fset.add_file("A", -1, 3);
        let _ = fset.add_file("B", -1, 5);
        assert_eq!(fset_string(&fset), "{A:1-4 B:5-10}");

        fset.add_existing_files(&[]);
        assert_eq!(fset_string(&fset), "{A:1-4 B:5-10}");

        let file_c = FileSet::new().add_file("C", 100, 5);
        let file_d = FileSet::new().add_file("D", 200, 5);
        let file_a_dup = fset.files()[0].clone();
        fset.add_existing_files(&[file_c.clone(), file_a_dup, file_d, file_c]);
        assert_eq!(fset_string(&fset), "{A:1-4 B:5-10 C:100-105 D:200-205}");

        let _ = fset.add_file("E", -1, 3);
        assert_eq!(
            fset_string(&fset),
            "{A:1-4 B:5-10 C:100-105 D:200-205 E:206-209}"
        );
    }

    fn fset_string(fset: &FileSet) -> String {
        let mut buf = String::from("{");
        let mut sep = "";
        fset.iterate(|f| {
            buf.push_str(sep);
            buf.push_str(f.name());
            buf.push(':');
            buf.push_str(&f.base().to_string());
            buf.push('-');
            buf.push_str(&f.end_pos().to_string());
            sep = " ";
            true
        });
        buf.push('}');
        buf
    }

    #[test]
    fn test_file_end() {
        let fset = FileSet::new();
        let f = fset.add_file("a.go", 100, 42);
        assert_eq!(f.base(), 100);
        assert_eq!(f.end(), Pos(142));
    }

    // Race-style tests, using std::thread::scope.

    #[test]
    fn test_file_set_race() {
        use std::thread;

        let fset = FileSet::new();
        for i in 0..100 {
            fset.add_file(&format!("file-{}", i), fset.base(), 1031);
        }
        let max_pos = fset.base();
        let mut state: u64 = 7;
        thread::scope(|s| {
            for _ in 0..2 {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                let mut seed = state;
                let fset = Arc::clone(&fset);
                s.spawn(move || {
                    for _ in 0..1000 {
                        // simple LCG to vary positions
                        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                        let p = (seed % max_pos as u64) as i64;
                        let _ = fset.position(Pos(p));
                    }
                });
            }
        });
    }

    #[test]
    fn test_file_set_race2() {
        use std::thread;
        const N: i64 = 1000;
        let fset = FileSet::new();
        let file = fset.add_file("", -1, N);

        thread::scope(|s| {
            let file1 = Arc::clone(&file);
            s.spawn(move || {
                for i in 0..N {
                    file1.add_line(i);
                }
            });
            let fset2 = Arc::clone(&fset);
            let file2 = Arc::clone(&file);
            s.spawn(move || {
                let pos = file2.pos(0);
                for _ in 0..N {
                    fset2.position_for(pos, false);
                }
            });
        });
    }

    #[test]
    fn test_remove_file_race() {
        use std::sync::mpsc;
        use std::thread;

        let fset = FileSet::new();
        let mut files = Vec::with_capacity(20000);
        for i in 0..20000 {
            let f = fset.add_file("f", -1, (i + 1) * 10);
            files.push(f);
        }

        let (tx1, rx1) = mpsc::channel::<Arc<File>>();
        let (tx2, rx2) = mpsc::channel::<Arc<File>>();
        let (start_tx, start_rx) = mpsc::channel::<()>();

        let files_for_governor = files.clone();
        let gov = thread::spawn(move || {
            for f in files_for_governor.iter() {
                let _ = start_rx.recv();
                tx1.send(f.clone()).unwrap();
                tx2.send(f.clone()).unwrap();
            }
            let _ = start_rx.recv();
            // drop senders to close the channels
        });

        let fset_for_reader = Arc::clone(&fset);
        let reader = thread::spawn(move || {
            while let Ok(f) = rx1.recv() {
                let _ = fset_for_reader.file(Pos(f.base() + 5));
            }
        });

        start_tx.send(()).unwrap();
        while let Ok(f) = rx2.recv() {
            fset.remove_file(&f);
            let got = fset.file(Pos(f.base() + 5));
            assert!(got.is_none(), "file was not removed correctly");
            start_tx.send(()).unwrap();
        }

        gov.join().unwrap();
        reader.join().unwrap();
    }

    #[test]
    fn test_removed_file_file_returns_nil() {
        let fset = FileSet::new();
        let mut files: Vec<Arc<File>> = Vec::with_capacity(1000);
        for i in 0..1000 {
            files.push(fset.add_file("f", -1, (i + 1) * 100));
        }

        // Fisher-Yates with a fixed seed.
        let mut state: u64 = 0xC0FFEE;
        for i in (1..files.len()).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let j = (state as usize) % (i + 1);
            files.swap(i, j);
        }

        for f in files {
            fset.remove_file(&f);
            let got = fset.file(Pos(f.base() + 10));
            assert!(
                got.is_none(),
                "file was not removed correctly; got file with base: {}",
                got.unwrap().base()
            );
        }
    }
}
