//! Port of the `Checker` core from `cmd/compile/internal/types2/check.go`.
//!
//! The [`Checker`] is the state machine that drives type-checking. In Go it
//! relies on the garbage-collected heap and global predeclared tables; here it
//! *owns* the four arenas (types/objects/scopes/packages) — this is the single
//! biggest structural difference from upstream. Existing free functions that
//! take `&mut TypeArena` / `&ObjectArena` / `&PackageArena` are called by
//! `Checker` methods via direct field projection (Rust permits disjoint
//! borrows of distinct fields, so `&mut self.types` + `&self.objects`
//! coexist).
//!
//! This chunk (18c) lands the struct, [`Checker::new`], and the delayed-action
//! machinery ([`Checker::later`] / [`Checker::process_delayed`]). The engine
//! (resolver/decl/typexpr/expr/stmt/call/…) is built on top in later chunks.
//!
//! ## Deferrals (chunk-18c, see §8 / D15)
//!
//! - `handleBailout` (panic recovery), `initFiles` version processing,
//!   `_aliasAny` legacy state, `cleanup`/`cleaners`, `pkgPathMap`/`seenPkgMap`,
//!   `recordTypeAndValueInSyntax` (StoreTypesInSyntax) — all omitted.
//! - `files`/`obj_map`/`obj_list`/`methods` land here in chunk 22 (the
//!   resolver phase). `imports`/`untyped`/`used_vars` are added when
//!   `expr.go` (chunk 25) needs them; `ExprInfo` isn't defined yet.

use std::collections::HashMap;

use guff_constant::Value;

use crate::api::{Config, Info, TypeCheckError};
use crate::arena::{ObjectArena, ObjectData, PackageArena, ScopeArena, TypeArena, TypeData};
use crate::basic::BasicKind;
use crate::context::Context;
use crate::merge::Remapper;
use crate::object::builtin::BuiltinId;
use crate::package::{new_package, Package};
use crate::scope::Scope;
use crate::universe::init_universe_full;
use crate::{ObjectId, PackageId, ScopeId, TypeId};

/// A (delayed) action, the Rust analogue of Go's `action`.
///
/// Go stores a bare `func()` closure that captures `check`. Because our
/// closures can't capture `&mut Checker` for longer than a call, the closure
/// instead *takes* `&mut Checker` as its argument when run.
pub struct Action {
    /// Effective Go language version captured when the action was queued.
    pub version: String,
    /// The work to run later. Receives the checker so it can mutate state.
    pub f: Box<dyn FnOnce(&mut Checker)>,
}

/// The environment within which the current object is type-checked.
///
/// Valid only for the duration of checking a specific object. Equivalent to
/// Go's embedded `environment` struct (`check.go`). Only the fields needed so
/// far are present; `errpos`, `in_tparam_list`, `has_label`, etc. are added on
/// demand.
#[derive(Debug, Default)]
pub struct Environment {
    /// Topmost scope for lookups.
    pub scope: Option<ScopeId>,
    /// Accepted Go version while checking this object.
    pub version: String,
    /// Value of `iota` while checking a constant declaration, if any.
    pub iota: Option<Value>,
    /// Signature of the function whose body is being checked, if any.
    pub sig: Option<TypeId>,
    /// The package-level declaration whose init expression or function body is
    /// currently being checked, keyed by its object (the `obj_map` key).
    /// `None` outside a package-level init expression. Equivalent to Go's
    /// `environment.decl` (`*declInfo`); we key by `ObjectId` because our
    /// `DeclInfo`s live in `Checker::obj_map`.
    pub decl: Option<ObjectId>,
}

/// The type checker state.
///
/// Created with [`Checker::new`]. Equivalent to `types2.Checker`, but it owns
/// the arenas — see the module docs.
pub struct Checker {
    // ---- arenas (moved out of the universe at construction) --------------
    pub types: TypeArena,
    pub objects: ObjectArena,
    pub scopes: ScopeArena,
    pub packages: PackageArena,

