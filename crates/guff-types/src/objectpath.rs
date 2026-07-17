//! Stable names for [`ObjectId`]s relative to their enclosing package.
//!
//! Port of `golang.org/x/tools/go/types/objectpath` (package-scope subset).
//!
//! Full OT/TT/TO paths (fields, methods, params, type params) are
//! `// DEFERRED:` — see DEVELOPMENT.md R24. Package-level objects are enough
//! for the fact types guff persists today (`IsDeprecated`, exhaustive enums,
//! modernize `NewLike`).

use crate::arena::{ObjectArena, ObjectId, PackageArena, PackageId, ScopeArena};
use crate::scope::lookup;

/// Opaque path identifying an object relative to its package.
///
/// For the package-scope subset this is simply the object's declared name
/// (Go's PO / `Package.Scope.Lookup` operator).
pub type Path = String;

/// Returns the path of `obj` relative to its package, or an error when the
/// object is not addressable from the package scope.
///
/// Equivalent to `objectpath.For` for package-level objects.
pub fn for_object(
    packages: &PackageArena,
    objects: &ObjectArena,
    scopes: &ScopeArena,
    obj: ObjectId,
) -> Result<Path, String> {
    let Some(pkg) = obj.pkg(objects) else {
        return Err("object has no package".into());
    };
    let pkg_scope = packages.get(pkg).scope();
    let name = obj.name(objects);
    if obj.parent(objects) != Some(pkg_scope) {
        return Err(format!(
            "object `{name}` is not package-scoped (full objectpath DEFERRED)"
        ));
    }
    match lookup(scopes, pkg_scope, name) {
        Some(found) if found == obj => Ok(name.to_string()),
        _ => Err(format!(
            "object `{name}` not found in package scope (shadowed or incomplete)"
        )),
    }
}

/// Resolves `path` to an object in `pkg`.
///
/// Equivalent to `objectpath.Object` for package-level paths (bare identifiers).
/// Complex paths containing `.` / type operators are DEFERRED.
pub fn object(
    packages: &PackageArena,
    scopes: &ScopeArena,
    pkg: PackageId,
    path: &str,
) -> Result<ObjectId, String> {
    if path.is_empty() {
        return Err("empty objectpath".into());
    }
    if !is_package_scope_path(path) {
        return Err(format!(
            "complex objectpath `{path}` DEFERRED (package-scope only)"
        ));
    }
    let scope = packages.get(pkg).scope();
    lookup(scopes, scope, path)
        .ok_or_else(|| format!("no package-scope object `{path}`"))
}

/// Reports whether `path` is a bare identifier (package-scope Lookup).
///
/// Nested Go objectpaths always contain `.` (the OT / `.Type()` operator)
/// after the package-scope name, e.g. `T.UF0`. A path without `.` is therefore
/// package-scope — even when the identifier happens to contain letters that
/// are also used as operators (`Map`, `Error`, …).
fn is_package_scope_path(path: &str) -> bool {
    !path.is_empty() && !path.contains('.')
}

#[cfg(test)]
mod tests {
    use guff::parser::{parse_file, Mode};
    use guff::position::FileSet;

    use super::*;
    use crate::scope::lookup as scope_lookup;
    use crate::{Checker, Config};

    fn check(src: &str) -> Checker {
        let fset = FileSet::new();
        let file = parse_file(&fset, "t.go", src.as_bytes(), Mode::NONE).expect("parse");
        let mut check = Checker::new(Config::default());
        check.check_files(vec![file]);
        check
    }

    #[test]
    fn package_scope_roundtrip() {
        let check = check("package p\n\ntype T int\nfunc F() {}\nvar V int\n");
        let pkg = check.pkg;
        let scope = check.packages.get(pkg).scope();

        for name in ["T", "F", "V"] {
            let obj = scope_lookup(&check.scopes, scope, name).expect(name);
            let path = for_object(&check.packages, &check.objects, &check.scopes, obj)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(path, name);
            let resolved =
                object(&check.packages, &check.scopes, pkg, &path).expect("resolve");
            assert_eq!(resolved, obj);
        }
    }

    #[test]
    fn rejects_local_var() {
        let check = check("package p\nfunc F() { var local int; _ = local }\n");
        let found_local = check
            .objects
            .ids()
            .find(|id| id.name(&check.objects) == "local");
        // Local vars may or may not remain in the arena depending on checker;
        // if present, for_object must reject them.
        if let Some(obj) = found_local {
            assert!(for_object(&check.packages, &check.objects, &check.scopes, obj).is_err());
        }
    }

    #[test]
    fn rejects_complex_path_on_decode() {
        let check = check("package p\ntype T struct{ X int }\n");
        let pkg = check.pkg;
        assert!(object(&check.packages, &check.scopes, pkg, "T.UF0").is_err());
    }
}
