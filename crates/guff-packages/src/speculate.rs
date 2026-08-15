//! Speculative seed prewarm during `go list` (PERF_TASKS_V2 C-7).
//!
//! On `--no-cache` / empty issue-cache runs, `go list` leaves most cores idle for
//! ~1s while format only uses a private 2-thread pool. If a previous run left a
//! golist stdout cache (and stdlib-export map) under `GUFF_CACHE`, we can start
//! [`crate::typecheck::build_source_seed`] against that remembered graph in
//! parallel with the authoritative `go list`. When the real graph's seed inputs
//! match, the speculative [`ExportSeed`] is reused; otherwise it is dropped and
//! the normal path rebuilds.
//!
//! Empty `GUFF_CACHE` has nothing to peek → speculation is a no-op (regress cold).
//! Warm issue-cache runs should not call this (go list is already ~0.07s).

use crate::hash::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use guff::position::FileSet;
use guff_types::ExportSeed;
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::golist::{self, GoListError};
use crate::package::Package;
use crate::typecheck::{self, TypecheckEnv};

/// Fingerprint of everything [`typecheck::build_source_seed`] reads for `targets`.
///
/// Includes, for every package in the transitive seed closure: id, sorted
/// `compiled_go_files`, sorted `deps`, and `export_file` (if it exists on disk).
/// Also folds the sorted target id list so a different miss set cannot reuse a
/// seed built for another set.
pub fn seed_input_fingerprint(
    targets: &[String],
    by_id: &HashMap<String, Arc<Package>>,
) -> String {
    let export_paths = collect_existing_exports(by_id);
    let needed = needed_ids(targets, by_id, &export_paths);

    let mut hasher = Sha256::new();
    hasher.update(b"seed-spec-v1\0");
    let mut sorted_targets = targets.to_vec();
    sorted_targets.sort();
    for t in &sorted_targets {
        hasher.update(t.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(b"\n");
    for id in &needed {
        hasher.update(id.as_bytes());
        hasher.update(b"\0");
        if let Some(pkg) = by_id.get(id) {
            let mut files = pkg
                .compiled_go_files
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            files.sort();
            for f in files {
                hasher.update(f.as_bytes());
                hasher.update(b"\0");
            }
            let mut deps = pkg.deps.clone();
            deps.sort();
            for d in deps {
                hasher.update(d.as_bytes());
                hasher.update(b"\0");
            }
            if let Some(exp) = export_paths.get(id) {
                hasher.update(b"E");
                hasher.update(exp.to_string_lossy().as_bytes());
            } else {
                hasher.update(b"S");
            }
            hasher.update(b"\n");
        }
    }
    hex_encode(hasher.finalize())
}

/// Every package the seed for `targets` reads, in the order the fingerprint
/// folds them.
fn needed_ids(
    targets: &[String],
    by_id: &HashMap<String, Arc<Package>>,
    export_paths: &HashMap<String, PathBuf>,
) -> Vec<String> {
    let mut needed: Vec<String> = Vec::new();
    let mut seen = HashSet::default();
    let mut stack: Vec<String> = Vec::new();
    for id in targets {
        if let Some(pkg) = by_id.get(id) {
            stack.extend(pkg.deps.iter().cloned());
            stack.extend(pkg.imports.keys().cloned());
        }
    }
    while let Some(path) = stack.pop() {
        if path == "unsafe" || path == "C" {
            continue;
        }
        if !seen.insert(path.clone()) {
            continue;
        }
        needed.push(path.clone());
        if let Some(pkg) = by_id.get(&path) {
            stack.extend(pkg.deps.iter().cloned());
        }
    }
    needed.retain(|p| {
        export_paths.contains_key(p)
            || by_id
                .get(p)
                .is_some_and(|pk| !pk.compiled_go_files.is_empty())
    });
    needed.sort();
    needed
}

fn collect_existing_exports(by_id: &HashMap<String, Arc<Package>>) -> HashMap<String, PathBuf> {
    let mut out = HashMap::default();
    for (id, pkg) in by_id {
        if pkg.export_file.as_os_str().is_empty() {
            continue;
        }
        if Path::new(&pkg.export_file).exists() {
            out.insert(id.clone(), pkg.export_file.clone());
        }
    }
    out
}

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Successful speculative seed, ready to inject into [`typecheck::typecheck_roots`].
pub struct SpeculativeSeed {
    pub seed: Arc<ExportSeed>,
    pub fset: Arc<FileSet>,
    pub fingerprint: String,
    pub targets: Vec<String>,
    /// The graph this seed was built from, kept for [`Self::explain_miss`].
    guessed: HashMap<String, Arc<Package>>,
}

impl SpeculativeSeed {
    /// What the guessed graph got wrong, for the debug line on a miss.
    ///
    /// A bare "MISS" is what let C-7 look like it was working for a month: it
    /// never hit on this corpus, and nothing said which of the two inputs —
    /// the target list or the package contents — disagreed. Both have been
    /// wrong at different times, and they have different causes.
    pub fn explain_miss(&self, all: &[Arc<Package>], miss_ids: &[String]) -> String {
        let mut a = self.targets.clone();
        let mut b = miss_ids.to_vec();
        a.sort();
        b.sort();
        if a != b {
            let only_guessed = a.iter().filter(|t| !b.contains(t)).count();
            let only_real = b.iter().filter(|t| !a.contains(t)).count();
            return format!(
                "targets: guessed {} vs real {} ({} only guessed, {} only real)",
                a.len(),
                b.len(),
                only_guessed,
                only_real,
            );
        }
        let real_by_id: HashMap<String, Arc<Package>> =
            all.iter().map(|p| (p.id.clone(), Arc::clone(p))).collect();
        let real_exports = collect_existing_exports(&real_by_id);
        let guessed_exports = collect_existing_exports(&self.guessed);
        let real_needed = needed_ids(miss_ids, &real_by_id, &real_exports);
        let guessed_needed = needed_ids(&self.targets, &self.guessed, &guessed_exports);
        if real_needed != guessed_needed {
            let only_guessed = guessed_needed
                .iter()
                .find(|id| !real_needed.contains(id))
                .cloned();
            let only_real = real_needed
                .iter()
                .find(|id| !guessed_needed.contains(id))
                .cloned();
            return format!(
                "dependency set: guessed {} vs real {} (e.g. only guessed: {:?}, only real: {:?})",
                guessed_needed.len(),
                real_needed.len(),
                only_guessed,
                only_real,
            );
        }
        for id in real_needed.iter().chain(miss_ids.iter()) {
            let real = all.iter().find(|p| &p.id == id);
            let guessed = self.guessed.get(id);
            match (real, guessed) {
                (Some(r), Some(g)) => {
                    if r.compiled_go_files != g.compiled_go_files {
                        return format!(
                            "{id}: compiled_go_files {} vs {}",
                            g.compiled_go_files.len(),
                            r.compiled_go_files.len(),
                        );
                    }
                    let (mut rd, mut gd) = (r.deps.clone(), g.deps.clone());
                    rd.sort();
                    gd.sort();
                    if rd != gd {
                        return format!("{id}: deps {} vs {}", gd.len(), rd.len());
                    }
                    if r.export_file != g.export_file {
                        return format!(
                            "{id}: export_file {:?} vs {:?}",
                            g.export_file, r.export_file
                        );
                    }
                }
                (None, Some(_)) => return format!("{id}: guessed but not in the real graph"),
                (Some(_), None) => return format!("{id}: real but not guessed"),
                (None, None) => {}
            }
        }
        "a dependency outside the target list".to_string()
    }

    /// True when the authoritative miss set and package graph match what we built.
    pub fn matches(&self, all: &[Arc<Package>], miss_ids: &[String]) -> bool {
        let mut a = self.targets.clone();
        let mut b = miss_ids.to_vec();
        a.sort();
        b.sort();
        if a != b {
            return false;
        }
        let by_id: HashMap<String, Arc<Package>> =
            all.iter().map(|p| (p.id.clone(), Arc::clone(p))).collect();
        seed_input_fingerprint(miss_ids, &by_id) == self.fingerprint
    }
}

/// Background job started before `load_graph`.
pub struct SpeculativeSeedJob {
    handle: JoinHandle<Option<SpeculativeSeed>>,
    started: std::time::Instant,
}

impl SpeculativeSeedJob {
    /// Wait for the job and return the seed only on an exact input match.
    pub fn finish_if_matches(
        self,
        all: &[Arc<Package>],
        miss_ids: &[String],
    ) -> Option<SpeculativeSeed> {
        let timing = crate::debug::enabled();
        let waited = self.started.elapsed();
        let built = match self.handle.join() {
            Ok(v) => v,
            Err(_) => {
                if timing {
                    eprintln!("guff:   seed speculate join panicked");
                }
                return None;
            }
        };
        let Some(spec) = built else {
            if timing {
                eprintln!(
                    "guff:   seed speculate miss (no result) after {:.2}s",
                    waited.as_secs_f64(),
                );
            }
            return None;
        };
        if spec.matches(all, miss_ids) {
            if timing {
                eprintln!(
                    "guff:   seed speculate HIT ({:.2}s wall since start, {} targets)",
                    waited.as_secs_f64(),
                    spec.targets.len(),
                );
            }
            Some(spec)
        } else {
            if timing {
                eprintln!(
                    "guff:   seed speculate MISS after {:.2}s ({}); rebuilding",
                    waited.as_secs_f64(),
                    spec.explain_miss(all, miss_ids),
                );
            }
            None
        }
    }
}

/// Peek the golist disk cache (ignoring `disable_cache`) and start seed build.
///
/// Returns `None` when there is nothing useful to speculate from (empty cache,
/// missing stdlib exports, no root packages with sources), or when
/// `GUFF_SEED_SPECULATE=0`.
pub fn start_seed_speculation(
    cfg: &Config,
    patterns: &[String],
    env: &TypecheckEnv,
) -> Option<SpeculativeSeedJob> {
    if !env.from_source {
        return None;
    }
    if let Ok(v) = std::env::var("GUFF_SEED_SPECULATE") {
        let v = v.to_ascii_lowercase();
        if matches!(v.as_str(), "0" | "false" | "off" | "no") {
            return None;
        }
    }
    let peeked = match golist::peek_cached_graph(cfg, patterns) {
        Ok(Some(g)) => g,
        Ok(None) => {
            if crate::debug::enabled() {
                eprintln!("guff:   seed speculate skip (no golist/stdlib cache peek)");
            }
            return None;
        }
        Err(e) => {
            if crate::debug::enabled() {
                eprintln!("guff:   seed speculate skip (peek error: {e})");
            }
            return None;
        }
    };
    let targets: Vec<String> = peeked
        .roots
        .iter()
        .filter(|id| {
            peeked
                .packages
                .iter()
                .find(|p| p.id == **id)
                .is_some_and(|p| !p.compiled_go_files.is_empty())
        })
        .cloned()
        .collect();
    if targets.is_empty() {
        return None;
    }
    let by_id: HashMap<String, Arc<Package>> = peeked
        .packages
        .iter()
        .map(|p| (p.id.clone(), Arc::clone(p)))
        .collect();
    let fingerprint = seed_input_fingerprint(&targets, &by_id);
    let packages = peeked.packages;
    let env = env.clone();
    let started = std::time::Instant::now();
    if crate::debug::enabled() {
        eprintln!(
            "guff:   seed speculate start ({} targets, {} pkgs)",
            targets.len(),
            packages.len(),
        );
    }
    let handle = std::thread::Builder::new()
        .name("guff-seed-speculate".into())
        .spawn(move || {
            let fset = FileSet::new();
            let export_paths = collect_existing_exports(&by_id);
            let dep_graph = crate::dedup::import_path_dep_graph(&by_id);
            let seed =
                typecheck::build_source_seed_for_speculate(
                    &targets,
                    &by_id,
                    &export_paths,
                    &dep_graph,
                    &fset,
                    &env,
                )?;
            Some(SpeculativeSeed {
                seed,
                fset,
                fingerprint,
                targets,
                guessed: by_id,
            })
        })
        .ok()?;
    Some(SpeculativeSeedJob { handle, started })
}

/// Re-export peek error mapping for callers that want details.
pub type PeekError = GoListError;
