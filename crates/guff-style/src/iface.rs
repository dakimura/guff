//! Port of [`github.com/uudashr/iface`](https://github.com/uudashr/iface)
//! v1.5.1 (golangci-lint wrapper in `pkg/golinters/iface`).
//!
//! | upstream | here |
//! |----------|------|
//! | `identical` | [`check_identical`] |
//! | `unused` | [`check_unused`] |
//! | `unusedmethod` | [`check_unusedmethod`] |
//! | `opaque` | [`check_opaque`] |
//! | `unexported` | [`check_unexported`] |
//! | `internal/directive` | [`parse_ignore`] / [`should_ignore`] |
//!
//! golangci-lint runs `identical` alone unless `enable` names analyzers, and
//! skips names it does not know. It wraps several analyzers in one linter, so
//! each message carries its analyzer's name as a prefix.
//!
//! guff analyzes files parsed without comments. The comments `go/parser`
//! hangs on nodes — the `//iface:ignore` directives, and the doc comments the
//! `unused` / `unusedmethod` fixes delete with their declaration — come from a
//! reparse ([`Comments`]).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::OnceLock;

use guff::ast::{CommentGroup, Decl, Expr, FieldList, File, FuncDecl, GenDecl, Spec};
use guff::commentmap::node_end;
use guff::position::Pos;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::comments::{file_source_contains, reparse_with_comments};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::arena::ObjectData;
use guff_types::basic::BasicKind;
use guff_types::predicates::{identical as types_identical, is_interface};
use guff_types::{Info, ObjectId, OperandMode, PackageId, TypeData, TypeId};

use crate::options::IfaceOptions;

const CHECK_IDENTICAL: &str = "identical";
const CHECK_UNUSED: &str = "unused";
const CHECK_OPAQUE: &str = "opaque";
const CHECK_UNEXPORTED: &str = "unexported";
const CHECK_UNUSEDMETHOD: &str = "unusedmethod";

