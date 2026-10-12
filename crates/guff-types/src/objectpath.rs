//! Stable names for [`ObjectId`]s relative to their enclosing package.
//!
//! Port of `golang.org/x/tools/go/types/objectpath`, for the paths guff's facts
//! need. guff type-checks every package in an arena of its own, so a fact is
//! moved from the exporting package to an importer by path even within one
//! run — where go/analysis hands the importer the same `types.Object`. That is
//! also why unexported package-level names are given a path here: upstream
//! never needs to encode one.
//!
//! Supported (upstream's operators: `.` Type, `U` Underlying, `M` Method, `F`
//! Field):
//! - `Name` — a package-scope object (`Package.Scope.Lookup`).
//! - `T.M<i>` — the `i`th method of the named type `T`: `concreteMethod`. A
//!   method of an instance is encoded as its origin.
//! - `T.UF<i>` — the `i`th field of `T`'s underlying struct.
//! - `V.F<i>` — the `i`th field of a package variable's struct type, as in
//!   `var Struct struct{ Wrapf func(…) }`.
//!
//! DEFERRED: the rest of upstream's traversal — interface methods,
//! parameters, results, type parameters, nested composite types.

use crate::arena::{ObjectArena, ObjectData, ObjectId, PackageArena, PackageId, ScopeArena};
use crate::arena::{TypeArena, TypeData, TypeId};
use crate::scope::lookup;

/// Opaque path identifying an object relative to its package.
pub type Path = String;

/// Returns the path of `obj` relative to its package, or an error when the
/// object has none guff can encode.
///
/// Equivalent to `objectpath.For`, for the forms in the module comment.
pub fn for_object(
    types: &TypeArena,
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
    if obj.parent(objects) == Some(pkg_scope) {
        return match lookup(scopes, pkg_scope, name) {
            Some(found) if found == obj => Ok(name.to_string()),
            _ => Err(format!(
                "object `{name}` not found in package scope (shadowed or incomplete)"
            )),
        };
    }
    match objects.get(obj) {
        ObjectData::Func(_) => concrete_method(types, packages, objects, scopes, obj),
        ObjectData::Var(v) if v.kind() == crate::object::var::VarKind::Field => {
            field_path(types, packages, objects, scopes, pkg, obj)
        }
        _ => Err(format!("no objectpath for `{name}` (DEFERRED)")),
    }
}

/// `Encoder.concreteMethod`: `T.M<i>` for a method declared on the named type
/// `T` — through a pointer receiver too — at index `i` of `T`'s methods.
fn concrete_method(
    types: &TypeArena,
    packages: &PackageArena,
    objects: &ObjectArena,
    scopes: &ScopeArena,
    meth: ObjectId,
) -> Result<Path, String> {
    let meth = crate::object::func::func_origin(objects, meth);
    let named = receiver_named(types, objects, meth).ok_or("method has no named receiver")?;
    let named = crate::named::named_origin(types, named);
    if matches!(
        types.get(named.underlying(types)),
        TypeData::Interface(_)
    ) {
        return Err("interface method (DEFERRED)".into());
    }
    let tn = crate::named::named_obj(types, named);
    let tname = package_scope_name(packages, objects, scopes, tn)?;
    (0..crate::named::named_num_methods(types, named))
        .find(|&i| crate::named::named_method(types, named, i) == meth)
        .map(|i| format!("{tname}.M{i}"))
        .ok_or_else(|| "method not among its type's methods".into())
}

/// The named type a method's receiver is declared on (`T` for `T` or `*T`).
fn receiver_named(types: &TypeArena, objects: &ObjectArena, meth: ObjectId) -> Option<TypeId> {
    let sig = meth.typ(objects)?;
    let recv = crate::signature::signature_recv(types, sig)?;
    let mut t = crate::alias::unalias_readonly(types, recv.typ(objects)?);
    if let TypeData::Pointer(p) = types.get(t) {
        t = crate::alias::unalias_readonly(types, p.elem());
    }
    matches!(types.get(t), TypeData::Named(_)).then_some(t)
}

/// `T.UF<i>` or `V.F<i>`: a field of a package-level named struct type, or of
/// the struct type of a package-level variable.
fn field_path(
    types: &TypeArena,
    packages: &PackageArena,
    objects: &ObjectArena,
    scopes: &ScopeArena,
    pkg: PackageId,
    field: ObjectId,
) -> Result<Path, String> {
    let field = crate::object::var::var_origin(objects, field);
    let scope = packages.get(pkg).scope();
    for name in scopes.get(scope).names() {
        let Some(member) = lookup(scopes, scope, &name) else {
            continue;
        };
        let (prefix, strct) = match objects.get(member) {
            ObjectData::TypeName(tn) => {
                let Some(t) = tn.typ() else { continue };
                if !matches!(types.get(t), TypeData::Named(_)) {
                    continue;
                }
                (format!("{name}.U"), t.underlying(types))
            }
            ObjectData::Var(v) => (format!("{name}."), v.typ()),
            _ => continue,
        };
        if !matches!(types.get(strct), TypeData::Struct(_)) {
            continue;
        }
        let n = crate::r#struct::struct_num_fields(types, strct);
        if let Some(i) = (0..n).find(|&i| crate::r#struct::struct_field(types, strct, i) == field) {
            return Ok(format!("{prefix}F{i}"));
        }
    }
    Err("field of no package-level struct (DEFERRED)".into())
}

