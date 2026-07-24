//! `inline` — suggest inlining of constants marked `//go:fix inline`.
//!
//! Port of the const-inlining subset of
//! `golang.org/x/tools/go/analysis/passes/inline` (the part golangci surfaces
//! as `Constant reflect.Ptr should be inlined`).
//!
//! Function/alias inlining is omitted. Stdlib packages are typically loaded
//! from export data (no source), so known stdlib inlinables such as
//! `reflect.Ptr` → `Pointer` are hardcoded in addition to discovering
//! `//go:fix inline` consts in packages that have syntax.
//!
//! Package load uses `Mode::NONE`, which drops lead comments after the package
//! clause, so local `//go:fix` discovery re-parses with `PARSE_COMMENTS`.

use std::collections::HashSet;
use std::fs;
use std::sync::OnceLock;

use guff::ast::{CommentGroup, Decl, Expr, GenDecl, Spec, ValueSpec};
use guff::parse_directive;
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::FileSet;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::object_pkg_path;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::{ObjectData, ObjectId};
use guff_types::scope::lookup as scope_lookup;

use crate::expreq::unparen;

/// Stdlib consts with `//go:fix inline` that we cannot discover from export data.
fn is_known_stdlib_inlinable(pkg_path: &str, name: &str) -> bool {
    matches!((pkg_path, name), ("reflect", "Ptr"))
}

fn has_fix_inline(doc: &Option<CommentGroup>) -> bool {
    let Some(doc) = doc else {
        return false;
    };
    for c in &doc.list {
        if let Some(d) = parse_directive(c.slash, &c.text) {
            if d.tool == "go" && d.name == "fix" && d.args.trim() == "inline" {
                return true;
            }
        }
    }
    false
}

fn rhs_is_named_const(val: &Expr) -> bool {
    matches!(unparen(val), Expr::Ident(_) | Expr::SelectorExpr(_))
}

fn rhs_is_iota(val: &Expr) -> bool {
    matches!(unparen(val), Expr::Ident(id) if id.name == "iota")
}

/// Collect LHS names of consts marked `//go:fix inline` in a reparsed file.
fn go_fix_const_names(file: &guff::ast::File) -> Vec<String> {
    let mut names = Vec::new();
    for decl in &file.decls {
        let Decl::GenDecl(GenDecl {
            doc,
            tok: Some(Token::CONST),
            specs,
            ..
        }) = decl
        else {
            continue;
        };
        let decl_inline = has_fix_inline(doc);
        for spec in specs {
            let Spec::ValueSpec(ValueSpec {
                doc: spec_doc,
                names: spec_names,
                values,
                ..
            }) = spec
            else {
                continue;
            };
            if !decl_inline && !has_fix_inline(spec_doc) {
                continue;
            }
            for (i, name) in spec_names.iter().enumerate() {
                if i >= values.len() {
                    break;
                }
                if rhs_is_iota(&values[i]) || !rhs_is_named_const(&values[i]) {
                    continue;
                }
                names.push(name.name.clone());
            }
        }
    }
    names
}

fn collect_inlinable_consts(pass: &Pass<'_>) -> HashSet<ObjectId> {
    let mut out = HashSet::new();
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return out;
    };
    let Some(type_pkg) = pass.type_pkg() else {
        return out;
    };
    let scope = artifacts.packages.get(type_pkg).scope();

    for (i, _) in pass.files().iter().enumerate() {
        let path = pass
            .pkg()
            .compiled_go_files
            .get(i)
            .cloned()
            .or_else(|| pass.pkg().go_files.get(i).cloned());
        let Some(path) = path else {
            continue;
        };
        let Ok(src) = fs::read(&path) else {
            continue;
        };
        // Cheap filter: almost no files carry `//go:fix inline`; avoid a full
        // PARSE_COMMENTS reparse on the common path.
        if !src.windows(b"go:fix inline".len()).any(|w| w == b"go:fix inline") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let re_fset = FileSet::new();
        let Ok(parsed) = parse_file(&re_fset, name, &src, PARSE_COMMENTS) else {
            continue;
        };
        for const_name in go_fix_const_names(&parsed) {
            let Some(obj) = scope_lookup(&artifacts.scopes, scope, &const_name) else {
                continue;
            };
            if matches!(artifacts.objects.get(obj), ObjectData::Const(_)) {
                out.insert(obj);
            }
        }
    }
    out
}

