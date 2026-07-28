//! Port of package-level object collection from
//! `cmd/compile/internal/types2/resolver.go`.
//!
//! Our AST crate (`guff`) is shaped like **`go/ast`**, not the compiler's
//! `syntax` package, so this port follows the `go/types/resolver.go` +
//! `go/types/decl.go::walkDecl` structure (which consume `*ast.GenDecl` /
//! `*ast.ValueSpec` / `*ast.TypeSpec`) rather than the `types2` originals.
//! The semantics are identical — only the node shapes differ.
//!
//! Chunk 22 lands [`DeclInfo`], [`Checker::collect_objects`] (const/var/type/
//! func declarations + method collection/association), [`Checker::declare`],
//! [`Checker::declare_pkg_obj`], and [`Checker::sort_objects`].
//!
//! ## Deferrals (chunk-22, see §8)
//!
//! - **Imports** (`ImportSpec`): there is no `Importer`, so the only package
//!   that can be resolved is the synthetic `unsafe` package (created in the
//!   universe). `import "unsafe"` binds a [`crate::PkgName`] in the file scope;
//!   any other import path is skipped (we cannot load it). Dot-imports, the
//!   `pkg.imports` list, unused-import tracking, and the package-vs-file-scope
//!   name-clash check are still omitted (D16).
//! - **Positions** (D07): objects are created with `pos = 0` (nopos); the
//!   constructors don't take a position and there's no `set_pos`. `scope_pos`
//!   is set to `0` in [`Checker::declare`].
//! - `Const`/`Var` objects are created with a `Typ[Invalid]` placeholder type
//!   (Go passes `nil`); `decl.rs` (chunk 23) fills in the real type during
//!   `objDecl`. `TypeName`/`Func` use the genuine two-phase `None`.
//! - `recordDef`/`recordImplicit`/`recordScope` are no-ops (Info recording is
//!   deferred, §18b).
//! - The `init`/`main` "must be func"/"must have a body" soft errors are
//!   reported where cheap; type-parameter version gating is omitted.
//! - `Func.hasPtrRecv_` early flag is not set during association (D-note in
//!   `func.rs`); pointer-ness is recovered later from the signature.
//! - `package_objects`/`unused_imports` are deferred — `package_objects` needs
//!   `objDecl` (chunk 23). Forward pointer at the bottom of this file.

use crate::hash::HashMap;

use guff::ast::{Decl, Expr, FuncDecl, ImportSpec, Spec, TypeSpec};
use guff::Token;
use guff_constant::make_int64;
use guff_types_errors::Code;

use crate::check::Checker;
use crate::object::const_::new_const;
use crate::object::func::new_func;
use crate::object::pkgname::new_pkg_name;
use crate::object::type_name::new_type_name;
use crate::object::var::new_var;
use crate::scope::{insert as scope_insert, lookup as scope_lookup, new_scope};
use crate::{ObjectId, ScopeId};

/// Describes a package-level const, type, var, or func declaration.
///
/// Equivalent to `types2.declInfo`. Pointers to AST nodes become owned clones
/// (the AST nodes are `Clone`); `*Scope`/`*Var`/`Object` become `ScopeId` /
/// `ObjectId`.
#[derive(Default)]
pub struct DeclInfo {
    /// Scope of the file containing this declaration.
    pub file_scope: Option<ScopeId>,
    /// Go version of the file containing this declaration.
    pub version: String,
    /// LHS of an n:1 variable declaration, or empty.
    pub lhs: Vec<ObjectId>,
    /// Declared type expression (const/var only), or `None`.
    pub vtyp: Option<Expr>,
    /// Init/orig expression (const/var only), or `None`.
    pub init: Option<Expr>,
    /// If set, the init expression is inherited from a previous constant.
    pub inherited: bool,
    /// Type declaration, or `None`.
    pub tdecl: Option<TypeSpec>,
    /// Func declaration, or `None`.
    pub fdecl: Option<FuncDecl>,
    /// Objects this declaration's init expression depends on (lazy).
    pub deps: HashMap<ObjectId, bool>,
}

