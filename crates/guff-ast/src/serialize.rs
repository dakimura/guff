// Port of Go's go/token/serialize.go to Rust.
//
// Original: Copyright 2011 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// The Go API takes a generic `encode/decode func(any) error`, which
// lets callers plug in gob, JSON, etc. There's no exact Rust analog
// without pulling in a serialization framework, so this port exposes
// plain data structs (`SerializedFile`, `SerializedFileSet`) plus a
// pair of converters on `FileSet`. Callers can then hand the structs
// to whatever serializer they like (serde, bincode, manual format).

use std::sync::Arc;

use crate::position::{File, FileSet, LineInfo};

/// 1:1 mirror of `File`'s persisted state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializedFile {
    pub name: String,
    pub base: i64,
    pub size: i64,
    pub lines: Vec<i64>,
    pub infos: Vec<LineInfo>,
}

/// 1:1 mirror of `FileSet`'s persisted state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SerializedFileSet {
    pub base: i64,
    pub files: Vec<SerializedFile>,
}

impl FileSet {
    /// Snapshot the FileSet into a plain data structure suitable for
    /// serializing with any encoder the caller chooses.
    pub fn to_serialized(&self) -> SerializedFileSet {
        let (base, files) = self.snapshot_for_serialize();
        let files = files
            .into_iter()
            .map(|f| {
                let (name, base, size, lines, infos) = f.snapshot_for_serialize();
                SerializedFile {
                    name,
                    base,
                    size,
                    lines,
                    infos,
                }
            })
            .collect();
        SerializedFileSet { base, files }
    }

    /// Replace the contents of `self` from a previously-serialized
    /// snapshot. Any current contents are dropped.
    pub fn from_serialized(&self, ss: SerializedFileSet) {
        let files: Vec<Arc<File>> = ss
            .files
            .into_iter()
            .map(|sf| File::from_serialized(sf.name, sf.base, sf.size, sf.lines, sf.infos))
            .collect();
        self.restore_from(ss.base, files);
    }
}

#[cfg(test)]
mod tests {
    use crate::position::FileSet;

    /// Roundtrip a FileSet through the serialized form, simulating the
    /// gob encode/decode dance from the Go test. Round-tripping via
    /// `clone()` stands in for the binary encoding step.
    fn check_serialize(p: &FileSet) {
        let ss = p.to_serialized();
        let ss2 = ss.clone();
        let q = FileSet::new();
        q.from_serialized(ss2);
        if let Some(diff) = p.diff(&q) {
            panic!("filesets not identical: {}", diff);
        }
    }

    #[test]
    fn test_serialization() {
        let p = FileSet::new();
        check_serialize(&p);
        for i in 0..10i64 {
            let f = p.add_file(&format!("file{}", i), p.base() + i, i * 100);
            check_serialize(&p);
            let mut line = 1000;
            let mut offs = 0i64;
            while offs < f.size() {
                f.add_line(offs);
                if offs % 7 == 0 {
                    f.add_line_info(offs, &format!("file{}", offs), line);
                    line += 33;
                }
                offs += 40 + i;
            }
            check_serialize(&p);
        }
    }
}