fn package_scope_name(
    packages: &PackageArena,
    objects: &ObjectArena,
    scopes: &ScopeArena,
    obj: ObjectId,
) -> Result<String, String> {
    let pkg = obj.pkg(objects).ok_or("object has no package")?;
    let name = obj.name(objects);
    match lookup(scopes, packages.get(pkg).scope(), name) {
        Some(found) if found == obj => Ok(name.to_string()),
        _ => Err(format!("`{name}` is not package-scoped")),
    }
}

/// Resolves `path` to an object in `pkg`.
///
/// Equivalent to `objectpath.Object`, for the forms in the module comment.
pub fn object(
    types: &TypeArena,
    packages: &PackageArena,
    objects: &ObjectArena,
    scopes: &ScopeArena,
    pkg: PackageId,
    path: &str,
) -> Result<ObjectId, String> {
    if path.is_empty() {
        return Err("empty objectpath".into());
    }
    let scope = packages.get(pkg).scope();
    let (head, ops) = match path.split_once('.') {
        Some((h, o)) => (h, Some(o)),
        None => (path, None),
    };
    let base = lookup(scopes, scope, head)
        .ok_or_else(|| format!("no package-scope object `{head}`"))?;
    let Some(ops) = ops else {
        return Ok(base);
    };
    let index = |digits: &str| -> Result<usize, String> {
        digits
            .parse::<usize>()
            .map_err(|_| format!("bad objectpath `{path}`"))
    };
    if let Some(i) = ops.strip_prefix('M') {
        let i = index(i)?;
        let ObjectData::TypeName(tn) = objects.get(base) else {
            return Err(format!("`{head}` is not a type"));
        };
        let t = tn.typ().ok_or("type name without a type")?;
        if !matches!(types.get(t), TypeData::Named(_)) || i >= crate::named::named_num_methods(types, t) {
            return Err(format!("no method {i} on `{head}`"));
        }
        return Ok(crate::named::named_method(types, t, i));
    }
    let (strct, i) = if let Some(i) = ops.strip_prefix("UF") {
        let ObjectData::TypeName(tn) = objects.get(base) else {
            return Err(format!("`{head}` is not a type"));
        };
        (tn.typ().ok_or("type name without a type")?.underlying(types), index(i)?)
    } else if let Some(i) = ops.strip_prefix('F') {
        let ObjectData::Var(v) = objects.get(base) else {
            return Err(format!("`{head}` is not a variable"));
        };
        (v.typ(), index(i)?)
    } else {
        return Err(format!("objectpath `{path}` DEFERRED"));
    };
    if !matches!(types.get(strct), TypeData::Struct(_))
        || i >= crate::r#struct::struct_num_fields(types, strct)
    {
        return Err(format!("no field {i} in `{head}`"));
    }
    Ok(crate::r#struct::struct_field(types, strct, i))
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
            let path = for_object(&check.types, &check.packages, &check.objects, &check.scopes, obj)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(path, name);
            let resolved =
                object(&check.types, &check.packages, &check.objects, &check.scopes, pkg, &path).expect("resolve");
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
            assert!(for_object(&check.types, &check.packages, &check.objects, &check.scopes, obj).is_err());
        }
    }

    #[test]
    fn methods_and_fields_roundtrip() {
        let check = check(
            "package p\ntype T struct{ X, Y int }\nfunc (T) A() {}\nfunc (*T) B() {}\n\
             type G[E any] struct{ v E }\nfunc (G[E]) C() {}\n\
             var V struct{ F func() }\n",
        );
        let pkg = check.pkg;
        for want in ["T.M0", "T.M1", "G.M0", "T.UF0", "T.UF1", "G.UF0", "V.F0"] {
            let obj = object(&check.types, &check.packages, &check.objects, &check.scopes, pkg, want)
                .unwrap_or_else(|e| panic!("{want}: {e}"));
            let got = for_object(&check.types, &check.packages, &check.objects, &check.scopes, obj)
                .unwrap_or_else(|e| panic!("{want}: {e}"));
            assert_eq!(got, want);
        }
    }

    #[test]
    fn rejects_deferred_paths() {
        let check = check("package p\ntype I interface{ M() }\n");
        let pkg = check.pkg;
        assert!(object(&check.types, &check.packages, &check.objects, &check.scopes, pkg, "I.UM0").is_err());
    }
}
