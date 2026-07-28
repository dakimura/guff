//! The `typeindex` analyzer — reverse index of type information.
//!
//! Port of `golang.org/x/tools/internal/typesinternal/typeindex` and the
//! helper analyzer at `golang.org/x/tools/internal/analysis/typeindex`
//! (vendored by staticcheck as `honnef.co/go/tools/internal/xtools-internal/...`).
//!
//! Like [`inspect`](super::inspect), this is a helper for later passes; it
//! reports no diagnostics of its own. Dependents use [`Index::uses`],
//! [`Index::calls`], [`Index::object`], and [`Index::selection`] to skip
//! packages that do not reference a symbol and to visit only relevant call
//! sites instead of the whole AST.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, File, Ident};
use guff::walk::{preorder, NodeRef};
use guff_types::api::Info;
use guff_types::arena::{ObjectArena, ObjectData, ObjectId, PackageArena, PackageId, TypeArena};
use guff_types::lookup::{lookup_field_or_method, LookupResult};
use guff_types::operand::OperandMode;
use guff_types::scope;

use crate::analyzer::{AnalysisResult, Analyzer, RunError, RunFn};
use crate::pass::Pass;
use crate::passes::inspect;

/// Inverse of [`Info`](guff_types::api::Info) for a single package.
///
/// Ident uses / defs are keyed by stamped [`Ident::id`]. Call sites are keyed
/// by stamped [`CallExpr::id`].
#[derive(Clone, Default)]
pub struct Index {
    /// Import path → package id for packages referenced from this package.
    packages: HashMap<String, PackageId>,
    /// Object → defining Ident id (in this package), if any.
    def: HashMap<ObjectId, u32>,
    /// Object → Ident ids that use it (in this package).
    uses: HashMap<ObjectId, Vec<u32>>,
    /// Object → CallExpr ids whose callee is that object ([`typeutil.Callee`]).
    calls: HashMap<ObjectId, Vec<u32>>,
}

impl Index {
    /// Build an index from type-annotated syntax.
    ///
    /// Port of `typeindex.New`. Generic field/method origin remapping is
    /// DEFERRED until `Func`/`Var` grow an `Origin()` API (R18).
    pub fn new(
        files: &[File],
        pkg: PackageId,
        info: &Info,
        objects: &ObjectArena,
        packages: &PackageArena,
    ) -> Self {
        let mut ix = Index::default();

        let mut add_package = |pkg2: PackageId| {
            if pkg2 != pkg {
                let path = packages.get(pkg2).path().to_string();
                ix.packages.insert(path, pkg2);
            }
        };

        for file in files {
            preorder(NodeRef::File(file), |n| {
                match n {
                    NodeRef::ImportSpec(spec) => {
                        if let Some(pkg_name) = pkg_name_of(info, spec) {
                            if let Some(imported) = pkg_name.imported_pkg(objects) {
                                add_package(imported);
                            }
                        }
                    }
                    NodeRef::Ident(id) => {
                        if let Some(Some(obj)) = info.defs.get(&id.id).copied() {
                            ix.def.insert(obj, id.id);
                        }
                        if let Some(obj) = info.uses.get(&id.id).copied() {
                            if !is_package_level(objects, packages, pkg, obj) {
                                if let Some(p) = obj.pkg(objects) {
                                    add_package(p);
                                }
                            }
                            ix.uses.entry(obj).or_default().push(id.id);
                            // DEFERRED: also record uses of generic Origin() for
                            // instantiated field/method selections.
                        }
                    }
                    NodeRef::CallExpr(call) => {
                        if let Some(obj) = callee(info, objects, &call.fun) {
                            ix.calls.entry(obj).or_default().push(call.id);
                        }
                    }
                    _ => {}
                }
                true
            });
        }

        ix
    }

