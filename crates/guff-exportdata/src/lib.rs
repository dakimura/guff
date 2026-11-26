//! Read Go compiler export data into `guff-types` packages.
//!
//! Port of `golang.org/x/tools/go/gcexportdata`.

mod archive;
mod error;
mod fake_fileset;
mod importer;
mod pkgbits;
mod predeclared;
mod reader;
mod ureader;

pub use error::Error;
pub use importer::ExportImporter;
pub use reader::{new_reader, read, read_export_data};

// Predeclared table is exported for tests / tooling.
pub use predeclared::predeclared_types;