/// `analyzersFromSettings`: `identical` alone by default, otherwise the known
/// names in `enable`.
fn enabled_checks(opts: &IfaceOptions) -> HashSet<&'static str> {
    if opts.enable.is_empty() {
        return HashSet::from([CHECK_IDENTICAL]);
    }
    let mut out = HashSet::new();
    for name in &opts.enable {
        for known in [
            CHECK_IDENTICAL,
            CHECK_UNUSED,
            CHECK_OPAQUE,
            CHECK_UNEXPORTED,
            CHECK_UNUSEDMETHOD,
        ] {
            // Unknown analyzers are skipped, as golangci does.
            if name == known {
                out.insert(known);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// internal/directive
// ---------------------------------------------------------------------------

/// `directive.ParseIgnore`: `None` when `doc` carries no `//iface:ignore`,
/// otherwise the names it lists (empty = every analyzer).
fn parse_ignore(doc: Option<&CommentGroup>) -> Option<Vec<String>> {
    for comment in &doc?.list {
        let text = comment.text.trim();
        if text == "//iface:ignore" {
            return Some(Vec::new());
        }
        if let Some(val) = text.strip_prefix("//iface:ignore=") {
            let val = val.trim();
            if val.is_empty() {
                return Some(Vec::new());
            }
            return Some(val.split(',').map(|n| n.trim().to_string()).collect());
        }
    }
    None
}

/// `directive.ShouldIgnore`.
fn should_ignore(doc: Option<&CommentGroup>, name: &str) -> bool {
    match parse_ignore(doc) {
        None => false,
        Some(names) => names.is_empty() || names.iter().any(|n| n == name),
    }
}

/// The `exclude` flag: a comma-separated list of package paths. golangci joins
/// a YAML list with commas before setting it; upstream trims each entry and
/// drops the empty ones.
fn split_exclude(flag: &str) -> Vec<&str> {
    if flag.is_empty() {
        return Vec::new();
    }
    flag.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Comments
// ---------------------------------------------------------------------------

/// The comments `go/parser` attaches to the nodes iface reads, keyed by the
/// analysis position of the node they belong to.
#[derive(Default)]
struct Docs {
    /// `GenDecl.Doc`, by `GenDecl.TokPos`.
    gen_decl: HashMap<i64, CommentGroup>,
    /// `TypeSpec.Doc`, by `TypeSpec.Name`.
    type_spec: HashMap<i64, CommentGroup>,
    /// `Field.Doc`, by `Field.Pos()`.
    field_doc: HashMap<i64, CommentGroup>,
    /// `Field.Comment`, by `Field.Pos()`.
    field_comment: HashMap<i64, CommentGroup>,
    /// `FuncDecl.Doc`, by `FuncDecl.Name`.
    func_decl: HashMap<i64, CommentGroup>,
}

fn build_docs(pass: &Pass<'_>, file: &File) -> Docs {
    let mut docs = Docs::default();
    let Some(r) = reparse_with_comments(pass, file) else {
        return docs;
    };
    walk::inspect(NodeRef::File(&r.file), |n| {
        match n {
            Some(NodeRef::GenDecl(g)) => {
                if let Some(doc) = &g.doc {
                    docs.gen_decl.insert(r.rebase(g.tok_pos).0, r.rebase_group(doc));
                }
            }
            Some(NodeRef::TypeSpec(ts)) => {
                if let Some(doc) = &ts.doc {
                    docs.type_spec
                        .insert(r.rebase(ts.name.pos()).0, r.rebase_group(doc));
                }
            }
            Some(NodeRef::Field(f)) => {
                let key = r.rebase(f.pos()).0;
                if let Some(doc) = &f.doc {
                    docs.field_doc.insert(key, r.rebase_group(doc));
                }
                if let Some(c) = &f.comment {
                    docs.field_comment.insert(key, r.rebase_group(c));
                }
            }
            Some(NodeRef::FuncDecl(fd)) => {
                if let Some(doc) = &fd.doc {
                    docs.func_decl
                        .insert(r.rebase(fd.name.pos()).0, r.rebase_group(doc));
                }
            }
            _ => {}
        }
        true
    });
    docs
}

#[derive(Clone, Copy)]
enum DocKind {
    GenDecl,
    TypeSpec,
    FieldDoc,
    FieldComment,
    FuncDecl,
}

/// Per-file comment lookups, reparsing each file at most once and only when
/// asked: for a directive when the source spells `//iface:ignore` at all, for
/// a fix range when a declaration is reported.
struct Comments {
    has_directive: Vec<bool>,
    docs: RefCell<HashMap<usize, Rc<Docs>>>,
}

impl Comments {
    fn new(pass: &Pass<'_>) -> Self {
        Comments {
            has_directive: pass
                .files()
                .iter()
                .map(|f| file_source_contains(pass, f, b"//iface:ignore"))
                .collect(),
            docs: RefCell::new(HashMap::new()),
        }
    }

    fn docs(&self, pass: &Pass<'_>, fi: usize) -> Rc<Docs> {
        self.docs
            .borrow_mut()
            .entry(fi)
            .or_insert_with(|| Rc::new(build_docs(pass, &pass.files()[fi])))
            .clone()
    }

    fn get(&self, pass: &Pass<'_>, fi: usize, kind: DocKind, at: Pos) -> Option<CommentGroup> {
        let docs = self.docs(pass, fi);
        let map = match kind {
            DocKind::GenDecl => &docs.gen_decl,
            DocKind::TypeSpec => &docs.type_spec,
            DocKind::FieldDoc => &docs.field_doc,
            DocKind::FieldComment => &docs.field_comment,
            DocKind::FuncDecl => &docs.func_decl,
        };
        map.get(&at.0).cloned()
    }

    /// `directive.ShouldIgnore(<the node's doc>, analyzer)`.
    fn ignores(&self, pass: &Pass<'_>, fi: usize, kind: DocKind, at: Pos, analyzer: &str) -> bool {
        if !self.has_directive[fi] {
            return false;
        }
        should_ignore(self.get(pass, fi, kind, at).as_ref(), analyzer)
    }
}

/// `inspect.Preorder([]ast.Node{(*ast.GenDecl)(nil)}, …)` for `type` decls:
/// every one in the package, local declarations included.
fn for_each_type_decl(pass: &Pass<'_>, mut f: impl FnMut(usize, &GenDecl)) {
    for (fi, file) in pass.files().iter().enumerate() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(NodeRef::GenDecl(g)) = n {
                if g.tok == Some(Token::TYPE) {
                    f(fi, g);
                }
            }
            true
        });
    }
}

fn def_of(info: &Info, id: u32) -> Option<ObjectId> {
    info.defs.get(&id).copied().flatten()
}

/// `pass.TypesInfo.TypeOf`.
fn type_of(info: &Info, e: &Expr) -> Option<TypeId> {
    if let Some(tv) = info.types.get(&e.id()) {
        return Some(tv.typ);
    }
    None
}

fn diag(pos: Pos, message: String) -> Diagnostic {
    Diagnostic {
        pos: pos.0 as u32,
        message,
        ..Diagnostic::default()
    }
}

fn remove_fix(message: &str, start: Pos, end: Pos) -> SuggestedFix {
    SuggestedFix {
        message: message.to_string(),
        text_edits: vec![TextEdit {
            pos: start.0 as u32,
            end: end.0 as u32,
            new_text: String::new(),
        }],
    }
}

// ---------------------------------------------------------------------------
// identical
// ---------------------------------------------------------------------------

fn check_identical(pass: &Pass<'_>, cm: &Comments, out: &mut Vec<Diagnostic>) {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let Some(info) = pass.types_info() else {
        return;
    };

    // Both keyed by name, as upstream's maps are: a later declaration of the
    // same name (a local type) replaces the earlier one.
    let mut iface_decls: HashMap<String, Pos> = HashMap::new();
    let mut iface_types: HashMap<String, TypeId> = HashMap::new();
    for_each_type_decl(pass, |fi, decl| {
        if cm.ignores(pass, fi, DocKind::GenDecl, decl.tok_pos, CHECK_IDENTICAL) {
            return;
        }
        for spec in &decl.specs {
            let Spec::TypeSpec(ts) = spec else {
                continue;
            };
            if !matches!(ts.ty, Expr::InterfaceType(_)) {
                continue;
            }
            if cm.ignores(pass, fi, DocKind::TypeSpec, ts.name.pos(), CHECK_IDENTICAL) {
                continue;
            }
            iface_decls.insert(ts.name.name.clone(), ts.name.pos());
            let Some(obj) = def_of(info, ts.name.id) else {
                continue;
            };
            let ObjectData::TypeName(tn) = artifacts.objects.get(obj) else {
                continue;
            };
            let Some(typ) = tn.typ() else {
                continue;
            };
            let under = typ.underlying(&artifacts.types);
            if !matches!(artifacts.types.get(under), TypeData::Interface(_)) {
                continue;
            }
            iface_types.insert(ts.name.name.clone(), under);
        }
    });

    let mut types = artifacts.types.clone();
    let mut identicals: HashMap<&str, Vec<&str>> = HashMap::new();
    for (name, &typ) in &iface_types {
        for (other, &other_typ) in &iface_types {
            if name == other {
                continue;
            }
            if !types_identical(
                &mut types,
                &artifacts.objects,
                &artifacts.packages,
                typ,
                other_typ,
            ) {
                continue;
            }
            identicals.entry(name).or_default().push(other);
        }
    }

    for (name, mut others) in identicals {
        others.sort();
        let other_names = others.join(", ");
        let pos = iface_decls.get(name).copied().unwrap_or_default();
        out.push(diag(
            pos,
            format!(
                "identical: interface '{name}' contains identical methods or type constraints with another interface, causing redundancy (see: {other_names})"
            ),
        ));
    }
}

// ---------------------------------------------------------------------------
// unused
// ---------------------------------------------------------------------------

