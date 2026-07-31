//! Load a package from a directory on disk.
//!
//! Port of `build.Context.ImportDir` and the directory scan in `Import`.

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::context::Context;
use crate::go_source::parse_go_file_info;
use crate::package::{BuildError, MultiplePackageError, NoGoError, Package};

impl Context {
    /// Loads the Go package in the named directory.
    ///
    /// Equivalent to `build.Context.ImportDir`. Includes `*_test.go` files.
    pub fn import_dir(&self, dir: impl AsRef<Path>) -> Result<Package, BuildError> {
        self.import_dir_with(dir, true)
    }

    /// Like [`Context::import_dir`], but when `include_tests` is false skips
    /// every `*_test.go` without opening it.
    ///
    /// Native `go list` only needs test files for pattern roots (`cfg.tests &&
    /// is_root`); dependency packages pay ~25% of header opens on `_test.go`
    /// that are then thrown away. Skipping them is the main empty-cold lever
    /// left in `load_package_files`.
    pub fn import_dir_with(
        &self,
        dir: impl AsRef<Path>,
        include_tests: bool,
    ) -> Result<Package, BuildError> {
        let dir = abs_or_canonicalize(dir.as_ref())?;
        let mut pkg = Package {
            dir: dir.clone(),
            import_path: ".".to_string(),
            ..Package::default()
        };
        self.fill_roots(&mut pkg);
        self.load_package_files(&mut pkg, include_tests)?;
        Ok(pkg)
    }

    /// Classifies `.go` files in `pkg.dir` into [`Package`] file lists.
    pub(crate) fn load_package_files(
        &self,
        pkg: &mut Package,
        include_tests: bool,
    ) -> Result<(), BuildError> {
        let mut all_tags = HashSet::new();
        let mut first_file: Option<String> = None;
        let mut first_pkg = String::new();
        let mut imports = Vec::new();
        let mut test_imports = Vec::new();
        let mut xtest_imports = Vec::new();
        let mut seen_imp = HashSet::new();
        let mut seen_test_imp = HashSet::new();
        let mut seen_xtest_imp = HashSet::new();

        let entries = fs::read_dir(&pkg.dir)?;
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect();
        names.sort();

        for name in names {
            if !name.ends_with(".go") {
                continue;
            }
            // Go's matchFile rejects these without reading; keep them out of
            // ignored_go_files too.
            if name.starts_with('_') || name.starts_with('.') {
                continue;
            }
            if !include_tests && name.ends_with("_test.go") {
                continue;
            }
            // Filename `_$GOOS` / `_$GOARCH` gates need no open (C-3c hot path).
            if !self.use_all_files && !self.good_os_arch_file(&name, &mut None) {
                pkg.ignored_go_files.push(name);
                continue;
            }

            let path = pkg.dir.join(&name);
            // Build tags + package + imports live in the file header. Full-file
            // reads dominate listing of GOMODCACHE (C-3c). 64 KiB covers large
            // leading block comments (e.g. math/big/natdiv.go places `package`
            // past 24 KiB) while still avoiding a full read of huge std files.
            let content = match read_go_header(&path) {
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
                pkg.ignored_go_files.push(name);
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
                    for imp in info.imports {
                        if seen_imp.insert(imp.clone()) {
                            imports.push(imp);
                        }
                    }
                } else {
                    pkg.ignored_go_files.push(name);
                }
            } else if is_xtest {
                pkg.xtest_go_files.push(name);
                for imp in info.imports {
                    if seen_xtest_imp.insert(imp.clone()) {
                        xtest_imports.push(imp);
                    }
                }
            } else if is_test {
                pkg.test_go_files.push(name);
                for imp in info.imports {
                    if seen_test_imp.insert(imp.clone()) {
                        test_imports.push(imp);
                    }
                }
            } else {
                pkg.go_files.push(name);
                for imp in info.imports {
                    if seen_imp.insert(imp.clone()) {
                        imports.push(imp);
                    }
                }
            }
        }

        pkg.imports = imports;
        pkg.test_imports = test_imports;
        pkg.xtest_imports = xtest_imports;
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

/// Bytes to read from each `.go` file for build-tag / package / import scan.
///
/// Must clear large leading documentation comments — `math/big/natdiv.go` puts
/// `package` past 24 KiB. Shrinking this broke `GUFF_NATIVE_LIST=verify`.
const GO_HEADER_BYTES: u64 = 64 * 1024;

fn read_go_header(path: &Path) -> std::io::Result<Vec<u8>> {
    let f = fs::File::open(path)?;
    let mut buf = Vec::with_capacity(GO_HEADER_BYTES as usize);
    let mut take = f.take(GO_HEADER_BYTES);
    take.read_to_end(&mut buf)?;
    Ok(buf)
}

/// Prefer the path as-is when absolute; canonicalize only when needed.
fn abs_or_canonicalize(dir: &Path) -> Result<PathBuf, BuildError> {
    if dir.is_absolute() {
        // Avoid canonicalize() on the listing hot path — it is a syscall per
        // package and dominates GOMODCACHE walks on Darwin.
        return Ok(dir.to_path_buf());
    }
    dir.canonicalize().map_err(BuildError::Io)
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
