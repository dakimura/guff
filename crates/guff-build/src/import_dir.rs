//! Load a package from a directory on disk.
//!
//! Port of `build.Context.ImportDir` and the directory scan in `Import`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::context::Context;
use crate::go_source::parse_go_file_info;
use crate::package::{BuildError, MultiplePackageError, NoGoError, Package};

impl Context {
    /// Loads the Go package in the named directory.
    ///
    /// Equivalent to `build.Context.ImportDir`.
    pub fn import_dir(&self, dir: impl AsRef<Path>) -> Result<Package, BuildError> {
        let dir = dir.as_ref().canonicalize().map_err(BuildError::Io)?;
        let mut pkg = Package {
            dir: dir.clone(),
            import_path: ".".to_string(),
            ..Package::default()
        };
        self.fill_roots(&mut pkg);
        self.load_package_files(&mut pkg)?;
        Ok(pkg)
    }

    /// Classifies `.go` files in `pkg.dir` into [`Package`] file lists.
    pub(crate) fn load_package_files(&self, pkg: &mut Package) -> Result<(), BuildError> {
        let mut all_tags = HashSet::new();
        let mut first_file: Option<String> = None;
        let mut first_pkg = String::new();

        let entries = fs::read_dir(&pkg.dir)?;
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect();
        names.sort();

        for name in names {
            if !name.ends_with(".go") {
                continue;
            }

            let path = pkg.dir.join(&name);
            let content = match fs::read(&path) {
                Ok(c) => c,
                Err(e) => {
                    pkg.invalid_go_files.push(name);
                    return Err(BuildError::Io(e));
                }
            };

            let matched = match self.match_file(&name, &content) {
                Ok(m) => m,
                Err(e) => {
                    pkg.invalid_go_files.push(name);
                    return Err(e.into());
                }
            };

            if !matched {
                if !name.starts_with('_') && !name.starts_with('.') {
                    pkg.ignored_go_files.push(name);
                }
                continue;
            }

            let info = match parse_go_file_info(&content) {
                Ok(i) => i,
                Err(_) => {
                    pkg.invalid_go_files.push(name.clone());
                    continue;
                }
            };

            if info.package_name == "documentation" {
                pkg.ignored_go_files.push(name);
                continue;
            }

            let is_test = name.ends_with("_test.go");
            let mut pkg_name = info.package_name.clone();
            let is_xtest = is_test && pkg_name.ends_with("_test") && pkg.name != pkg_name;
            if is_xtest {
                pkg_name = pkg_name.trim_end_matches("_test").to_string();
            }

            if pkg.name.is_empty() {
                pkg.name = pkg_name.clone();
                first_pkg = pkg_name;
                first_file = Some(name.clone());
            } else if pkg_name != pkg.name {
                return Err(BuildError::MultiplePackages(MultiplePackageError {
                    dir: pkg.dir.clone(),
                    packages: [first_pkg.clone(), pkg_name],
                    files: [
                        first_file.clone().unwrap_or_default(),
                        name.clone(),
                    ],
                }));
            }

            if info.imports_c {
                all_tags.insert("cgo".to_string());
                if self.cgo_enabled {
                    pkg.cgo_files.push(name);
                } else {
                    pkg.ignored_go_files.push(name);
                }
            } else if is_xtest {
                pkg.xtest_go_files.push(name);
            } else if is_test {
                pkg.test_go_files.push(name);
            } else {
                pkg.go_files.push(name);
            }
        }

        pkg.all_tags = all_tags.into_iter().collect();
        pkg.all_tags.sort();

        if pkg.go_files.is_empty()
            && pkg.cgo_files.is_empty()
            && pkg.test_go_files.is_empty()
            && pkg.xtest_go_files.is_empty()
        {
            return Err(BuildError::NoGo(NoGoError {
                dir: pkg.dir.clone(),
            }));
        }

        Ok(())
    }
}

/// Returns the absolute path of `dir`, using `ctxt.dir` as base when relative.
pub(crate) fn abs_dir(ctxt: &Context, dir: &Path) -> Result<PathBuf, BuildError> {
    if dir.is_absolute() {
        return Ok(dir.to_path_buf());
    }
    if !ctxt.dir.is_empty() {
        return Err(BuildError::Import(format!(
            "Dir is non-empty, so relative path is not allowed: {}",
            dir.display()
        )));
    }
    std::env::current_dir()
        .map_err(BuildError::Io)?
        .join(dir)
        .canonicalize()
        .map_err(BuildError::Io)
}
