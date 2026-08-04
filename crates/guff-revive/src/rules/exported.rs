//! `exported` — naming and commenting conventions on exported symbols.
//!
//! Load uses `Mode::NONE` (no `PARSE_COMMENTS`), so declaration docs after the
//! package clause are dropped on the type-checked AST. Re-parse with comments
//! (same pattern as `blank-imports` / ST1020) and remap positions onto the
//! package `FileSet`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use guff::ast::{Decl, Expr, FieldList, File, FuncDecl, FuncType, GenDecl, Spec, TypeSpec, ValueSpec};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::FileSet;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{
    first_comment_line, has_prefix_insensitive, is_importable_package, receiver_type_key,
};

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
    gen_decl_missing: HashMap<usize, bool>,
    skip_file: bool,
    skip_stutter: bool,
    /// When false (upstream default), skip exported methods on unexported receivers.
    check_private_receivers: bool,
    /// Receiver type keys that implement `sort.Interface` (Len+Less+Swap).
    sortable: HashSet<String>,
    /// `PARSE_COMMENTS` reparse for the current file (docs + private FileSet).
    comments: Option<(Arc<FileSet>, File)>,
}

impl<'a> Checker<'a> {
    pub fn try_new(pass: &'a Pass<'a>) -> Option<Self> {
        if !is_importable_package(&pass.pkg().name) {
            return None;
        }
        let skip_stutter =
            crate::config::rule_has_string_option(pass, "exported", "disableStutteringCheck");
        // Upstream default: PrivateReceivers=true (skip methods on unexported
        // receivers). `checkPrivateReceivers` disables that skip.
        let check_private_receivers =
            crate::config::rule_has_string_option(pass, "exported", "checkPrivateReceivers");
        Some(Self {
            pass,
            failures: Vec::new(),
            gen_decl_missing: HashMap::new(),
            skip_file: false,
            skip_stutter,
            check_private_receivers,
            sortable: scan_sortable(pass.files()),
            comments: None,
        })
    }