fn package_has_go_fix_inline(pass: &Pass<'_>) -> bool {
    for path in pass
        .pkg()
        .compiled_go_files
        .iter()
        .chain(pass.pkg().go_files.iter())
    {
        let Ok(src) = fs::read(path) else {
            continue;
        };
        if src
            .windows(b"go:fix inline".len())
            .any(|w| w == b"go:fix inline")
        {
            return true;
        }
    }
    false
}

fn is_inlinable_const(
    pass: &Pass<'_>,
    obj: ObjectId,
    local: &mut Option<HashSet<ObjectId>>,
) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    if !matches!(artifacts.objects.get(obj), ObjectData::Const(_)) {
        return false;
    }
    let name = obj.name(&artifacts.objects);
    if let Some(pkg_path) = object_pkg_path(pass, obj) {
        if is_known_stdlib_inlinable(&pkg_path, name) {
            return true;
        }
    }
    // Local `//go:fix inline` — same PackageId as the package under analysis
    // (path strings are often empty for the current package).
    let Some(obj_pkg) = obj.pkg(&artifacts.objects) else {
        return false; // universe / unpackaged
    };
    let Some(type_pkg) = pass.type_pkg() else {
        return false;
    };
    if obj_pkg != type_pkg {
        return false;
    }
    let set = local.get_or_insert_with(|| {
        if !package_has_go_fix_inline(pass) {
            return HashSet::new();
        }
        collect_inlinable_consts(pass)
    });
    set.contains(&obj)
}

fn format_expr_name(expr: &Expr) -> String {
    match unparen(expr) {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", format_expr_name(&sel.x), sel.sel.name),
        _ => "?".into(),
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "inline requires inspect analyzer".to_string())?
        .clone();

    // Idents that appear as SelectorExpr.Sel are reported via the selector.
    let mut selector_sels = HashSet::new();
    inspect.preorder(pass.files(), |n| {
        if let NodeRef::SelectorExpr(sel) = n {
            selector_sels.insert(sel.sel.id);
        }
    });

    // Local `//go:fix inline` discovery re-reads sources; defer until a
    // non-stdlib candidate appears (prometheus typically only hits reflect.Ptr).
    let mut local: Option<HashSet<ObjectId>> = None;
    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |n| {
        match n {
            NodeRef::SelectorExpr(sel) => {
                let info = match pass.types_info() {
                    Some(i) => i,
                    None => return,
                };
                let Some(obj) = info.uses.get(&sel.sel.id).copied() else {
                    return;
                };
                if !is_inlinable_const(pass, obj, &mut local) {
                    return;
                }
                let name = format_expr_name(&Expr::SelectorExpr(sel.clone()));
                pending.push((
                    sel.x.pos().0 as u32,
                    format!("Constant {name} should be inlined"),
                ));
            }
            NodeRef::Ident(id) => {
                if selector_sels.contains(&id.id) {
                    return;
                }
                let info = match pass.types_info() {
                    Some(i) => i,
                    None => return,
                };
                // Uses only — definitions of inlinable consts are not reported.
                let Some(obj) = info.uses.get(&id.id).copied() else {
                    return;
                };
                if !is_inlinable_const(pass, obj, &mut local) {
                    return;
                }
                pending.push((
                    id.pos().0 as u32,
                    format!("Constant {} should be inlined", id.name),
                ));
            }
            _ => {}
        }
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "inline",
        doc: "apply fixes based on go:fix inline directives (constants)",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/inline",
        run: run as RunFn,
        // Match x/tools and golangci: still report when the package has
        // soft type errors (e.g. prometheus discovery under hybrid check).
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