    /// Ident node ids in this package that refer to `obj`.
    pub fn uses(&self, obj: ObjectId) -> &[u32] {
        self.uses.get(&obj).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Reports whether any of `objs` has at least one use in this package.
    ///
    /// Nil / missing objects are ignored so callers can pass
    /// [`Index::object`] results directly.
    pub fn used(&self, objs: &[Option<ObjectId>]) -> bool {
        objs.iter().any(|o| o.is_some_and(|id| self.uses.contains_key(&id)))
    }

    /// Defining Ident id for `obj` in this package, if any.
    pub fn def(&self, obj: ObjectId) -> Option<u32> {
        self.def.get(&obj).copied()
    }

    /// Package of the given import path, if referenced from this package.
    pub fn package(&self, path: &str) -> Option<PackageId> {
        self.packages.get(path).copied()
    }

    /// Package-level symbol `name` in package `path`, if visible.
    pub fn object(
        &self,
        packages: &PackageArena,
        scopes: &guff_types::arena::ScopeArena,
        path: &str,
        name: &str,
    ) -> Option<ObjectId> {
        let pkg = self.package(path)?;
        let scope = packages.get(pkg).scope();
        scope::lookup(scopes, scope, name)
    }

    /// Named method or field of the package-level type `typename` in `path`.
    pub fn selection(
        &self,
        types: &TypeArena,
        objects: &ObjectArena,
        packages: &PackageArena,
        scopes: &guff_types::arena::ScopeArena,
        path: &str,
        typename: &str,
        name: &str,
    ) -> Option<ObjectId> {
        let obj = self.object(packages, scopes, path, typename)?;
        let ObjectData::TypeName(tn) = objects.get(obj) else {
            return None;
        };
        let typ = tn.typ()?;
        let mut types = types.clone();
        match lookup_field_or_method(
            &mut types,
            objects,
            packages,
            typ,
            true,
            obj.pkg(objects),
            name,
        ) {
            LookupResult::Found { obj, .. } => Some(obj),
            _ => None,
        }
    }

    /// CallExpr node ids that call `callee` ([`typeutil.Callee`] semantics).
    pub fn calls(&self, callee: ObjectId) -> &[u32] {
        self.calls.get(&callee).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Visit each [`CallExpr`] in `files` whose callee is `callee`.
    pub fn for_each_call<'a, F>(&self, callee: ObjectId, files: &'a [File], mut f: F)
    where
        F: FnMut(&'a CallExpr) -> bool,
    {
        let ids = self.calls(callee);
        if ids.is_empty() {
            return;
        }
        let want: std::collections::HashSet<u32> = ids.iter().copied().collect();
        for file in files {
            let mut cont = true;
            preorder(NodeRef::File(file), |n| {
                if !cont {
                    return false;
                }
                if let NodeRef::CallExpr(call) = n {
                    if want.contains(&call.id) && !f(call) {
                        cont = false;
                        return false;
                    }
                }
                true
            });
            if !cont {
                return;
            }
        }
    }
}

fn pkg_name_of(info: &Info, spec: &guff::ast::ImportSpec) -> Option<ObjectId> {
    if let Some(name) = &spec.name {
        info.defs.get(&name.id).copied().flatten()
    } else {
        info.implicits.get(&spec.id).copied()
    }
}

fn is_package_level(
    objects: &ObjectArena,
    packages: &PackageArena,
    _pkg: PackageId,
    obj: ObjectId,
) -> bool {
    let Some(parent) = obj.parent(objects) else {
        return false;
    };
    let Some(obj_pkg) = obj.pkg(objects) else {
        return false;
    };
    parent == packages.get(obj_pkg).scope()
}

/// Port of `typeutil.Callee` / `usedIdent`.
fn callee(info: &Info, objects: &ObjectArena, fun: &Expr) -> Option<ObjectId> {
    let id = used_ident(info, fun)?;
    let obj = info.uses.get(&id.id).copied()?;
    if matches!(objects.get(obj), ObjectData::TypeName(_)) {
        return None;
    }
    Some(obj)
}

fn used_ident<'a>(info: &Info, fun: &'a Expr) -> Option<&'a Ident> {
    let mut e = unparen(fun);
    match e {
        Expr::IndexExpr(ix) => {
            if info
                .types
                .get(&ix.index.id())
                .is_some_and(|tv| tv.mode == OperandMode::TypeExpr)
            {
                e = unparen(&ix.x);
            }
        }
        Expr::IndexListExpr(ix) => {
            e = unparen(&ix.x);
        }
        _ => {}
    }
    match unparen(e) {
        Expr::Ident(id) => Some(id),
        Expr::SelectorExpr(sel) => Some(&sel.sel),
        _ => None,
    }
}

