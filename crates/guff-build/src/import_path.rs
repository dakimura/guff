//! Import path resolution.
//!
//! Port of `build.Context.Import` (module mode subset) and `IsLocalImport`.

use std::path::{Path, PathBuf};

use crate::context::Context;
use crate::import_dir::abs_dir;
use crate::module::{find_module_root, module_import_dir, parse_mod_file};
use crate::package::{BuildError, ImportMode, Package};

/// Reports whether `path` is a local import (`"."`, `".."`, `"./foo"`, etc.).
///
/// Equivalent to `build.IsLocalImport`.
pub fn is_local_import(path: &str) -> bool {
    path == "." || path == ".." || path.starts_with("./") || path.starts_with("../")
}

impl Context {
    /// Returns details about the Go package named by `import_path`.
    ///
    /// `src_dir` is the directory containing the importing file, used to resolve
    /// local imports and locate the module root. Pass the package directory for
    /// [`import_dir`](Self::import_dir)-style lookups (`"."` + `src_dir`).
    ///
    /// Equivalent to `build.Context.Import` (Phase 1 subset: module + GOROOT/GOPATH).
    pub fn import(
        &self,
        import_path: &str,
        src_dir: &Path,
        mode: ImportMode,
    ) -> Result<Package, BuildError> {
        if import_path.is_empty() {
            return Err(BuildError::Import(format!(
                "import {import_path:?}: invalid import path"
            )));
        }

        let mut pkg = Package {
            import_path: import_path.to_string(),
            ..Package::default()
        };

        pkg.dir = self.resolve_package_dir(import_path, src_dir)?;
        self.fill_roots(&mut pkg);

        if mode.contains(ImportMode::FIND_ONLY) {
            return Ok(pkg);
        }

        self.load_package_files(&mut pkg)?;
        Ok(pkg)
    }

    fn resolve_package_dir(&self, import_path: &str, src_dir: &Path) -> Result<PathBuf, BuildError> {
        if is_local_import(import_path) {
            let base = abs_dir(self, src_dir)?;
            let dir = base.join(import_path);
            return dir.canonicalize().map_err(BuildError::Io);
        }

        if import_path.starts_with('/') {
            return Err(BuildError::Import(format!(
                "import {import_path:?}: cannot import absolute path"
            )));
        }

        if let Some(dir) = self.resolve_module_import(import_path, src_dir)? {
            return dir.canonicalize().map_err(BuildError::Io);
        }

        if !self.goroot.is_empty() {
            let dir = Path::new(&self.goroot).join("src").join(import_path);
            if dir.is_dir() {
                return Ok(dir);
            }
        }

        for gopath_entry in self.gopath().iter() {
            let dir = Path::new(gopath_entry)
                .join("src")
                .join(import_path);
            if dir.is_dir() {
                return Ok(dir);
            }
        }

        Err(BuildError::Import(format!(
            "cannot find package {import_path:?}"
        )))
    }

    fn resolve_module_import(
        &self,
        import_path: &str,
        src_dir: &Path,
    ) -> Result<Option<PathBuf>, BuildError> {
        let abs_src = abs_dir(self, src_dir)?;
        let module_root = match find_module_root(&abs_src) {
            Some(root) => root,
            None => return Ok(None),
        };
        let mod_file = parse_mod_file(&module_root.join("go.mod"))?;
        let dir = module_import_dir(&module_root, &mod_file.module_path, import_path);
        Ok(dir.filter(|d| d.is_dir()))
    }

    pub(crate) fn fill_roots(&self, pkg: &mut Package) {
        if !self.goroot.is_empty() {
            let goroot_src = Path::new(&self.goroot).join("src");
            if has_subdir(&goroot_src, &pkg.dir) {
                pkg.goroot = true;
                pkg.root = self.goroot.clone();
                if pkg.import_path == "." {
                    if let Some(sub) = subdir_path(&goroot_src, &pkg.dir) {
                        pkg.import_path = sub;
                    }
                }
                return;
            }
        }

        for entry in self.gopath() {
            let src_root = Path::new(&entry).join("src");
            if let Some(sub) = subdir_path(&src_root, &pkg.dir) {
                if pkg.import_path == "." {
                    pkg.import_path = sub;
                }
                pkg.root = entry.clone();
                return;
            }
        }

        if let Some(module_root) = find_module_root(&pkg.dir) {
            if let Ok(mod_file) = parse_mod_file(&module_root.join("go.mod")) {
                pkg.root = module_root.to_string_lossy().into_owned();
                if pkg.import_path == "." {
                    if let Some(sub) = subdir_within(&module_root, &mod_file.module_path, &pkg.dir) {
                        pkg.import_path = sub;
                    }
                }
            }
        }
    }

    fn gopath(&self) -> Vec<String> {
        let sep = if cfg!(windows) { ';' } else { ':' };
        self.gopath
            .split(sep)
            .filter(|p| !p.is_empty() && *p != self.goroot)
            .map(|s| s.to_string())
            .collect()
    }
}

/// Returns the import-path suffix if `dir` is under `root`.
fn subdir_path(root: &Path, dir: &Path) -> Option<String> {
    let root = root.canonicalize().ok()?;
    let dir = dir.canonicalize().ok()?;
    let rel = dir.strip_prefix(&root).ok()?;
    if rel.as_os_str().is_empty() {
        return None;
    }
    Some(rel.to_string_lossy().replace('\\', "/"))
}

fn subdir_within(module_root: &Path, module_path: &str, dir: &Path) -> Option<String> {
    let rel = dir.strip_prefix(module_root).ok()?;
    let rel = rel.to_string_lossy().replace('\\', "/");
    let rel = rel.trim_start_matches('/');
    if rel.is_empty() {
        Some(module_path.to_string())
    } else {
        Some(format!("{module_path}/{rel}"))
    }
}

fn has_subdir(root: &Path, dir: &Path) -> bool {
    subdir_path(root, dir).is_some()
}

trait ImportModeExt {
    fn contains(self, flag: ImportMode) -> bool;
}

impl ImportModeExt for ImportMode {
    fn contains(self, flag: ImportMode) -> bool {
        self.0 & flag.0 != 0
    }
}