    // ---- universe-derived predeclared data -------------------------------
    /// Predeclared `Basic` table, indexed by `BasicKind as usize`
    /// (so `self.typ[BasicKind::Invalid as usize]` is `Typ[Invalid]`).
    pub typ: Vec<TypeId>,
    /// The universe scope (predeclared identifiers).
    pub universe_scope: ScopeId,
    /// The `unsafe` package.
    pub unsafe_pkg: PackageId,
    /// Predeclared `error` type.
    pub universe_error: TypeId,
    /// Predeclared `any` type (alias of empty interface).
    pub universe_any: TypeId,
    /// Predeclared `comparable` type.
    pub universe_comparable: TypeId,
    /// Predeclared `nil` object.
    pub universe_nil: ObjectId,
    /// Builtin functions, keyed by id.
    pub builtins: HashMap<BuiltinId, ObjectId>,

    // ---- package information (valid for the checker's lifetime) ----------
    pub conf: Config,
    /// Resolves non-`unsafe` import paths to packages, allocating them into the
    /// checker's arenas. `None` means unresolvable imports are skipped (D16).
    /// Set via [`Checker::set_importer`]. (Go: `Config.Importer`.)
    pub importer: Option<Box<dyn crate::importer::Importer>>,
    /// Caches imported packages by path so each is imported at most once and
    /// `import`ing the same path twice yields the same package.
    pub import_cache: HashMap<String, PackageId>,
    /// Source files of dependency packages, keyed by import path. When an
    /// `import "path"` names an entry here, the checker type-checks it
    /// recursively into the shared arenas (a built-in source importer), taking
    /// precedence over the pluggable [`Importer`]. Set via
    /// [`Checker::add_dependency_source`].
    pub sources: HashMap<String, Vec<guff::ast::File>>,
    /// Import paths currently being checked, for import-cycle detection during
    /// recursive source importing.
    pub importing: Vec<String>,
    /// Context for de-duplicating instances.
    pub ctxt: Context,
    /// The package being checked.
    pub pkg: PackageId,
    /// Where results are recorded.
    pub info: Info,
    /// Unique id source for type parameters (first valid id is 1).
    pub next_id: u64,

    // ---- diagnostics -----------------------------------------------------
    /// Collected errors. Go reports eagerly; we collect (§6.6).
    pub errors: Vec<TypeCheckError>,
    /// First error encountered (message), mirroring Go's `firstErr`.
    pub first_err: Option<String>,

    // ---- file-checking-phase state (populated by the resolver, chunk 22) -
    /// The source files of the package being checked.
    pub files: Vec<guff::ast::File>,
    /// Maps each package-level (and method) object to its declaration info.
    pub obj_map: HashMap<ObjectId, crate::resolver::DeclInfo>,
    /// `obj_map`'s keys, sorted by source order (filled by `sort_objects`).
    pub obj_list: Vec<ObjectId>,
    /// Methods collected per receiver base `TypeName`, awaiting type-checking.
    pub methods: HashMap<ObjectId, Vec<ObjectId>>,
    /// Local variables that have been read (used). A local `Var` of the
    /// checker's own package not in this set at the end of its function body
    /// triggers a "declared and not used" error. Equivalent to `usedVars`.
    pub used_vars: std::collections::HashSet<ObjectId>,
    /// The `PkgName` objects bound by this package's `import` declarations, in
    /// source order. Any not in `used_pkg_names` at the end of checking is
    /// reported as an unused import. Equivalent to `check.imports`.
    pub imports: Vec<ObjectId>,
    /// The set of `PkgName` objects referred to by a qualified identifier
    /// (`pkg.X`). Equivalent to `check.usedPkgNames`.
    pub used_pkg_names: std::collections::HashSet<ObjectId>,
    /// Maps each dot-imported package to the `PkgName` that dot-imported it.
    /// When a bare identifier resolves to an object of one of these packages,
    /// the corresponding `PkgName` is marked used. Simplification of Go's
    /// `dotImportMap` (which keys on `(fileScope, name)`); resolving on the
    /// object's package is sufficient here because dot-imported objects belong
    /// to the imported package.
    pub dot_imported: HashMap<PackageId, ObjectId>,
    /// Map of expressions that do not yet have a final (typed) type, keyed on
    /// the AST node id (see `guff::ast::Ident::id`). Entries are narrowed
    /// in place by `update_expr_type` as their context becomes known and
    /// flushed by `record_untyped` at the end of checking. Equivalent to Go's
    /// `check.untyped`.
    pub untyped: std::collections::HashMap<u32, crate::recording::ExprInfo>,

    // ---- delayed actions and cycle path ----------------------------------
    /// Stack of delayed action segments; processed in FIFO order.
    pub delayed: Vec<Action>,
    /// Path of object dependencies during checking (for cycle reporting).
    pub obj_path: Vec<ObjectId>,

