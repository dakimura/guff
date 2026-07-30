//! Import path → filesystem directory via workspace modules + requires + GOROOT.

use std::path::{Path, PathBuf};

use guff_build::{module_import_dir, ModFile, Replace};

use crate::bail::{Bail, BailReason};
use crate::modcache::{module_dir, ModCache};
use crate::workspace::{Workspace, WorkspaceModule};

/// Module that owns a resolved package directory.
#[derive(Debug, Clone)]
pub struct ResolvedModule {
    pub path: String,
    pub version: String,
    pub dir: PathBuf,
    pub go_mod: PathBuf,
    pub main: bool,
    pub indirect: bool,
    pub go_version: String,
    pub standard: bool,
    /// Package id as reported by `go list` (may be `vendor/...`).
    pub pkg_id: String,
}

/// Resolves `import_path` to a package directory.
///
/// `from_standard` is true when the importing package lives in GOROOT (so
/// `GOROOT/src/vendor/` wins over GOMODCACHE, matching `go list`).
pub fn resolve_import(
    import_path: &str,
    workspace: &Workspace,
    cache: &ModCache,
    goroot: &Path,
    from_standard: bool,
) -> Result<(PathBuf, ResolvedModule), Bail> {
    if import_path == "unsafe" || import_path == "C" {
        return Err(Bail::new(
            BailReason::UnresolvedImport,
            format!("{import_path} is handled by the caller"),
        ));
    }

    // 1. Any workspace `use` module (all are main modules).
    if let Some(wm) = workspace.module_for_import(import_path) {
        if let Some(dir) = module_import_dir(&wm.dir, &wm.mod_file.module_path, import_path) {
            if dir.is_dir() {
                return Ok((
                    dir,
                    ResolvedModule {
                        path: wm.mod_file.module_path.clone(),
                        version: String::new(),
                        dir: wm.dir.clone(),
                        go_mod: wm.dir.join("go.mod"),
                        main: true,
                        indirect: false,
                        go_version: wm.mod_file.go_version.clone().unwrap_or_default(),
                        standard: false,
                        pkg_id: import_path.to_string(),
                    },
                ));
            }
        }
    }

    // 2. GOROOT vendor — only when the importer is itself in GOROOT.
    if from_standard && !goroot.as_os_str().is_empty() {
        let vendor_dir = goroot.join("src").join("vendor").join(import_path);
        if vendor_dir.is_dir() {
            return Ok((
                vendor_dir,
                ResolvedModule {
                    path: String::new(),
                    version: String::new(),
                    dir: goroot.to_path_buf(),
                    go_mod: PathBuf::new(),
                    main: false,
                    indirect: false,
                    go_version: String::new(),
                    standard: true,
                    pkg_id: format!("vendor/{import_path}"),
                },
            ));
        }
    }

    // 3. replace / require → GOMODCACHE (workspace replaces first).
    if let Some(resolved) = resolve_via_modules(import_path, workspace, cache)? {
        return Ok(resolved);
    }

    // 4. GOROOT stdlib.
    if !goroot.as_os_str().is_empty() {
        let dir = goroot.join("src").join(import_path);
        if dir.is_dir() {
            return Ok((
                dir,
                ResolvedModule {
                    path: String::new(),
                    version: String::new(),
                    dir: goroot.to_path_buf(),
                    go_mod: PathBuf::new(),
                    main: false,
                    indirect: false,
                    go_version: String::new(),
                    standard: true,
                    pkg_id: import_path.to_string(),
                },
            ));
        }
    }

    Err(Bail::new(
        BailReason::UnresolvedImport,
        format!("cannot find package {import_path:?}"),
    ))
}

fn resolve_via_modules(
    import_path: &str,
    workspace: &Workspace,
    cache: &ModCache,
) -> Result<Option<(PathBuf, ResolvedModule)>, Bail> {
    let Some((mod_path, version, indirect, replace, anchor)) =
        select_module(import_path, workspace)
    else {
        return Ok(None);
    };

    let (mod_dir, effective_path, effective_version) = match replace {
        Some(r) if r.new_version.is_empty() => {
            let local = if Path::new(&r.new_path).is_absolute() {
                PathBuf::from(&r.new_path)
            } else {
                // Relative replace is relative to the file that declared it:
                // workspace replace → go.work dir; module replace → that module.
                anchor.join(&r.new_path)
            };
            let local = local.canonicalize().map_err(|e| {
                Bail::new(
                    BailReason::Io,
                    format!("replace {} => {}: {e}", r.old_path, r.new_path),
                )
            })?;
            (local, r.old_path.clone(), String::new())
        }
        Some(r) => {
            let dir = module_dir(&cache.root, &r.new_path, &r.new_version).ok_or_else(|| {
                Bail::new(
                    BailReason::UnresolvedImport,
                    format!("cannot escape replace target {}", r.new_path),
                )
            })?;
            if !dir.is_dir() {
                return Err(Bail::new(
                    BailReason::ModuleNotInCache,
                    format!("{}@{} not in GOMODCACHE", r.new_path, r.new_version),
                ));
            }
            (dir, r.old_path.clone(), r.new_version.clone())
        }
        None => {
            let dir = module_dir(&cache.root, &mod_path, &version).ok_or_else(|| {
                Bail::new(
                    BailReason::UnresolvedImport,
                    format!("cannot escape module path {mod_path}"),
                )
            })?;
            if !dir.is_dir() {
                return Err(Bail::new(
                    BailReason::ModuleNotInCache,
                    format!(
                        "{mod_path}@{version} not in GOMODCACHE ({})",
                        cache.root.display()
                    ),
                ));
            }
            (dir, mod_path.clone(), version.clone())
        }
    };

    let pkg_dir = module_import_dir(&mod_dir, &effective_path, import_path).ok_or_else(|| {
        Bail::new(
            BailReason::UnresolvedImport,
            format!("{import_path} not under module {effective_path}"),
        )
    })?;
    if !pkg_dir.is_dir() {
        return Err(Bail::new(
            BailReason::UnresolvedImport,
            format!(
                "package dir missing for {import_path} under {}",
                pkg_dir.display()
            ),
        ));
    }

    Ok(Some((
        pkg_dir,
        ResolvedModule {
            path: effective_path,
            version: effective_version,
            dir: mod_dir.clone(),
            go_mod: mod_dir.join("go.mod"),
            main: false,
            indirect,
            go_version: String::new(),
            standard: false,
            pkg_id: import_path.to_string(),
        },
    )))
}

