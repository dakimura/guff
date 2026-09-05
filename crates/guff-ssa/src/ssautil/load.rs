//! Package-loading utilities — port of go/ssa/ssautil/load.go.
//!
//! [`build_package`] is the primary entry point for type-checking source files
//! and building a single SSA package. [`packages`] and [`all_packages`] mirror
//! the go/packages-driven helpers for programs already type-checked in a shared
//! arena.

use crate::hash::{HashMap, HashSet};
use std::sync::Arc;

use guff::ast::File;
use guff::position::FileSet;
use guff_packages::TypecheckArtifacts;
use guff_types::{Checker, Config, PackageId as TypePackageId, TypeCheckError};

use crate::builder::build_package;
use crate::create::{create_package, imported_type_packages_closure, populate_package_members};
use crate::ids::PackageId;
use crate::mode::BuilderMode;
use crate::program::Program;

/// Result of a successful [`build_package_from_source`] call.
pub struct BuildPackageResult {
    pub prog: Program,
    pub pkg: PackageId,
    pub type_pkg: TypePackageId,
}

/// Builds SSA from an already type-checked [`guff_packages::Package`].
///
/// Takes ownership of [`TypecheckArtifacts`](guff_packages::TypecheckArtifacts),
/// syntax, and the file set from `pkg` (via [`Option::take`]). The package
/// retains `types` / `types_info` handles but the arenas move into the SSA
/// program.
pub fn build_package_from_loaded(
    pkg: &mut guff_packages::Package,
    mode: BuilderMode,
) -> Result<BuildPackageResult, BuildFromLoadedError> {
    if pkg.ill_typed {
        return Err(BuildFromLoadedError::IllTyped);
    }
    let artifacts = pkg
        .type_artifacts
        .take()
        .ok_or(BuildFromLoadedError::MissingTypes)?;
    let files = std::mem::take(&mut pkg.syntax);
    if files.is_empty() {
        return Err(BuildFromLoadedError::MissingSyntax);
    }
    let fset = pkg
        .fset
        .take()
        .ok_or(BuildFromLoadedError::MissingFileSet)?;

    let type_pkg = artifacts.type_pkg;
    let mut prog = Program::new(
        mode,
        artifacts.info,
        artifacts.types,
        artifacts.objects,
        artifacts.packages,
    );
    prog.set_fset(fset);

    create_import_packages(&mut prog, type_pkg);

    let ssa_pkg = create_package(&mut prog, type_pkg);
    populate_package_members(&mut prog, ssa_pkg, &files);
    // Import members are created on demand via `ensure_package_member` during
    // `build_package`. Eager `populate_imported_package_members` used to dominate
    // buildir CPU (~1.8s) while most imported objects were never referenced
    // (PERF_TASKS_V2 §B-2 follow-up 2026-07-29).
    build_package(&mut prog, ssa_pkg, &files);

    Ok(BuildPackageResult {
        type_pkg,
        pkg: ssa_pkg,
        prog,
    })
}

/// Builds SSA from a snapshot of type-checker artifacts without mutating the
/// loaded [`guff_packages::Package`].
///
/// Used by the `buildir` analysis pass. `no_return` is the set of function
/// objects that cannot return normally (`ctrlflow`'s answer); pass an empty
/// set where the caller has no such analysis, which leaves the dead code
/// after `log.Fatal(…)` in the IR.
pub fn build_package_for_analysis(
    artifacts: TypecheckArtifacts,
    files: &[File],
    fset: Arc<FileSet>,
    mode: BuilderMode,
    no_return: std::collections::HashSet<guff_types::ObjectId>,
) -> Result<BuildPackageResult, BuildFromLoadedError> {
    if files.is_empty() {
        return Err(BuildFromLoadedError::MissingSyntax);
    }
    let type_pkg = artifacts.type_pkg;
    let mut prog = Program::new(
        mode,
        artifacts.info,
        artifacts.types,
        artifacts.objects,
        artifacts.packages,
    );
    prog.set_fset(fset);
    // Must be installed before `build_package`: `emit_call` reads it as each
    // call is emitted. (Go: `buildssa` calls `prog.SetNoReturn` right after
    // `ssa.NewProgram`, before `ssapkg.Build()`.)
    prog.set_no_return(no_return.into_iter().collect());

    create_import_packages(&mut prog, type_pkg);

    let ssa_pkg = create_package(&mut prog, type_pkg);
    populate_package_members(&mut prog, ssa_pkg, files);
    // See comment in `build_package_from_loaded`: lazy import members.
    build_package(&mut prog, ssa_pkg, files);

    Ok(BuildPackageResult {
        type_pkg,
        pkg: ssa_pkg,
        prog,
    })
}

/// Error building SSA from a loaded package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildFromLoadedError {
    IllTyped,
    MissingTypes,
    MissingSyntax,
    MissingFileSet,
}

impl std::fmt::Display for BuildFromLoadedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IllTyped => write!(f, "package is ill-typed"),
            Self::MissingTypes => write!(f, "package has no type artifacts"),
            Self::MissingSyntax => write!(f, "package has no syntax"),
            Self::MissingFileSet => write!(f, "package has no file set"),
        }
    }
}

impl std::error::Error for BuildFromLoadedError {}

