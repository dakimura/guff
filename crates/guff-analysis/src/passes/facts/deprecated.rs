//! Deprecated fact analyzer — marks objects and packages with `Deprecated:` docs.
//!
//! Port of `honnef.co/go/tools/analysis/facts/deprecated`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{File, GenDecl, Ident};
use guff::token::Token;
use guff::walk::{preorder_prune, NodeRef};
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

    fn type_name(&self) -> &'static str {
        "IsDeprecated"
    }

    fn encode_payload(&self) -> serde_json::Value {
        serde_json::json!({ "msg": self.msg })
    }
}

fn decode_is_deprecated(payload: serde_json::Value) -> Option<Box<dyn Fact>> {
    let msg = payload.get("msg")?.as_str()?.to_string();
    Some(Box::new(IsDeprecated { msg }))
}

/// Register the [`IsDeprecated`] fact decoder (called from builtin init).
pub(crate) fn register_deprecated_fact_decoder() {
    crate::fact_codec::register_fact_decoder("IsDeprecated", decode_is_deprecated);
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
                // No trim: upstream is `strings.Replace(alt, "\n", " ", -1)`
                // over a `doc.Text()` that ends in a newline, so the last
                // paragraph's message carries a trailing space into the
                // message text. golangci-lint prints it, and the golden tier
                // compares messages byte for byte.
                return Some(rest.replace('\n', " "));
            }
        }
    }
    None
}

