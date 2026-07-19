//! `confusing-naming` — warn on methods/fields that differ only by capitalization.

use std::collections::HashMap;

use guff::ast::{Decl, Expr, FuncDecl, Ident, StructType};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{receiver_type_key, unparen};

const PACKAGE_FUNCS: &str = "_";

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    let mut methods: HashMap<String, HashMap<String, (String, String)>> = HashMap::new();

    for file in pass.files() {
        let file_name = file
            .name
            .name
            .as_str(); // fallback; real path resolved per compiled file below
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
                        &mut methods,
                        &mut failures,
                    );
                }
                Decl::GenDecl(g) => {
                    for spec in &g.specs {
                        let guff::ast::Spec::TypeSpec(ts) = spec else {
                            continue;
                        };
                        if let Expr::StructType(st) = unparen(&ts.ty) {
                            check_struct_fields(&ts.name.name, &st.fields.list, &mut failures);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    failures
}

fn receiver_holder(f: &FuncDecl) -> String {
    f.recv
        .as_ref()
        .and_then(|r| r.list.first())
        .map(|field| receiver_type_key(field.ty.as_ref().expect("recv type")))
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
            confidence: None,
        });
        return;
    }
    entry.insert(
        norm,
        (file_name.to_string(), id.name.clone()),
    );
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
            confidence: None,
        });
            } else {
                seen.insert(norm, ());
            }
        }
    }
}