struct UnusedEntry {
    name: String,
    fi: usize,
    ts_pos: Pos,
    ts_end: Pos,
    decl_pos: Pos,
    decl_end: Pos,
    single_spec: bool,
}

fn check_unused(pass: &Pass<'_>, cm: &Comments, exclude: &str, out: &mut Vec<Diagnostic>) {
    if split_exclude(exclude).contains(&pass.pkg().pkg_path.as_str()) {
        return;
    }
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let Some(info) = pass.types_info() else {
        return;
    };

    let mut ifaces: HashMap<ObjectId, UnusedEntry> = HashMap::new();
    for_each_type_decl(pass, |fi, decl| {
        if cm.ignores(pass, fi, DocKind::GenDecl, decl.tok_pos, CHECK_UNUSED) {
            return;
        }
        for spec in &decl.specs {
            let Spec::TypeSpec(ts) = spec else {
                continue;
            };
            if !matches!(ts.ty, Expr::InterfaceType(_)) {
                continue;
            }
            if cm.ignores(pass, fi, DocKind::TypeSpec, ts.name.pos(), CHECK_UNUSED) {
                continue;
            }
            let Some(obj) = def_of(info, ts.name.id) else {
                continue;
            };
            if !matches!(artifacts.objects.get(obj), ObjectData::TypeName(_)) {
                continue;
            }
            ifaces.insert(
                obj,
                UnusedEntry {
                    name: ts.name.name.clone(),
                    fi,
                    ts_pos: ts.name.pos(),
                    ts_end: ts.ty.end(),
                    decl_pos: decl.tok_pos,
                    decl_end: node_end(NodeRef::GenDecl(decl)),
                    single_spec: decl.specs.len() == 1,
                },
            );
        }
    });

    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(NodeRef::Ident(ident)) = n {
                if let Some(obj) = info.uses.get(&ident.id) {
                    ifaces.remove(obj);
                }
            }
            true
        });
    }

    for entry in ifaces.into_values() {
        let (start, end) = if entry.single_spec {
            let start = cm
                .get(pass, entry.fi, DocKind::GenDecl, entry.decl_pos)
                .map_or(entry.decl_pos, |d| d.pos());
            (start, entry.decl_end)
        } else {
            let start = cm
                .get(pass, entry.fi, DocKind::TypeSpec, entry.ts_pos)
                .map_or(entry.ts_pos, |d| d.pos());
            (start, entry.ts_end)
        };
        let mut d = diag(
            entry.ts_pos,
            format!(
                "unused: interface '{}' is declared but not used within the package",
                entry.name
            ),
        );
        d.suggested_fixes = vec![remove_fix("Remove the unused interface declaration", start, end)];
        out.push(d);
    }
}

// ---------------------------------------------------------------------------
// unusedmethod
// ---------------------------------------------------------------------------

struct MethodEntry {
    iface_name: String,
    fi: usize,
    pos: Pos,
    end: Pos,
}

fn check_unusedmethod(pass: &Pass<'_>, cm: &Comments, exclude: &str, out: &mut Vec<Diagnostic>) {
    if split_exclude(exclude).contains(&pass.pkg().pkg_path.as_str()) {
        return;
    }
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let Some(info) = pass.types_info() else {
        return;
    };

    let mut unused_methods: HashMap<ObjectId, MethodEntry> = HashMap::new();
    for_each_type_decl(pass, |fi, decl| {
        if cm.ignores(pass, fi, DocKind::GenDecl, decl.tok_pos, CHECK_UNUSEDMETHOD) {
            return;
        }
        for spec in &decl.specs {
            let Spec::TypeSpec(ts) = spec else {
                continue;
            };
            let Expr::InterfaceType(iface) = &ts.ty else {
                continue;
            };
            if cm.ignores(pass, fi, DocKind::TypeSpec, ts.name.pos(), CHECK_UNUSEDMETHOD) {
                continue;
            }
            for field in &iface.methods.list {
                if !matches!(field.ty, Some(Expr::FuncType(_))) {
                    continue;
                }
                let Some(name) = field.names.first() else {
                    continue;
                };
                let obj = def_of(info, name.id);
                if cm.ignores(pass, fi, DocKind::FieldDoc, field.pos(), CHECK_UNUSEDMETHOD) {
                    continue;
                }
                if cm.ignores(pass, fi, DocKind::FieldComment, field.pos(), CHECK_UNUSEDMETHOD) {
                    continue;
                }
                let Some(obj) = obj else {
                    continue;
                };
                if !matches!(artifacts.objects.get(obj), ObjectData::Func(_)) {
                    continue;
                }
                unused_methods.insert(
                    obj,
                    MethodEntry {
                        iface_name: ts.name.name.clone(),
                        fi,
                        pos: field.pos(),
                        end: field.end(),
                    },
                );
            }
        }
    });

    // A method is used only through a selector: a call, a method value or a
    // method expression on the interface. Implementing it does not count.
    for sel in info.selections.values() {
        if matches!(artifacts.objects.get(sel.obj()), ObjectData::Func(_)) {
            unused_methods.remove(&sel.obj());
        }
    }

    for (obj, entry) in unused_methods {
        let ObjectData::Func(f) = artifacts.objects.get(obj) else {
            continue;
        };
        let start = cm
            .get(pass, entry.fi, DocKind::FieldDoc, entry.pos)
            .map_or(entry.pos, |d| d.pos());
        let end = cm
            .get(pass, entry.fi, DocKind::FieldComment, entry.pos)
            .map_or(entry.end, |c| c.end());
        let mut d = diag(
            entry.pos,
            format!(
                "unusedmethod: method '{}()' is declared on interface '{}' but not used within the package",
                f.name(),
                entry.iface_name
            ),
        );
        d.suggested_fixes = vec![remove_fix("Remove the unused method", start, end)];
        out.push(d);
    }
}

// ---------------------------------------------------------------------------
// Shared type helpers (opaque, unexported)
// ---------------------------------------------------------------------------