    // ---- per-object environment ------------------------------------------
    pub env: Environment,

    /// Monomorphization flow graph (cycle detection for unbounded recursive
    /// instantiation). Populated during instantiation via
    /// `mono.record_instance`, analyzed by `monomorph()` at the end of
    /// `check_files`. Equivalent to Go's `check.mono`.
    pub mono: crate::mono::MonoGraph,
    /// When set, function and method bodies are not queued for checking
    /// (Go's `Config.IgnoreFuncBodies`). Signatures are still resolved, so the
    /// package's exported API is complete. Used for dependency packages in the
    /// source-seed path: importers only need exported types, and skipping bodies
    /// is a large speedup. Never set for target packages (they need full body
    /// checks to produce findings).
    pub ignore_func_bodies: bool,
}

/// Shared, already-decoded export-data graph for parallel type-checks (R24.3).
///
/// Built once via [`Checker::capture_export_seed`] after preloading dependency
/// `.a` files; each parallel worker clones into a fresh [`Checker`] with
/// [`Checker::from_seed`] so stdlib/common deps are not re-decoded per package.
#[derive(Clone)]
pub struct ExportSeed {
    types: TypeArena,
    objects: ObjectArena,
    scopes: ScopeArena,
    packages: PackageArena,
    typ: Vec<TypeId>,
    universe_scope: ScopeId,
    unsafe_pkg: PackageId,
    universe_error: TypeId,
    universe_any: TypeId,
    universe_comparable: TypeId,
    universe_nil: ObjectId,
    builtins: HashMap<BuiltinId, ObjectId>,
    import_cache: HashMap<String, PackageId>,
}

/// One finished worker's own arena allocations (its overlays), extracted via
/// [`Checker::into_worker_overlays`] for merging into a shared seed (R25).
pub struct WorkerOverlays {
    types: Vec<TypeData>,
    objects: Vec<ObjectData>,
    scopes: Vec<Scope>,
    packages: Vec<Package>,
    /// Import-cache entries this worker added (its own package path → id). The
    /// `PackageId` is worker-local and relocated during [`ExportSeed::merge_wave`].
    cache_delta: Vec<(String, PackageId)>,
}

impl ExportSeed {
    /// Number of packages present in the import cache (deps + unsafe if any).
    pub fn cached_import_count(&self) -> usize {
        self.import_cache.len()
    }

    /// Fold one topological wave of independently-checked worker packages into
    /// this seed (R25).
    ///
    /// Every worker in `workers` was cloned from *this same* seed (via
    /// [`Checker::from_seed`]) before the wave ran, so each numbers its own
    /// allocations starting just past the seed's current length. We append the
    /// overlays back-to-back and shift each worker's own ids by the total length
    /// of the overlays merged ahead of it (`delta`); ids that point into the
    /// shared base (`<= base_len`) are left untouched. Ordering of `workers`
    /// must be deterministic (callers pass a stable, path-sorted wave) so the
    /// merged arena layout — and therefore all downstream findings — is
    /// reproducible.
    ///
    /// Correctness rests on wave independence: workers at the same topological
    /// level never import each other, so a worker's overlay references only the
    /// shared base and its own overlay, never a sibling's.
    pub fn merge_wave(&mut self, workers: Vec<WorkerOverlays>) {
        // Base lengths are fixed for the whole wave (all workers cloned from the
        // seed as it was on entry); only the deltas accumulate.
        let ty_base = self.types.len() as u32;
        let ob_base = self.objects.len() as u32;
        let sc_base = self.scopes.len() as u32;
        let pk_base = self.packages.len() as u32;
        let (mut ty_delta, mut ob_delta, mut sc_delta, mut pk_delta) = (0u32, 0u32, 0u32, 0u32);

        for w in workers {
            let r = Remapper {
                ty_base,
                ty_delta,
                ob_base,
                ob_delta,
                sc_base,
                sc_delta,
                pk_base,
                pk_delta,
            };
            let WorkerOverlays {
                mut types,
                mut objects,
                mut scopes,
                mut packages,
                cache_delta,
            } = w;

            for t in &mut types {
                crate::merge::remap_type(t, &r);
            }
            for o in &mut objects {
                crate::merge::remap_object(o, &r);
            }
            for s in &mut scopes {
                crate::merge::remap_scope(s, &r);
            }
            for p in &mut packages {
                crate::merge::remap_package(p, &r);
            }
            for (path, id) in cache_delta {
                self.import_cache.insert(path, r.pkg(id));
            }

            ty_delta += types.len() as u32;
            ob_delta += objects.len() as u32;
            sc_delta += scopes.len() as u32;
            pk_delta += packages.len() as u32;

            self.types.extend_base(types);
            self.objects.extend_base(objects);
            self.scopes.extend_base(scopes);
            self.packages.extend_base(packages);
        }

        // Accounting tripwire (debug/test builds): the seed must have grown by
        // exactly the merged overlays. A mismatch means the extend/delta math
        // drifted. Semantic remap completeness is covered by the regress gate.
        debug_assert_eq!(self.types.len(), (ty_base + ty_delta) as usize);
        debug_assert_eq!(self.objects.len(), (ob_base + ob_delta) as usize);
        debug_assert_eq!(self.scopes.len(), (sc_base + sc_delta) as usize);
        debug_assert_eq!(self.packages.len(), (pk_base + pk_delta) as usize);
    }
}

