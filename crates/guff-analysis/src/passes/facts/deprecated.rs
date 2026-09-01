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
    // The doc of the declaration itself. Upstream calls `doDocs` once per node
    // with only that node's own doc, so a group's `Deprecated:` covers every
    // name it declares and nothing else.
    let group_msg = extract_deprecated_message(docs);
    for name in names {
        // The fallback is per name, not per group. The analysis AST carries no
        // doc comments (see `docs_by_offset`), so for the package being
        // analysed every message comes from the reparse, keyed by the byte
        // offset of the declared name — and asking only the *first* name that
        // happens to have an entry, then stamping the answer on all of them,
        // marks a whole `const (…)` block deprecated because one member is.
        let msg = match &group_msg {
            Some(m) => m.clone(),
            None => match offset_of(pass, name.pos())
                .and_then(|off| docs_by_offset.get(&off).cloned())
            {
                Some(m) => m,
                None => continue,
            },
        };
        if let Some(obj) = pass.types_info().and_then(|info| {
            info.defs
                .get(&name.id)
                .and_then(|o| *o)
                .or_else(|| info.uses.get(&name.id).copied())
        }) {
            pass.export_object_fact(obj, Box::new(IsDeprecated { msg }));
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
    docs_by_offset_from_source(&base, &src)
}

/// The reparse half of [`deprecated_docs_by_offset`], split out so the walk can
/// be tested without a `Pass`: `Deprecated:` messages declared in `src`, keyed
/// by the byte offset of the declared name.
fn docs_by_offset_from_source(base: &str, src: &[u8]) -> HashMap<i64, String> {
    let mut out = HashMap::new();
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
    let Ok(rfile) = guff::parser::parse_file(&rfset, base, src, guff::parser::COMMENTS_ONLY)
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
                    // Overwrite, not `or_insert`: the group is visited before
                    // its specs, and upstream's later `ExportObjectFact` for a
                    // spec replaces the fact the group put there, so a spec's
                    // own message wins over the group's.
                    out.insert(off, msg.clone());
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
            // Only the group's own doc: upstream appends `node.Doc` and the
            // *names* of the specs, never the specs' docs. The specs' docs are
            // read on the way down, one spec at a time.
            let mut names: Vec<&Ident> = Vec::new();
            for spec in &decl.specs {
                match spec {
                    guff::ast::Spec::ValueSpec(vs) => names.extend(vs.names.iter()),
                    guff::ast::Spec::TypeSpec(ts) => names.push(&ts.name),
                    _ => {}
                }
            }
            record(&names, &[&decl.doc]);
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
    // Only the group's own doc — see `deprecated_docs_by_offset`.
    let mut names: Vec<&Ident> = Vec::new();
    for spec in &decl.specs {
        match spec {
            guff::ast::Spec::ValueSpec(vs) => names.extend(vs.names.iter()),
            guff::ast::Spec::TypeSpec(ts) => names.push(&ts.name),
            _ => {}
        }
    }
    export_deprecated(pass, docs_by_offset, &names, &[&decl.doc]);
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

    /// Every shape the group/spec split discriminates on, in one file. The
    /// grid was measured against golangci-lint 2.12.2 before it was written
    /// down, as a two-package fixture whose second package uses every name:
    /// the twelve reachable findings and the six silent names both match.
    const GRID: &[u8] = br#"package dep

const (
	KindA Kind = 0
	// Deprecated: Marked as deprecated in x.proto.
	KindC Kind = 2
	KindE Kind = 4
)

// Deprecated: whole group is gone.
const (
	GroupA = 1
	GroupB = 2
)

// Deprecated: group message.
const (
	MixA = 1
	// Deprecated: spec message.
	MixB = 2
	MixC = 3
)

const (
	// Deprecated: pair message.
	PairA, PairB = 1, 2
	PairC        = 3
)

// Deprecated: type group message.
type (
	TypeA struct{}
	TypeB struct{}
)

const (
	LineA = 1 // Deprecated: trailing comment, not a doc.
	LineB = 2
)

const (
	// Some prose first.
	//
	// Deprecated: second paragraph.
	ParaA = 1
	ParaB = 2
)

// Deprecated: use NewThing.
func OldThing() {}

func NewThing() {}

type Fields struct {
	Plain int
	// Deprecated: field message.
	Old  int
	Also int
}

type Iface interface {
	Plain()
	// Deprecated: method message.
	Old()
	Also()
}
"#;

    /// Read the identifier that starts at `off`, so the assertions below can be
    /// written in names rather than byte offsets.
    fn ident_at(src: &[u8], off: i64) -> String {
        let start = off as usize;
        let end = src[start..]
            .iter()
            .position(|c| !(c.is_ascii_alphanumeric() || *c == b'_'))
            .map(|n| start + n)
            .unwrap_or(src.len());
        String::from_utf8_lossy(&src[start..end]).into_owned()
    }

    fn grid_messages() -> Vec<(String, String)> {
        let map = docs_by_offset_from_source("dep.go", GRID);
        let mut out: Vec<(String, String)> = map
            .into_iter()
            .map(|(off, msg)| (ident_at(GRID, off), msg))
            .collect();
        out.sort();
        out
    }

    /// The defect this grid was written for: a `Deprecated:` doc on one member
    /// of a `const (…)` group was pooled with the group's own doc and stamped
    /// on every name the group declared. syncthing's `lib/protocol` used five
    /// `bep.FileInfoType_*` constants of which upstream deprecates two, and
    /// guff reported all five.
    #[test]
    fn spec_doc_does_not_leak_to_its_siblings() {
        let msgs = grid_messages();
        let named: Vec<&str> = msgs.iter().map(|(n, _)| n.as_str()).collect();
        assert!(named.contains(&"KindC"), "{msgs:?}");
        for silent in ["KindA", "KindE", "PairC", "LineA", "LineB", "ParaB"] {
            assert!(
                !named.contains(&silent),
                "{silent} is not deprecated but was marked: {msgs:?}"
            );
        }
    }

    /// A doc on the group covers every name the group declares — including the
    /// names of a `type (…)` group and of a multi-name spec.
    #[test]
    fn group_doc_covers_every_name_it_declares() {
        let msgs = grid_messages();
        // Counted, not `any(contains(…))`: the grid declares exactly fourteen
        // deprecated names, and a rule that marks one name too many or too few
        // has to change this number.
        assert_eq!(msgs.len(), 14, "{msgs:?}");
        assert_eq!(
            msgs,
            vec![
                ("GroupA".into(), "whole group is gone. ".into()),
                ("GroupB".into(), "whole group is gone. ".into()),
                ("KindC".into(), "Marked as deprecated in x.proto. ".into()),
                ("MixA".into(), "group message. ".into()),
                // The spec's own message wins over the group's: upstream calls
                // `ExportObjectFact` for the group first and for the spec
                // second, and the later call replaces the fact.
                ("MixB".into(), "spec message. ".into()),
                ("MixC".into(), "group message. ".into()),
                ("Old".into(), "field message. ".into()),
                ("Old".into(), "method message. ".into()),
                ("OldThing".into(), "use NewThing. ".into()),
                ("PairA".into(), "pair message. ".into()),
                ("PairB".into(), "pair message. ".into()),
                ("ParaA".into(), "second paragraph. ".into()),
                ("TypeA".into(), "type group message. ".into()),
                ("TypeB".into(), "type group message. ".into()),
            ],
            "{msgs:?}"
        );
    }

    #[test]
    fn deprecated_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
