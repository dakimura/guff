//! Shared helpers for revive rules.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use guff::ast::{
    BasicLit, BinaryExpr, CallExpr, Expr, File, Ident, IndexExpr, SelectorExpr, StarExpr, UnaryExpr,
};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::FileSet;
use guff::scanner::{Scanner, SCAN_COMMENTS};
use guff::token::Token;
use guff_analysis::Pass;
use guff_types::arena::TypeData;
use guff_types::basic::{BasicKind, IS_INTEGER};
use guff_types::TypeId;

pub fn unparen<'a>(expr: &'a Expr) -> &'a Expr {
    match expr {
        Expr::ParenExpr(p) => unparen(&p.x),
        other => other,
    }
}

pub fn is_blank(ident: &Ident) -> bool {
    ident.name == "_"
}

pub fn is_pkg_dot_name(fun: &Expr, pkg: &str, name: &str) -> bool {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(fun) else {
        return false;
    };
    matches!(unparen(x), Expr::Ident(Ident { name: pkg_name, .. }) if pkg_name == pkg)
        && sel.name == name
}

pub fn basic_lit_string(lit: &BasicLit) -> Option<&str> {
    if lit.kind != Some(Token::STRING) {
        return None;
    }
    let raw = lit.value.as_str();
    if raw.len() < 2 {
        return None;
    }
    Some(&raw[1..raw.len() - 1])
}

pub fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

pub fn is_duration_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let s = guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    );
    s == "time.Duration" || s == "*time.Duration"
}

pub fn receiver_type_key(recv_ty: &Expr) -> String {
    match unparen(recv_ty) {
        Expr::Ident(id) => id.name.clone(),
        Expr::StarExpr(star) => format!("*{}", receiver_type_key(&star.x)),
        Expr::SelectorExpr(sel) => {
            let pkg = match unparen(&sel.x) {
                Expr::Ident(id) => id.name.clone(),
                other => format!("{other:?}"),
            };
            format!("{pkg}.{}", sel.sel.name)
        }
        other => format!("{other:?}"),
    }
}

pub fn is_ident(expr: &Expr, name: &str) -> bool {
    matches!(unparen(expr), Expr::Ident(Ident { name: n, .. }) if n == name)
}

pub fn is_blank_ident(expr: &Expr) -> bool {
    matches!(unparen(expr), Expr::Ident(id) if is_blank(id))
}

pub fn is_pkg_dot_type(expr: &Expr, pkg: &str, name: &str) -> bool {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(expr) else {
        return false;
    };
    matches!(unparen(x), Expr::Ident(Ident { name: pkg_name, .. }) if pkg_name == pkg)
        && sel.name == name
}

pub fn is_test_package(pkg_name: &str) -> bool {
    pkg_name.ends_with("_test")
}

pub fn is_importable_package(pkg_name: &str) -> bool {
    pkg_name != "main" && !is_test_package(pkg_name)
}

pub fn first_comment_line(doc: Option<&guff::ast::CommentGroup>) -> String {
    let Some(doc) = doc else {
        return String::new();
    };
    for line in doc.text().lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("Deprecated: ") {
            break;
        }
        return line.to_string();
    }
    String::new()
}

pub fn has_prefix_insensitive(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len()
        && s.chars()
            .zip(prefix.chars())
            .all(|(a, b)| a.eq_ignore_ascii_case(&b))
}

/// Render `typ` the way a revive message does.
///
/// Upstream formats types with `%s`, i.e. `types.Type.String()`. Its packages
/// were type-checked by revive itself with the package *name* as the import
/// path (`lint.Package.TypeCheck` hands `config.Check` the name off the first
/// file), so the qualifier a user sees is `revivetest.unexported`, never the
/// module-relative `example.com/…/revivetest.unexported` that guff's real
/// import paths would otherwise produce.
pub fn type_string(pass: &Pass<'_>, typ: TypeId) -> String {
    pass.pkg()
        .type_artifacts
        .as_ref()
        .map(|a| {
            let qf = |pkg: guff_types::PackageId,
                      parena: &guff_types::arena::PackageArena|
             -> String { parena.get(pkg).name().to_string() };
            guff_types::typestring::type_string(
                &a.types,
                &a.objects,
                &a.packages,
                typ,
                Some(&qf),
            )
        })
        .unwrap_or_else(|| "<type>".into())
}