impl Checker {
    /// Create a fresh checker. Takes ownership of a freshly-built universe
    /// (arenas + predeclared tables) and allocates an (initially empty)
    /// package to check.
    ///
    /// Equivalent to `NewChecker` (`check.go`), but it also seeds the arenas
    /// from [`init_universe_full`]. The package path/name start empty and are
    /// set from the package clause during object collection (chunk 22).
    pub fn new(conf: Config) -> Checker {
        let mut u = init_universe_full();

        // Pull the predeclared handles out first (all Copy, or moved).
        let typ = u.typ.to_vec();
        let universe_scope = u.universe_scope;
        let unsafe_pkg = u.unsafe_pkg;
        let universe_error = u.error;
        let universe_any = u.any;
        let universe_comparable = u.comparable;
        let universe_nil = u.nil;
        let builtins = std::mem::take(&mut u.builtins);

        // Move the arenas out of the universe (partial move; Universe has no
        // Drop impl, so this is allowed). Only `scopes`/`packages` are mutated
        // here (to allocate the package); `types`/`objects` are moved as-is.
        let types = u.type_arena;
        let objects = u.object_arena;
        let mut scopes = u.scope_arena;
        let mut packages = u.package_arena;

        // Allocate the package being checked, parented at the universe scope.
        let pkg = new_package(&mut packages, &mut scopes, universe_scope, "", "");

        Checker {
            types,
            objects,
            scopes,
            packages,
            typ,
            universe_scope,
            unsafe_pkg,
            universe_error,
            universe_any,
            universe_comparable,
            universe_nil,
            builtins,
            conf,
            importer: None,
            import_cache: HashMap::new(),
            sources: HashMap::new(),
            importing: Vec::new(),
            ctxt: Context::new(),
            pkg,
            info: Info::default(),
            next_id: 1,
            errors: Vec::new(),
            first_err: None,
            files: Vec::new(),
            used_vars: std::collections::HashSet::new(),
            imports: Vec::new(),
            used_pkg_names: std::collections::HashSet::new(),
            dot_imported: HashMap::new(),
            untyped: std::collections::HashMap::new(),
            obj_map: HashMap::new(),
            obj_list: Vec::new(),
            methods: HashMap::new(),
            delayed: Vec::new(),
            obj_path: Vec::new(),
            env: Environment::default(),
            mono: crate::mono::MonoGraph::default(),
            ignore_func_bodies: false,
        }
    }

    /// Install the [`Importer`](crate::importer::Importer) used to resolve
    /// non-`unsafe` import paths. (Go sets this via `Config.Importer`; here it
    /// lives on the checker so `Config` stays a plain data struct.)
    pub fn set_importer(&mut self, importer: Box<dyn crate::importer::Importer>) {
        self.importer = Some(importer);
    }