    pub fn on_file(&mut self, file: &File) {
        let fi = self
            .pass
            .files()
            .iter()
            .position(|f| std::ptr::eq(f, file))
            .unwrap_or(0);
        self.skip_file = self
            .pass
            .pkg()
            .compiled_go_files
            .get(fi)
            .is_some_and(|p| p.to_string_lossy().ends_with("_test.go"));
        self.comments = None;
        if self.skip_file {
            return;
        }
        if let Some(path) = self.pass.pkg().compiled_go_files.get(fi) {
            self.comments = reparse_with_comments(path, self.pass.pkg().source_bytes(fi));
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        if self.skip_file {
            return;
        }
        // Package-level decls only (mirrors the previous `file.decls` walk).
        let NodeRef::File(report) = n else {
            return;
        };
        let pkg = self
            .pass
            .files()
            .first()
            .map(|f| f.name.name.as_str())
            .unwrap_or("");

        let mut batch = Vec::new();
        if let Some((ref comments_fset, ref comments_file)) = self.comments {
            check_file(
                comments_file,
                pkg,
                self.skip_stutter,
                self.check_private_receivers,
                &self.sortable,
                &mut self.gen_decl_missing,
                &mut batch,
            );
            for mut f in batch {
                f.pos = remap_pos(self.pass, report, comments_fset, f.pos);
                self.failures.push(f);
            }
        } else {
            check_file(
                report,
                pkg,
                self.skip_stutter,
                self.check_private_receivers,
                &self.sortable,
                &mut self.gen_decl_missing,
                &mut self.failures,
            );
        }
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let Some(mut c) = Checker::try_new(pass) else {
        return Vec::new();
    };
    for file in pass.files() {
        c.on_file(file);
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}

fn reparse_with_comments(path: &Path, cached: Option<&[u8]>) -> Option<(Arc<FileSet>, File)> {
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
    Some((fset, file))
}

fn remap_pos(pass: &Pass<'_>, report: &File, comments_fset: &FileSet, pos: u32) -> u32 {
    let p = comments_fset.position(guff::Pos(pos as i64));
    let Some(ft) = pass.fset().file(report.pos()) else {
        return pos;
    };
    if p.line <= 0 || p.line as usize > ft.line_count() {
        return pos;
    }
    let start = ft.line_start(p.line as usize).0 as u32;
    let col = p.column.max(1) as u32;
    start.saturating_add(col.saturating_sub(1))
}

/// Methods that commonly implement std interfaces — upstream skips them.
const COMMON_METHODS: &[&str] = &["Error", "Read", "ServeHTTP", "String", "Write", "Unwrap"];

const BF_LEN: u8 = 1 << 0;
const BF_LESS: u8 = 1 << 1;
const BF_SWAP: u8 = 1 << 2;
const BF_SORTABLE: u8 = BF_LEN | BF_LESS | BF_SWAP;

fn scan_sortable(files: &[File]) -> HashSet<String> {
    let mut flags: HashMap<String, u8> = HashMap::new();
    for file in files {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            if f.recv.as_ref().is_none_or(|r| r.list.is_empty()) {
                continue;
            }
            let recv = f
                .recv
                .as_ref()
                .and_then(|r| r.list.first())
                .and_then(|fld| fld.ty.as_ref())
                .map(receiver_type_key)
                .unwrap_or_default();
            if let Some(bit) = sortable_method_flag(f) {
                *flags.entry(recv).or_default() |= bit;
            }
        }
    }
    flags
        .into_iter()
        .filter_map(|(recv, ms)| (ms == BF_SORTABLE).then_some(recv))
        .collect()
}

fn sortable_method_flag(f: &FuncDecl) -> Option<u8> {
    match f.name.name.as_str() {
        "Len" if func_signature_is(f, &[], &["int"]) => Some(BF_LEN),
        "Less" if func_signature_is(f, &["int", "int"], &["bool"]) => Some(BF_LESS),
        "Swap" if func_signature_is(f, &["int", "int"], &[]) => Some(BF_SWAP),
        _ => None,
    }
}

fn func_signature_is(f: &FuncDecl, want_params: &[&str], want_results: &[&str]) -> bool {
    let FuncType {
        params, results, ..
    } = &f.ty;
    let got_params = field_type_names(params.as_ref());
    let got_results = field_type_names(results.as_ref());
    got_params.len() == want_params.len()
        && got_params.iter().zip(want_params).all(|(a, b)| a == b)
        && got_results.len() == want_results.len()
        && got_results.iter().zip(want_results).all(|(a, b)| a == b)
}

fn field_type_names(fields: Option<&FieldList>) -> Vec<String> {
    let Some(fields) = fields else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for field in &fields.list {
        let Some(ty) = field.ty.as_ref() else {
            continue;
        };
        let name = ast_type_name(ty).to_string();
        let n = if field.names.is_empty() {
            1
        } else {
            field.names.len()
        };
        for _ in 0..n {
            out.push(name.clone());
        }
    }
    out
}

fn ast_type_name(ty: &Expr) -> &str {
    match ty {
        Expr::Ident(id) => id.name.as_str(),
        Expr::StarExpr(s) => match s.x.as_ref() {
            Expr::Ident(id) => id.name.as_str(),
            _ => "",
        },
        _ => "",
    }
}

fn check_file(
    file: &File,
    pkg: &str,
    skip_stutter: bool,
    check_private_receivers: bool,
    sortable: &HashSet<String>,
    gen_decl_missing: &mut HashMap<usize, bool>,
    failures: &mut Vec<Failure>,
) {
    let mut last_gen: Option<&GenDecl> = None;
    for decl in &file.decls {
        match decl {
            Decl::GenDecl(g) => {
                if matches!(g.tok, Some(Token::CONST | Token::TYPE | Token::VAR)) {
                    last_gen = Some(g);
                }
                for spec in &g.specs {
                    match spec {
                        Spec::TypeSpec(ts) => {
                            lint_type_doc(ts, g, failures);
                            if !skip_stutter {
                                check_repetitive(
                                    pkg,
                                    &ts.name.name,
                                    "type",
                                    ts.name.name_pos.0,
                                    failures,
                                );
                            }
                        }
                        Spec::ValueSpec(vs) => {
                            if let Some(gd) = last_gen {
                                lint_value_spec(vs, gd, gen_decl_missing, failures);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Decl::FuncDecl(f) => {
                lint_func_doc(f, check_private_receivers, sortable, failures);
                if f.recv.is_none() && !skip_stutter {
                    check_repetitive(pkg, &f.name.name, "func", f.name.name_pos.0, failures);
                }
            }
            _ => {}
        }
    }
}

fn must_check_method(
    f: &FuncDecl,
    check_private_receivers: bool,
    sortable: &HashSet<String>,
) -> bool {
    let recv = f
        .recv
        .as_ref()
        .and_then(|r| r.list.first())
        .and_then(|fld| fld.ty.as_ref())
        .map(receiver_type_key)
        .unwrap_or_default();
    // Strip pointer star for export check (upstream `typeparams.ReceiverType`).
    let recv_name = recv.trim_start_matches('*');
    if !guff::ast::ast_is_exported(recv_name) && !check_private_receivers {
        return false;
    }
    if COMMON_METHODS.contains(&f.name.name.as_str()) {
        return false;
    }
    if matches!(f.name.name.as_str(), "Len" | "Less" | "Swap") && sortable.contains(&recv) {
        return false;
    }
    true
}

fn lint_func_doc(
    f: &FuncDecl,
    check_private_receivers: bool,
    sortable: &HashSet<String>,
    failures: &mut Vec<Failure>,
) {
    if !f.name.is_exported() {
        return;
    }
    let (kind, name) = if f.recv.is_some() {
        if !must_check_method(f, check_private_receivers, sortable) {
            return;
        }
        let recv = f
            .recv
            .as_ref()
            .and_then(|r| r.list.first())
            .and_then(|fld| fld.ty.as_ref())
            .map(receiver_type_key)
            .unwrap_or_default();
        ("method", format!("{recv}.{}", f.name.name))
    } else {
        ("function", f.name.name.clone())
    };
    let first = first_comment_line(f.doc.as_ref());
    if first.is_empty() {
        failures.push(Failure {
            rule: "exported",
            pos: f.name.name_pos.0 as u32,
            message: format!("exported {kind} {name} should have comment or be unexported"),
            confidence: None,
        });
        return;
    }
    let expected = format!("{} ", f.name.name);
    if !first.starts_with(&expected) && has_prefix_insensitive(&first, &expected) {
        failures.push(Failure {
            rule: "exported",
            pos: f.doc.as_ref().map(|d| d.pos().0).unwrap_or(f.name.name_pos.0) as u32,
            message: format!(
                r#"comment on exported {kind} {name} should be of the form "{} ...""#,
                f.name.name
            ),
            confidence: None,
        });
    }
}

fn lint_type_doc(ts: &TypeSpec, gd: &GenDecl, failures: &mut Vec<Failure>) {
    if !ts.name.is_exported() {
        return;
    }
    let mut doc = ts.doc.as_ref();
    let mut first = first_comment_line(doc);
    if first.is_empty() {
        doc = gd.doc.as_ref();
        first = first_comment_line(doc);
    }
    if first.is_empty() {
        failures.push(Failure {
            rule: "exported",
            pos: ts.name.name_pos.0 as u32,
            message: format!(
                "exported type {} should have comment or be unexported",
                ts.name.name
            ),
            confidence: None,
        });
        return;
    }
    let expected = ts.name.name.clone();
    if !first.starts_with(&expected) && has_prefix_insensitive(&first, &format!("{expected} ")) {
        failures.push(Failure {
            rule: "exported",
            pos: doc.map(|d| d.pos().0).unwrap_or(ts.name.name_pos.0) as u32,
            message: format!(
                r#"comment on exported type {} should be of the form "{} ..." (with optional leading article)"#,
                ts.name.name, ts.name.name
            ),
            confidence: None,
        });
    }
}

fn lint_value_spec(
    vs: &ValueSpec,
    gd: &GenDecl,
    gen_decl_missing: &mut HashMap<usize, bool>,
    failures: &mut Vec<Failure>,
) {
    let kind = if gd.tok == Some(Token::CONST) {
        "const"
    } else {
        "var"
    };
    if vs.names.len() >= 2 {
        for name in vs.names.iter().skip(1) {
            if name.is_exported() {
                failures.push(Failure {
                    rule: "exported",
                    pos: name.name_pos.0 as u32,
                    message: format!("exported {kind} {} should have its own declaration", name.name),
                    confidence: None,
                });
                return;
            }
        }
    }
    let Some(name) = vs.names.first() else {
        return;
    };
    if !name.is_exported() {
        return;
    }
    let vs_first = first_comment_line(vs.doc.as_ref());
    let gd_first = first_comment_line(gd.doc.as_ref());
    if vs_first.is_empty() && gd_first.is_empty() {
        let key = gd.tok_pos.0 as usize;
        if gen_decl_missing.get(&key).copied().unwrap_or(false) {
            return;
        }
        let block = if kind == "const" && gd.lparen.is_valid() {
            " (or a comment on this block)"
        } else {
            ""
        };
        failures.push(Failure {
            rule: "exported",
            pos: name.name_pos.0 as u32,
            message: format!(
                "exported {kind} {} should have comment{block} or be unexported",
                name.name
            ),
            confidence: None,
        });
        gen_decl_missing.insert(key, true);
        return;
    }
    if !gd_first.is_empty() && gd.lparen.is_valid() {
        return;
    }
    let doc_line = if !vs_first.is_empty() {
        vs_first
    } else {
        gd_first
    };
    if !doc_line.starts_with(&name.name) && has_prefix_insensitive(&doc_line, &format!("{} ", name.name))
    {
        failures.push(Failure {
            rule: "exported",
            pos: name.name_pos.0 as u32,
            message: format!(
                r#"comment on exported {kind} {} should be of the form "{} ...""#,
                name.name, name.name
            ),
            confidence: None,
        });
    }
}

fn check_repetitive(pkg: &str, name: &str, thing: &str, pos: i64, failures: &mut Vec<Failure>) {
    if !guff::ast::ast_is_exported(name) || name.len() <= pkg.len() {
        return;
    }
    if !name[..pkg.len()].eq_ignore_ascii_case(pkg) {
        return;
    }
    let rem = &name[pkg.len()..];
    let Some(next) = rem.chars().next() else {
        return;
    };
    if next == '_' || next.is_uppercase() {
        failures.push(Failure {
            rule: "exported",
            pos: pos as u32,
            message: format!(
                "{thing} name will be used as {pkg}.{name} by other packages, and that stutters; consider calling this {rem}"
            ),
            confidence: None,
        });
    }
}

#[cfg(test)]
mod doc_reparse_tests {
    use super::*;
    use guff::ast::Decl;

    #[test]
    fn parse_comments_keeps_const_block_doc() {
        let src = br#"package p

// Flags for the bitfield.
const (
	ChangeIgnoreCtime = 1 << iota
)
"#;
        let fset = FileSet::new();
        let file = parse_file(&fset, "p.go", src, PARSE_COMMENTS).unwrap();
        let Decl::GenDecl(g) = &file.decls[0] else { panic!("not gen") };
        assert!(g.doc.is_some(), "GenDecl.doc missing with PARSE_COMMENTS");
        assert!(!first_comment_line(g.doc.as_ref()).is_empty());
    }
}