pub fn is_error_ident_type(expr: &Expr) -> bool {
    is_ident(expr, "error")
}

pub fn is_interface_type_expr(expr: &Expr) -> bool {
    matches!(unparen(expr), Expr::InterfaceType(_))
}

pub fn expr_equal(a: &Expr, b: &Expr) -> bool {
    match (unparen(a), unparen(b)) {
        (Expr::Ident(Ident { name: na, .. }), Expr::Ident(Ident { name: nb, .. })) => na == nb,
        (
            Expr::BasicLit(BasicLit { value: va, kind: ka, .. }),
            Expr::BasicLit(BasicLit { value: vb, kind: kb, .. }),
        ) => va == vb && ka == kb,
        (
            Expr::SelectorExpr(SelectorExpr { x: xa, sel: sa, .. }),
            Expr::SelectorExpr(SelectorExpr { x: xb, sel: sb, .. }),
        ) => sa.name == sb.name && expr_equal(xa, xb),
        (Expr::StarExpr(StarExpr { x: xa, .. }), Expr::StarExpr(StarExpr { x: xb, .. })) => {
            expr_equal(xa, xb)
        }
        (
            Expr::UnaryExpr(UnaryExpr { op: oa, x: xa, .. }),
            Expr::UnaryExpr(UnaryExpr { op: ob, x: xb, .. }),
        ) => oa == ob && expr_equal(xa, xb),
        (
            Expr::BinaryExpr(BinaryExpr { op: oa, x: xa, y: ya, .. }),
            Expr::BinaryExpr(BinaryExpr { op: ob, x: xb, y: yb, .. }),
        ) => oa == ob && expr_equal(xa, xb) && expr_equal(ya, yb),
        (
            Expr::CallExpr(CallExpr { fun: fa, args: aa, .. }),
            Expr::CallExpr(CallExpr { fun: fb, args: ab, .. }),
        ) => aa.len() == ab.len() && expr_equal(fa, fb) && aa.iter().zip(ab).all(|(x, y)| expr_equal(x, y)),
        (Expr::IndexExpr(IndexExpr { x: xa, index: ia, .. }), Expr::IndexExpr(IndexExpr { x: xb, index: ib, .. })) => {
            expr_equal(xa, xb) && expr_equal(ia, ib)
        }
        _ => false,
    }
}

pub fn is_named_type(pass: &Pass<'_>, typ: TypeId, pkg: &str, name: &str) -> bool {
    type_string(pass, typ) == format!("{pkg}.{name}")
}

pub fn is_string_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    matches!(
        artifacts.types.get(typ.underlying(&artifacts.types)),
        TypeData::Basic(b) if b.kind() == BasicKind::String
    )
}

pub fn is_integer_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let TypeData::Basic(b) = artifacts.types.get(typ.underlying(&artifacts.types)) else {
        return false;
    };
    if !b.info().contains(IS_INTEGER) {
        return false;
    }
    !matches!(
        b.kind(),
        BasicKind::Uint8 | BasicKind::Int32 | BasicKind::UntypedRune
    )
}

pub fn basic_lit_string_value(lit: &BasicLit) -> Option<&str> {
    if lit.kind != Some(Token::STRING) {
        return None;
    }
    let raw = lit.value.as_str();
    if raw.len() < 2 {
        return None;
    }
    Some(&raw[1..raw.len() - 1])
}

pub fn imports_package(pass: &Pass<'_>, import_path: &str) -> bool {
    if pass.pkg().imports.contains_key(import_path) {
        return true;
    }
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::GenDecl(g) = decl else {
                continue;
            };
            if g.tok != Some(Token::IMPORT) {
                continue;
            }
            for spec in &g.specs {
                let guff::ast::Spec::ImportSpec(is) = spec else {
                    continue;
                };
                if is.path.value.trim_matches('"') == import_path {
                    return true;
                }
            }
        }
    }
    false
}