impl DeclInfo {
    /// Reports whether the declared object has an initialization expression or
    /// a function body. Equivalent to `declInfo.hasInitializer`.
    pub fn has_initializer(&self) -> bool {
        self.init.is_some()
            || self
                .fdecl
                .as_ref()
                .map(|f| f.body.is_some())
                .unwrap_or(false)
    }

    /// Adds `obj` to the set of objects this declaration depends on.
    /// Equivalent to `declInfo.addDep`.
    pub fn add_dep(&mut self, obj: ObjectId) {
        self.deps.insert(obj, true);
    }
}

/// A method collected during object collection, awaiting receiver-type
/// association. Mirrors the local `methodInfo` struct in `collectObjects`.
struct MethodInfo {
    /// The method `Func` object.
    obj: ObjectId,
    /// True if the receiver is a pointer.
    ptr: bool,
    /// Receiver base type name (an identifier).
    recv: String,
}

impl Checker {
    /// Declares `obj` in `scope` (unless it's the blank `_`). On a name clash
    /// reports a `DuplicateDecl` error and leaves the scope unchanged.
    ///
    /// Equivalent to `Checker.declare` (`decl.go`). `recordDef` is a no-op.
    pub fn declare(&mut self, scope: ScopeId, obj: ObjectId, pos: u32) {
        if obj.name(&self.objects) != "_" {
            if let Some(_alt) = scope_insert(&mut self.scopes, &mut self.objects, scope, obj) {
                let name = obj.name(&self.objects).to_string();
                let at = obj.pos(&self.objects);
                self.error(
                    at,
                    Code::DuplicateDecl,
                    format!("{} redeclared in this block", name),
                );
                return;
            }
            obj.set_scope_pos(&mut self.objects, pos);
        }
        // DEFERRED: recordDef(id, obj) — Info recording (§18b).
    }

    /// Declares `obj` in the package scope, records its decl info in
    /// `obj_map`, and assigns its source order. The object must not be a
    /// function or method.
    ///
    /// Equivalent to `Checker.declarePkgObj`.
    fn declare_pkg_obj(&mut self, ident: &str, obj: ObjectId, d: DeclInfo) {
        debug_assert_eq!(ident, obj.name(&self.objects));

        // spec: a package-scope identifier named "init" must be a func.
        if ident == "init" {
            self.error(
                obj.pos(&self.objects),
                Code::InvalidInitDecl,
                "cannot declare init - must be func",
            );
            return;
        }
        // spec: in package main, "main" must be a func.
        if ident == "main" && self.packages.get(self.pkg).name() == "main" {
            self.error(
                obj.pos(&self.objects),
                Code::InvalidMainDecl,
                "cannot declare main - must be func",
            );
            return;
        }

        let pkg_scope = self.packages.get(self.pkg).scope();
        self.declare(pkg_scope, obj, 0 /* nopos */);
        self.obj_map.insert(obj, d);
        let order = self.obj_map.len() as u32;
        obj.set_order(&mut self.objects, order);
    }

