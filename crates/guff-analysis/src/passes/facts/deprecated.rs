//! Deprecated fact analyzer — marks objects and packages with `Deprecated:` docs.
//!
//! Port of `honnef.co/go/tools/analysis/facts/deprecated`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{File, GenDecl, Ident};
use guff::token::Token;
use guff::walk::{NodeRef, preorder};
use guff_types::arena::{ObjectId, PackageId};

use crate::analyzer::{AnalysisResult, Analyzer, RunError, RunFn};
use crate::facts::{Fact, FactTypeId};
use crate::pass::Pass;
use crate::passes::inspect;

/// Fact attached to deprecated objects and packages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsDeprecated {
    pub msg: String,
}

impl Fact for IsDeprecated {
    fn fact_type_id(&self) -> FactTypeId {
        FactTypeId::of::<Self>()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn clone_fact(&self) -> Box<dyn Fact> {
        Box::new(self.clone())
    }
}

/// Aggregated deprecated facts for the package and its dependencies.
#[derive(Clone, Default)]
pub struct DeprecatedResult {
    pub objects: HashMap<ObjectId, IsDeprecated>,
    pub packages: HashMap<PackageId, IsDeprecated>,
}

fn extract_deprecated_message(docs: &[&Option<guff::ast::CommentGroup>]) -> Option<String> {
    for doc in docs {
        let Some(doc) = doc else {
            continue;
        };
        for part in doc.text().split("\n\n") {
            if let Some(rest) = part.strip_prefix("Deprecated: ") {
                return Some(rest.replace('\n', " ").trim().to_string());
            }
        }
    }
    None
}

fn export_deprecated(pass: &mut Pass<'_>, names: &[&Ident], docs: &[&Option<guff::ast::CommentGroup>]) {
    let Some(msg) = extract_deprecated_message(docs) else {
        return;
    };
    for name in names {
        if let Some(obj) = pass.types_info().and_then(|info| {
            info.defs
                .get(&name.id)
                .and_then(|o| *o)
                .or_else(|| info.uses.get(&name.id).copied())
        }) {
            pass.export_object_fact(obj, Box::new(IsDeprecated { msg: msg.clone() }));
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut result = DeprecatedResult::default();

    // Package-level deprecation from file docs.
    let mut pkg_docs: Vec<&Option<guff::ast::CommentGroup>> = Vec::new();
    for file in pass.files() {
        pkg_docs.push(&file.doc);
    }
    if let Some(msg) = extract_deprecated_message(&pkg_docs) {
        if pass.pkg().pkg_path != "syscall" {
            if let Some(pkg) = pass.type_pkg() {
                pass.export_package_fact(pkg, Box::new(IsDeprecated { msg: msg.clone() }));
                result.packages.insert(pkg, IsDeprecated { msg });
            }
        }
    }

    for fact in pass.all_object_facts() {
        if let Some(dep) = fact.fact.as_any().downcast_ref::<IsDeprecated>() {
            result.objects.insert(fact.object, dep.clone());
        }
    }
    for fact in pass.all_package_facts() {
        if let Some(dep) = fact.fact.as_any().downcast_ref::<IsDeprecated>() {
            result.packages.insert(fact.package, dep.clone());
        }
    }

    let files: Vec<_> = pass.files().to_vec();
    for file in &files {
        walk_file(pass, file);
    }

    for fact in pass.all_object_facts() {
        if let Some(dep) = fact.fact.as_any().downcast_ref::<IsDeprecated>() {
            result.objects.insert(fact.object, dep.clone());
        }
    }
    for fact in pass.all_package_facts() {
        if let Some(dep) = fact.fact.as_any().downcast_ref::<IsDeprecated>() {
            result.packages.insert(fact.package, dep.clone());
        }
    }

    Ok(Some(Box::new(result)))
}

fn walk_file(pass: &mut Pass<'_>, file: &File) {
    preorder(NodeRef::File(file), |node| {
        match node {
            NodeRef::GenDecl(decl) => walk_gen_decl(pass, decl),
            NodeRef::FuncDecl(decl) => {
                export_deprecated(pass, &[&decl.name], &[&decl.doc]);
                false
            }
            NodeRef::TypeSpec(spec) => {
                export_deprecated(pass, &[&spec.name], &[&spec.doc]);
                true
            }
            NodeRef::ValueSpec(spec) => {
                let names: Vec<&Ident> = spec.names.iter().collect();
                export_deprecated(pass, &names, &[&spec.doc]);
                false
            }
            NodeRef::StructType(st) => {
                for field in &st.fields.list {
                    let names: Vec<&Ident> = field.names.iter().collect();
                    export_deprecated(pass, &names, &[&field.doc]);
                }
                false
            }
            NodeRef::InterfaceType(it) => {
                for method in &it.methods.list {
                    let names: Vec<&Ident> = method.names.iter().collect();
                    export_deprecated(pass, &names, &[&method.doc]);
                }
                false
            }
            _ => true,
        }
    });
}

fn walk_gen_decl(pass: &mut Pass<'_>, decl: &GenDecl) -> bool {
    match decl.tok {
        Some(Token::TYPE) | Some(Token::CONST) | Some(Token::VAR) => {}
        _ => return false,
    }
    let mut docs: Vec<&Option<guff::ast::CommentGroup>> = vec![&decl.doc];
    let mut names: Vec<&Ident> = Vec::new();
    for spec in &decl.specs {
        match spec {
            guff::ast::Spec::ValueSpec(vs) => {
                docs.push(&vs.doc);
                names.extend(vs.names.iter());
            }
            guff::ast::Spec::TypeSpec(ts) => {
                docs.push(&ts.doc);
                names.push(&ts.name);
            }
            _ => {}
        }
    }
    export_deprecated(pass, &names, &docs);
    true
}

fn deprecated_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "fact_deprecated",
        doc: "mark deprecated objects and packages from doc comments",
        url: "https://staticcheck.dev/docs/checks/",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![FactTypeId::of::<IsDeprecated>()],
    }
}

/// Deprecated fact analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(deprecated_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff::parser::{parse_file, Mode};
    use guff::FileSet;
    use crate::validate;

    #[test]
    fn extracts_package_deprecation_from_file_doc() {
        let fset = FileSet::new();
        let src = b"// Deprecated: use New instead.\npackage old\n\nfunc Old() {}\n";
        let file = parse_file(&fset, "old.go", src, Mode::NONE).expect("parse");
        assert!(file.doc.is_some(), "expected file doc, got {:?}", file.doc);
        let docs = [&file.doc];
        let msg = extract_deprecated_message(&docs).expect("deprecated msg");
        assert_eq!(msg, "use New instead.");
    }

    #[test]
    fn deprecated_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
