//! `Importer` adapter that loads dependencies from compiler export files.
//!
//! Port of `gcexportdata/importer.go`.

use rustc_hash::FxHashMap as HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use guff::position::FileSet;
use guff_types::importer::{ImportCtx, Importer};
use guff_types::universe::Universe;
use guff_types::{init_universe_full, PackageId};

use crate::reader::{new_reader, read};

/// Resolves import paths by reading gc export data from pre-mapped `.a` files.
pub struct ExportImporter {
    fset: Arc<FileSet>,
    universe: Universe,
    paths: HashMap<String, PathBuf>,
    cache: HashMap<String, PackageId>,
    importing: Vec<String>,
}

impl ExportImporter {
    pub fn new(fset: Arc<FileSet>, universe: Universe) -> Self {
        Self {
            fset,
            universe,
            paths: HashMap::default(),
            cache: HashMap::default(),
            importing: Vec::new(),
        }
    }

    pub fn with_fset(fset: Arc<FileSet>) -> Self {
        Self::new(fset, init_universe_full())
    }

    pub fn set_path(&mut self, import_path: impl Into<String>, export_file: impl Into<PathBuf>) {
        self.paths.insert(import_path.into(), export_file.into());
    }

    pub fn paths(&self) -> &HashMap<String, PathBuf> {
        &self.paths
    }

    pub fn cache(&self) -> &HashMap<String, PackageId> {
        &self.cache
    }

    pub fn universe(&self) -> &Universe {
        &self.universe
    }

    fn load_file(&mut self, ctx: &mut ImportCtx<'_>, path: &str, file: &Path) -> Option<PackageId> {
        let data = fs::read(file).ok()?;
        let payload = new_reader(&data).ok()?;
        let fset = self.fset.clone();
        let universe = &self.universe;
        // Resolve transitive packages from the cache (preloaded in dependency
        // order by `preload_exports`). A true NoopImporter made `do_pkg` mint
        // stub PackageIds for already-decoded deps (e.g. embed → io/fs), so
        // named types like `fs.File` were duplicated and Implements failed
        // (`embed.FS` ↛ `fs.FS`).
        let mut cache_imp = CacheImporter {
            cache: &self.cache,
            unsafe_pkg: self.universe.unsafe_pkg,
        };
        let pkg = read(&mut cache_imp, ctx, universe, payload, path, &fset).ok()?;
        self.cache.insert(path.to_string(), pkg);
        Some(pkg)
    }
}

impl Importer for ExportImporter {
    fn import(&mut self, ctx: &mut ImportCtx<'_>, path: &str) -> Option<PackageId> {
        if path == "unsafe" {
            return Some(self.universe.unsafe_pkg);
        }

        if let Some(&pkg) = self.cache.get(path) {
            if ctx.packages.get(pkg).complete() {
                return Some(pkg);
            }
        }

        if self.importing.iter().any(|p| p == path) {
            return None;
        }

        let file = self.paths.get(path)?.clone();
        self.importing.push(path.to_string());
        let result = self.load_file(ctx, path, &file);
        self.importing.pop();
        result
    }
}

/// Importer used while decoding a single export blob.
///
/// Transitive dependencies must already be in [`ExportImporter::cache`]
/// (loaded by prior [`ExportImporter::import`] calls via topo preload).
struct CacheImporter<'a> {
    cache: &'a HashMap<String, PackageId>,
    unsafe_pkg: PackageId,
}

impl Importer for CacheImporter<'_> {
    fn import(&mut self, ctx: &mut ImportCtx<'_>, path: &str) -> Option<PackageId> {
        if path == "unsafe" {
            return Some(self.unsafe_pkg);
        }
        let &pkg = self.cache.get(path)?;
        if ctx.packages.get(pkg).complete() {
            Some(pkg)
        } else {
            None
        }
    }
}