    /// Handles a single `import` spec: resolve the package (via
    /// [`Checker::import_package`], i.e. `unsafe` directly or the installed
    /// [`Importer`](crate::importer::Importer)), then bind a [`crate::PkgName`]
    /// in `file_scope`. An unresolvable path is left unbound. The bound name is
    /// recorded as a `Def` (explicit alias) or `Implicit` (name-less import).
    ///
    /// Simplified from `Checker.importPackage` + the `ImportSpec` branch of
    /// `Checker.collectObjects`. Dot-imports, blank imports, the `pkg.imports`
    /// list, and unused-import tracking are deferred (D16).
    fn import_decl(&mut self, spec: &ImportSpec, file_scope: ScopeId) {
        let path = unquote_import_path(&spec.path.value);

        // Resolve the package: `unsafe` directly, everything else via the
        // installed importer. An unresolvable path (no importer / unknown path)
        // leaves the import unbound.
        let imported = match self.import_package(&path) {
            Some(p) => p,
            None => return,
        };

        // The local name is the explicit alias, or the imported package's name.
        let name = match &spec.name {
            Some(id) => id.name.clone(),
            None => self.packages.get(imported).name().to_string(),
        };

        // Dot-import (`import . "path"`): merge the imported package's exported
        // objects into the importing file's scope so they can be referred to
        // without qualification. Equivalent to the `name == "."` branch of Go's
        // `importDecl` (resolver.go).
        if name == "." {
            let invalid = self.invalid_type();
            let obj = new_pkg_name(&mut self.objects, ".", imported, invalid);
            obj.set_pkg(&mut self.objects, self.pkg);
            let dot_pos = spec
                .name
                .as_ref()
                .map(|id| id.pos().0 as u32)
                .unwrap_or(spec.path.value_pos.0 as u32);
            obj.set_pos(&mut self.objects, dot_pos);
            // The `.` binds an explicit local name; record it and track it for
            // both the unused-import check and dot-import use-marking.
            match &spec.name {
                Some(id) => self.record_def(id, Some(obj)),
                None => self.record_implicit(spec.id, obj),
            }
            self.imports.push(obj);
            self.dot_imported.insert(imported, obj);

            // Merge exported objects into the file scope, without reparenting
            // them (they belong to the imported package). A collision with an
            // existing file-scope name is a redeclaration error.
            let pkg_scope = self.packages.get(imported).scope();
            let pos = spec.path.value_pos.0 as u32;
            for nm in self.scopes.get(pkg_scope).names() {
                if !crate::object::is_exported(&nm) {
                    continue;
                }
                let o = self
                    .scopes
                    .get(pkg_scope)
                    .lookup_local(&nm)
                    .expect("name came from this scope");
                if crate::scope::insert_no_reparent(&mut self.scopes, file_scope, &nm, o).is_some() {
                    self.error(
                        pos,
                        Code::DuplicateDecl,
                        format!("{} redeclared in this block", nm),
                    );
                }
            }
            return;
        }

        // A blank import (`import _ "path"`) is resolved for its side effects
        // only; it binds a `_` PkgName that `declare` keeps out of the scope,
        // and the unused-import check skips it (`obj.name != "_"`). All other
        // imports bind a usable package name.
        let invalid = self.invalid_type();
        let obj = new_pkg_name(&mut self.objects, name, imported, invalid);
        obj.set_pkg(&mut self.objects, self.pkg);
        let pos = spec.path.value_pos.0 as u32;
        // The PkgName's declaration position is the alias identifier if present,
        // otherwise the import path literal (Go's `ImportSpec.Pos()`).
        let name_pos = spec
            .name
            .as_ref()
            .map(|id| id.pos().0 as u32)
            .unwrap_or(pos);
        obj.set_pos(&mut self.objects, name_pos);
        self.declare(file_scope, obj, pos);
        // Track the import for the unused-import check (Go: `check.imports`).
        self.imports.push(obj);
        // Record the package name: an explicit alias binds a real identifier
        // (`recordDef`), while a name-less import synthesises the name from the
        // package (`recordImplicit`, keyed on the ImportSpec). (Go resolver.go.)
        match &spec.name {
            Some(id) => self.record_def(id, Some(obj)),
            None => self.record_implicit(spec.id, obj),
        }
    }

    /// Checks that a const/var spec's LHS names and RHS init values have
    /// matching counts. Equivalent to `Checker.arityMatch` (const/var paths).
    /// `const_decl` mirrors Go's `init != nil` (constants get their values via
    /// the inherited `last` spec; vars are passed `nil`). Positions for the
    /// "extra init expr at" sub-message are dropped (D07).
    fn arity_match(
        &mut self,
        names: &[String],
        values_len: usize,
        has_type: bool,
        names_pos: u32,
        const_decl: bool,
    ) {
        let l = names.len();
        let r = values_len;
        let code = Code::WrongAssignCount;
        if !const_decl && r == 0 {
            // var decl w/o init expr — fine as long as it has a type.
            if !has_type {
                self.error(names_pos, code, "missing type or init expr");
            }
        } else if l < r {
            self.error(names_pos, code, "extra init expr");
        } else if l > r && (const_decl || r != 1) {
            // if r == 1 it may be a multi-valued function; can't say yet.
            self.error(
                names_pos,
                code,
                format!("missing init expr for {}", names[r]),
            );
        }
    }