pub fn is_exported_ident(name: &str) -> bool {
    name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
}

pub fn is_bool_type_expr(expr: &Expr) -> bool {
    matches!(unparen(expr), Expr::Ident(Ident { name, .. }) if name == "bool")
}

/// A `PARSE_COMMENTS` reparse of one file, in its own private [`FileSet`].
///
/// Positions belong to `fset`, not to `pass.fset()`. Line numbers agree because
/// the bytes are the same, but a `Pos` does not: to report at one of these, map
/// it with [`map_reparsed_pos`].
pub struct Reparsed {
    pub fset: Arc<FileSet>,
    pub file: File,
}

thread_local! {
    /// Reparses for the package currently being linted, keyed by path.
    static REPARSE_CACHE: RefCell<HashMap<PathBuf, Option<Arc<Reparsed>>>> =
        RefCell::new(HashMap::new());
}

/// Re-parse `path` with `PARSE_COMMENTS`, reusing the result within a package.
///
/// The analysis load runs with `Mode::NONE`, so the shared AST keeps only the
/// comments the parser attaches as docs — a comment inside a function body is
/// simply not there. Six rules need a reparse to see them (package-comments,
/// blank-imports, exported, comments-density, empty-lines, comment-spacings)
/// and each one that forgot silently under-reported.
///
/// They used to hold a private copy of this function each, so enabling *n* of
/// them parsed every file *n* times. Measured on prometheus `./...`, the
/// reparse for comment-spacings alone cost ~0.06s of a ~1.8s run; the cache
/// pays that once no matter how many rules ask.
pub fn reparse_with_comments(path: &Path, cached: Option<&[u8]>) -> Option<Arc<Reparsed>> {
    if let Some(hit) = REPARSE_CACHE.with(|c| c.borrow().get(path).cloned()) {
        return hit;
    }
    let parsed = parse_with_comments(path, cached).map(Arc::new);
    REPARSE_CACHE.with(|c| {
        c.borrow_mut().insert(path.to_path_buf(), parsed.clone());
    });
    parsed
}

fn parse_with_comments(path: &Path, cached: Option<&[u8]>) -> Option<Reparsed> {
    let owned;
    let src: &[u8] = if let Some(b) = cached {
        b
    } else {
        owned = fs::read(path).ok()?;
        &owned
    };
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, src, PARSE_COMMENTS).ok()?;
    Some(Reparsed { fset, file })
}

/// Drop the reparse cache. Called around each revive run so the ASTs of a
/// finished package do not outlive it.
pub fn clear_reparse_cache() {
    REPARSE_CACHE.with(|c| c.borrow_mut().clear());
}

/// One comment, with its position already in `pass.fset()`.
pub struct ScannedComment {
    pub pos: u32,
    pub text: String,
}

/// Every comment in file `index`, obtained by scanning rather than parsing.
///
/// A rule that only reads comment *text* does not need an AST, and building one
/// is most of the cost: on prometheus `./...` the full `PARSE_COMMENTS` reparse
/// this replaces cost ~0.06s of a ~1.9s run. The scanner walks the bytes once
/// and allocates only the comments themselves.
///
/// Comments arrive in source order, matching the order upstream sees when it
/// walks `file.AST.Comments`.
pub fn scan_comments(pass: &Pass<'_>, index: usize) -> Option<Vec<ScannedComment>> {
    let pkg = pass.pkg();
    let path = pkg.compiled_go_files.get(index)?;
    let owned;
    let src: &[u8] = match pkg.source_bytes(index) {
        Some(b) => b,
        None => {
            owned = fs::read(path).ok()?;
            &owned
        }
    };

    // Scan against a private File so the shared FileSet's line table is not
    // touched, then convert each offset into the pass's position space.
    let scratch = FileSet::new();
    let sfile = scratch.add_file(
        path.file_name()?.to_str()?,
        scratch.base(),
        src.len() as i64,
    );
    let target = pass.fset().file(pass.files().get(index)?.pos())?;

    let mut s: Scanner<'_> = Scanner::new();
    s.init(Arc::clone(&sfile), src, None, SCAN_COMMENTS);
    let mut out = Vec::new();
    loop {
        let (pos, tok, lit) = s.scan();
        match tok {
            Token::EOF => break,
            Token::COMMENT => {
                let offset = sfile.offset(pos);
                if offset < 0 || offset > target.size() {
                    continue;
                }
                out.push(ScannedComment {
                    pos: target.pos(offset).0 as u32,
                    text: lit.into_owned(),
                });
            }
            _ => {}
        }
    }
    Some(out)
}