/// The package's type arenas.
struct Arenas<'a> {
    types: &'a guff_types::arena::TypeArena,
    objects: &'a guff_types::arena::ObjectArena,
    packages: &'a guff_types::arena::PackageArena,
}

struct TypeCx<'a> {
    pass: &'a Pass<'a>,
    info: &'a Info,
    art: Arenas<'a>,
    pkg: Option<PackageId>,
    /// `types.Universe.Lookup("error").Type()` / `("any")`.
    error_t: Option<TypeId>,
    any_t: Option<TypeId>,
    /// Where each local variable's type was constructed (see [`Origin`]).
    var_srcs: OnceLock<HashMap<ObjectId, VarSrc<'a>>>,
}

impl<'a> TypeCx<'a> {
    fn new(pass: &'a Pass<'a>) -> Option<Self> {
        let art = pass.pkg().type_artifacts.as_ref()?;
        let info = pass.types_info()?;
        let mut error_t = None;
        let mut any_t = None;
        for oid in art.objects.ids() {
            let ObjectData::TypeName(tn) = art.objects.get(oid) else {
                continue;
            };
            if oid.pkg(&art.objects).is_some() {
                continue;
            }
            match tn.name() {
                "error" if error_t.is_none() => error_t = tn.typ(),
                "any" if any_t.is_none() => any_t = tn.typ(),
                _ => {}
            }
            if error_t.is_some() && any_t.is_some() {
                break;
            }
        }
        Some(TypeCx {
            pass,
            info,
            art: Arenas {
                types: &art.types,
                objects: &art.objects,
                packages: &art.packages,
            },
            pkg: pass.type_pkg(),
            error_t,
            any_t,
            var_srcs: OnceLock::new(),
        })
    }

    /// `pass.TypesInfo.TypeOf`: the recorded type, or for an identifier the
    /// type of the object it denotes.
    fn type_of(&self, e: &Expr) -> Option<TypeId> {
        if let Some(t) = type_of(self.info, e) {
            return Some(t);
        }
        let Expr::Ident(id) = e else {
            return None;
        };
        match self.art.objects.get(self.obj_of(id.id)?) {
            ObjectData::Var(v) => Some(v.typ()),
            ObjectData::Const(c) => Some(c.typ()),
            ObjectData::TypeName(tn) => tn.typ(),
            ObjectData::Func(f) => f.typ(),
            _ => None,
        }
    }

    fn data(&self, t: TypeId) -> &TypeData {
        self.art.types.get(t)
    }

    fn is_interface(&self, t: TypeId) -> bool {
        is_interface(self.art.types, t)
    }

    fn identical(&self, x: TypeId, y: Option<TypeId>) -> bool {
        let Some(y) = y else {
            return false;
        };
        let mut types = self.art.types.clone();
        types_identical(&mut types, self.art.objects, self.art.packages, x, y)
    }

    /// `types.Identical(t, error) || types.Identical(t, any)`.
    fn is_error_or_any(&self, t: TypeId) -> bool {
        self.identical(t, self.error_t) || self.identical(t, self.any_t)
    }

    fn is_untyped_nil(&self, t: TypeId) -> bool {
        matches!(self.data(t), TypeData::Basic(b) if b.kind() == BasicKind::UntypedNil)
    }

    fn obj_of(&self, id: u32) -> Option<ObjectId> {
        self.info.uses.get(&id).copied().or_else(|| def_of(self.info, id))
    }

    /// `types.TypeString(t, nil)`: every package qualified by its path.
    fn type_string(&self, t: TypeId) -> String {
        guff_types::typestring::type_string(
            self.art.types,
            self.art.objects,
            self.art.packages,
            t,
            None,
        )
    }

    /// `types.TypeString(t, q)` with unexported's qualifier: nothing for the
    /// package under analysis, the package *name* for the others.
    fn type_string_local(&self, t: Option<TypeId>) -> String {
        let Some(t) = t else {
            return "<nil>".to_string();
        };
        let current = self.pkg;
        let qf = move |p: PackageId, parena: &guff_types::arena::PackageArena| {
            if Some(p) == current {
                String::new()
            } else {
                parena.get(p).name().to_string()
            }
        };
        guff_types::typestring::type_string(
            self.art.types,
            self.art.objects,
            self.art.packages,
            t,
            Some(&qf),
        )
    }