fn export_deprecated(
    pass: &mut Pass<'_>,
    docs_by_offset: &HashMap<i64, String>,
    names: &[&Ident],
    docs: &[&Option<guff::ast::CommentGroup>],
) {
    let msg = match extract_deprecated_message(docs) {
        Some(m) => Some(m),
        // The analysis AST carries no doc comments (see `docs_by_offset`), so
        // for the package being analysed every message comes from the reparse,
        // keyed by the byte offset of the declared name.
        None => names
            .iter()
            .find_map(|n| offset_of(pass, n.pos()).and_then(|off| docs_by_offset.get(&off).cloned())),
    };
    let Some(msg) = msg else {
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

/// Byte offset of `pos` within its file, in the analysis `FileSet`.
fn offset_of(pass: &Pass<'_>, pos: guff::position::Pos) -> Option<i64> {
    let f = pass.fset().file(pos)?;
    let off = f.offset(pos);
    (off >= 0).then_some(off)
}

/// `Deprecated:` messages of this package's own declarations, keyed by the byte
/// offset of the declared name.
///
/// The shared load parses without `PARSE_COMMENTS` (`guff-packages`'
/// `typecheck.rs`), so `decl.doc` on the analysis AST is always `None` and this
/// pass exported **nothing for the package being analysed** — every fact it had
/// came from a dependency. Nothing depended on the missing half until SA1019's
/// "a deprecated function may use deprecated symbols" guard, whose whole input
/// is the enclosing function's own deprecation: it asked `deprs.objects` a
/// question that could only ever be answered "no", so the guard was inert and
/// controller-runtime got two findings upstream does not make.
///
/// COMPAT-HARDENING §4 records this same root cause diagnosed separately for
/// buildtag, directive, comments-density, comment-spacings, S1008 and four
/// others. This is the tenth.
///
/// Offsets, not node identity: both parses read the same bytes, so a name's
/// offset is the same in either tree, and no position mapping is needed beyond
/// asking each `FileSet` for it.
fn deprecated_docs_by_offset(pass: &Pass<'_>, file: &File) -> HashMap<i64, String> {
    let mut out = HashMap::new();
    let Some(fname) = pass.fset().file(file.pos()).map(|f| f.name().to_string()) else {
        return out;
    };
    let base = std::path::Path::new(&fname)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(fname.as_str())
        .to_string();
    let Some(path) = pass
        .pkg()
        .compiled_go_files
        .iter()
        .find(|p| p.file_name().and_then(|s| s.to_str()) == Some(base.as_str()))
    else {
        return out;
    };
    let Ok(src) = std::fs::read(path) else {
        return out;
    };
    // Cheap filter before the reparse, the same shape `inline`, `directive` and
    // `buildtag` already use for theirs: the only thing this function can
    // extract is a `Deprecated: ` paragraph, so a file whose bytes never
    // mention it cannot contribute a fact. Without this the pass paid a disk
    // read and a full `PARSE_COMMENTS` parse for *every* file in the package,
    // and deprecations are rare — on prometheus `./...` the marker appears in a
    // handful of files out of ~1500.
    if !src
        .windows(b"Deprecated:".len())
        .any(|w| w == b"Deprecated:")
    {
        return out;
    }
    let rfset = guff::position::FileSet::new();
    let Ok(rfile) = guff::parser::parse_file(&rfset, &base, &src, guff::parser::COMMENTS_ONLY)
    else {
        return out;
    };

    let mut record = |names: &[&Ident], docs: &[&Option<guff::ast::CommentGroup>]| {
        let Some(msg) = extract_deprecated_message(docs) else {
            return;
        };
        for n in names {
            if let Some(f) = rfset.file(n.pos()) {
                let off = f.offset(n.pos());
                if off >= 0 {
                    out.entry(off).or_insert_with(|| msg.clone());
                }
            }
        }
    };

    // Same shape as `walk_file` below, over the tree that has the comments.
    preorder_prune(NodeRef::File(&rfile), |node| match node {
        NodeRef::GenDecl(decl) => {
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
            record(&names, &docs);
            true
        }
        NodeRef::FuncDecl(decl) => {
            record(&[&decl.name], &[&decl.doc]);
            false
        }
        NodeRef::TypeSpec(spec) => {
            record(&[&spec.name], &[&spec.doc]);
            true
        }
        NodeRef::ValueSpec(spec) => {
            let names: Vec<&Ident> = spec.names.iter().collect();
            record(&names, &[&spec.doc]);
            false
        }
        NodeRef::StructType(st) => {
            for field in &st.fields.list {
                let names: Vec<&Ident> = field.names.iter().collect();
                record(&names, &[&field.doc]);
            }
            false
        }
        NodeRef::InterfaceType(it) => {
            for method in &it.methods.list {
                let names: Vec<&Ident> = method.names.iter().collect();
                record(&names, &[&method.doc]);
            }
            false
        }
        _ => true,
    });
    out
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
    let docs_by_offset = deprecated_docs_by_offset(pass, file);
    preorder_prune(NodeRef::File(file), |node| {
        match node {
            NodeRef::GenDecl(decl) => walk_gen_decl(pass, &docs_by_offset, decl),
            NodeRef::FuncDecl(decl) => {
                export_deprecated(pass, &docs_by_offset, &[&decl.name], &[&decl.doc]);
                false
            }
            NodeRef::TypeSpec(spec) => {
                export_deprecated(pass, &docs_by_offset, &[&spec.name], &[&spec.doc]);
                true
            }
            NodeRef::ValueSpec(spec) => {
                let names: Vec<&Ident> = spec.names.iter().collect();
                export_deprecated(pass, &docs_by_offset, &names, &[&spec.doc]);
                false
            }
            NodeRef::StructType(st) => {
                for field in &st.fields.list {
                    let names: Vec<&Ident> = field.names.iter().collect();
                    export_deprecated(pass, &docs_by_offset, &names, &[&field.doc]);
                }
                false
            }
            NodeRef::InterfaceType(it) => {
                for method in &it.methods.list {
                    let names: Vec<&Ident> = method.names.iter().collect();
                    export_deprecated(pass, &docs_by_offset, &names, &[&method.doc]);
                }
                false
            }
            _ => true,
        }
    });
}

fn walk_gen_decl(
    pass: &mut Pass<'_>,
    docs_by_offset: &HashMap<i64, String>,
    decl: &GenDecl,
) -> bool {
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
    export_deprecated(pass, docs_by_offset, &names, &docs);
    true
}

fn deprecated_analyzer_impl() -> Analyzer {
    register_deprecated_fact_decoder();
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
        // Trailing space intended: `doc.Text()` ends in a newline and upstream
        // only replaces newlines with spaces, so the space reaches the printed
        // SA1019 message. golangci-lint prints it too, and the golden tier
        // compares message text byte for byte.
        assert_eq!(msg, "use New instead. ");
    }

    #[test]
    fn deprecated_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