    /// Collects all package objects and inserts them into the package scope,
    /// then associates methods with their receiver base type names.
    ///
    /// Equivalent to `Checker.collectObjects`. See the module-level deferral
    /// list for what's intentionally omitted (imports, positions, recording).
    pub fn collect_objects(&mut self) {
        // Take the files out so we can iterate while mutating `self` freely
        // (Go iterates `check.files` directly under a GC). Restored at the end.
        let files = std::mem::take(&mut self.files);

        let pkg_scope = self.packages.get(self.pkg).scope();

        // Simplified `initFiles`: adopt the package name from the first file's
        // package clause if not already set (full multi-file consistency check
        // is deferred).
        if self.packages.get(self.pkg).name().is_empty() {
            if let Some(f) = files.first() {
                let nm = f.name.name.clone();
                self.packages.get_mut(self.pkg).set_name(nm);
            }
        }

        let mut methods: Vec<MethodInfo> = Vec::new();
        // File scopes, in file order — used after collection for the
        // package-vs-file-scope name-clash check (Go's `fileScopes`).
        let mut file_scopes: Vec<ScopeId> = Vec::new();

        for file in &files {
            self.env.version = file.go_version.clone();
            if !file.go_version.is_empty() {
                self.info
                    .file_versions
                    .insert(file.id, file.go_version.clone());
            }

            // The package identifier denotes the current package; no object.
            // DEFERRED: recordDef(file.Name, None).

            let fpos = file.pos().0 as u32;
            let fend = file.end().0 as u32;
            let file_scope = new_scope(
                &mut self.scopes,
                Some(pkg_scope),
                Some(self.universe_scope),
                fpos,
                fend,
                "",
            );
            self.record_scope(file.id, file_scope);
            file_scopes.push(file_scope);

            for decl in &file.decls {
                match decl {
                    Decl::BadDecl(_) => { /* ignore */ }
                    Decl::GenDecl(gd) => {
                        // Tracks the last ValueSpec carrying a type or init
                        // values, for inherited constant initializers.
                        let mut last_type: Option<Expr> = None;
                        let mut last_values: Vec<Expr> = Vec::new();
                        let mut have_last = false;

                        for (iota, spec) in gd.specs.iter().enumerate() {
                            match spec {
                                Spec::ImportSpec(is) => {
                                    self.import_decl(is, file_scope);
                                }
                                Spec::ValueSpec(vs) => match gd.tok {
                                    Some(Token::CONST) => {
                                        // Determine which init exprs to use.
                                        let mut inherited = true;
                                        if vs.ty.is_some() || !vs.values.is_empty() {
                                            last_type = vs.ty.clone();
                                            last_values = vs.values.clone();
                                            have_last = true;
                                            inherited = false;
                                        } else if !have_last {
                                            last_type = None;
                                            last_values = Vec::new();
                                            have_last = true;
                                            inherited = false;
                                        }

                                        let names: Vec<String> =
                                            vs.names.iter().map(|n| n.name.clone()).collect();
                                        let names_pos =
                                            vs.names.first().map(|n| n.pos().0 as u32).unwrap_or(0);
                                        self.arity_match(
                                            &names,
                                            last_values.len(),
                                            last_type.is_some(),
                                            names_pos,
                                            true,
                                        );

                                        let iota_val = make_int64(iota as i64);
                                        for (i, name) in names.iter().enumerate() {
                                            let invalid = self.invalid_type();
                                            let obj = new_const(
                                                &mut self.objects,
                                                name.clone(),
                                                invalid,
                                                iota_val.clone(),
                                            );
                                            obj.set_pkg(&mut self.objects, self.pkg);
                                            obj.set_pos(
                                                &mut self.objects,
                                                vs.names[i].pos().0 as u32,
                                            );

                                            let init = last_values.get(i).cloned();
                                            let d = DeclInfo {
                                                file_scope: Some(file_scope),
                                                version: self.env.version.clone(),
                                                vtyp: last_type.clone(),
                                                init,
                                                inherited,
                                                ..DeclInfo::default()
                                            };
                                            self.declare_pkg_obj(name, obj, d);
                                            self.record_def(&vs.names[i], Some(obj));
                                        }
                                    }
                                    Some(Token::VAR) => {
                                        let names: Vec<String> =
                                            vs.names.iter().map(|n| n.name.clone()).collect();
                                        let names_pos =
                                            vs.names.first().map(|n| n.pos().0 as u32).unwrap_or(0);
                                        self.arity_match(
                                            &names,
                                            vs.values.len(),
                                            vs.ty.is_some(),
                                            names_pos,
                                            false,
                                        );

                                        // n:1 var decl: one shared rhs feeds all
                                        // lhs vars (so each depends on it).
                                        let shared = vs.values.len() == 1;

                                        // Pre-allocate the lhs vars so the shared
                                        // DeclInfo can list them all.
                                        let mut lhs: Vec<ObjectId> =
                                            Vec::with_capacity(names.len());
                                        for (i, name) in names.iter().enumerate() {
                                            let invalid = self.invalid_type();
                                            let obj =
                                                new_var(&mut self.objects, name.clone(), invalid);
                                            obj.set_pkg(&mut self.objects, self.pkg);
                                            obj.set_pos(
                                                &mut self.objects,
                                                vs.names[i].pos().0 as u32,
                                            );
                                            lhs.push(obj);
                                        }

                                        for (i, name) in names.iter().enumerate() {
                                            let obj = lhs[i];
                                            let d = if shared {
                                                DeclInfo {
                                                    file_scope: Some(file_scope),
                                                    version: self.env.version.clone(),
                                                    lhs: lhs.clone(),
                                                    vtyp: vs.ty.clone(),
                                                    init: vs.values.first().cloned(),
                                                    ..DeclInfo::default()
                                                }
                                            } else {
                                                DeclInfo {
                                                    file_scope: Some(file_scope),
                                                    version: self.env.version.clone(),
                                                    vtyp: vs.ty.clone(),
                                                    init: vs.values.get(i).cloned(),
                                                    ..DeclInfo::default()
                                                }
                                            };
                                            self.declare_pkg_obj(name, obj, d);
                                            self.record_def(&vs.names[i], Some(obj));
                                        }
                                    }
                                    _ => {
                                        let at = spec.pos().0 as u32;
                                        self.error(
                                            at,
                                            Code::InvalidSyntaxTree,
                                            "invalid token in value spec",
                                        );
                                    }
                                },
                                Spec::TypeSpec(ts) => {
                                    let name = ts.name.name.clone();
                                    let obj = new_type_name(&mut self.objects, name.clone(), None);
                                    obj.set_pkg(&mut self.objects, self.pkg);
                                    obj.set_pos(&mut self.objects, ts.name.pos().0 as u32);
                                    let d = DeclInfo {
                                        file_scope: Some(file_scope),
                                        version: self.env.version.clone(),
                                        tdecl: Some(ts.clone()),
                                        ..DeclInfo::default()
                                    };
                                    self.declare_pkg_obj(&name, obj, d);
                                    self.record_def(&ts.name, Some(obj));
                                }
                            }
                        }
                    }
                    Decl::FuncDecl(fd) => {
                        self.collect_func_decl(fd, file_scope, &mut methods);
                    }
                }
            }
        }

        // Verify that objects in the package scope and the file scopes have
        // different names: an import must not collide with a package-level
        // declaration. Equivalent to the `fileScopes` clash loop in Go's
        // `collectObjects` (resolver.go). File scopes currently hold only
        // import `PkgName`s (dot-import scope merge is deferred, D16).
        let pkg_scope = self.packages.get(self.pkg).scope();
        for &fscope in &file_scopes {
            for name in self.scopes.get(fscope).names() {
                let alt = match scope_lookup(&self.scopes, pkg_scope, &name) {
                    Some(a) => a,
                    None => continue,
                };
                let obj = self
                    .scopes
                    .get(fscope)
                    .lookup_local(&name)
                    .expect("name came from this scope");
                let msg = match self.objects.get(obj) {
                    crate::arena::ObjectData::PkgName(p) => {
                        let path = self.packages.get(p.imported()).path().to_string();
                        format!("{} already declared through import of package {:?}", name, path)
                    }
                    // A dot-imported object (deferred); report generically.
                    _ => format!("{} already declared through dot-import", name),
                };
                let at = alt.pos(&self.objects);
                self.error(at, Code::DuplicateDecl, msg);
            }
        }

        // Restore files before associating methods.
        self.files = files;

        // Associate methods with their receiver base type name where possible.
        if methods.is_empty() {
            return;
        }
        for m in &methods {
            if let Some(base) = self.resolve_base_type_name(m.ptr, &m.recv) {
                // DEFERRED: set Func.hasPtrRecv_ (recovered from signature
                // later). Just record the association.
                self.methods.entry(base).or_default().push(m.obj);
            }
        }
    }