    /// opaque's `fromSamePackage`.
    fn from_same_package(&self, t: TypeId) -> bool {
        match self.data(t) {
            TypeData::Named(n) => n.obj().pkg(self.art.objects) == self.pkg,
            TypeData::Pointer(p) => self.from_same_package(p.elem()),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// opaque
// ---------------------------------------------------------------------------

/// Where a type value was constructed.
///
/// opaque collects the types it sees in a `map[types.Type]struct{}`, so two
/// return statements agree only when they hold the *same* `types.Type`.
/// go/types shares a `*Named` or `*Basic` everywhere, but builds a fresh
/// `*Pointer` (`*Slice`, …) for every `&x`, every `new(T)` and every type
/// expression it evaluates — `return &s{}` twice is two implementations to
/// upstream. guff hash-conses those types, so the key pairs the type with
/// the construction site go/types would have used.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Origin {
    /// A shared type (`Basic`, `Named`, `Alias`, `TypeParam`).
    Canon,
    /// Built by the node with this id.
    Fresh(u32),
    /// The declared type of this object (a func's signature, a field's type).
    Obj(ObjectId),
    /// The i'th result of a call to a callee of this origin.
    Result(Box<Origin>, usize),
    /// An element type of a container of this origin.
    Elem(Box<Origin>),
}

/// How a local variable got its type.
#[derive(Clone, Copy)]
enum VarSrc<'a> {
    /// From a type expression evaluated for it (or for its field).
    Fresh(u32),
    /// From its initializer.
    Expr(&'a Expr),
    /// From the i'th result of a call.
    Result(&'a Expr, usize),
    /// From the element type of a ranged-over value.
    Elem(&'a Expr),
}

type TypeKey = (TypeId, Origin);

const ORIGIN_DEPTH: u32 = 32;

impl<'a> TypeCx<'a> {
    fn is_shared(&self, t: TypeId) -> bool {
        matches!(
            self.data(t),
            TypeData::Basic(_) | TypeData::Named(_) | TypeData::Alias(_) | TypeData::TypeParam(_)
        )
    }

    fn key(&self, t: TypeId, origin: impl FnOnce() -> Origin) -> TypeKey {
        if self.is_shared(t) {
            (t, Origin::Canon)
        } else {
            (t, origin())
        }
    }

    fn var_srcs(&self) -> &HashMap<ObjectId, VarSrc<'a>> {
        self.var_srcs.get_or_init(|| {
            let mut out = HashMap::new();
            let files: &'a [File] = self.pass.files();
            for file in files {
                collect_var_srcs(self.info, file, &mut out);
            }
            out
        })
    }

    fn var_origin(&self, v: ObjectId, depth: u32) -> Origin {
        match self.var_srcs().get(&v) {
            Some(VarSrc::Fresh(id)) => Origin::Fresh(*id),
            Some(VarSrc::Expr(e)) => self.origin(e, depth + 1),
            Some(VarSrc::Result(fun, i)) => {
                Origin::Result(Box::new(self.origin(fun, depth + 1)), *i)
            }
            Some(VarSrc::Elem(x)) => Origin::Elem(Box::new(self.origin(x, depth + 1))),
            None => Origin::Obj(v),
        }
    }

    fn obj_origin(&self, obj: ObjectId, depth: u32) -> Origin {
        match self.art.objects.get(obj) {
            ObjectData::Var(_) => self.var_origin(obj, depth),
            ObjectData::Const(_) => Origin::Canon,
            _ => Origin::Obj(obj),
        }
    }

    fn origin(&self, e: &Expr, depth: u32) -> Origin {
        if depth > ORIGIN_DEPTH {
            return Origin::Fresh(e.id());
        }
        let next = depth + 1;
        match e {
            Expr::ParenExpr(p) => self.origin(&p.x, next),
            Expr::UnaryExpr(u) if u.op == Token::ARROW => {
                Origin::Elem(Box::new(self.origin(&u.x, next)))
            }
            Expr::StarExpr(s) => Origin::Elem(Box::new(self.origin(&s.x, next))),
            Expr::Ident(id) => match self.obj_of(id.id) {
                Some(obj) => self.obj_origin(obj, next),
                None => Origin::Fresh(e.id()),
            },
            Expr::SelectorExpr(s) => {
                if let Some(sel) = self.info.selections.get(&s.id) {
                    return Origin::Obj(sel.obj());
                }
                match self.obj_of(s.sel.id) {
                    Some(obj) => self.obj_origin(obj, next),
                    None => Origin::Fresh(e.id()),
                }
            }
            Expr::CallExpr(c) => match self.info.types.get(&c.fun.id()).map(|tv| tv.mode) {
                // A conversion: the type the type expression evaluates to.
                Some(OperandMode::TypeExpr) => Origin::Fresh(c.fun.id()),
                Some(OperandMode::Builtin) => {
                    let name = match strip_parens(&c.fun) {
                        Expr::Ident(id) => self
                            .obj_of(id.id)
                            .and_then(|o| match self.art.objects.get(o) {
                                ObjectData::Builtin(b) => Some(b.name().to_string()),
                                _ => None,
                            }),
                        _ => None,
                    };
                    match (name.as_deref(), c.args.first()) {
                        (Some("append"), Some(first)) => self.origin(first, next),
                        _ => Origin::Fresh(e.id()),
                    }
                }
                _ => Origin::Result(Box::new(self.origin(&c.fun, next)), 0),
            },
            Expr::IndexExpr(ix) => {
                // `f[int]` names an instantiated function; anything else
                // indexes a container.
                let is_func = self.type_of(&ix.x)
                    .is_some_and(|t| matches!(self.data(t), TypeData::Signature(_)));
                if is_func {
                    self.origin(&ix.x, next)
                } else {
                    Origin::Elem(Box::new(self.origin(&ix.x, next)))
                }
            }
            Expr::IndexListExpr(ix) => self.origin(&ix.x, next),
            Expr::SliceExpr(s) => {
                let of_slice = self.type_of(&s.x).is_some_and(|t| {
                    matches!(self.data(t.underlying(self.art.types)), TypeData::Slice(_))
                });
                if of_slice {
                    self.origin(&s.x, next)
                } else {
                    Origin::Fresh(e.id())
                }
            }
            Expr::TypeAssertExpr(t) => match &t.ty {
                Some(ty) => Origin::Fresh(ty.id()),
                None => Origin::Fresh(e.id()),
            },
            Expr::CompositeLit(c) => match &c.ty {
                Some(ty) => Origin::Fresh(ty.id()),
                None => Origin::Fresh(e.id()),
            },
            _ => Origin::Fresh(e.id()),
        }
    }
}

fn strip_parens(e: &Expr) -> &Expr {
    match e {
        Expr::ParenExpr(p) => strip_parens(&p.x),
        _ => e,
    }
}

/// Record, for each variable `file` declares, where go/types took its type
/// from (`collectParams` shares one type among a field's names; `varDecl`
/// evaluates a `var` spec's type once per name).
fn collect_var_srcs<'a>(info: &Info, file: &'a File, out: &mut HashMap<ObjectId, VarSrc<'a>>) {
    fn fields<'a>(info: &Info, list: Option<&'a FieldList>, out: &mut HashMap<ObjectId, VarSrc<'a>>) {
        let Some(list) = list else {
            return;
        };
        for field in &list.list {
            let Some(ty) = &field.ty else {
                continue;
            };
            for name in &field.names {
                if let Some(obj) = def_of(info, name.id) {
                    out.insert(obj, VarSrc::Fresh(ty.id()));
                }
            }
        }
    }
    fn values<'a>(
        info: &Info,
        names: impl Iterator<Item = (usize, u32)>,
        rhs: &'a [Expr],
        n: usize,
        out: &mut HashMap<ObjectId, VarSrc<'a>>,
    ) {
        for (i, id) in names {
            let Some(obj) = def_of(info, id) else {
                continue;
            };
            let src = if rhs.len() == n {
                VarSrc::Expr(&rhs[i])
            } else if rhs.len() == 1 {
                match &rhs[0] {
                    Expr::CallExpr(c) => VarSrc::Result(&c.fun, i),
                    // `v, ok := m[k]` and the other comma-ok forms.
                    e if i == 0 => VarSrc::Expr(e),
                    _ => continue,
                }
            } else {
                continue;
            };
            out.insert(obj, src);
        }
    }

