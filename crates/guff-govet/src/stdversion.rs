//! `stdversion` — report uses of standard library symbols that are too new
//! for the Go version in force in the referring file.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/stdversion` (v0.50.0) and of
//! the `typesinternal.TooNewStdSymbols` it calls. The symbol table is
//! [`crate::stdlib_manifest`].

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use guff::walk::{self, NodeRef};
use guff_analysis::code::{effective_file_go_version, version_compare};
use guff_analysis::{AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn};
use guff_analysis::passes::inspect;
use guff_types::arena::{ObjectArena, ObjectData, PackageId, TypeArena, TypeData};
use guff_types::lookup::lookup_field_or_method;
use guff_types::ObjectId;

use crate::stdlib_manifest::{package_symbols, Kind, Symbol};

/// `versions.Lang`: `go1.22.3` → `go1.22`. Empty for anything that is not a
/// Go version.
fn lang(v: &str) -> String {
    let Some(rest) = v.strip_prefix("go") else {
        return String::new();
    };
    let mut parts = rest.split('.');
    let (Some(major), Some(minor)) = (parts.next(), parts.next()) else {
        return String::new();
    };
    let minor: String = minor.chars().take_while(char::is_ascii_digit).collect();
    if major.is_empty() || !major.bytes().all(|b| b.is_ascii_digit()) || minor.is_empty() {
        return String::new();
    }
    format!("go{major}.{minor}")
}

/// `versions.Before(x, y)`, for the valid versions this analyzer compares.
fn before(x: &str, y: &str) -> bool {
    version_compare(x, y) < 0
}

/// `pass.Pkg.GoVersion()`: the module's `go` line as go/packages hands it to
/// the type checker (`go1.22.0` stays `go1.22.0`), or "" when unknown.
fn package_go_version(pass: &Pass<'_>) -> String {
    let from_module = pass.pkg().module.as_ref().map(|m| m.go_version.trim()).filter(|v| !v.is_empty());
    if let Some(v) = from_module {
        return if v.starts_with("go") { v.to_string() } else { format!("go{v}") };
    }
    pass.type_pkg()
        .and_then(|pkg| {
            let artifacts = pass.pkg().type_artifacts.as_ref()?;
            Some(artifacts.packages.get(pkg).go_version().to_string())
        })
        .unwrap_or_default()
}

type Disallowed = Rc<HashMap<ObjectId, Symbol>>;

struct TooNew<'a> {
    pass: &'a Pass<'a>,
    memo: HashMap<(PackageId, String), Disallowed>,
    /// `lookup_field_or_method` may intern types; it gets a private copy of
    /// the arena, made only once a field or method actually needs looking up.
    scratch: Option<TypeArena>,
}

impl TooNew<'_> {
    fn disallowed(&mut self, pkg: PackageId, version: &str) -> Disallowed {
        let key = (pkg, version.to_string());
        if let Some(d) = self.memo.get(&key) {
            return Rc::clone(d);
        }
        let d = Rc::new(self.too_new_std_symbols(pkg, version));
        self.memo.insert(key, Rc::clone(&d));
        d
    }

    /// `typesinternal.TooNewStdSymbols`.
    fn too_new_std_symbols(&mut self, pkg: PackageId, version: &str) -> HashMap<ObjectId, Symbol> {
        let mut disallowed = HashMap::new();
        let Some(a) = self.pass.pkg().type_artifacts.as_ref() else {
            return disallowed;
        };
        let path = a.packages.get(pkg).path();

        // Some symbols are accessible before their release but only with
        // specific build tags unknown to us here.
        if path == "testing/synctest" && !before(version, "go1.24") {
            // requires go1.24 && goexperiment.synctest || go1.25
            return disallowed;
        }
        if (path == "encoding/json/v2" || path == "encoding/json/jsontext") && !before(version, "go1.25") {
            // requires go1.25 && goexperiment.jsonv2 || go1.27
            return disallowed;
        }

        let Some(symbols) = package_symbols(path) else {
            return disallowed;
        };
        let scope = a.packages.get(pkg).scope();
        let lookup = |name: &str| guff_types::scope::lookup(&a.scopes, scope, name);

        // Pass 1: package-level symbols.
        for sym in symbols {
            if before(version, &sym.version()) {
                if let Kind::Func | Kind::Var | Kind::Const | Kind::Type = sym.kind {
                    if let Some(obj) = lookup(sym.name) {
                        disallowed.insert(obj, *sym);
                    }
                }
            }
        }

        // Pass 2: fields and methods, allowed when their type is itself too
        // new (a compatibility shim may declare the type differently on older
        // releases). In the presence of embedding two or more "pkg.T.name"
        // strings may map to one object; the shorter index path wins.
        let mut depth: HashMap<ObjectId, usize> = HashMap::new();
        for sym in symbols {
            if !before(version, &sym.version()) {
                continue;
            }
            let (typename, ptr, name) = match sym.kind {
                Kind::Field => {
                    let (t, n) = sym.split_field();
                    (t, false, n)
                }
                Kind::Method => {
                    let (p, t, n) = sym.split_method();
                    (t, p, n)
                }
                _ => continue,
            };
            let Some(t) = lookup(typename) else {
                continue;
            };
            if disallowed.contains_key(&t) {
                continue;
            }
            let Some(typ) = t.typ(&a.objects) else {
                continue;
            };
            let types = self.scratch.get_or_insert_with(|| a.types.clone());
            let found = lookup_field_or_method(types, &a.objects, &a.packages, typ, ptr, Some(pkg), name);
            if let Some((obj, indices, _)) = found.found() {
                let d = indices.len();
                if depth.get(&obj).is_none_or(|&min| d < min) {
                    depth.insert(obj, d);
                    disallowed.insert(obj, *sym);
                }
            }
        }
        disallowed
    }
}