    /// Collects a single function or method declaration. Regular functions are
    /// declared in the package scope; methods are recorded in `obj_map` and
    /// queued for receiver association.
    fn collect_func_decl(
        &mut self,
        fd: &FuncDecl,
        file_scope: ScopeId,
        methods: &mut Vec<MethodInfo>,
    ) {
        let name = fd.name.name.clone();
        let obj = new_func(&mut self.objects, name.clone(), None);
        obj.set_pkg(&mut self.objects, self.pkg);
        obj.set_pos(&mut self.objects, fd.name.pos().0 as u32);

        let is_method = fd
            .recv
            .as_ref()
            .map(|r| !r.list.is_empty())
            .unwrap_or(false);

        if !is_method {
            // regular function
            if name == "init" {
                // init functions are invisible — not declared in pkg scope.
                let pkg_scope = self.packages.get(self.pkg).scope();
                obj.set_parent(&mut self.objects, pkg_scope);
                // DEFERRED: recordDef + MissingInitBody soft error.
                if fd.body.is_none() {
                    self.error(
                        obj.pos(&self.objects),
                        Code::MissingInitBody,
                        "func init must have a body",
                    );
                }
            } else {
                let pkg_scope = self.packages.get(self.pkg).scope();
                self.declare(pkg_scope, obj, 0 /* nopos */);
            }
        } else {
            // method: unpack the receiver base type name and queue it.
            let recv_ty = fd.recv.as_ref().unwrap().list[0].ty.clone();
            if let Some(rty) = recv_ty {
                let (ptr, base) = unpack_recv(&rty);
                if let Some(recv) = base {
                    if name != "_" {
                        methods.push(MethodInfo { obj, ptr, recv });
                    }
                }
            }
        }

        // Record the defining identifier for every function/method (Go records
        // this in `collectObjects` for regular funcs, methods, and `init`).
        self.record_def(&fd.name, Some(obj));

        let info = DeclInfo {
            file_scope: Some(file_scope),
            version: self.env.version.clone(),
            fdecl: Some(fd.clone()),
            ..DeclInfo::default()
        };
        // Methods aren't package-level objects, but we still track them in the
        // object map so they can be handled like regular functions.
        self.obj_map.insert(obj, info);
        let order = self.obj_map.len() as u32;
        obj.set_order(&mut self.objects, order);
    }

