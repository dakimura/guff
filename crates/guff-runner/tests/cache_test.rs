//! R10: persistent issues cache — hit/miss and invalidation on file change.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use guff_packages::{typecheck_package, LoadMode, Package, TypecheckEnv};
use guff_runner::{run_on_packages, CacheStats, IssueCache, RunnerOptions};
use guff_types::default_sizes;
use tempfile::TempDir;

fn copy_fixture(name: &str, dest: &Path) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/smoke")
        .join(name);
    fs::create_dir_all(dest).unwrap();
    for entry in fs::read_dir(&src).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), dest.join(entry.file_name())).unwrap();
    }
}

fn load_pkg(dir: &Path, id: &str) -> Arc<Package> {
    let mut pkg = Package {
        id: id.into(),
        pkg_path: id.into(),
        dir: dir.to_path_buf(),
        compiled_go_files: vec![dir.join("main.go")],
        ..Package::default()
    };
    let fset = guff::position::FileSet::new();
    // Let inference pick the driver's hasher rather than naming it here.
    let export_paths = Default::default();
    let dep_graph = Default::default();
    typecheck_package(
        &mut pkg,
        &fset,
        &export_paths,
        &dep_graph,
        default_sizes(),
        &TypecheckEnv::default(),
        LoadMode::LOAD_SYNTAX,
    );
    Arc::new(pkg)
}

fn diag_keys(diags: &[(String, guff_analysis::Diagnostic)]) -> Vec<(String, String)> {
    let mut keys: Vec<_> = diags
        .iter()
        .map(|(id, d)| (id.clone(), d.message.clone()))
        .collect();
    keys.sort();
    keys
}

#[test]
fn second_run_hits_cache() {
    let tmp = TempDir::new().unwrap();
    let pkg_dir = tmp.path().join("printf");
    copy_fixture("printf", &pkg_dir);
    let cache = Arc::new(IssueCache::open(tmp.path().join("cache"), "test-salt").unwrap());
    let pkg = load_pkg(&pkg_dir, "example.com/smoke/printf");
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);

    let analyzers = [guff_analysis::passes::printf_analyzer()];
    let opts = RunnerOptions {
        sequential: true,
        cache: Some(Arc::clone(&cache)),
        ..RunnerOptions::default()
    };

    let first = run_on_packages(&analyzers, &[Arc::clone(&pkg)], &opts).expect("first");
    assert_eq!(
        first.cache_stats,
        CacheStats {
            hits: 0,
            misses: 1,
            hit_packages: vec![],
            miss_packages: vec![pkg.pkg_path.clone()],
        }
    );
    assert!(first.cached_diagnostics.is_empty());
    let fresh = diag_keys(&first.diagnostics());
    assert!(!fresh.is_empty(), "printf fixture should report");

    let second = run_on_packages(&analyzers, &[pkg], &opts).expect("second");
    assert_eq!(second.cache_stats.hits, 1);
    assert_eq!(second.cache_stats.misses, 0);
    assert!(second.graph.roots.is_empty(), "cache hit skips analysis");
    assert_eq!(diag_keys(&second.diagnostics()), fresh);
}

#[test]
fn changed_file_only_that_package_misses() {
    let tmp = TempDir::new().unwrap();
    let printast_dir = tmp.path().join("printast");
    let printf_dir = tmp.path().join("printf");
    copy_fixture("printast", &printast_dir);
    copy_fixture("printf", &printf_dir);

    let cache_dir = tmp.path().join("cache");
    let cache = Arc::new(IssueCache::open(cache_dir.clone(), "test-salt").unwrap());
    let printast = load_pkg(&printast_dir, "example.com/smoke/printast");
    let printf = load_pkg(&printf_dir, "example.com/smoke/printf");
    let analyzers = [
        guff_analysis::passes::printast_analyzer(),
        guff_analysis::passes::printf_analyzer(),
    ];
    let opts = RunnerOptions {
        sequential: true,
        cache: Some(Arc::clone(&cache)),
        ..RunnerOptions::default()
    };

    let warm = run_on_packages(
        &analyzers,
        &[Arc::clone(&printast), Arc::clone(&printf)],
        &opts,
    )
    .expect("warm");
    assert_eq!(warm.cache_stats.misses, 2);

    let mut body = fs::read_to_string(printf_dir.join("main.go")).unwrap();
    body.push_str("\n// cache-bust\n");
    fs::write(printf_dir.join("main.go"), &body).unwrap();

    let cache2 = Arc::new(IssueCache::open(cache_dir, "test-salt").unwrap());
    let opts2 = RunnerOptions {
        sequential: true,
        cache: Some(cache2),
        ..RunnerOptions::default()
    };
    let printf2 = load_pkg(&printf_dir, "example.com/smoke/printf");

    let result = run_on_packages(&analyzers, &[printast, printf2], &opts2).expect("partial");

    assert_eq!(result.cache_stats.hits, 1);
    assert_eq!(result.cache_stats.misses, 1);
    assert!(result
        .cache_stats
        .hit_packages
        .iter()
        .any(|p| p.contains("printast")));
    assert!(result
        .cache_stats
        .miss_packages
        .iter()
        .any(|p| p.contains("printf")));
}