fn unparen(expr: &Expr) -> &Expr {
    let mut e = expr;
    while let Expr::ParenExpr(p) = e {
        e = &p.x;
    }
    e
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "typeindex requires inspect analyzer".to_string())?;

    let Some(info) = pass.types_info() else {
        return Ok(Some(Box::new(Index::default())));
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return Ok(Some(Box::new(Index::default())));
    };
    let Some(pkg) = pass.type_pkg() else {
        return Ok(Some(Box::new(Index::default())));
    };

    let index = Index::new(
        pass.files(),
        pkg,
        info,
        &artifacts.objects,
        &artifacts.packages,
    );
    Ok(Some(Box::new(index)))
}

fn typeindex_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "typeindex",
        doc: "indexes of type information for later passes",
        url: "https://pkg.go.dev/golang.org/x/tools/internal/analysis/typeindex",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// The `typeindex` analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(typeindex_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use guff::position::FileSet;
    use guff_packages::{typecheck_package, LoadMode, Package, TypecheckEnv};
    use guff_types::default_sizes;

    use super::*;
    use crate::pass::PassInput;

    fn typechecked_local_calls() -> Package {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/typeindex");
        let path = dir.join("calls.go");
        let mut pkg = Package {
            id: "example.com/typeindex".into(),
            pkg_path: "example.com/typeindex".into(),
            name: "p".into(),
            dir: dir.clone(),
            compiled_go_files: vec![path.clone()],
            go_files: vec![path],
            ..Package::default()
        };
        let fset = FileSet::new();
        typecheck_package(
            &mut pkg,
            &fset,
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            default_sizes(),
            &TypecheckEnv::default(),
            LoadMode::LOAD_SYNTAX,
        );
        pkg
    }

    #[test]
    fn typeindex_validates() {
        assert!(crate::validate::validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn calls_indexes_local_helper() {
        let pkg = typechecked_local_calls();
        assert!(!pkg.ill_typed, "{:?}", pkg.errors);

        let fset = pkg.fset.clone().expect("fset");
        let mut diags = Vec::new();
        let mut facts = crate::facts::FactStore::default();

        let inspect_result = {
            let mut pass = PassInput {
                analyzer: inspect::analyzer(),
                fset: &fset,
                files: &pkg.syntax,
                pkg: &pkg,
                pkg_arc: None,
                types_info: pkg.types_info.as_deref(),
                types_sizes: default_sizes(),
                diagnostics: &mut diags,
                result_of: std::collections::HashMap::new(),
                facts: &mut facts,
                settings: std::sync::Arc::new(crate::SettingsBag::default()),
            }
            .build();
            (inspect::analyzer().run)(&mut pass).unwrap().unwrap()
        };

        let mut result_of = std::collections::HashMap::new();
        result_of.insert(
            inspect::analyzer().name,
            std::sync::Arc::from(inspect_result),
        );

        let mut pass = PassInput {
            analyzer: analyzer(),
            fset: &fset,
            files: &pkg.syntax,
            pkg: &pkg,
            pkg_arc: None,
            types_info: pkg.types_info.as_deref(),
            types_sizes: default_sizes(),
            diagnostics: &mut diags,
            result_of,
            facts: &mut facts,
            settings: std::sync::Arc::new(crate::SettingsBag::default()),
        }
        .build();

        let index = run(&mut pass)
            .expect("typeindex run")
            .unwrap()
            .downcast::<Index>()
            .expect("Index");

        let info = pkg.types_info.as_deref().unwrap();
        let artifacts = pkg.type_artifacts.as_ref().unwrap();
        // Resolve `helper` via Defs of its FuncDecl name.
        let helper = pkg
            .syntax
            .iter()
            .flat_map(|f| f.decls.iter())
            .find_map(|d| match d {
                guff::ast::Decl::FuncDecl(fd) if fd.name.name == "helper" => {
                    info.defs.get(&fd.name.id).copied().flatten()
                }
                _ => None,
            })
            .expect("helper def");
        assert!(matches!(
            artifacts.objects.get(helper),
            ObjectData::Func(_)
        ));
        assert!(index.used(&[Some(helper)]));
        assert_eq!(index.calls(helper).len(), 2);

        let mut n = 0;
        index.for_each_call(helper, &pkg.syntax, |_c| {
            n += 1;
            true
        });
        assert_eq!(n, 2);
    }
}
