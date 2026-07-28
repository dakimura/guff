//! `exported` — naming and commenting conventions on exported symbols.

use std::collections::HashMap;

use guff::ast::{Decl, File, FuncDecl, GenDecl, Spec, TypeSpec, ValueSpec};
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
}

impl<'a> Checker<'a> {
    pub fn try_new(pass: &'a Pass<'a>) -> Option<Self> {
        if !is_importable_package(&pass.pkg().name) {
            return None;
        }
        Some(Self {
            pass,
            failures: Vec::new(),
            gen_decl_missing: HashMap::new(),
            skip_file: false,
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
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        if self.skip_file {
            return;
        }
        // Package-level decls only (mirrors the previous `file.decls` walk).
        let NodeRef::File(file) = n else {
            return;
        };
        let pkg = self
            .pass
            .files()
            .first()
            .map(|f| f.name.name.as_str())
            .unwrap_or("");
        check_file(file, pkg, &mut self.gen_decl_missing, &mut self.failures);
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

fn check_file(
    file: &File,
    pkg: &str,
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
                            check_repetitive(pkg, &ts.name.name, "type", ts.name.name_pos.0, failures);
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
                lint_func_doc(f, failures);
                if f.recv.is_none() {
                    check_repetitive(pkg, &f.name.name, "func", f.name.name_pos.0, failures);
                }
            }
            _ => {}
        }
    }
}

fn lint_func_doc(f: &FuncDecl, failures: &mut Vec<Failure>) {
    if !f.name.is_exported() {
        return;
    }
    let (kind, name) = if f.recv.is_some() {
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
