//! Public export-data reader API.
//!
//! Port of `golang.org/x/tools/go/gcexportdata`.

use std::collections::HashMap;
use std::sync::Arc;

use guff::position::FileSet;
use guff_types::importer::{ImportCtx, Importer};
use guff_types::package::new_package;
use guff_types::universe::Universe;
use guff_types::PackageId;

use crate::archive::find_export_data;
use crate::error::Error;
use crate::ureader::read_unified_package;

/// Extract export data bytes from a `.a` archive or object file payload.
///
/// Port of `gcexportdata.NewReader`.
pub fn new_reader(data: &[u8]) -> Result<&[u8], Error> {
    find_export_data(data)
}

/// Decode unified export data (`u` format) into the checker's arenas.
///
/// Port of `gcimporter.UImportData`.
pub fn read_export_data<'a>(
    importer: &mut dyn Importer,
    ctx: &mut ImportCtx<'a>,
    universe: &Universe,
    imports: HashMap<String, PackageId>,
    data: &[u8],
    path: &str,
    fset: Arc<FileSet>,
) -> Result<PackageId, Error> {
    read_unified_package(importer, ctx, universe, imports, fset, data, path)
        .map_err(Error::Decode)
}

/// Read export data and construct a type-checked package in `ctx`'s arenas.
///
/// Port of `gcexportdata.Read`.
pub fn read(
    importer: &mut dyn Importer,
    ctx: &mut ImportCtx<'_>,
    universe: &Universe,
    data: &[u8],
    path: &str,
    fset: &Arc<FileSet>,
) -> Result<PackageId, Error> {
    if data.starts_with(b"!<arch>") {
        return Err(Error::ArchiveDirect {
            path: path.to_string(),
        });
    }

    let payload = if data.is_empty() {
        return Err(Error::Empty {
            path: path.to_string(),
        });
    } else {
        data
    };

    let unified = match payload[0] {
        b'u' => &payload[1..],
        b'v' | b'c' | b'd' => {
            return Err(Error::BinaryFormat {
                c: payload[0] as char,
            });
        }
        b'i' => {
            return Err(Error::Decode(
                "indexed ('i') import format is not supported".to_string(),
            ));
        }
        _ => {
            let l = payload.len().min(10);
            return Err(Error::UnexpectedPrefix {
                prefix: String::from_utf8_lossy(&payload[..l]).into_owned(),
                path: path.to_string(),
            });
        }
    };

    let mut imports = HashMap::new();
    let name = path.rsplit('/').next().unwrap_or(path);
    let pkg = new_package(
        ctx.packages,
        ctx.scopes,
        ctx.universe_scope,
        path,
        name,
    );
    imports.insert(path.to_string(), pkg);

    read_export_data(
        importer,
        ctx,
        universe,
        imports,
        unified,
        path,
        fset.clone(),
    )
}