/// Builds an SSA program for a single package: type-checks `files`, creates SSA
/// shells for all imports, populates and builds the primary package.
/// (Go: `ssautil.BuildPackage`.)
///
/// The package path is taken from the type-checker's package after checking.
/// Returns an error if type-checking fails.
pub fn build_package_from_source(
    fset: Arc<FileSet>,
    config: Config,
    files: Vec<File>,
    mode: BuilderMode,
) -> Result<BuildPackageResult, Vec<TypeCheckError>> {
    if files.is_empty() {
        panic!("build_package_from_source: no files");
    }

    let mut check = Checker::new(config);
    check.check_files(files.clone());
    if !check.errors.is_empty() {
        return Err(check.errors);
    }

    let type_pkg = check.pkg;
    let mut prog = Program::new(
        mode,
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    prog.set_fset(fset);

    create_import_packages(&mut prog, type_pkg);

    let ssa_pkg = create_package(&mut prog, type_pkg);
    populate_package_members(&mut prog, ssa_pkg, &files);
    // Lazy import members — see `build_package_from_loaded`.
    build_package(&mut prog, ssa_pkg, &files);

    Ok(BuildPackageResult {
        type_pkg,
        pkg: ssa_pkg,
        prog,
    })
}

/// Input describing one initial package for [`packages`] / [`all_packages`],
/// analogous to a well-typed `packages.Package` with optional syntax.
pub struct LoadedPackage<'a> {
    pub type_pkg: TypePackageId,
    pub files: &'a [File],
    pub ill_typed: bool,
}

/// Creates SSA packages for `initial` and their dependencies. The returned
/// slice aligns with `initial`; entries are `None` when the corresponding
/// package was ill-typed. Bodies are not built — call [`build_package`] on each
/// package that needs them. (Go: `ssautil.Packages`.)
///
/// Only initial packages receive syntax (`populate_package_members`); direct
/// dependencies get import shells unless `include_dep_syntax` is set via
/// [`all_packages`].
pub fn packages<'a>(
    prog: &mut Program,
    initial: &[LoadedPackage<'a>],
    include_dep_syntax: bool,
) -> Vec<Option<PackageId>> {
    do_packages(prog, initial, include_dep_syntax)
}

/// Like [`packages`], but supplies syntax for dependency packages too when
/// available in `LoadedPackage::files`. (Go: `ssautil.AllPackages`.)
pub fn all_packages<'a>(
    prog: &mut Program,
    initial: &[LoadedPackage<'a>],
) -> Vec<Option<PackageId>> {
    do_packages(prog, initial, true)
}

fn do_packages<'a>(
    prog: &mut Program,
    initial: &[LoadedPackage<'a>],
    include_dep_syntax: bool,
) -> Vec<Option<PackageId>> {
    let mut is_initial = HashSet::default();
    for lp in initial {
        is_initial.insert(lp.type_pkg);
    }

    let mut ssamap = HashMap::<TypePackageId, Option<PackageId>>::default();
    for lp in initial {
        visit_packages(prog, lp.type_pkg, &is_initial, include_dep_syntax, lp, &mut ssamap);
    }

    initial
        .iter()
        .map(|lp| ssamap.get(&lp.type_pkg).copied().flatten())
        .collect()
}

fn visit_packages<'a>(
    prog: &mut Program,
    type_pkg: TypePackageId,
    is_initial: &HashSet<TypePackageId>,
    include_dep_syntax: bool,
    root: &LoadedPackage<'a>,
    ssamap: &mut HashMap<TypePackageId, Option<PackageId>>,
) {
    if ssamap.contains_key(&type_pkg) {
        return;
    }

    // Mirror packages.Visit: skip ill-typed packages (represented as None).
    let ill_typed = if is_initial.contains(&type_pkg) {
        root.ill_typed
    } else {
        false
    };

    if ill_typed {
        ssamap.insert(type_pkg, None);
        return;
    }

    let files = if include_dep_syntax || is_initial.contains(&type_pkg) {
        if is_initial.contains(&type_pkg) {
            root.files
        } else {
            &[]
        }
    } else {
        &[]
    };

    let ssa_pkg = if let Some(&existing) = prog.package_map.get(&type_pkg) {
        existing
    } else {
        create_import_packages(prog, type_pkg);
        let id = create_package(prog, type_pkg);
        if !files.is_empty() {
            populate_package_members(prog, id, files);
        }
        id
    };
    ssamap.insert(type_pkg, Some(ssa_pkg));

    let imports: Vec<TypePackageId> = prog.package_arena.get(type_pkg).imports().to_vec();
    for imp in imports {
        visit_packages(prog, imp, is_initial, include_dep_syntax, root, ssamap);
    }
}

/// Creates SSA package shells for every package `type_pkg` imports transitively,
/// without syntax or member population.
///
/// Uses `imported_type_packages_closure`, which walks the recorded import edges
/// (`Package.Imports()`) — O(reachable packages), not O(all objects). With
/// R24.3's shared export seed the object arena holds the union of every root's
/// dependencies, so the old `imported_type_packages` scan created shells for
/// hundreds of unrelated packages; combined with member population that took
/// seconds per `buildir` even for 1-file packages (Prometheus full run: ~250s).
fn create_import_packages(prog: &mut Program, type_pkg: TypePackageId) {
    for imp in imported_type_packages_closure(prog, type_pkg) {
        if !prog.package_map.contains_key(&imp) {
            create_package(prog, imp);
        }
    }
}
