// Port of Go's go/ast/filter.go to Rust.
//
// Original: Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// Filters and merge logic for trimming/merging an AST in place. The
// `MergePackageFiles` half is deprecated in Go and here too (the
// `[Package]` type itself is deprecated upstream) — kept for parity.

use std::collections::HashMap;

use crate::ast::{
    Comment, CommentGroup, CompositeLit, Decl, Expr, Field, FieldList, File, FuncDecl, Ident,
    ImportSpec, Package, Spec,
};
use crate::position::Pos;
use crate::token::is_exported;

// ====================================================================
// Filter type
// ====================================================================

/// Predicate over identifier names. Use with [`filter_decl`],
/// [`filter_file`], and [`filter_package`].
pub type Filter<'a> = &'a dyn Fn(&str) -> bool;

/// Predicate that keeps only exported identifiers.
pub fn export_filter(name: &str) -> bool {
    is_exported(name)
}

// ====================================================================
// File / Package exports
// ====================================================================

/// Trim `src` in place to keep only exported nodes. Returns true iff
/// any exported declaration remains.
pub fn file_exports(src: &mut File) -> bool {
    filter_file(src, &export_filter, true)
}

/// Trim `pkg`'s files in place. The `pkg.files` list is *not* changed,
/// so file names and top-level comments survive. Returns true iff any
/// exported declaration remains in any file.
pub fn package_exports(pkg: &mut Package) -> bool {
    filter_package(pkg, &export_filter, true)
}

// ====================================================================
// General filtering
// ====================================================================

/// Trim `decl` (and its sub-tree) in place. Returns true iff any
/// declared name survives.
pub fn filter_decl(decl: &mut Decl, f: Filter<'_>) -> bool {
    filter_decl_impl(decl, f, false)
}

/// Trim `src` in place by removing declarations whose names don't pass
/// the filter. Import declarations are always removed. Returns true iff
/// any top-level declaration remains.
pub fn filter_file(src: &mut File, f: Filter<'_>, export: bool) -> bool {
    filter_file_impl(src, f, export)
}

/// Trim every file of `pkg` in place. `pkg.files` is not changed.
pub fn filter_package(pkg: &mut Package, f: Filter<'_>, export: bool) -> bool {
    let mut has_decls = false;
    for src in pkg.files.values_mut() {
        if filter_file_impl(src, f, export) {
            has_decls = true;
        }
    }
    has_decls
}

// -- private implementations ------------------------------------------

fn filter_ident_list(list: &mut Vec<Ident>, f: Filter<'_>) {
    list.retain(|x| f(&x.name));
}

fn field_name(x: &Expr) -> Option<&Ident> {
    match x {
        Expr::Ident(id) => Some(id),
        Expr::SelectorExpr(s) => match s.x.as_ref() {
            Expr::Ident(_) => Some(&s.sel),
            _ => None,
        },
        Expr::StarExpr(s) => field_name(&s.x),
        _ => None,
    }
}

fn filter_field_list(fields: &mut FieldList, f: Filter<'_>, export: bool) -> bool {
    let mut removed_fields = false;
    let mut new_list: Vec<Field> = Vec::with_capacity(fields.list.len());
    for mut field in std::mem::take(&mut fields.list) {
        let keep;
        if field.names.is_empty() {
            // Anonymous field — keep iff its inferred name passes.
            keep = match &field.ty {
                Some(ty) => field_name(ty).map(|n| f(&n.name)).unwrap_or(false),
                None => false,
            };
        } else {
            let n_before = field.names.len();
            filter_ident_list(&mut field.names, f);
            if field.names.len() < n_before {
                removed_fields = true;
            }
            keep = !field.names.is_empty();
        }
        if keep {
            if export {
                if let Some(ty) = field.ty.as_mut() {
                    filter_type(ty, f, export);
                }
            }
            new_list.push(field);
        }
    }
    if new_list.len() < fields.list.capacity() {
        // (note: list is already taken; we replace below)
    }
    let kept = new_list.len();
    let had = kept; // bookkeeping; total compared via removed_fields below
    fields.list = new_list;
    if !removed_fields && kept != had {
        removed_fields = true;
    }
    removed_fields
}

fn filter_composite_lit(lit: &mut CompositeLit, f: Filter<'_>, export: bool) {
    let n = lit.elts.len();
    let elts = std::mem::take(&mut lit.elts);
    lit.elts = filter_expr_list(elts, f, export);
    if lit.elts.len() < n {
        lit.incomplete = true;
    }
}