    /// Snapshot arenas + import cache after preloading export data, for reuse
    /// across parallel package checks (R24.3).
    ///
    /// The captured package under check (`self.pkg`) is intentionally *not*
    /// reused — each [`Self::from_seed`] allocates a fresh package.
    pub fn capture_export_seed(mut self) -> ExportSeed {
        // Freeze in place so seed capture does not deep-clone overlay arenas
        // (which would briefly double peak RSS during hybrid seed build).
        self.types.freeze();
        self.objects.freeze();
        self.scopes.freeze();
        self.packages.freeze();
        ExportSeed {
            types: self.types,
            objects: self.objects,
            scopes: self.scopes,
            packages: self.packages,
            typ: self.typ,
            universe_scope: self.universe_scope,
            unsafe_pkg: self.unsafe_pkg,
            universe_error: self.universe_error,
            universe_any: self.universe_any,
            universe_comparable: self.universe_comparable,
            universe_nil: self.universe_nil,
            builtins: self.builtins,
            import_cache: self.import_cache,
        }
    }

    /// Extract a finished worker's own arena allocations for merging into a
    /// shared seed (R25 wave-parallel seed build).
    ///
    /// The worker was cloned from a shared frozen seed via [`Self::from_seed`],
    /// so its four arenas are `{ shared base, private overlay }`; we keep only
    /// the overlays (the worker's own contribution) and drop the base — which is
    /// the seed we cloned from (and may be a diverged copy-on-write copy if the
    /// worker touched a base element's lazy cache; that mutation is idempotent
    /// and recomputed later). `own_path`/`own_pkg` are this package's import path
    /// and the (worker-local) package id it allocated, forming the single new
    /// import-cache entry the worker added; the id is relocated during merge.
    pub fn into_worker_overlays(self, own_path: String, own_pkg: PackageId) -> WorkerOverlays {
        WorkerOverlays {
            types: self.types.into_overlay(),
            objects: self.objects.into_overlay(),
            scopes: self.scopes.into_overlay(),
            packages: self.packages.into_overlay(),
            cache_delta: vec![(own_path, own_pkg)],
        }
    }

    /// Build a checker from a shared [`ExportSeed`], skipping re-decode of the
    /// preloaded dependency graph.
    pub fn from_seed(seed: &ExportSeed, conf: Config) -> Checker {
        // Share the seed's frozen bases (Arc bumps); the checker appends this
        // package's own types/objects/scopes/packages into private overlays.
        let mut packages = seed.packages.shared_clone();
        let mut scopes = seed.scopes.shared_clone();
        let pkg = new_package(&mut packages, &mut scopes, seed.universe_scope, "", "");
        Checker {
            types: seed.types.shared_clone(),
            objects: seed.objects.shared_clone(),
            scopes,
            packages,
            typ: seed.typ.clone(),
            universe_scope: seed.universe_scope,
            unsafe_pkg: seed.unsafe_pkg,
            universe_error: seed.universe_error,
            universe_any: seed.universe_any,
            universe_comparable: seed.universe_comparable,
            universe_nil: seed.universe_nil,
            builtins: seed.builtins.clone(),
            conf,
            importer: None,
            import_cache: seed.import_cache.clone(),
            sources: HashMap::new(),
            importing: Vec::new(),
            ctxt: Context::new(),
            pkg,
            info: Info::default(),
            next_id: 1,
            errors: Vec::new(),
            first_err: None,
            files: Vec::new(),
            used_vars: std::collections::HashSet::new(),
            imports: Vec::new(),
            used_pkg_names: std::collections::HashSet::new(),
            dot_imported: HashMap::new(),
            untyped: std::collections::HashMap::new(),
            obj_map: HashMap::new(),
            obj_list: Vec::new(),
            methods: HashMap::new(),
            delayed: Vec::new(),
            obj_path: Vec::new(),
            env: Environment::default(),
            mono: crate::mono::MonoGraph::default(),
            // A from_seed checker always checks a *target* package fully.
            ignore_func_bodies: false,
        }
    }

    /// Enable/disable skipping of function and method bodies
    /// (`Config.IgnoreFuncBodies`). See the field docs on [`Checker`].
    pub fn set_ignore_func_bodies(&mut self, ignore: bool) {
        self.ignore_func_bodies = ignore;
    }

    /// Register the source files of a dependency package under its import path.
    /// When the checked package (or a transitive dependency) imports `path`, the
    /// checker type-checks these files into the shared arenas and uses the
    /// resulting package. This built-in source importer takes precedence over
    /// any [`Importer`](crate::importer::Importer) installed via
    /// [`set_importer`](Self::set_importer).
    pub fn add_dependency_source(
        &mut self,
        path: impl Into<String>,
        files: Vec<guff::ast::File>,
    ) {
        self.sources.insert(path.into(), files);
    }

