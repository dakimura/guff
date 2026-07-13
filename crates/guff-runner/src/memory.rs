//! Optional memory trimming after analysis (P6-d / PL06).
//!
//! Full `decUse` semantics from golangci-lint `runner_loadingpackage.go` are
//! deferred; this module documents the intended hook and provides a minimal
//! implementation behind [`RunnerOptions::release_memory`].

use std::sync::Arc;

use guff_packages::Package;

/// Drops heavy package fields that are no longer needed after analysis.
///
/// When `keep_syntax` is true (root / initial packages), syntax trees are
/// retained for fix applications and diagnostics.
pub fn trim_package_memory(pkg: &mut Package, keep_syntax: bool) {
    if !keep_syntax {
        pkg.syntax.clear();
    }
    pkg.type_artifacts = None;
    if !keep_syntax {
        pkg.types_info = None;
        pkg.fset = None;
        pkg.types = None;
    }
}

/// Applies [`trim_package_memory`] to every package referenced by `packages`.
pub fn trim_packages(packages: &mut [Arc<Package>], root_ids: &[String]) {
    let roots: std::collections::HashSet<&str> =
        root_ids.iter().map(String::as_str).collect();
    for pkg in packages {
        let keep = roots.contains(pkg.id.as_str());
        trim_package_memory(Arc::make_mut(pkg), keep);
    }
}