fn filter_expr_list(list: Vec<Expr>, f: Filter<'_>, export: bool) -> Vec<Expr> {
    let mut out: Vec<Expr> = Vec::with_capacity(list.len());
    for mut exp in list {
        let keep = match &mut exp {
            Expr::CompositeLit(c) => {
                filter_composite_lit(c, f, export);
                true
            }
            Expr::KeyValueExpr(kv) => {
                if let Expr::Ident(id) = kv.key.as_ref() {
                    if !f(&id.name) {
                        continue;
                    }
                }
                if let Expr::CompositeLit(c) = kv.value.as_mut() {
                    filter_composite_lit(c, f, export);
                }
                true
            }
            _ => true,
        };
        if keep {
            out.push(exp);
        }
    }
    out
}

fn filter_param_list(fields: &mut FieldList, f: Filter<'_>, export: bool) -> bool {
    let mut b = false;
    for field in fields.list.iter_mut() {
        if let Some(ty) = field.ty.as_mut() {
            if filter_type(ty, f, export) {
                b = true;
            }
        }
    }
    b
}

fn filter_type(typ: &mut Expr, f: Filter<'_>, export: bool) -> bool {
    match typ {
        Expr::Ident(id) => f(&id.name),
        Expr::ParenExpr(p) => filter_type(&mut p.x, f, export),
        Expr::ArrayType(a) => filter_type(&mut a.elt, f, export),
        Expr::StructType(s) => {
            if filter_field_list(&mut s.fields, f, export) {
                s.incomplete = true;
            }
            !s.fields.list.is_empty()
        }
        Expr::FuncType(ft) => {
            let b1 = ft
                .params
                .as_mut()
                .map(|p| filter_param_list(p, f, export))
                .unwrap_or(false);
            let b2 = ft
                .results
                .as_mut()
                .map(|r| filter_param_list(r, f, export))
                .unwrap_or(false);
            b1 || b2
        }
        Expr::InterfaceType(it) => {
            if filter_field_list(&mut it.methods, f, export) {
                it.incomplete = true;
            }
            !it.methods.list.is_empty()
        }
        Expr::MapType(m) => {
            let b1 = filter_type(&mut m.key, f, export);
            let b2 = filter_type(&mut m.value, f, export);
            b1 || b2
        }
        Expr::ChanType(c) => filter_type(&mut c.value, f, export),
        _ => false,
    }
}

fn filter_spec(spec: &mut Spec, f: Filter<'_>, export: bool) -> bool {
    match spec {
        Spec::ValueSpec(s) => {
            filter_ident_list(&mut s.names, f);
            let vs = std::mem::take(&mut s.values);
            s.values = filter_expr_list(vs, f, export);
            if !s.names.is_empty() {
                if export {
                    if let Some(t) = s.ty.as_mut() {
                        filter_type(t, f, export);
                    }
                }
                return true;
            }
            false
        }
        Spec::TypeSpec(s) => {
            if f(&s.name.name) {
                if export {
                    filter_type(&mut s.ty, f, export);
                }
                return true;
            }
            if !export {
                return filter_type(&mut s.ty, f, export);
            }
            false
        }
        Spec::ImportSpec(_) => false,
    }
}

fn filter_spec_list(list: &mut Vec<Spec>, f: Filter<'_>, export: bool) {
    let mut out: Vec<Spec> = Vec::with_capacity(list.len());
    for mut s in std::mem::take(list) {
        if filter_spec(&mut s, f, export) {
            out.push(s);
        }
    }
    *list = out;
}

fn filter_decl_impl(decl: &mut Decl, f: Filter<'_>, export: bool) -> bool {
    match decl {
        Decl::GenDecl(d) => {
            filter_spec_list(&mut d.specs, f, export);
            !d.specs.is_empty()
        }
        Decl::FuncDecl(d) => f(&d.name.name),
        Decl::BadDecl(_) => false,
    }
}

fn filter_file_impl(src: &mut File, f: Filter<'_>, export: bool) -> bool {
    let mut out: Vec<Decl> = Vec::with_capacity(src.decls.len());
    for mut d in std::mem::take(&mut src.decls) {
        if filter_decl_impl(&mut d, f, export) {
            out.push(d);
        }
    }
    src.decls = out;
    !src.decls.is_empty()
}