    /// Type-check a dependency package from its registered source files,
    /// recursively (its own imports resolve the same way), into the shared
    /// arenas. Per-package checker state is saved and restored around the nested
    /// run so the enclosing package's in-progress check is unaffected; any
    /// diagnostics found in the dependency are appended to `self.errors`.
    /// Returns `None` on an import cycle.
    fn check_dependency(
        &mut self,
        path: &str,
        files: Vec<guff::ast::File>,
    ) -> Option<PackageId> {
        // Import-cycle guard: if `path` is already being checked, bail.
        if self.importing.iter().any(|p| p == path) {
            return None;
        }
        self.importing.push(path.to_string());

        // Allocate the dependency's package and swap in fresh per-package state.
        let dep_pkg = new_package(
            &mut self.packages,
            &mut self.scopes,
            self.universe_scope,
            path,
            "",
        );
        let saved_pkg = std::mem::replace(&mut self.pkg, dep_pkg);
        let saved_info = std::mem::take(&mut self.info);
        let saved_files = std::mem::take(&mut self.files);
        let saved_used = std::mem::take(&mut self.used_vars);
        let saved_imports = std::mem::take(&mut self.imports);
        let saved_used_pkg_names = std::mem::take(&mut self.used_pkg_names);
        let saved_dot_imported = std::mem::take(&mut self.dot_imported);
        let saved_untyped = std::mem::take(&mut self.untyped);
        let saved_obj_map = std::mem::take(&mut self.obj_map);
        let saved_obj_list = std::mem::take(&mut self.obj_list);
        let saved_methods = std::mem::take(&mut self.methods);
        let saved_delayed = std::mem::take(&mut self.delayed);
        let saved_obj_path = std::mem::take(&mut self.obj_path);
        let saved_env = std::mem::take(&mut self.env);
        let saved_mono = std::mem::take(&mut self.mono);
        let saved_errors = std::mem::take(&mut self.errors);
        let saved_first_err = self.first_err.take();

        // Cache before the recursive run so a diamond dependency resolves to the
        // same package (and a cycle back to us hits the `importing` guard).
        self.import_cache.insert(path.to_string(), dep_pkg);

        self.check_files(files);

        // Collect the dependency's diagnostics, then restore the caller's state.
        let dep_errors = std::mem::replace(&mut self.errors, saved_errors);
        let dep_first_err = self.first_err.take();
        self.pkg = saved_pkg;
        self.info = saved_info;
        self.files = saved_files;
        self.used_vars = saved_used;
        self.imports = saved_imports;
        self.used_pkg_names = saved_used_pkg_names;
        self.dot_imported = saved_dot_imported;
        self.untyped = saved_untyped;
        self.obj_map = saved_obj_map;
        self.obj_list = saved_obj_list;
        self.methods = saved_methods;
        self.delayed = saved_delayed;
        self.obj_path = saved_obj_path;
        self.env = saved_env;
        self.mono = saved_mono;
        // Surface dependency diagnostics alongside the caller's.
        self.errors.extend(dep_errors);
        self.first_err = saved_first_err.or(dep_first_err);

        self.importing.pop();
        Some(dep_pkg)
    }

    /// Preload a dependency package into the checker's arenas (typically from
    /// export data via the installed [`Importer`](crate::importer::Importer)).
    /// Used by `guff-packages` to load transitive dependencies before
    /// type-checking an initial package.
    pub fn preload_import(&mut self, path: &str) -> Option<PackageId> {
        self.import_package(path)
    }

