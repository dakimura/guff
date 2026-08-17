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
    pending: &mut Vec<(u32, String)>,
) {
    let want_g602 = enabled.contains("G602");
    let want_g115 = enabled.contains("G115");
    if !want_g602 && !want_g115 {
        return;
    }
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let files = pass.files();
    if files.is_empty() {
        return;
    }
    let Ok(built) = build_package_for_analysis(
        artifacts.snapshot(),
        files,
        pass.fset().clone(),
        BuilderMode::GLOBAL_DEBUG,
    ) else {
        return;
    };

    let prog = &built.prog;
    let src_funcs = collect_src_funcs_with_methods(prog, built.pkg);
    if want_g602 {
        crate::gosec_g602::collect_g602(prog, &src_funcs, pending);
    }
    if want_g115 {
        crate::gosec_g115::collect_g115(prog, &src_funcs, pending);
    }
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