// ====================================================================
// MergePackageFiles
// ====================================================================

/// Flags controlling [`merge_package_files`] behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MergeMode(pub u32);

impl MergeMode {
    pub fn contains(self, other: MergeMode) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for MergeMode {
    type Output = MergeMode;
    fn bitor(self, rhs: Self) -> Self {
        MergeMode(self.0 | rhs.0)
    }
}

/// Exclude duplicate function declarations.
pub const FILTER_FUNC_DUPLICATES: MergeMode = MergeMode(1 << 0);
/// Exclude comments not attached to any AST node.
pub const FILTER_UNASSOCIATED_COMMENTS: MergeMode = MergeMode(1 << 1);
/// Exclude duplicate import declarations.
pub const FILTER_IMPORT_DUPLICATES: MergeMode = MergeMode(1 << 2);

/// Merge a package's files into a single synthesized [`File`].
///
/// Deprecated like Go's `ast.MergePackageFiles` — kept for parity.
pub fn merge_package_files(pkg: &Package, mode: MergeMode) -> File {
    // Sort filenames for deterministic iteration order.
    let mut filenames: Vec<&String> = pkg.files.keys().collect();
    filenames.sort();

    // Pass 1: count docs/comments/decls, compute min/max file positions.
    let mut ndocs = 0usize;
    let mut ncomments = 0usize;
    let mut ndecls = 0usize;
    let mut min_pos = Pos::default();
    let mut max_pos = Pos::default();
    for (idx, fname) in filenames.iter().enumerate() {
        let f = &pkg.files[*fname];
        if let Some(doc) = &f.doc {
            ndocs += doc.list.len() + 1; // +1 for separator
        }
        ncomments += f.comments.len();
        ndecls += f.decls.len();
        if idx == 0 || f.file_start.0 < min_pos.0 {
            min_pos = f.file_start;
        }
        if idx == 0 || f.file_end.0 > max_pos.0 {
            max_pos = f.file_end;
        }
    }

    // Collect package docs into a single CommentGroup, separating
    // groups with an empty "//" comment.
    let mut doc: Option<CommentGroup> = None;
    let mut pos = Pos::default();
    if ndocs > 0 {
        let mut list: Vec<Comment> = Vec::with_capacity(ndocs.saturating_sub(1));
        let mut first = true;
        for fname in &filenames {
            let f = &pkg.files[*fname];
            if let Some(d) = &f.doc {
                if !first {
                    list.push(separator());
                }
                for c in &d.list {
                    list.push(c.clone());
                }
                if f.package.0 > pos.0 {
                    pos = f.package;
                }
                first = false;
            }
        }
        doc = Some(CommentGroup { list });
    }

    // Collect declarations.
    let mut decls: Vec<Option<Decl>> = Vec::with_capacity(ndecls);
    let mut funcs: HashMap<String, usize> = HashMap::new();
    let mut n_filtered = 0usize;
    for fname in &filenames {
        let f = &pkg.files[*fname];
        for d in &f.decls {
            let mut entry = Some(d.clone());
            if mode.contains(FILTER_FUNC_DUPLICATES) {
                if let Some(Decl::FuncDecl(fd)) = entry.as_ref() {
                    let name = name_of(fd);
                    if let Some(&j) = funcs.get(&name) {
                        let existing_has_no_doc = match decls[j].as_ref() {
                            Some(Decl::FuncDecl(prev)) => prev.doc.is_none(),
                            _ => false,
                        };
                        if existing_has_no_doc {
                            decls[j] = None;
                        } else {
                            entry = None;
                        }
                        n_filtered += 1;
                    } else {
                        funcs.insert(name, decls.len());
                    }
                }
            }
            decls.push(entry);
        }
    }
    let decls: Vec<Decl> = if n_filtered > 0 {
        decls.into_iter().flatten().collect()
    } else {
        decls
            .into_iter()
            .map(|d| d.expect("no filtered entries"))
            .collect()
    };

    // Collect imports.
    let mut imports: Vec<ImportSpec> = Vec::new();
    if mode.contains(FILTER_IMPORT_DUPLICATES) {
        let mut seen: HashMap<String, bool> = HashMap::new();
        for fname in &filenames {
            for imp in &pkg.files[*fname].imports {
                if !seen.contains_key(&imp.path.value) {
                    seen.insert(imp.path.value.clone(), true);
                    imports.push(imp.clone());
                }
            }
        }
    } else {
        for fname in &filenames {
            imports.extend(pkg.files[*fname].imports.iter().cloned());
        }
    }

    // Collect comments.
    let mut comments: Vec<CommentGroup> = Vec::new();
    if !mode.contains(FILTER_UNASSOCIATED_COMMENTS) {
        comments.reserve(ncomments);
        for fname in &filenames {
            for cg in &pkg.files[*fname].comments {
                comments.push(cg.clone());
            }
        }
    }

    File {
        doc,
        package: pos,
        name: Ident::new_ident(&pkg.name),
        decls,
        file_start: min_pos,
        file_end: max_pos,
        scope: pkg.scope.clone(),
        imports,
        unresolved: Vec::new(),
        comments,
        go_version: String::new(),
        id: 0,
    }
}

/// Empty `//`-style separator comment used between merged doc groups.
fn separator() -> Comment {
    Comment {
        slash: Pos::default(),
        text: "//".to_string(),
    }
}

/// Name of `f`: bare function name, or `Recv.f` for a method.
fn name_of(f: &FuncDecl) -> String {
    if let Some(recv) = &f.recv {
        if recv.list.len() == 1 {
            let ty = recv.list[0].ty.as_ref();
            if let Some(mut t) = ty {
                if let Expr::StarExpr(s) = t {
                    t = &s.x;
                }
                if let Expr::Ident(id) = t {
                    return format!("{}.{}", id.name, f.name.name);
                }
            }
        }
    }
    f.name.name.clone()
}

// ====================================================================
// Tests
// ====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{BlockStmt, FuncDecl, FuncType, GenDecl, Ident};
    use crate::position::Pos;
    use crate::token::Token;