    /// Returns the non-alias base `TypeName` for the given receiver name,
    /// following non-generic alias declarations. Returns `None` if no such
    /// package-scope type name exists.
    ///
    /// Equivalent to `Checker.resolveBaseTypeName` (the `ptr` return is dropped
    /// here — `Func.hasPtrRecv_` is recovered from the signature later).
    fn resolve_base_type_name(&self, mut ptr: bool, name: &str) -> Option<ObjectId> {
        let pkg_scope = self.packages.get(self.pkg).scope();
        let mut seen: Vec<ObjectId> = Vec::new();
        let mut cur = Some(name.to_string());

        while let Some(n) = cur {
            // name must denote an object in the current package scope.
            let obj = scope_lookup(&self.scopes, pkg_scope, &n)?;

            // it must be a type name we have not seen before.
            if !matches!(self.objects.get(obj), crate::arena::ObjectData::TypeName(_)) {
                return None;
            }
            if seen.contains(&obj) {
                return None;
            }

            // done if the decl describes a defined type (not an alias).
            let tdecl = self.obj_map.get(&obj)?.tdecl.as_ref()?;
            let is_alias = tdecl.assign.is_valid();
            if !is_alias {
                return Some(obj);
            }
            // a generic alias must not be traversed (go.dev/issue/70417).
            if tdecl.type_params.is_some() {
                return None;
            }
            seen.push(obj);

            // continue through the alias RHS, dereferencing one pointer.
            let mut typ = unparen(&tdecl.ty);
            if let Expr::StarExpr(s) = typ {
                if ptr {
                    return None; // already saw a pointer
                }
                ptr = true;
                typ = unparen(&s.x);
            }
            // RHS must be a locally defined type name to keep resolving.
            cur = match typ {
                Expr::Ident(id) => Some(id.name.clone()),
                _ => None,
            };
        }
        None
    }

