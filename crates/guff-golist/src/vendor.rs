//! Parse `vendor/modules.txt` and resolve imports from `vendor/`.
//!
//! Format matches `cmd/go/internal/modload.readVendorList` (Go 1.14+):
//! - `# module path version [=> replacement…]` — module header
//! - `## explicit; go 1.XX` — metadata for the preceding module
//! - bare import path — package provided by that module under `vendor/<path>`

use std::fs;
use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use crate::bail::{Bail, BailReason};

/// Index of packages available under a module's `vendor/` directory.
#[derive(Debug, Clone, Default)]
pub struct VendorIndex {
    /// Absolute path to `…/vendor`.
    pub dir: PathBuf,
    /// Import path → owning module metadata.
    pub packages: FxHashMap<String, VendorModule>,
}

/// Module that owns one or more packages in `vendor/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorModule {
    pub path: String,
    pub version: String,
    pub go_version: String,
}

/// Loads `vendor/modules.txt` when present.
///
/// Returns `Ok(None)` when there is no `vendor/` directory (non-vendored module).
/// Returns `Err(BailReason::Vendor)` when `vendor/` exists but `modules.txt` is
/// missing or empty of packages (unknown / pre-modules layout).
pub fn load_vendor_index(module_root: &Path) -> Result<Option<VendorIndex>, Bail> {
    let vendor_dir = module_root.join("vendor");
    if !vendor_dir.is_dir() {
        return Ok(None);
    }
    let modules_txt = vendor_dir.join("modules.txt");
    if !modules_txt.is_file() {
        return Err(Bail::new(
            BailReason::Vendor,
            "vendor/ present but vendor/modules.txt missing",
        ));
    }
    let text = fs::read_to_string(&modules_txt).map_err(|e| {
        Bail::new(
            BailReason::Io,
            format!("read {}: {e}", modules_txt.display()),
        )
    })?;
    let index = parse_modules_txt(&vendor_dir, &text)?;
    if index.packages.is_empty() {
        return Err(Bail::new(
            BailReason::Vendor,
            "vendor/modules.txt has no packages",
        ));
    }
    Ok(Some(index))
}

/// Parses `modules.txt` body. `vendor_dir` is stored on the index as-is.
pub fn parse_modules_txt(vendor_dir: &Path, text: &str) -> Result<VendorIndex, Bail> {
    let mut packages = FxHashMap::default();
    let mut cur_path = String::new();
    let mut cur_version = String::new();
    let mut cur_go_version = String::new();

    for raw in text.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("# ") {
            // Module header — reset current module.
            cur_path.clear();
            cur_version.clear();
            cur_go_version.clear();
            let fields: Vec<&str> = rest.split_whitespace().collect();
            if fields.len() < 2 {
                continue;
            }
            if is_valid_semver(fields[1]) {
                // `# path version [=> …]`
                cur_path = fields[0].to_string();
                cur_version = fields[1].to_string();
            } else if fields[1] == "=>" {
                // `# path => replacement` (wildcard replace annotation)
                cur_path = fields[0].to_string();
                cur_version = String::new();
            }
            // else: unrecognised header — leave cur_path empty so packages are skipped
            continue;
        }

        if cur_path.is_empty() {
            continue;
        }

        if let Some(annotations) = line.strip_prefix("## ") {
            for entry in annotations.split(';') {
                let entry = entry.trim();
                if let Some(ver) = entry.strip_prefix("go ") {
                    cur_go_version = ver.trim().to_string();
                }
                // `explicit` and unknown tokens ignored for listing.
            }
            continue;
        }

        // Package line: single import-path field.
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() != 1 {
            continue;
        }
        let pkg = fields[0];
        if !looks_like_import_path(pkg) {
            continue;
        }
        packages.insert(
            pkg.to_string(),
            VendorModule {
                path: cur_path.clone(),
                version: cur_version.clone(),
                go_version: cur_go_version.clone(),
            },
        );
    }

    Ok(VendorIndex {
        dir: vendor_dir.to_path_buf(),
        packages,
    })
}

impl VendorIndex {
    /// Directory containing sources for `import_path`, if listed in modules.txt.
    pub fn package_dir(&self, import_path: &str) -> Option<PathBuf> {
        if !self.packages.contains_key(import_path) {
            return None;
        }
        let dir = self.dir.join(import_path);
        dir.is_dir().then_some(dir)
    }

    pub fn module_for(&self, import_path: &str) -> Option<&VendorModule> {
        self.packages.get(import_path)
    }
}

/// Go's `semver.IsValid` subset used by modules.txt headers (`v1.2.3`,
/// pseudo-versions, `v0.0.0`).
fn is_valid_semver(v: &str) -> bool {
    if !v.starts_with('v') || v.len() < 2 {
        return false;
    }
    let rest = &v[1..];
    // Must start with a digit.
    rest.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn looks_like_import_path(s: &str) -> bool {
    !s.is_empty()
        && !s.contains(' ')
        && !s.starts_with('.')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '-' | '_' | '~' | '+'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_and_replace_modules_txt() {
        let text = r#"
# example.com/dep v0.0.0 => ../dep
## explicit; go 1.22
example.com/dep
# golang.org/x/sync v0.8.0
## explicit; go 1.18
golang.org/x/sync/errgroup
# example.com/dep => ../dep
"#;
        let idx = parse_modules_txt(Path::new("/mod/vendor"), text).unwrap();
        assert_eq!(idx.packages.len(), 2);
        let dep = idx.module_for("example.com/dep").unwrap();
        assert_eq!(dep.path, "example.com/dep");
        assert_eq!(dep.version, "v0.0.0");
        assert_eq!(dep.go_version, "1.22");
        let sync = idx.module_for("golang.org/x/sync/errgroup").unwrap();
        assert_eq!(sync.path, "golang.org/x/sync");
        assert_eq!(sync.version, "v0.8.0");
        assert_eq!(sync.go_version, "1.18");
    }

    #[test]
    fn empty_vendor_dir_is_none() {
        let tmp = std::env::temp_dir().join(format!(
            "guff-vendor-none-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        assert!(load_vendor_index(&tmp).unwrap().is_none());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn vendor_without_modules_txt_bails() {
        let tmp = std::env::temp_dir().join(format!(
            "guff-vendor-bail-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("vendor")).unwrap();
        let err = load_vendor_index(&tmp).unwrap_err();
        assert_eq!(err.reason, BailReason::Vendor);
        let _ = fs::remove_dir_all(&tmp);
    }
}