    walk::inspect(NodeRef::File(file), |n| {
        match n {
            Some(NodeRef::FuncDecl(fd)) => {
                fields(info, fd.recv.as_ref(), out);
            }
            Some(NodeRef::FuncType(ft)) => {
                fields(info, ft.params.as_ref(), out);
                fields(info, ft.results.as_ref(), out);
            }
            Some(NodeRef::ValueSpec(vs)) => {
                if vs.ty.is_some() {
                    for name in &vs.names {
                        if let Some(obj) = def_of(info, name.id) {
                            out.insert(obj, VarSrc::Fresh(name.id));
                        }
                    }
                } else {
                    let ids = vs.names.iter().map(|n| n.id).enumerate();
                    values(info, ids, &vs.values, vs.names.len(), out);
                }
            }
            Some(NodeRef::AssignStmt(a)) if a.tok == Some(Token::DEFINE) => {
                let ids = a.lhs.iter().enumerate().filter_map(|(i, l)| match l {
                    Expr::Ident(id) => Some((i, id.id)),
                    _ => None,
                });
                values(info, ids, &a.rhs, a.lhs.len(), out);
            }
            Some(NodeRef::RangeStmt(r)) if r.tok == Some(Token::DEFINE) => {
                if let Some(Expr::Ident(v)) = &r.value {
                    if let Some(obj) = def_of(info, v.id) {
                        out.insert(obj, VarSrc::Elem(&r.x));
                    }
                }
            }
            _ => {}
        }
        true
    });
}

/// opaque's `positionStr`.
fn position_str(idx: usize) -> String {
    match idx {
        0 => "1st".to_string(),
        1 => "2nd".to_string(),
        2 => "3rd".to_string(),
        _ => format!("{}th", idx + 1),
    }
}

/// opaque's `removePkgPrefix`: everything after the last `.`, keeping
/// leading `*`s.
fn remove_pkg_prefix(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    if let Some(rest) = s.strip_prefix('*') {
        return format!("*{}", remove_pkg_prefix(rest));
    }
    match s.rfind('.') {
        Some(i) => s[i + 1..].to_string(),
        None => s.to_string(),
    }
}

/// An insertion-ordered set of [`TypeKey`]s (`map[types.Type]struct{}`).
#[derive(Default)]
struct KeySet(Vec<TypeKey>);

impl KeySet {
    fn insert(&mut self, k: TypeKey) {
        if !self.0.contains(&k) {
            self.0.push(k);
        }
    }
}

fn check_opaque(cx: &TypeCx<'_>, cm: &Comments, out: &mut Vec<Diagnostic>) {
    let pass = cx.pass;
    let info = cx.info;
    for (fi, file) in pass.files().iter().enumerate() {
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            check_opaque_func(cx, cm, fi, fd, info, out);
        }
    }
}