    /// Type-check all package objects (but not function bodies), in the order
    /// Go uses: non-alias type declarations first, then aliases, then
    /// everything else. This avoids most cases where an alias's type is needed
    /// before it is available.
    ///
    /// Equivalent to `Checker.packageObjects` (the acyclic core). The
    /// "re-collect methods for already-typed types" multi-`Files` loop is
    /// deferred; `check.methods` is cleared at the end.
    pub fn package_objects(&mut self) {
        // Classify up-front (no obj_decl while borrowing the arena).
        let mut nonalias_types: Vec<ObjectId> = Vec::new();
        let mut aliases: Vec<ObjectId> = Vec::new();
        let mut others: Vec<ObjectId> = Vec::new();
        for &obj in &self.obj_list {
            if matches!(self.objects.get(obj), crate::arena::ObjectData::TypeName(_)) {
                let is_alias = self
                    .obj_map
                    .get(&obj)
                    .and_then(|d| d.tdecl.as_ref())
                    .map(|t| t.assign.is_valid())
                    .unwrap_or(false);
                if is_alias {
                    aliases.push(obj);
                } else {
                    nonalias_types.push(obj);
                }
            } else {
                others.push(obj);
            }
        }

        for obj in nonalias_types {
            self.obj_decl(obj);
        }
        for obj in aliases {
            self.obj_decl(obj);
        }
        for obj in others {
            self.obj_decl(obj);
        }

        // Any methods left here had receiver base types that weren't found;
        // errors were already reported when declaring them. Discard the map.
        self.methods.clear();
    }

    /// Sorts package-level objects by source order for reproducible
    /// processing. Equivalent to `Checker.sortObjects`.
    pub fn sort_objects(&mut self) {
        let mut list: Vec<ObjectId> = self.obj_map.keys().copied().collect();
        list.sort_by_key(|o| o.order(&self.objects));
        self.obj_list = list;
    }
}

/// Unpacks a receiver type expression: returns whether it's a pointer receiver
/// and the receiver base type name (stripped of type parameters), if it is an
/// identifier. Equivalent to `Checker.unpackRecv` with `unpackParams = false`.
fn unpack_recv(rtyp: &Expr) -> (bool, Option<String>) {
    let mut ptr = false;
    let mut base = unparen(rtyp);
    if let Expr::StarExpr(s) = base {
        ptr = true;
        base = unparen(&s.x);
    }
    // strip type parameters: `T[A, _]` -> `T`
    let base = match base {
        Expr::IndexExpr(ix) => &ix.x,
        Expr::IndexListExpr(ix) => &ix.x,
        other => other,
    };
    let name = match base {
        Expr::Ident(id) => Some(id.name.clone()),
        _ => None,
    };
    (ptr, name)
}

/// Removes the surrounding quotes from an import path literal (`"unsafe"` →
/// `unsafe`). Import paths are always interpreted (double-quoted) or raw
/// (backquoted) string literals; we only need the enclosed text.
fn unquote_import_path(lit: &str) -> String {
    let bytes = lit.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'`' && last == b'`') {
            return lit[1..lit.len() - 1].to_string();
        }
    }
    lit.to_string()
}

/// Strips enclosing parentheses from an expression. Mirrors `ast.Unparen`.
fn unparen(e: &Expr) -> &Expr {
    let mut cur = e;
    while let Expr::ParenExpr(p) = cur {
        cur = &p.x;
    }
    cur
}

// ===== DEFERRED (forward pointers) =====
// Go: Checker.packageObjects (resolver.go) — type-checks each package object by
//   calling objDecl, which is not ported yet. Land it in chunk 32 once decl.rs
//   (chunk 23) provides objDecl. It also calls collectMethods on already-typed
//   TypeNames and then clears check.methods.
// Go: Checker.unusedImports / errorUnusedPkg — need PkgName + use tracking.
// Go: Checker.importPackage / validatedImportPath — need a Config.Importer.