    fn func_decl(name: &str) -> FuncDecl {
        FuncDecl {
            doc: None,
            recv: None,
            name: Ident::new_ident(name),
            ty: FuncType {
                id: 0,
                func: Pos(1),
                type_params: None,
                params: Some(FieldList::default()),
                results: None,
            },
            body: Some(BlockStmt {
                lbrace: Pos::default(),
                list: vec![],
                rbrace: Pos(1),
                id: 0,
            }),
        }
    }

    fn gen_decl(specs: Vec<Spec>) -> GenDecl {
        GenDecl {
            tok: Some(Token::VAR),
            tok_pos: Pos(1),
            specs,
            ..Default::default()
        }
    }

    #[test]
    fn file_exports_keeps_only_exported_funcs() {
        let mut f = File {
            decls: vec![
                Decl::FuncDecl(func_decl("Foo")),
                Decl::FuncDecl(func_decl("bar")),
                Decl::FuncDecl(func_decl("Baz")),
            ],
            ..Default::default()
        };
        let had = file_exports(&mut f);
        assert!(had);
        let names: Vec<String> = f
            .decls
            .iter()
            .map(|d| match d {
                Decl::FuncDecl(fd) => fd.name.name.clone(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(names, vec!["Foo", "Baz"]);
    }

    #[test]
    fn file_exports_returns_false_when_nothing_remains() {
        let mut f = File {
            decls: vec![Decl::FuncDecl(func_decl("private"))],
            ..Default::default()
        };
        assert!(!file_exports(&mut f));
        assert!(f.decls.is_empty());
    }

    #[test]
    fn filter_decl_value_spec_drops_non_matching_names() {
        let mut decl = Decl::GenDecl(gen_decl(vec![Spec::ValueSpec(crate::ast::ValueSpec {
            names: vec![
                Ident::new_ident("keep1"),
                Ident::new_ident("drop"),
                Ident::new_ident("keep2"),
            ],
            ..Default::default()
        })]));
        let allow_keep: &dyn Fn(&str) -> bool = &|n: &str| n.starts_with("keep");
        let any = filter_decl(&mut decl, allow_keep);
        assert!(any);
        if let Decl::GenDecl(g) = &decl {
            if let Spec::ValueSpec(v) = &g.specs[0] {
                let names: Vec<String> = v.names.iter().map(|n| n.name.clone()).collect();
                assert_eq!(names, vec!["keep1", "keep2"]);
            }
        }
    }

    #[test]
    fn filter_decl_drops_empty_gendecl() {
        let mut decl = Decl::GenDecl(gen_decl(vec![Spec::ValueSpec(crate::ast::ValueSpec {
            names: vec![Ident::new_ident("private")],
            ..Default::default()
        })]));
        let allow_none: &dyn Fn(&str) -> bool = &|_| false;
        assert!(!filter_decl(&mut decl, allow_none));
    }

    #[test]
    fn import_decls_are_always_dropped_by_file_filter() {
        let mut f = File {
            decls: vec![Decl::GenDecl(GenDecl {
                tok: Some(Token::IMPORT),
                specs: vec![Spec::ImportSpec(crate::ast::ImportSpec::default())],
                ..Default::default()
            })],
            ..Default::default()
        };
        let allow_all: &dyn Fn(&str) -> bool = &|_| true;
        assert!(!filter_file(&mut f, allow_all, false));
        assert!(f.decls.is_empty());
    }

    #[test]
    fn merge_package_files_concatenates_decls_in_filename_order() {
        let mut pkg = Package {
            name: "p".to_string(),
            files: Default::default(),
            ..Default::default()
        };
        let mut fa = File::default();
        fa.decls.push(Decl::FuncDecl(func_decl("a")));
        let mut fb = File::default();
        fb.decls.push(Decl::FuncDecl(func_decl("b")));
        pkg.files.insert("z.go".to_string(), fb);
        pkg.files.insert("a.go".to_string(), fa);

        let merged = merge_package_files(&pkg, MergeMode::default());
        let names: Vec<String> = merged
            .decls
            .iter()
            .map(|d| match d {
                Decl::FuncDecl(fd) => fd.name.name.clone(),
                _ => unreachable!(),
            })
            .collect();
        // Sorted filename order means a.go before z.go.
        assert_eq!(names, vec!["a", "b"]);
        assert_eq!(merged.name.name, "p");
    }

    #[test]
    fn merge_package_files_filter_func_duplicates_prefers_documented() {
        let mut pkg = Package {
            name: "p".to_string(),
            files: Default::default(),
            ..Default::default()
        };
        // f.go has func "f" with a doc comment.
        let mut undoc = func_decl("f");
        let mut doc = func_decl("f");
        doc.doc = Some(CommentGroup {
            list: vec![Comment {
                slash: Pos(1),
                text: "// f docs".to_string(),
            }],
        });
        // The kept-then-overwritten rule: when a second declaration is
        // encountered, the existing wins if it has docs, else the new
        // one replaces it.
        undoc.name.name = "f".to_string();
        let mut f1 = File::default();
        f1.decls.push(Decl::FuncDecl(undoc));
        let mut f2 = File::default();
        f2.decls.push(Decl::FuncDecl(doc));
        // Sorted filename order processes "a.go" first.
        pkg.files.insert("a.go".to_string(), f1);
        pkg.files.insert("b.go".to_string(), f2);

        let merged = merge_package_files(&pkg, FILTER_FUNC_DUPLICATES);
        assert_eq!(merged.decls.len(), 1, "duplicate filtered");
        if let Decl::FuncDecl(fd) = &merged.decls[0] {
            assert!(fd.doc.is_some(), "documented declaration wins");
        }
    }

    #[test]
    fn merge_package_files_filter_import_duplicates() {
        let mut pkg = Package {
            name: "p".to_string(),
            files: Default::default(),
            ..Default::default()
        };
        let mk = |path: &str| crate::ast::ImportSpec {
            doc: None,
            name: None,
            path: crate::ast::BasicLit {
                id: 0,
                value_pos: Pos(0),
                value_end: Pos(0),
                kind: Some(Token::STRING),
                value: format!("\"{}\"", path),
            },
            comment: None,
            end_pos: Pos(0),
            id: 0,
        };
        let mut f1 = File::default();
        f1.imports = vec![mk("foo"), mk("bar")];
        let mut f2 = File::default();
        f2.imports = vec![mk("bar"), mk("baz")];
        pkg.files.insert("a.go".to_string(), f1);
        pkg.files.insert("b.go".to_string(), f2);

        let merged = merge_package_files(&pkg, FILTER_IMPORT_DUPLICATES);
        let paths: Vec<String> = merged
            .imports
            .iter()
            .map(|i| i.path.value.clone())
            .collect();
        assert_eq!(paths, vec!["\"foo\"", "\"bar\"", "\"baz\""]);
    }
}