fn check_opaque_func(
    cx: &TypeCx<'_>,
    cm: &Comments,
    fi: usize,
    fd: &FuncDecl,
    info: &Info,
    out: &mut Vec<Diagnostic>,
) {
    // Methods are skipped: an interface the receiver satisfies may dictate
    // the return type.
    if fd.recv.is_some() {
        return;
    }
    let Some(body) = &fd.body else {
        return;
    };
    let Some(results) = &fd.ty.results else {
        return;
    };
    if cm.ignores(cx.pass, fi, DocKind::FuncDecl, fd.name.pos(), CHECK_OPAQUE) {
        return;
    }

    let mut has_interface_return = false;
    let mut out_count = 0usize;
    let mut named_returns: HashMap<ObjectId, usize> = HashMap::new();
    let mut idx = 0usize;
    for result in &results.list {
        for name in &result.names {
            if let Some(obj) = def_of(info, name.id) {
                if matches!(cx.art.objects.get(obj), ObjectData::Var(_)) {
                    named_returns.insert(obj, idx);
                }
            }
            idx += 1;
        }
        if result.names.is_empty() {
            idx += 1;
        }
        out_count += result.names.len().max(1);
        if !has_interface_return {
            has_interface_return = result
                .ty
                .as_ref()
                .and_then(|t| cx.type_of(t))
                .is_some_and(|t| cx.is_interface(t));
        }
    }
    if !has_interface_return {
        return;
    }

    // The types seen on every return statement, by result position.
    let mut ret_stmt_types: Vec<KeySet> = (0..out_count).map(|_| KeySet::default()).collect();
    let add = |sets: &mut Vec<KeySet>, i: usize, k: TypeKey| {
        if let Some(set) = sets.get_mut(i) {
            set.insert(k);
        }
    };
    let named_var = |id: u32| -> Option<usize> {
        let obj = info.uses.get(&id)?;
        named_returns.get(obj).copied()
    };

    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        match n {
            NodeRef::FuncLit(_) => false,
            NodeRef::AssignStmt(a) => {
                if a.tok != Some(Token::ASSIGN) {
                    return true;
                }
                for (i, lhs) in a.lhs.iter().enumerate() {
                    let Expr::Ident(ident) = lhs else {
                        continue;
                    };
                    let Some(obj) = info.uses.get(&ident.id) else {
                        continue;
                    };
                    if !matches!(cx.art.objects.get(*obj), ObjectData::Var(_)) {
                        continue;
                    }
                    let Some(&pos) = named_returns.get(obj) else {
                        continue;
                    };
                    let key = if a.rhs.len() == 1 {
                        let rhs = &a.rhs[0];
                        match cx.type_of(rhs) {
                            Some(t) => match cx.data(t) {
                                TypeData::Tuple(tuple) => {
                                    if i < tuple.len() {
                                        let ObjectData::Var(v) = cx.art.objects.get(tuple.at(i))
                                        else {
                                            continue;
                                        };
                                        let et = v.typ();
                                        Some(cx.key(et, || match rhs {
                                            Expr::CallExpr(c) => Origin::Result(
                                                Box::new(cx.origin(&c.fun, 0)),
                                                i,
                                            ),
                                            _ if i == 0 => cx.origin(rhs, 0),
                                            _ => Origin::Canon,
                                        }))
                                    } else {
                                        None
                                    }
                                }
                                _ => Some(cx.key(t, || cx.origin(rhs, 0))),
                            },
                            None => None,
                        }
                    } else if i < a.rhs.len() {
                        cx.type_of(&a.rhs[i]).map(|t| cx.key(t, || cx.origin(&a.rhs[i], 0)))
                    } else {
                        None
                    };
                    if let Some(key) = key {
                        if !cx.is_untyped_nil(key.0) {
                            add(&mut ret_stmt_types, pos, key);
                        }
                    }
                }
                true
            }
            NodeRef::ReturnStmt(r) => {
                for (i, result) in r.results.iter().enumerate() {
                    match result {
                        Expr::CallExpr(c) => {
                            // A multi-value call records each result by position.
                            let Some(t) = cx.type_of(result) else {
                                continue;
                            };
                            match cx.data(t) {
                                TypeData::Tuple(tuple) => {
                                    for j in 0..tuple.len() {
                                        let ObjectData::Var(v) = cx.art.objects.get(tuple.at(j))
                                        else {
                                            continue;
                                        };
                                        let key = cx.key(v.typ(), || {
                                            Origin::Result(Box::new(cx.origin(&c.fun, 0)), j)
                                        });
                                        add(&mut ret_stmt_types, j, key);
                                    }
                                }
                                _ => {
                                    let key = cx.key(t, || cx.origin(result, 0));
                                    add(&mut ret_stmt_types, i, key);
                                }
                            }
                        }
                        Expr::Ident(ident) => {
                            // A bare named result contributes what was assigned to it.
                            if named_var(ident.id).is_some() {
                                continue;
                            }
                            let Some(t) = cx.type_of(result) else {
                                continue;
                            };
                            if !cx.is_untyped_nil(t) {
                                let key = cx.key(t, || cx.origin(result, 0));
                                add(&mut ret_stmt_types, i, key);
                            }
                        }
                        _ => {
                            let Some(t) = cx.type_of(result) else {
                                continue;
                            };
                            let key = cx.key(t, || cx.origin(result, 0));
                            add(&mut ret_stmt_types, i, key);
                        }
                    }
                }
                false
            }
            _ => true,
        }
    });

    // Compare each interface result with what the return statements hold.
    let mut next_idx = 0usize;
    for result in &results.list {
        let consume = result.names.len().max(1);
        let current = next_idx;
        next_idx += consume;

        let Some(res_ty) = &result.ty else {
            continue;
        };
        let Some(typ) = cx.type_of(res_ty) else {
            continue;
        };
        if !cx.is_interface(typ) {
            continue;
        }
        if cx.is_error_or_any(typ) {
            continue;
        }
        if !cx.from_same_package(typ) {
            // An interface of another package.
            continue;
        }
        let Some(set) = ret_stmt_types.get(current) else {
            continue;
        };
        // More than one implementation, or a bare `return` of a named result
        // nothing was assigned to.
        if set.0.len() != 1 {
            continue;
        }
        let stmt_typ = set.0[0].0;
        if cx.is_interface(stmt_typ) {
            continue;
        }
        match cx.data(stmt_typ) {
            TypeData::Basic(b) if b.kind() == BasicKind::UntypedNil => continue,
            TypeData::Named(_) => {
                let under = stmt_typ.underlying(cx.art.types);
                if matches!(cx.data(under), TypeData::Signature(_)) {
                    continue;
                }
            }
            _ => {}
        }

        let mut ret_type_name = cx.type_string(typ);
        if cx.from_same_package(typ) {
            ret_type_name = remove_pkg_prefix(&ret_type_name);
        }
        let mut stmt_typ_name = cx.type_string(stmt_typ);
        if cx.from_same_package(stmt_typ) {
            stmt_typ_name = remove_pkg_prefix(&stmt_typ_name);
        }

        let mut d = diag(
            res_ty.pos(),
            format!(
                "opaque: '{}' function return '{}' interface at the {} result, abstract a single concrete implementation of '{}'",
                fd.name.name,
                ret_type_name,
                position_str(current),
                stmt_typ_name
            ),
        );
        d.suggested_fixes = vec![SuggestedFix {
            message: "Replace the interface return type with the concrete type".to_string(),
            text_edits: vec![TextEdit {
                pos: res_ty.pos().0 as u32,
                end: res_ty.end().0 as u32,
                new_text: stmt_typ_name,
            }],
        }];
        out.push(d);
    }
}

// ---------------------------------------------------------------------------
// unexported
// ---------------------------------------------------------------------------

/// unexported's `findIdent`: unwrap pointers, variadics, containers and
/// instantiations down to the named type.
fn find_ident(mut e: &Expr) -> Option<&Expr> {
    loop {
        e = match e {
            Expr::StarExpr(s) => &s.x,
            Expr::Ellipsis(el) => el.elt.as_deref()?,
            Expr::ArrayType(a) => &a.elt,
            Expr::IndexExpr(ix) => &ix.x,
            Expr::IndexListExpr(ix) => &ix.x,
            Expr::ChanType(c) => &c.value,
            Expr::MapType(m) => &m.value,
            Expr::Ident(_) | Expr::SelectorExpr(_) => return Some(e),
            _ => return None,
        };
    }
}