    /// Resolve an import path to a package, allocating it into the checker's
    /// arenas on first use and caching it by path. `unsafe` is resolved
    /// directly; every other path goes through the installed
    /// [`Importer`](crate::importer::Importer). Returns `None` when there is no
    /// importer or it cannot resolve the path.
    pub(crate) fn import_package(&mut self, path: &str) -> Option<PackageId> {
        if path == "unsafe" {
            return Some(self.unsafe_pkg);
        }
        if path == "C" {
            if !self.conf.fake_import_c {
                return None;
            }
            if let Some(&pkg) = self.import_cache.get(path) {
                return Some(pkg);
            }
            let pkg = new_package(
                &mut self.packages,
                &mut self.scopes,
                self.universe_scope,
                "C",
                "C",
            );
            self.packages.get_mut(pkg).mark_complete();
            self.import_cache.insert(path.to_string(), pkg);
            return Some(pkg);
        }
        if let Some(&pkg) = self.import_cache.get(path) {
            return Some(pkg);
        }
        // Built-in source importer: if the path's source is registered, check it
        // recursively (this takes precedence over the pluggable importer).
        // Take ownership so the AST is not retained in `sources` after type-check
        // (hybrid seed otherwise doubles peak RSS by keeping both the map entry
        // and the in-flight `files` Vec during `check_dependency`).
        if let Some(files) = self.sources.remove(path) {
            return self.check_dependency(path, files);
        }
        // Take the importer out so its `import` call can borrow the arenas
        // (disjoint fields) without also borrowing `self.importer`.
        let mut importer = self.importer.take()?;
        let mut ctx = crate::importer::ImportCtx {
            types: &mut self.types,
            objects: &mut self.objects,
            scopes: &mut self.scopes,
            packages: &mut self.packages,
            universe_scope: self.universe_scope,
        };
        let result = importer.import(&mut ctx, path);
        self.importer = Some(importer);
        if let Some(pkg) = result {
            self.import_cache.insert(path.to_string(), pkg);
        }
        result
    }

    /// The predeclared `Typ[Invalid]` type.
    pub fn invalid_type(&self) -> TypeId {
        self.typ[BasicKind::Invalid as usize]
    }

    /// A predeclared `Basic` type by kind (e.g. `self.basic(BasicKind::Int)`).
    pub fn basic(&self, kind: BasicKind) -> TypeId {
        self.typ[kind as usize]
    }