/// `origin`: the original uninstantiated symbol for `obj`.
fn origin(objects: &ObjectArena, types: &TypeArena, obj: ObjectId) -> ObjectId {
    match objects.get(obj) {
        ObjectData::Var(_) => guff_types::object::var::var_origin(objects, obj),
        ObjectData::Func(_) => guff_types::object::func::func_origin(objects, obj),
        ObjectData::TypeName(tn) => match tn.typ() {
            // (don't unalias)
            Some(t) if matches!(types.get(t), TypeData::Named(_)) => {
                guff_types::named::named_obj(types, guff_types::named::named_origin(types, t))
            }
            _ => obj,
        },
        _ => obj,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    // Don't report diagnostics for modules marked before go1.21, since at
    // that time the go directive wasn't clearly specified as a toolchain
    // requirement.
    let pkg_version = package_go_version(pass);
    if lang(&pkg_version).is_empty() || before(&pkg_version, "go1.21") {
        return Ok(None);
    }
    let (Some(info), Some(a)) = (pass.types_info(), pass.pkg().type_artifacts.as_ref()) else {
        return Ok(None);
    };

    let mut too_new = TooNew {
        pass,
        memo: HashMap::new(),
        scratch: None,
    };
    let mut diags = Vec::new();
    for file in pass.files() {
        if guff::ast::is_generated(file) {
            // Suppress diagnostics in generated files (such as cgo).
            continue;
        }
        let file_version = lang(&effective_file_go_version(pass, file.file_start.0 as u32));
        if file_version.is_empty() {
            continue;
        }
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::Ident(id)) = n else {
                return true;
            };
            let Some(&obj) = info.uses.get(&id.id) else {
                return true;
            };
            let Some(pkg) = obj.pkg(&a.objects) else {
                return true;
            };
            let disallowed = too_new.disallowed(pkg, &file_version);
            if let Some(sym) = disallowed.get(&origin(&a.objects, &a.types, obj)) {
                diags.push(Diagnostic {
                    pos: id.pos().0 as u32,
                    end: id.end().0 as u32,
                    message: format!(
                        "{}.{} requires {} or later ({} is {})",
                        a.packages.get(pkg).name(),
                        sym.name,
                        sym.version(),
                        if file_version != pkg_version { "file" } else { "module" },
                        file_version
                    ),
                    ..Diagnostic::default()
                });
            }
            true
        });
    }
    drop(too_new);
    for d in diags {
        pass.report(d);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "stdversion",
        doc: "report uses of too-new standard library symbols",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/stdversion",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::lang;

    #[test]
    fn lang_drops_the_patch_and_prerelease() {
        assert_eq!(lang("go1.22.0"), "go1.22");
        assert_eq!(lang("go1.22"), "go1.22");
        assert_eq!(lang("go1.23rc1"), "go1.23");
        assert_eq!(lang(""), "");
        assert_eq!(lang("1.22"), "");
    }
}
