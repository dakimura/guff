//! `import-shadowing` — spot identifiers that shadow an import name.

use std::collections::HashSet;

use guff::token::Token;
use guff::ast::{Decl, File, Ident, ImportSpec, Spec};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        let import_names = collect_import_names(file);
        if import_names.is_empty() {
            continue;
        }
        let mut walker = ImportShadowingWalker {
            package_name: file.name.name.clone(),
            import_names,
            already_seen: HashSet::new(),
            skip_idents: HashSet::new(),
            failures: &mut failures,
        };
        walk::inspect(NodeRef::File(file), |n| walker.visit(n));
    }
    failures
}

fn collect_import_names(file: &File) -> HashSet<String> {
    let mut names = HashSet::new();
    for decl in &file.decls {
        let Decl::GenDecl(g) = decl else {
            continue;
        };
        if g.tok != Some(Token::IMPORT) {
            continue;
        }
        for spec in &g.specs {
            let Spec::ImportSpec(is) = spec else {
                continue;
            };
            names.insert(import_name(is));
        }
    }
    names
}

fn import_name(spec: &ImportSpec) -> String {
    if let Some(name) = &spec.name {
        return name.name.clone();
    }
    let path = spec.path.value.trim_matches('"');
    let mut parts: Vec<&str> = path.split('/').collect();
    let last = parts.pop().unwrap_or(path);
    if is_version(last) && parts.len() >= 2 {
        return parts[parts.len() - 1].to_string();
    }
    last.to_string()
}

fn is_version(name: &str) -> bool {
    name.len() >= 2
        && name.starts_with('v')
        && name[1..].chars().all(|c| c.is_ascii_digit())
}

struct ImportShadowingWalker<'a> {
    package_name: String,
    import_names: HashSet<String>,
    already_seen: HashSet<u32>,
    skip_idents: HashSet<u32>,
    failures: &'a mut Vec<Failure>,
}

impl ImportShadowingWalker<'_> {
    fn visit(&mut self, n: Option<NodeRef<'_>>) -> bool {
        let Some(n) = n else {
            return true;
        };
        match n {
            NodeRef::AssignStmt(assign) => {
                if assign.tok == Some(Token::DEFINE) {
                    return true;
                }
                false
            }
            NodeRef::CallExpr(_)
            | NodeRef::ImportSpec(_)
            | NodeRef::KeyValueExpr(_)
            | NodeRef::ReturnStmt(_)
            | NodeRef::SelectorExpr(_)
            | NodeRef::StructType(_) => false,
            NodeRef::FuncDecl(f) => {
                if f.recv.is_some() {
                    self.skip_idents.insert(f.name.id);
                }
                true
            }
            NodeRef::Ident(id) => {
                self.check_ident(id);
                true
            }
            _ => true,
        }
    }

    fn check_ident(&mut self, id: &Ident) {
        if id.name == self.package_name || id.name == "_" {
            return;
        }
        if !self.import_names.contains(&id.name) {
            return;
        }
        if self.already_seen.contains(&id.id) || self.skip_idents.contains(&id.id) {
            return;
        }
        self.already_seen.insert(id.id);
        self.failures.push(Failure {
            rule: "import-shadowing",
            pos: id.name_pos.0 as u32,
            message: format!("The name '{}' shadows an import name", id.name),
            confidence: None,
        });
    }
}
