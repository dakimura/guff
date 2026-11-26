//! `confusing-naming` — warn on methods/fields that differ only by capitalization.

use std::collections::HashMap;

use guff::ast::{Decl, Expr, File, FuncDecl, Ident};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{confusing_naming_holder, unparen};

const PACKAGE_FUNCS: &str = "_";

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
    methods: HashMap<String, HashMap<String, (String, String)>>,
}

impl<'a> Checker<'a> {
    pub fn new(pass: &'a Pass<'a>) -> Self {
        Self {
            pass,
            failures: Vec::new(),
            methods: HashMap::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        // Package-level decls only (mirrors the previous `file.decls` walk).
        let NodeRef::File(file) = n else {
            return;
        };
        check_file(self.pass, file, &mut self.methods, &mut self.failures);
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new(pass);
    for file in pass.files() {
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
    pass: &Pass<'_>,
    file: &File,
    methods: &mut HashMap<String, HashMap<String, (String, String)>>,
    failures: &mut Vec<Failure>,
) {
    let file_name = file.name.name.as_str();
    // Preserve historical file label: always the first compiled path's basename.
    let file_label = pass
        .pkg()
        .compiled_go_files
        .first()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or(file_name);

    for decl in &file.decls {
        match decl {
            Decl::FuncDecl(f) => {
                check_method_name(
                    receiver_holder(f),
                    &f.name,
                    file_label,
                    methods,
                    failures,
                );
            }
            Decl::GenDecl(g) => {
                for spec in &g.specs {
                    let guff::ast::Spec::TypeSpec(ts) = spec else {
                        continue;
                    };
                    if let Expr::StructType(st) = unparen(&ts.ty) {
                        check_struct_fields(&ts.name.name, &st.fields.list, failures);
                    }
                }
            }
            _ => {}
        }
    }
}

fn receiver_holder(f: &FuncDecl) -> String {
    f.recv
        .as_ref()
        .and_then(|r| r.list.first())
        .map(|field| confusing_naming_holder(field.ty.as_ref().expect("recv type")))
        .unwrap_or_else(|| PACKAGE_FUNCS.into())
}

fn check_method_name(
    holder: String,
    id: &Ident,
    file_name: &str,
    methods: &mut HashMap<String, HashMap<String, (String, String)>>,
    failures: &mut Vec<Failure>,
) {
    if id.name == "init" && holder == PACKAGE_FUNCS {
        return;
    }
    let norm = id.name.to_ascii_uppercase();
    let entry = methods.entry(holder.clone()).or_default();
    if let Some((ref_file, ref_name)) = entry.get(&norm) {
        let kind = if holder == PACKAGE_FUNCS {
            "function"
        } else {
            "method"
        };
        let where_ = if ref_file == file_name {
            "the same source file"
        } else {
            ref_file.as_str()
        };
        failures.push(Failure {
            rule: "confusing-naming",
            pos: id.name_pos.0 as u32,
            message: format!(
                "Method '{}' differs only by capitalization to {} '{}' in {}",
                id.name, kind, ref_name, where_
            ),
            ..Failure::default()
        });
        return;
    }
    entry.insert(norm, (file_name.to_string(), id.name.clone()));
}

fn check_struct_fields(struct_name: &str, fields: &[guff::ast::Field], failures: &mut Vec<Failure>) {
    let mut seen = HashMap::new();
    for field in fields {
        for id in &field.names {
            if id.name == "_" {
                continue;
            }
            let norm = id.name.to_ascii_uppercase();
            if seen.contains_key(&norm) {
                failures.push(Failure {
                    rule: "confusing-naming",
                    pos: id.name_pos.0 as u32,
                    message: format!(
                        "Field '{}' differs only by capitalization to other field in the struct type {}",
                        id.name, struct_name
                    ),
                    ..Failure::default()
                });
            } else {
                seen.insert(norm, ());
            }
        }
    }
}