/// Translate a position from a [`reparse_with_comments`] `FileSet` into the
/// pass's, so a comment found only in the reparse can still be reported.
///
/// Both parses cover the same bytes, so the byte offset is the bridge; the
/// `Pos` values themselves belong to different `FileSet`s and are not
/// comparable. `file` is the pass's AST for the same file.
pub fn map_reparsed_pos(
    pass: &Pass<'_>,
    file: &File,
    reparsed_fset: &FileSet,
    pos: i64,
) -> Option<u32> {
    let from = reparsed_fset.file(guff::position::Pos(pos))?;
    let to = pass.fset().file(file.pos())?;
    let offset = from.offset(guff::position::Pos(pos));
    if offset < 0 || offset > to.size() {
        return None;
    }
    Some(to.pos(offset).0 as u32)
}

pub fn line_of(pass: &Pass<'_>, pos: i64) -> usize {
    pass.fset()
        .position(guff::position::Pos(pos))
        .line
        .max(0) as usize
}

/// Returns true when the configured Go version is at least `major.minor`, or
/// unknown.
///
/// golangci-lint hands revive a version rather than letting it read the module:
/// the loader copies `run.go` into `Settings.Revive.Go`, and revive's
/// `IsAtLeastGoVersion` answers from that. So a config can move
/// `range-val-in-closure` and the other version-gated rules without touching
/// the go directive the toolchain compiles against. Only when `run.go` is unset
/// does the module's own version decide — which is also what the loader does
/// (it detects the version and then assigns it just the same).
pub fn go_version_at_least(pass: &Pass<'_>, major: u32, minor: u32) -> bool {
    let configured = crate::config::configured_go_version(pass);
    let module = pass
        .pkg()
        .module
        .as_ref()
        .map(|m| m.go_version.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let Some(version) = configured.filter(|s| !s.trim().is_empty()).or(module) else {
        return true;
    };
    let (maj, min) = parse_go_version(&version);
    maj > major || (maj == major && min >= minor)
}

fn parse_go_version(version: &str) -> (u32, u32) {
    let stripped = version.strip_prefix("go").unwrap_or(version);
    let mut parts = stripped.split('.');
    let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    (major, minor)
}

pub fn is_ident_dot_name(fun: &Expr, recv: &str, name: &str) -> bool {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(fun) else {
        return false;
    };
    matches!(unparen(x), Expr::Ident(Ident { name: n, .. }) if n == recv) && sel.name == name
}

pub fn expr_string(e: &Expr) -> String {
    match e {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", expr_string(&sel.x), sel.sel.name),
        Expr::StarExpr(s) => format!("*{}", expr_string(&s.x)),
        Expr::ArrayType(a) => {
            let len = a
                .len
                .as_ref()
                .map(|e| expr_string(e))
                .unwrap_or_default();
            format!("[{len}]{}", expr_string(&a.elt))
        }
        Expr::InterfaceType(_) => "interface{}".into(),
        _ => "<type>".into(),
    }
}

/// Start of an import spec: the local name when the import has one (`.`, `_`,
/// or an alias), the path otherwise.
///
/// This is `ast.ImportSpec.Pos()`, the node every upstream import rule attaches
/// its failure to. Reporting the path instead puts the caret two columns right
/// of where golangci-lint points for `import _ "os"`.
pub fn import_spec_pos(imp: &guff::ast::ImportSpec) -> u32 {
    imp.name
        .as_ref()
        .map(|n| n.name_pos.0)
        .unwrap_or(imp.path.pos().0) as u32
}
