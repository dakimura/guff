use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("reading export data for {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("can't read export data for {path} directly from an archive file (call new_reader first)")]
    ArchiveDirect { path: String },
    #[error("binary ({c}) import format is no longer supported")]
    BinaryFormat { c: char },
    #[error("unexpected export data with prefix {prefix:?} for path {path}")]
    UnexpectedPrefix { prefix: String, path: String },
    #[error("empty export data for {path}")]
    Empty { path: String },
    #[error("{0}")]
    Decode(String),
    #[error("export data desync: package {pkg}, section {section}, index {index}, offset {offset}: found {found}, expected {expected}")]
    Desync {
        pkg: String,
        section: i32,
        index: i32,
        offset: u64,
        found: i32,
        expected: i32,
    },
    #[error("reading export data: {file}: {source}")]
    ImportFile {
        file: String,
        #[source]
        source: std::io::Error,
    },
    #[error("can't find import: {path}")]
    ImportNotFound { path: String },
}