    /// Push `f` onto the stack of actions to run later (at the end of the
    /// current statement, or before a local const/var enters scope).
    ///
    /// Equivalent to `Checker.later`. The action captures the effective Go
    /// version at queue time.
    pub fn later<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Checker) + 'static,
    {
        let version = self.env.version.clone();
        self.delayed.push(Action {
            version,
            f: Box::new(f),
        });
    }

    /// Run delayed actions in `self.delayed[top..]`, in FIFO order. Actions
    /// may append further actions, which are also processed (the loop tracks
    /// the growing length). Afterwards the segment is truncated back to `top`.
    ///
    /// Equivalent to `Checker.processDelayed`.
    pub fn process_delayed(&mut self, top: usize) {
        let saved_version = self.env.version.clone();
        let mut i = top;
        while i < self.delayed.len() {
            // Take the closure out (leaving a no-op behind) so we can call it
            // without holding a borrow on `self.delayed`.
            let version = self.delayed[i].version.clone();
            let noop: Box<dyn FnOnce(&mut Checker)> = Box::new(|_: &mut Checker| {});
            let f = std::mem::replace(&mut self.delayed[i].f, noop);
            self.env.version = version; // re-establish captured version
            f(self); // may append to self.delayed
            i += 1;
        }
        assert!(top <= self.delayed.len(), "delayed stack must not shrink");
        self.delayed.truncate(top);
        self.env.version = saved_version;
    }

    /// Type-check `files` as the package being checked.
    ///
    /// Equivalent to `Checker.checkFiles` (the acyclic core): collect objects,
    /// sort them, type-check all package objects, then run delayed actions
    /// (function bodies). Marks the package complete.
    ///
    /// **Deferred**: `initFiles` multi-file version reconciliation, `cleanup`,
    /// and `unusedImports` (later chunks). Returns nothing; errors accumulate in
    /// `self.errors`.
    pub fn check_files(&mut self, files: Vec<guff::ast::File>) {
        self.files = files;
        self.collect_objects();
        self.sort_objects();
        self.direct_cycles(); // report direct name-chain type cycles (cycles.go)
        self.package_objects();
        self.process_delayed(0); // includes all function bodies (once stmt.go lands)
        self.record_untyped(); // flush still-untyped expressions into Info.Types
        self.init_order(); // compute Info.init_order from the dependency graph
        self.unused_imports(); // report imports never referred to (resolver.go)

        // Detect unbounded recursive instantiation (non-monomorphizable
        // packages). Go only runs this when no error has been reported yet.
        if self.first_err.is_none() {
            self.monomorph();
        }

        // Record the package's directly imported packages (Go:
        // `Package.Imports()`). Decoded (export-data) packages get this from
        // `ureader`; a source-checked package sets it here from its import
        // PkgNames. SSA uses it to walk the transitive import closure instead of
        // scanning the whole (seed-inflated) object arena.
        let mut import_ids: Vec<PackageId> = Vec::new();
        let mut seen_imports: std::collections::HashSet<PackageId> =
            std::collections::HashSet::new();
        for &obj in &self.imports {
            if let crate::arena::ObjectData::PkgName(p) = self.objects.get(obj) {
                let imported = p.imported();
                if seen_imports.insert(imported) {
                    import_ids.push(imported);
                }
            }
        }
        self.packages.get_mut(self.pkg).set_imports(import_ids);

        let go_version = self.conf.go_version.clone();
        let pkg = self.packages.get_mut(self.pkg);
        pkg.set_go_version(go_version);
        pkg.mark_complete();
    }

    /// Report imported packages that are never referred to.
    ///
    /// Equivalent to `Checker.unusedImports` + `errorUnusedPkg`
    /// (resolver.go). Blank (`_`) and dot (`.`) imports are never bound
    /// (deferred, D16), so they never enter `self.imports` and are correctly
    /// never reported. Go skips this entirely when function bodies are not
    /// checked (`IgnoreFuncBodies`); we always check them, so we always run it.
    /// A soft error is used to match Go's `softErrorf` (checking continues).
    fn unused_imports(&mut self) {
        // Snapshot (pos, path, name) for each unused import before reporting to
        // avoid borrowing `self` while iterating `self.imports`.
        let unused: Vec<(u32, String, String)> = self
            .imports
            .iter()
            .filter(|obj| !self.used_pkg_names.contains(obj))
            .filter_map(|&obj| match self.objects.get(obj) {
                // A blank import (`_`) is intentionally unused — skip it.
                crate::arena::ObjectData::PkgName(p) if p.name() != "_" => {
                    let path = self.packages.get(p.imported()).path().to_string();
                    Some((obj.pos(&self.objects), path, p.name().to_string()))
                }
                _ => None,
            })
            .collect();

        for (pos, path, name) in unused {
            // Show the local name only if it differs from the final path
            // element (a renamed import, or an unconventional package name).
            let elem = path.rsplit('/').next().unwrap_or(path.as_str());
            let msg = if name.is_empty() || name == "." || name == elem {
                format!("{:?} imported and not used", path)
            } else {
                format!("{:?} imported as {} and not used", path, name)
            };
            self.error(pos, guff_types_errors::Code::UnusedImport, msg);
        }
    }

    /// If `obj` belongs to a dot-imported package, mark that package's
    /// `PkgName` as used so it is not reported as an unused import. Called
    /// wherever a bare identifier resolves to an object (Go marks
    /// `usedPkgNames` via `dotImportMap` in `expr.go`/`typexpr.go`).
    pub fn mark_dot_import_use(&mut self, obj: ObjectId) {
        if self.dot_imported.is_empty() {
            return;
        }
        if let Some(pkg) = obj.pkg(&self.objects) {
            if let Some(&pname) = self.dot_imported.get(&pkg) {
                self.used_pkg_names.insert(pname);
            }
        }
    }

    /// Push `obj` onto the object-dependency path (for cycle reporting).
    ///
    /// Equivalent to `Checker.push` (the `objPathIdx` map is deferred until a
    /// cycle check needs it).
    pub fn push(&mut self, obj: ObjectId) {
        self.obj_path.push(obj);
    }

    /// Pop the most-recently-pushed object off the dependency path.
    ///
    /// Equivalent to `Checker.pop`.
    pub fn pop(&mut self) -> Option<ObjectId> {
        self.obj_path.pop()
    }

    /// Add the dependency edge `check.decl -> to` if `check.decl` exists and
    /// `to` is a package-level object (in `obj_map`).
    ///
    /// Equivalent to `Checker.addDeclDep` (`check.go`). Called when an
    /// identifier in a package-level init expression (or function body) denotes
    /// a constant, variable, or function — these edges drive `init_order`.
    pub fn add_decl_dep(&mut self, to: ObjectId) {
        let from = match self.env.decl {
            Some(f) => f,
            None => return, // not in a package-level init expression
        };
        if !self.obj_map.contains_key(&to) {
            return; // `to` is not a package-level object
        }
        if let Some(d) = self.obj_map.get_mut(&from) {
            d.add_dep(to);
        }
    }
}
