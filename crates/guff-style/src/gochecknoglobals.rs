//! Port of [`github.com/leighmcculloch/gochecknoglobals`](https://github.com/leighmcculloch/gochecknoglobals).
//!
//! Reports package-level `var` declarations, with a few upstream exceptions:
//! `_`, `version`, `err*`/`Err*` that implement `error`, `regexp.MustCompile`,
//! and `//go:embed` variables.
//!
//! Re-parses with `PARSE_COMMENTS` because load uses `Mode::NONE` (declaration
//! docs after the package clause are dropped otherwise). Embed association
//! approximates `ast.CommentMap` by scanning comments between the previous
//! sibling and the current node.

use std::collections::HashSet;
use std::fs;
use std::sync::{Arc, OnceLock};

use guff::ast::{
    CommentGroup, Decl, Expr, File, GenDecl, Ident, Spec, ValueSpec,
};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::{FileSet, Pos};
use guff::token::Token;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::api_predicates::api_implements;
use guff_types::arena::ObjectData;
use guff_types::TypeId;

fn looks_like_error(name: &str) -> bool {
    let exported = name.chars().next().is_some_and(|c| c.is_uppercase());
    if exported {
        name.starts_with("Err")
    } else {
        name.starts_with("err")
    }
}

fn universe_error_type(pass: &Pass<'_>) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    for oid in artifacts.objects.ids() {
        let ObjectData::TypeName(tn) = artifacts.objects.get(oid) else {
            continue;
        };
        if tn.name() != "error" {
            continue;
        }
        if oid.pkg(&artifacts.objects).is_some() {
            continue;
        }
        return tn.typ();
    }
    None
}

fn ident_type(pass: &Pass<'_>, ident: &Ident) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let info = pass.types_info()?;
    if let Some(Some(obj)) = info.defs.get(&ident.id) {
        return obj.typ(&artifacts.objects);
    }
    if let Some(obj) = info.uses.get(&ident.id) {
        return obj.typ(&artifacts.objects);
    }
    None
}

/// Upstream `types.Implements(TypeOf(ident), error)` — no `*T` fallback.
fn implements_error(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(err) = universe_error_type(pass) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        err,
    )
}

fn comment_group_has_embed(cg: &CommentGroup) -> bool {
    cg.list.iter().any(|c| c.text.starts_with("//go:embed "))
}

fn has_embed_doc(doc: &Option<CommentGroup>) -> bool {
    doc.as_ref().is_some_and(comment_group_has_embed)
}

fn comments_have_embed_between(file: &File, from: Pos, to: Pos) -> bool {
    for cg in &file.comments {
        for c in &cg.list {
            if c.pos().0 >= from.0
                && c.end().0 <= to.0
                && c.text.starts_with("//go:embed ")
            {
                return true;
            }
        }
    }
    false
}

fn is_allowed_selector(expr: &Expr) -> bool {
    let Expr::SelectorExpr(sel) = expr else {
        return false;
    };
    let Expr::Ident(x) = sel.x.as_ref() else {
        return false;
    };
    x.name == "regexp" && sel.sel.name == "MustCompile"
}

fn is_allowed_value(expr: &Expr) -> bool {
    match expr {
        Expr::CallExpr(call) => is_allowed_selector(&call.fun),
        Expr::CompositeLit(lit) => lit
            .ty
            .as_ref()
            .is_some_and(|ty| is_allowed_selector(ty.as_ref())),
        _ => false,
    }
}

fn is_allowed_ident(pass: &Pass<'_>, ident: &Ident, embed_names: &HashSet<String>) -> bool {
    if ident.name == "_" || ident.name == "version" {
        return true;
    }
    if embed_names.contains(&ident.name) {
        return true;
    }
    if looks_like_error(&ident.name) {
        if let Some(typ) = ident_type(pass, ident) {
            return implements_error(pass, typ);
        }
    }
    false
}

fn collect_embed_names(file: &File) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut prev_end = file.package;
    for decl in &file.decls {
        let Decl::GenDecl(g) = decl else {
            prev_end = decl.end();
            continue;
        };
        if g.tok != Some(Token::VAR) {
            prev_end = decl.end();
            continue;
        }
        let gen_has_embed =
            has_embed_doc(&g.doc) || comments_have_embed_between(file, prev_end, g.tok_pos);
        let mut spec_prev = if g.lparen.0 != 0 {
            g.tok_pos
        } else {
            prev_end
        };
        for spec in &g.specs {
            let Spec::ValueSpec(vs) = spec else {
                continue;
            };
            let spec_pos = vs
                .names
                .first()
                .map(|n| n.pos())
                .unwrap_or(spec_prev);
            let vs_has_embed = gen_has_embed
                || has_embed_doc(&vs.doc)
                || comments_have_embed_between(file, spec_prev, spec_pos);
            if vs_has_embed {
                for n in &vs.names {
                    names.insert(n.name.clone());
                }
            }
            spec_prev = vs.comment.as_ref().map(|c| c.end()).unwrap_or_else(|| {
                vs.values
                    .last()
                    .map(|v| v.end())
                    .or_else(|| vs.ty.as_ref().map(|t| t.end()))
                    .or_else(|| vs.names.last().map(|n| n.end()))
                    .unwrap_or(spec_pos)
            });
        }
        prev_end = decl.end();
    }
    names
}

fn reparse(path: &std::path::Path) -> Option<(Arc<FileSet>, File)> {
    let src = fs::read(path).ok()?;
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, &src, PARSE_COMMENTS).ok()?;
    Some((fset, file))
}

fn check_value_spec(
    pass: &Pass<'_>,
    vs: &ValueSpec,
    embed_names: &HashSet<String>,
    pending: &mut Vec<(u32, String)>,
) {
    if !vs.values.is_empty() && vs.values.iter().all(is_allowed_value) {
        return;
    }
    for name in &vs.names {
        if is_allowed_ident(pass, name, embed_names) {
            continue;
        }
        pending.push((
            name.name_pos.0 as u32,
            format!("{} is a global variable", name.name),
        ));
    }
}

fn check_gen_decl(
    pass: &Pass<'_>,
    g: &GenDecl,
    embed_names: &HashSet<String>,
    pending: &mut Vec<(u32, String)>,
) {
    if g.tok != Some(Token::VAR) {
        return;
    }
    for spec in &g.specs {
        let Spec::ValueSpec(vs) = spec else {
            continue;
        };
        check_value_spec(pass, vs, embed_names, pending);
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "gochecknoglobals requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    let paths = pass.pkg().compiled_go_files.clone();
    let n = pass.files().len();

    for i in 0..n {
        let orig = &pass.files()[i];
        let embed_names = if let Some(path) = paths.get(i) {
            reparse(path)
                .map(|(_fset, parsed)| collect_embed_names(&parsed))
                .unwrap_or_default()
        } else {
            // Fallback: use whatever docs survived on the loaded AST.
            collect_embed_names(orig)
        };
        for decl in &orig.decls {
            if let Decl::GenDecl(g) = decl {
                check_gen_decl(pass, g, &embed_names, &mut pending);
            }
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "gochecknoglobals",
        doc: "checks that no global variables exist",
        url: "https://github.com/leighmcculloch/gochecknoglobals",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