/// unexported's `formatType`.
fn format_type(cx: &TypeCx<'_>, expr: &Expr, info_type: Option<TypeId>) -> String {
    if let Expr::Ellipsis(el) = expr {
        let elem = el.elt.as_deref().and_then(|e| cx.type_of(e));
        return format!("...{}", cx.type_string_local(elem));
    }
    cx.type_string_local(info_type)
}

fn check_unexported(cx: &TypeCx<'_>, cm: &Comments, out: &mut Vec<Diagnostic>) {
    let pass = cx.pass;
    for (fi, file) in pass.files().iter().enumerate() {
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            if cm.ignores(pass, fi, DocKind::FuncDecl, fd.name.pos(), CHECK_UNEXPORTED) {
                continue;
            }
            let recv_name = fd
                .recv
                .as_ref()
                .and_then(|r| r.list.first())
                .and_then(|f| f.ty.as_ref())
                .map(|t| recv_name(cx, t))
                .unwrap_or_default();
            if !fd.name.is_exported() {
                continue;
            }
            for (list, role) in [
                (fd.ty.params.as_ref(), "parameter"),
                (fd.ty.results.as_ref(), "return value"),
            ] {
                let Some(list) = list else {
                    continue;
                };
                for field in &list.list {
                    if let Some(ty) = &field.ty {
                        check_unexported_type(cx, ty, fd, &recv_name, role, out);
                    }
                }
            }
        }
    }
}

fn check_unexported_type(
    cx: &TypeCx<'_>,
    expr: &Expr,
    fd: &FuncDecl,
    recv_name: &str,
    role: &str,
    out: &mut Vec<Diagnostic>,
) {
    let info_type = cx.type_of(expr);
    let Some(ident) = find_ident(expr) else {
        return;
    };
    let Expr::Ident(id) = ident else {
        // A type of another package.
        return;
    };
    if id.is_exported() {
        return;
    }
    let Some(ident_type) = cx.type_of(ident) else {
        return;
    };
    if cx.is_error_or_any(ident_type) {
        return;
    }
    if !cx.is_interface(ident_type) {
        return;
    }
    let (kind, name) = if recv_name.is_empty() {
        ("function", fd.name.name.clone())
    } else {
        ("method", format!("{recv_name}.{}", fd.name.name))
    };
    let type_str = format_type(cx, expr, info_type);
    out.push(diag(
        id.pos(),
        format!("unexported: unexported interface '{type_str}' used as {role} in exported {kind} '{name}'"),
    ));
}

/// unexported's `recvName`.
fn recv_name(cx: &TypeCx<'_>, recv: &Expr) -> String {
    let inner = match recv {
        Expr::StarExpr(s) => &*s.x,
        e => e,
    };
    let Some(info_type) = cx.type_of(inner) else {
        return String::new();
    };
    format_type(cx, inner, Some(info_type))
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "iface requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<IfaceOptions>("iface")
        .cloned()
        .unwrap_or_default();
    let checks = enabled_checks(&opts);
    if checks.is_empty() {
        return Ok(None);
    }

    let mut pending = Vec::new();
    {
        let pass: &Pass<'_> = pass;
        let cm = Comments::new(pass);
        if checks.contains(CHECK_IDENTICAL) {
            check_identical(pass, &cm, &mut pending);
        }
        if checks.contains(CHECK_UNUSED) {
            check_unused(pass, &cm, &opts.unused_exclude, &mut pending);
        }
        if checks.contains(CHECK_UNUSEDMETHOD) {
            check_unusedmethod(pass, &cm, &opts.unusedmethod_exclude, &mut pending);
        }
        if checks.contains(CHECK_OPAQUE) || checks.contains(CHECK_UNEXPORTED) {
            if let Some(cx) = TypeCx::new(pass) {
                if checks.contains(CHECK_OPAQUE) {
                    check_opaque(&cx, &cm, &mut pending);
                }
                if checks.contains(CHECK_UNEXPORTED) {
                    check_unexported(&cx, &cm, &mut pending);
                }
            }
        }
    }

    for d in pending {
        pass.report(d);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "iface",
        doc: "detects incorrect use of interfaces (interface pollution)",
        url: "https://github.com/uudashr/iface",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(lines: &[&str]) -> CommentGroup {
        CommentGroup {
            list: lines
                .iter()
                .map(|t| guff::ast::Comment {
                    slash: Pos(1),
                    text: t.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn directive_parse_ignore() {
        assert_eq!(parse_ignore(None), None);
        assert_eq!(parse_ignore(Some(&group(&["// hello"]))), None);
        assert_eq!(parse_ignore(Some(&group(&["//iface:ignore"]))), Some(vec![]));
        assert_eq!(parse_ignore(Some(&group(&["//iface:ignore= "]))), Some(vec![]));
        assert_eq!(
            parse_ignore(Some(&group(&["// doc", "//iface:ignore=unused, identical"]))),
            Some(vec!["unused".to_string(), "identical".to_string()])
        );
        // Only a line of its own: `// iface:ignore` is prose.
        assert_eq!(parse_ignore(Some(&group(&["// iface:ignore"]))), None);
        assert!(should_ignore(Some(&group(&["//iface:ignore=unused"])), "unused"));
        assert!(!should_ignore(Some(&group(&["//iface:ignore=unused"])), "identical"));
        assert!(should_ignore(Some(&group(&["//iface:ignore"])), "opaque"));
    }

    #[test]
    fn exclude_flag_is_trimmed_and_drops_empties() {
        assert!(split_exclude("").is_empty());
        assert_eq!(split_exclude(" a , ,b,"), vec!["a", "b"]);
    }

    #[test]
    fn opaque_helpers() {
        assert_eq!(position_str(0), "1st");
        assert_eq!(position_str(3), "4th");
        assert_eq!(remove_pkg_prefix("*example.com/z.DoerImpl"), "*DoerImpl");
        assert_eq!(remove_pkg_prefix("int"), "int");
        assert_eq!(remove_pkg_prefix(""), "");
    }
}