/// Longest-prefix require (after workspace + module replaces) matching `import_path`.
///
/// Same-path requires across workspace modules are resolved with a crude MVS:
/// the highest version string wins (semver-ish via Go's version sort rules for
/// common `vN.N.N` forms; good enough for listing).
///
/// Returns `(module_path, version, indirect, replace, replace_anchor_dir)`.
fn select_module(
    import_path: &str,
    workspace: &Workspace,
) -> Option<(String, String, bool, Option<Replace>, PathBuf)> {
    let mut best: Option<(String, String, bool, Option<Replace>, PathBuf)> = None;
    let mut best_len = 0usize;

    // Workspace-level replace alone can introduce a module.
    for r in &workspace.replaces {
        if !path_prefix_match(import_path, &r.old_path) {
            continue;
        }
        if r.old_path.len() < best_len {
            continue;
        }
        best = Some((
            r.old_path.clone(),
            r.old_version.clone(),
            false,
            Some(r.clone()),
            workspace.root.clone(),
        ));
        best_len = r.old_path.len();
    }

    for wm in &workspace.modules {
        for req in &wm.mod_file.requires {
            if !path_prefix_match(import_path, &req.path) {
                continue;
            }
            if req.path.len() < best_len {
                continue;
            }
            if req.path.len() == best_len {
                if let Some((_, ref ver, _, _, _)) = best {
                    if version_cmp(&req.version, ver) != std::cmp::Ordering::Greater {
                        continue;
                    }
                }
            }
            let replace = find_replace_ws(workspace, wm, &req.path, &req.version);
            let anchor = match &replace {
                Some(r) if workspace.replaces.iter().any(|w| w == r) => workspace.root.clone(),
                _ => wm.dir.clone(),
            };
            best = Some((
                req.path.clone(),
                req.version.clone(),
                req.indirect,
                replace,
                anchor,
            ));
            best_len = req.path.len();
        }
        for r in &wm.mod_file.replaces {
            if !path_prefix_match(import_path, &r.old_path) {
                continue;
            }
            if r.old_path.len() < best_len {
                continue;
            }
            if workspace
                .replaces
                .iter()
                .any(|w| w.old_path == r.old_path)
            {
                continue;
            }
            best = Some((
                r.old_path.clone(),
                r.old_version.clone(),
                false,
                Some(r.clone()),
                wm.dir.clone(),
            ));
            best_len = r.old_path.len();
        }
    }

    let _ = best_len;
    best
}

/// Compare Go module versions roughly (`v1.2.3` / pseudo-versions).
fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    parse_semver(a).cmp(&parse_semver(b))
}

fn parse_semver(v: &str) -> (u64, u64, u64, String) {
    let v = v.strip_prefix('v').unwrap_or(v);
    let (num, rest) = match v.find('-') {
        Some(i) => (&v[..i], &v[i..]),
        None => (v, ""),
    };
    let mut parts = num.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch, rest.to_string())
}

fn find_replace_ws(
    workspace: &Workspace,
    wm: &WorkspaceModule,
    path: &str,
    version: &str,
) -> Option<Replace> {
    // Workspace replaces take precedence.
    if let Some(r) = workspace.replaces.iter().find(|r| {
        r.old_path == path && (r.old_version.is_empty() || r.old_version == version)
    }) {
        return Some(r.clone());
    }
    wm.mod_file.replaces.iter().find_map(|r| {
        if r.old_path != path {
            return None;
        }
        if !r.old_version.is_empty() && r.old_version != version {
            return None;
        }
        Some(r.clone())
    })
}

fn path_prefix_match(import_path: &str, module_path: &str) -> bool {
    import_path == module_path
        || (import_path.starts_with(module_path)
            && import_path.as_bytes().get(module_path.len()) == Some(&b'/'))
}

/// Convenience for single-module callers / tests.
#[allow(dead_code)]
pub fn resolve_import_single(
    import_path: &str,
    module_root: &Path,
    mod_file: &ModFile,
    cache: &ModCache,
    goroot: &Path,
    from_standard: bool,
) -> Result<(PathBuf, ResolvedModule), Bail> {
    let ws = Workspace {
        root: module_root.to_path_buf(),
        go_version: mod_file.go_version.clone(),
        modules: vec![WorkspaceModule {
            dir: module_root.to_path_buf(),
            mod_file: mod_file.clone(),
        }],
        replaces: Vec::new(),
    };
    resolve_import(import_path, &ws, cache, goroot, from_standard)
}
