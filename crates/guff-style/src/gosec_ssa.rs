//! The SSA program gosec's analyzers share, and the `SrcFuncs` list they walk.
//!
//! gosec ships two kinds of check: rules, which see the AST and type info, and
//! analyzers, which see `buildssa`'s SSA. guff's G602 and G115 are the two
//! analyzers, and both need the same thing — one SSA package for the package
//! under lint, plus the source-level function list.
//!
//! The package is built **privately**, not through the shared `buildir` pass:
//! `buildir` interns one IR per package keyed by its mode flags, so asking it
//! for methods-from-source or build-despite-errors would flip those flags for
//! staticcheck too and move SA5011's findings. That was the reason G602 built
//! its own, and the reason this build is shared between the two analyzers
//! rather than done once per analyzer: enabling gosec must cost one SSA build,
//! not one per SSA rule.

use std::collections::HashSet;

use guff_analysis::Pass;
use guff_ssa::ids::{FuncId, PackageId};
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_ssa::ssautil::build_package_for_analysis;

/// Runs every SSA-based gosec analyzer that `enabled` selects, over one shared
/// SSA build of the package.
pub(crate) fn check_ssa_analyzers(
    pass: &mut Pass<'_>,
    enabled: &HashSet<&'static str>,
    pending: &mut Vec<(u32, u32, String)>,
) {
    let want_g602 = enabled.contains("G602");
    let want_g115 = enabled.contains("G115");
    let want_g118 = enabled.contains("G118");
    let want_g123 = enabled.contains("G123");
    let taint_rules: Vec<&'static crate::gosec_taint::TaintRule> = crate::gosec_taint::TAINT_RULES
        .iter()
        .copied()
        .filter(|r| enabled.contains(r.id))
        .collect();
    if !want_g602 && !want_g115 && !want_g118 && !want_g123 && taint_rules.is_empty() {
        return;
    }
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let files = pass.files();
    if files.is_empty() {
        return;
    }
    let Ok(mut built) = build_package_for_analysis(
        artifacts.snapshot(),
        files,
        pass.fset().clone(),
        BuilderMode::GLOBAL_DEBUG,
    ) else {
        return;
    };

    let src_funcs = collect_src_funcs_with_methods(&built.prog, built.pkg);
    // `cha.CallGraph`'s node set, which the taint engine reads directly: a
    // method of an unexported type that is never boxed into an interface is
    // not in it, and so never learns taint from its callers. Computed here
    // because it needs `&mut Program` (it may build methods on demand) while
    // the analyzers below want `&Program`.
    let reachable: HashSet<FuncId> = if taint_rules.is_empty() {
        HashSet::new()
    } else {
        guff_ssa::ssautil::all_functions(&mut built.prog)
            .into_iter()
            .collect()
    };
    // G118 first, and it alone takes `&mut Program`: `types.Identical` has to
    // compute an interface's type set, which the arena caches in place.
    if want_g118 {
        crate::gosec_g118::collect_g118(&mut built.prog, &src_funcs, pending);
    }
    let prog = &built.prog;
    if want_g602 {
        crate::gosec_g602::collect_g602(prog, &src_funcs, pending);
    }
    if want_g115 {
        crate::gosec_g115::collect_g115(prog, &src_funcs, pending);
    }
    if want_g123 {
        crate::gosec_g123::collect_g123(prog, &src_funcs, pending);
    }
    // G702 / G703 / G706 / G710 share one engine and one call graph.
    crate::gosec_taint::collect_taint(prog, &src_funcs, reachable, &taint_rules, pending);
}

/// `buildssa.SrcFuncs`: the functions of this package that came from syntax,
/// methods and closures included.
pub(crate) fn collect_src_funcs_with_methods(prog: &Program, pkg: PackageId) -> Vec<FuncId> {
    let mut seen = HashSet::new();
    let mut named: Vec<(String, FuncId)> = Vec::new();
    for (fid, f) in prog.functions.iter() {
        if f.pkg != Some(pkg) {
            continue;
        }
        if f.object.is_none() {
            continue;
        }
        if f.blocks.is_empty() {
            continue;
        }
        if matches!(
            f.synthetic.as_deref(),
            Some("from type information (on demand)" | "missing generic origin")
        ) {
            continue;
        }
        // `buildssa` walks `file.Decls` for `*ast.FuncDecl`s, so the package
        // initializer — which has no declaration — is not a source function,
        // and neither is anything nested in it. A func literal in a
        // package-level `var` therefore goes unanalysed: dapr's
        // `pkg/components/state/pluggable.go` puts two `uint64(…)` conversions
        // in one. (honnef's `buildir` differs here: it starts from
        // `irpkg.Functions`, which does include the initializer.)
        if f.synthetic.as_deref() == Some("package initializer") {
            continue;
        }
        if !seen.insert(fid) {
            continue;
        }
        named.push((f.name.clone(), fid));
    }
    named.sort_by(|(a, _), (b, _)| a.cmp(b));
    let mut funcs: Vec<FuncId> = named.into_iter().map(|(_, f)| f).collect();
    // Also package-level members (in case the object filter missed some).
    let ssa_pkg = prog.packages.get(pkg);
    let mut top: Vec<(&str, FuncId)> = ssa_pkg
        .members
        .iter()
        .filter_map(|(name, m)| match m {
            MemberData::Function(fid) => Some((name.as_str(), *fid)),
            _ => None,
        })
        .collect();
    top.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (_, fid) in top {
        if prog.functions.get(fid).synthetic.as_deref() == Some("package initializer") {
            continue;
        }
        if seen.insert(fid) {
            funcs.push(fid);
        }
        collect_anon_funcs(prog, fid, &mut funcs, &mut seen);
    }
    for &fid in funcs.clone().iter() {
        collect_anon_funcs(prog, fid, &mut funcs, &mut seen);
    }
    funcs
}

fn collect_anon_funcs(
    prog: &Program,
    fid: FuncId,
    out: &mut Vec<FuncId>,
    seen: &mut HashSet<FuncId>,
) {
    let anon = prog.functions.get(fid).anon_funcs.clone();
    for child in anon {
        if seen.insert(child) {
            out.push(child);
            collect_anon_funcs(prog, child, out, seen);
        }
    }
}
