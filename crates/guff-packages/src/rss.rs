//! RSS category attribution for loaded packages (PERF_TASKS_V2 C-8).
//!
//! Enabled with `GUFF_DEBUG_RSS=1`. Walks type-checked packages and estimates
//! retained bytes for type arenas, `Info` maps, AST syntax, and source bytes.
//! Arc-sharing (seed base, shared `Info`) is deduped via pointer identity.

use std::sync::{Arc, LazyLock};

use guff::ast::{Expr, File};
use guff::walk::{self, NodeRef};
use guff_types::{account_typecheck_arenas, RetainedBytes};

use crate::package::Package;
use crate::hash::HashSet;

static ENABLED: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("GUFF_DEBUG_RSS").is_some());

/// Whether `GUFF_DEBUG_RSS` is set.
pub fn enabled() -> bool {
    *ENABLED
}

/// The process's actual resident size, in bytes.
///
/// [`attribute_packages`] can only name what it can walk, and on prometheus
/// `./...` it names 1.29 GiB of a 2.2 GiB process (PERF_TASKS_V6 §4.1). Naming
/// the gap needs the other number at the same instants, and the gap is not a
/// constant: it matters whether it appears while the seed is being built (a
/// retained structure nobody accounts for) or after (allocator pages that were
/// freed but not returned).
///
/// `ps` rather than a platform crate: this runs a handful of times per run
/// behind `GUFF_DEBUG_RSS`, so ~10ms of subprocess is free, and it keeps a
/// debug-only probe from adding a dependency that ships in every build.
pub fn process_rss_bytes() -> Option<u64> {
    let pid = std::process::id().to_string();
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()?;
    let kib: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kib.saturating_mul(1024))
}

/// Print the process RSS with a label, and how it moved since the last call.
pub fn report_process(label: &str) {
    if !enabled() {
        return;
    }
    let Some(bytes) = process_rss_bytes() else {
        return;
    };
    static LAST: std::sync::Mutex<u64> = std::sync::Mutex::new(0);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let delta = bytes as i64 - *last as i64;
    let first = *last == 0;
    *last = bytes;
    drop(last);
    let mib = |b: f64| b / (1024.0 * 1024.0);
    if first {
        eprintln!("guff:   rss now {:.0} MiB ({label})", mib(bytes as f64));
    } else {
        eprintln!(
            "guff:   rss now {:.0} MiB ({label}, {:+.0} MiB)",
            mib(bytes as f64),
            mib(delta as f64),
        );
    }
}

/// Category totals after walking `packages`.
#[derive(Debug, Default)]
pub struct PackageRssReport {
    pub packages: usize,
    pub with_artifacts: usize,
    pub with_syntax: usize,
    pub arenas: RetainedBytes,
    pub source_bytes: usize,
    pub source_files: usize,
    pub ast_nodes: usize,
    pub ast_bytes_est: usize,
    pub syntax_string_bytes: usize,
}

impl PackageRssReport {
    pub fn attributed_total(&self) -> usize {
        self.arenas.attributed_total()
            + self.source_bytes
            + self.ast_bytes_est
            + self.syntax_string_bytes
    }
}

/// Attribute retained memory held by type-checked packages.
pub fn attribute_packages(packages: &[Arc<Package>]) -> PackageRssReport {
    let mut report = PackageRssReport {
        packages: packages.len(),
        ..PackageRssReport::default()
    };
    let mut seen_src: HashSet<usize> = Default::default();
    let avg_node = std::mem::size_of::<Expr>().max(64);

    for pkg in packages {
        if let Some(art) = pkg.type_artifacts.as_ref() {
            report.with_artifacts += 1;
            account_typecheck_arenas(
                &art.types,
                &art.objects,
                &art.scopes,
                &art.packages,
                &art.info,
                &mut report.arenas,
            );
        } else if let Some(info) = pkg.types_info.as_ref() {
            guff_types::account_info(info, &mut report.arenas);
        }

        for src in &pkg.source_files {
            let ptr = Arc::as_ptr(src) as *const u8 as usize;
            if seen_src.insert(ptr) {
                report.source_bytes = report.source_bytes.saturating_add(src.len());
                report.source_files += 1;
            }
        }

        if !pkg.syntax.is_empty() {
            report.with_syntax += 1;
            let (nodes, str_bytes) = estimate_syntax(&pkg.syntax);
            report.ast_nodes = report.ast_nodes.saturating_add(nodes);
            report.ast_bytes_est = report
                .ast_bytes_est
                .saturating_add(nodes.saturating_mul(avg_node));
            report.syntax_string_bytes = report.syntax_string_bytes.saturating_add(str_bytes);
        }
    }
    report
}

fn estimate_syntax(files: &[File]) -> (usize, usize) {
    let mut nodes = 0usize;
    let mut str_bytes = 0usize;
    for f in files {
        str_bytes = str_bytes.saturating_add(f.go_version.len());
        walk::inspect(NodeRef::File(f), |n| {
            if let Some(NodeRef::Ident(id)) = n {
                str_bytes = str_bytes.saturating_add(id.name.len());
                nodes += 1;
            } else if let Some(NodeRef::BasicLit(lit)) = n {
                str_bytes = str_bytes.saturating_add(lit.value.len());
                nodes += 1;
            } else if n.is_some() {
                nodes += 1;
            }
            true
        });
    }
    (nodes, str_bytes)
}

/// Print a one-shot attribution report to stderr.
pub fn report_packages(label: &str, packages: &[Arc<Package>]) {
    if !enabled() {
        return;
    }
    let r = attribute_packages(packages);
    let mib = |b: usize| b as f64 / (1024.0 * 1024.0);
    eprintln!(
        "guff: rss attribution ({label}): pkgs={} artifacts={} syntax_pkgs={}",
        r.packages, r.with_artifacts, r.with_syntax
    );
    eprintln!(
        "guff:   type arenas: slots types={:.1}MiB objects={:.1}MiB scopes={:.1}MiB \
         packages={:.1}MiB names={:.1}MiB intern={:.1}MiB  (types_total={:.1}MiB)",
        mib(r.arenas.type_slots),
        mib(r.arenas.object_slots),
        mib(r.arenas.scope_slots),
        mib(r.arenas.package_slots),
        mib(r.arenas.name_bytes),
        mib(r.arenas.intern_tables),
        mib(r.arenas.types_total()),
    );
    eprintln!(
        "guff:   Info maps: {:.1}MiB",
        mib(r.arenas.info_maps)
    );
    eprintln!(
        "guff:   source bytes: {:.1}MiB ({} files, Arc-deduped)",
        mib(r.source_bytes),
        r.source_files
    );
    eprintln!(
        "guff:   AST est: {:.1}MiB envelope ({} nodes × ~{}B) + {:.1}MiB strings",
        mib(r.ast_bytes_est),
        r.ast_nodes,
        std::mem::size_of::<Expr>().max(64),
        mib(r.syntax_string_bytes),
    );
    eprintln!(
        "guff:   attributed sum: {:.1}MiB (lower bound; excludes SSA IR, allocator metadata, stacks)",
        mib(r.attributed_total()),
    );
}
