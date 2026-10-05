//! Port of [`dev.gaijin.team/go/exhaustruct/v5`](https://github.com/GaijinEntertainment/go-exhaustruct)
//! v5.2.0 (golangci-lint wrapper `pkg/golinters/exhaustruct/exhaustruct_v5.go`,
//! linter `exhaustruct_v5`).
//!
//! v5 is a rewrite of the v4 analyzer in [`crate::exhaustruct`], not a patch on
//! it: optionality comes from `//exhaustruct:` comment directives instead of the
//! `exhaustruct:"optional"` tag (which is now reported, with a fix), patterns
//! can name a field as `Type#Field`, embedded structs are opened, `_` is never
//! required by name, and a use site can ignore or enforce one literal.
//!
//! The layout follows upstream's packages:
//!
//! | upstream | here |
//! |----------|------|
//! | `internal/directive` (`Parse`, `Scanner`) | [`parse_directive`], [`Scanner`], [`scan_file`] |
//! | `internal/astutil` (`FileParser`) | [`Scanner::lookup`] / [`load_file_directives`] |
//! | `internal/pattern` | [`PatternList`] |
//! | `internal/structure` | [`Processor`], [`StructMeta`] |
//! | `analyzer/missing-fields-visitor.go` | [`LiteralVisitor`] |
//! | `analyzer/tag-migration-visitor.go` | [`visit_struct_type`] and the tag helpers |
//!
//! guff loads analysis files without comments, so a package's own files are
//! reparsed for their directives, and a dependency's directives are read from
//! its source on disk the first time a type declared there is resolved —
//! upstream does the same for dependencies, and keeps one cache for the run.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use guff::ast::{CompositeLit, Expr, Field as AstField, File, ReturnStmt, StructType};
use guff::commentmap::{node_end, node_pos};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::{FileSet, Pos, NO_POS};
use guff::token::Token;
use guff::walk::{self, expr_ref, NodeRef};
use guff_analysis::code::{effective_file_go_version, version_compare};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_gostd::strconv;
use guff_types::alias::unalias_readonly;
use guff_types::api_predicates::{api_identical, api_implements};
use guff_types::arena::{
    ObjectArena, ObjectData, ObjectId, PackageArena, PackageId, ScopeArena, TypeArena, TypeData,
};
use guff_types::named::named_origin;
use guff_types::object::is_exported;
use guff_types::TypeId;
use regex::Regex;

use crate::options::ExhaustructV5Options;

// ============================================================================
// internal/directive — Parse
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Directive {
    Ignore,
    Enforce,
    Optional,
}

impl Directive {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "ignore" => Some(Directive::Ignore),
            "enforce" => Some(Directive::Enforce),
            "optional" => Some(Directive::Optional),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
struct Optionality {
    optional: bool,
    enforced: bool,
}

fn optionality(ds: &[Directive]) -> Optionality {
    Optionality {
        optional: ds.contains(&Directive::Optional),
        enforced: ds.contains(&Directive::Enforce),
    }
}

const DIRECTIVE_PREFIX: &str = "exhaustruct:";
const PROSE_PREFIX: &[char] = &[' ', '\t', '\n'];

fn comment_body(text: &str) -> &str {
    if let Some(body) = text.strip_prefix("//") {
        return body;
    }
    if let Some(body) = text.strip_prefix("/*") {
        return body.strip_suffix("*/").unwrap_or(body);
    }
    text
}

/// `directive.Parse`: `None` when the comment is not a directive; otherwise the
/// directives it names and the errors it carries, already rendered the way
/// golib's `e.Err` prints them (`reason (key=value)`).
fn parse_directive(text: &str) -> Option<(Vec<Directive>, Vec<String>)> {
    let text = comment_body(text).strip_prefix(DIRECTIVE_PREFIX)?;
    let (list, prose) = match text.find(PROSE_PREFIX) {
        Some(i) => (&text[..i], &text[i..]),
        None => (text, ""),
    };
    if list.is_empty() {
        return Some((Vec::new(), vec!["empty directive".to_string()]));
    }
    let (result, mut errs) = parse_parts(list);
    if result.is_empty() && errs.is_empty() {
        errs.push("empty directive".to_string());
    }
    if list.ends_with(',') {
        let word = first_word(prose);
        if Directive::from_str(word).is_some() {
            errs.push(format!("directive after a space is not read (directive={word})"));
        }
    }
    Some((result, errs))
}

fn first_word(prose: &str) -> &str {
    let prose = prose.trim_start_matches(PROSE_PREFIX);
    match prose.find([' ', '\t', '\n', ',']) {
        Some(i) => &prose[..i],
        None => prose,
    }
}

fn parse_parts(text: &str) -> (Vec<Directive>, Vec<String>) {
    let mut result = Vec::new();
    let mut errs = Vec::new();
    let mut has_dups = false;
    for part in text.split(',') {
        if part.is_empty() {
            continue;
        }
        let Some(d) = Directive::from_str(part) else {
            errs.push(format!("unknown directive (directive={part})"));
            continue;
        };
        if result.contains(&d) {
            has_dups = true;
            continue;
        }
        result.push(d);
    }
    if has_dups {
        errs.push("duplicate directives".to_string());
    }
    (result, errs)
}

// ============================================================================
// internal/directive — Scanner (one file)
// ============================================================================

/// Directives of one file, by the physical line they target.
type LineDirectives = HashMap<i64, Vec<Directive>>;

struct ParsedDirective {
    pos: Pos,
    line: i64,
    end_line: i64,
    target_line: i64,
    directives: Vec<Directive>,
}

impl ParsedDirective {
    fn sharable_lines(&self) -> [i64; 2] {
        [self.line, self.end_line]
    }

    fn code_line(&self, code_lines: &HashMap<i64, i64>) -> Option<i64> {
        self.sharable_lines()
            .iter()
            .find_map(|l| code_lines.get(l).copied())
    }
}

/// `ast.Node.Pos()`. [`node_pos`] starts a field at its doc comment, which Go's
/// `Field.Pos()` does not.
fn go_pos(n: NodeRef<'_>) -> Pos {
    match n {
        NodeRef::Field(f) => f.pos(),
        n => node_pos(n),
    }
}

/// `ast.Node.End()`. [`node_end`] ends a field at its line comment.
fn go_end(n: NodeRef<'_>) -> Pos {
    match n {
        NodeRef::Field(f) => f.end(),
        n => node_end(n),
    }
}

/// `astutil.PhysicalLine`: the line on disk, whatever a `//line` directive says.
fn physical_line(fset: &FileSet, pos: Pos) -> i64 {
    fset.position_for(pos, false).line
}

/// `Scanner.parseFileDirectives` on a file parsed with comments. Diagnostics
/// are positioned in `fset`.
fn scan_file(fset: &FileSet, file: &File) -> (LineDirectives, Vec<(Pos, String)>) {
    let (mut directives, mut diags) = parse_comment_directives(fset, file);
    if directives.is_empty() {
        return (LineDirectives::new(), diags);
    }
    let asked: HashSet<i64> = directives
        .values()
        .flatten()
        .flat_map(|d| d.sharable_lines())
        .collect();
    let code = code_lines(fset, file, &asked);
    for ds in directives.values_mut() {
        for d in ds.iter_mut() {
            if let Some(line) = d.code_line(&code) {
                d.target_line = line;
            }
        }
    }
    let (result, conflicts) = reduce_by_target(directives);
    diags.extend(conflicts);
    (result, diags)
}

fn parse_comment_directives(
    fset: &FileSet,
    file: &File,
) -> (HashMap<i64, Vec<ParsedDirective>>, Vec<(Pos, String)>) {
    let mut directives: HashMap<i64, Vec<ParsedDirective>> = HashMap::new();
    let mut diags = Vec::new();
    for cg in &file.comments {
        let mut has_directive = false;
        for c in &cg.list {
            let Some((parsed, errs)) = parse_directive(&c.text) else {
                continue;
            };
            let pos = c.pos();
            for err in errs {
                diags.push((pos, err));
            }
            if parsed.is_empty() {
                continue;
            }
            if has_directive {
                diags.push((
                    pos,
                    "multiple exhaustruct directives in a single comment group, ignoring".to_string(),
                ));
                continue;
            }
            has_directive = true;
            let line = physical_line(fset, pos);
            directives.entry(line).or_default().push(ParsedDirective {
                pos,
                line,
                end_line: physical_line(fset, c.end()),
                // The whole group carries the directive down to the code, so a
                // note written under it does not cut the reach short.
                target_line: physical_line(fset, cg.end()) + 1,
                directives: parsed,
            });
        }
    }
    (directives, diags)
}

/// `codeLines`: for each asked line holding code, the line a directive beside
/// it targets — itself, or for the line a multiline construct closes on, the
/// latest line such a construct opened on.
fn code_lines(fset: &FileSet, file: &File, asked: &HashSet<i64>) -> HashMap<i64, i64> {
    let mut lines: HashMap<i64, i64> = HashMap::new();
    walk::inspect(NodeRef::File(file), |n| {
        let Some(n) = n else {
            return true;
        };
        if matches!(n, NodeRef::Comment(_) | NodeRef::CommentGroup(_)) {
            return false;
        }
        let start = physical_line(fset, go_pos(n));
        let end = physical_line(fset, go_end(n));
        if end != start && asked.contains(&end) {
            match lines.get(&end) {
                Some(&target) if start <= target => {}
                _ => {
                    lines.insert(end, start);
                }
            }
        }
        if asked.contains(&start) {
            lines.insert(start, start);
        }
        true
    });
    lines
}

fn reduce_by_target(
    directives: HashMap<i64, Vec<ParsedDirective>>,
) -> (LineDirectives, Vec<(Pos, String)>) {
    let mut by_target: HashMap<i64, Vec<ParsedDirective>> = HashMap::new();
    for (_, ds) in directives {
        for d in ds {
            by_target.entry(d.target_line).or_default().push(d);
        }
    }
    let mut diags = Vec::new();
    let mut result = LineDirectives::new();
    for (target, mut ds) in by_target {
        ds.sort_by_key(|d| d.pos.0);
        let mut it = ds.into_iter();
        if let Some(first) = it.next() {
            result.insert(target, first.directives);
        }
        for d in it {
            diags.push((
                d.pos,
                "directive ignored, conflicting directive already exists for the same target line"
                    .to_string(),
            ));
        }
    }
    (result, diags)
}

// ============================================================================
// internal/astutil — files read from disk, cached for the run
// ============================================================================

struct CachedFile {
    stamp: Option<(u64, SystemTime)>,
    directives: Arc<LineDirectives>,
    types: Arc<TypeDecls>,
}

/// The type declarations of one file: for each name, the physical line it is
/// declared on and, where the declaration spells a struct, the line of each of
/// its fields in `types.Struct` order.
type TypeDecls = HashMap<String, TypeDecl>;

struct TypeDecl {
    name_line: i64,
    struct_fields: Option<Vec<i64>>,
}

/// Indexes the package-level type declarations of a parsed file.
fn index_type_decls(fset: &FileSet, file: &File) -> TypeDecls {
    let mut out = TypeDecls::new();
    for decl in &file.decls {
        let guff::ast::Decl::GenDecl(gen) = decl else {
            continue;
        };
        for spec in &gen.specs {
            let guff::ast::Spec::TypeSpec(ts) = spec else {
                continue;
            };
            let struct_fields = match unparen(&ts.ty) {
                Expr::StructType(st) => Some(
                    st.fields
                        .list
                        .iter()
                        .flat_map(|f| {
                            if f.names.is_empty() {
                                vec![physical_line(fset, f.pos())]
                            } else {
                                f.names.iter().map(|n| physical_line(fset, n.pos())).collect()
                            }
                        })
                        .collect(),
                ),
                _ => None,
            };
            out.insert(
                ts.name.name.clone(),
                TypeDecl {
                    name_line: physical_line(fset, ts.name.pos()),
                    struct_fields,
                },
            );
        }
    }
    out
}

/// Upstream keeps one `FileParser` per analyzer, shared by every package of the
/// run, so a dependency file is parsed once. Entries carry the file's size and
/// mtime, since a guff process can outlive one run.
fn file_cache() -> &'static Mutex<HashMap<String, CachedFile>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedFile>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn file_stamp(path: &str) -> Option<(u64, SystemTime)> {
    let md = std::fs::metadata(path).ok()?;
    Some((md.len(), md.modified().ok()?))
}

fn goroot() -> Option<&'static str> {
    static GOROOT: OnceLock<Option<String>> = OnceLock::new();
    GOROOT
        .get_or_init(|| {
            if let Ok(v) = std::env::var("GOROOT") {
                if !v.is_empty() {
                    return Some(v);
                }
            }
            let out = std::process::Command::new("go")
                .args(["env", "GOROOT"])
                .output()
                .ok()?;
            let v = String::from_utf8(out.stdout).ok()?.trim().to_string();
            (!v.is_empty()).then_some(v)
        })
        .as_deref()
}

fn has_path_prefix(path: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    if prefix.is_empty() {
        return false;
    }
    match path.strip_prefix(prefix) {
        Some(rest) => rest.is_empty() || rest.starts_with('/'),
        None => false,
    }
}

/// `isGoRootFile`: the standard library is never read for directives.
pub(crate) fn is_goroot_file(filename: &str) -> bool {
    has_path_prefix(filename, "$GOROOT") || goroot().is_some_and(|g| has_path_prefix(filename, g))
}

/// `FileParser.ProcessFilename` + the scanner callback, for a file no pass of
/// this run owns: its directives, or none when it cannot be read or parsed.
fn load_file_directives(filename: &str) -> Arc<LineDirectives> {
    load_file(filename).0
}

/// A file read from disk: its directives and its type declarations, both
/// empty when it is not read or cannot be parsed.
fn load_file(filename: &str) -> (Arc<LineDirectives>, Arc<TypeDecls>) {
    if is_goroot_file(filename) || !std::path::Path::new(filename).is_absolute() {
        return (Arc::new(LineDirectives::new()), Arc::new(TypeDecls::new()));
    }
    let stamp = file_stamp(filename);
    if let Some(entry) = file_cache().lock().unwrap().get(filename) {
        if entry.stamp == stamp {
            return (entry.directives.clone(), entry.types.clone());
        }
    }
    let (directives, types) = std::fs::read(filename)
        .ok()
        .and_then(|src| {
            // A file without a directive answers nothing either index is
            // asked for: its types are wanted only for the lines to look
            // directives up at.
            if !src.windows(DIRECTIVE_PREFIX.len()).any(|w| w == DIRECTIVE_PREFIX.as_bytes()) {
                return None;
            }
            let fset = FileSet::new();
            let file = parse_file(&fset, filename, &src, PARSE_COMMENTS).ok()?;
            Some((scan_file(&fset, &file).0, index_type_decls(&fset, &file)))
        })
        .unwrap_or_default();
    let (directives, types) = (Arc::new(directives), Arc::new(types));
    file_cache().lock().unwrap().insert(
        filename.to_string(),
        CachedFile {
            stamp,
            directives: directives.clone(),
            types: types.clone(),
        },
    );
    (directives, types)
}

/// The directive lookups of one pass: the pass's own files, scanned up front,
/// and every other file through [`load_file_directives`].
struct Scanner {
    files: HashMap<String, Arc<LineDirectives>>,
}

impl Scanner {
    /// `Scanner.ProcessFiles`: scans the pass's own files and returns their
    /// directive diagnostics, positioned in the pass's `FileSet`.
    fn process_files(pass: &Pass<'_>) -> (Self, Vec<(u32, String)>) {
        let mut files = HashMap::new();
        let mut diags = Vec::new();
        for file in pass.files() {
            let Some(to) = pass.fset().file(file.pos()) else {
                continue;
            };
            let name = to.name().to_string();
            let Some(src) = file_source(pass, &name) else {
                continue;
            };
            // Every directive spells this, and most files hold none: they are
            // not worth a second parse.
            if !src.windows(DIRECTIVE_PREFIX.len()).any(|w| w == DIRECTIVE_PREFIX.as_bytes()) {
                files.insert(name, Arc::new(LineDirectives::new()));
                continue;
            }
            let rfset = FileSet::new();
            let base = std::path::Path::new(&name)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&name)
                .to_string();
            let Ok(rfile) = parse_file(&rfset, &base, &src, PARSE_COMMENTS) else {
                continue;
            };
            let Some(from) = rfset.file(rfile.package) else {
                continue;
            };
            let (directives, file_diags) = scan_file(&rfset, &rfile);
            for (pos, msg) in file_diags {
                let offset = from.offset(pos);
                if offset < 0 || offset > to.size() {
                    continue;
                }
                diags.push((to.pos(offset).0 as u32, msg));
            }
            let directives = Arc::new(directives);
            if std::path::Path::new(&name).is_absolute() {
                file_cache().lock().unwrap().insert(
                    name.clone(),
                    CachedFile {
                        stamp: file_stamp(&name),
                        directives: directives.clone(),
                        types: Arc::new(index_type_decls(&rfset, &rfile)),
                    },
                );
            }
            files.insert(name, directives);
        }
        (Scanner { files }, diags)
    }

    /// `Scanner.LookupPos`.
    fn lookup_pos(&mut self, fset: &FileSet, pos: u32) -> Vec<Directive> {
        if pos == 0 {
            return Vec::new();
        }
        let p = fset.position_for(Pos(pos as i64), false);
        self.lookup(&p.filename, p.line)
    }

    /// `Scanner.Lookup`.
    fn lookup(&mut self, filename: &str, line: i64) -> Vec<Directive> {
        if filename.is_empty() {
            return Vec::new();
        }
        let fd = match self.files.get(filename) {
            Some(fd) => fd.clone(),
            None => {
                let fd = load_file_directives(filename);
                self.files.insert(filename.to_string(), fd.clone());
                fd
            }
        };
        fd.get(&line).cloned().unwrap_or_default()
    }
}

/// The bytes of the pass file the analysis `FileSet` names `name`.
fn file_source(pass: &Pass<'_>, name: &str) -> Option<Vec<u8>> {
    let base = std::path::Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(name);
    let compiled = &pass.pkg().compiled_go_files;
    let index = compiled
        .iter()
        .position(|p| p.to_str() == Some(name))
        .or_else(|| {
            compiled
                .iter()
                .position(|p| p.file_name().and_then(|s| s.to_str()) == Some(base))
        });
    if let Some(i) = index {
        if let Some(b) = pass.pkg().source_bytes(i) {
            return Some(b.to_vec());
        }
        if let Ok(b) = std::fs::read(&compiled[i]) {
            return Some(b);
        }
    }
    std::fs::read(name).ok()
}

// ============================================================================
// internal/pattern
// ============================================================================

/// `pattern.List`. Go compiles each pattern with `Longest()` and asks whether
/// the leftmost-longest match spans the whole string; that holds exactly when
/// the pattern matches the whole string, which an anchored regexp answers.
struct PatternList(Vec<Regex>);

impl PatternList {
    fn new(patterns: &[String], flag: &str) -> Result<Self, String> {
        let mut out = Vec::with_capacity(patterns.len());
        for p in patterns {
            if p.is_empty() {
                return Err(format!(
                    "compile {flag} patterns: empty regular expression is not allowed"
                ));
            }
            let re = Regex::new(&format!(r"\A(?:{p})\z")).map_err(|err| {
                format!("compile {flag} patterns: compile regular expression (pattern={p}): {err}")
            })?;
            out.push(re);
        }
        Ok(PatternList(out))
    }

    fn match_full(&self, target: &str) -> bool {
        self.0.iter().any(|re| re.is_match(target))
    }

    fn match_full_except(&self, target: &str, excepted: &str) -> bool {
        self.0
            .iter()
            .any(|re| re.is_match(target) && !re.is_match(excepted))
    }
}

// ============================================================================
// internal/structure
// ============================================================================

const ANONYMOUS_NAME: &str = "<anonymous>";
const BLANK_FIELD_NAME: &str = "_";

/// `fieldInfo` / `structFields`: what a struct type says, before any pattern or
/// caller is applied.
struct RawFields {
    strct: TypeId,
    package_path: String,
    fields: Vec<RawField>,
}

struct RawField {
    name: String,
    exported: bool,
    enforced: bool,
    optional: bool,
    embedded: Option<Rc<RawFields>>,
}

#[derive(Clone)]
struct Field {
    name: String,
    exported: bool,
    enforced: bool,
    optional: bool,
    pattern_enforced: bool,
    pattern_optional: bool,
    shadowed: bool,
    /// Index of the promoted level in [`StructMeta::levels`].
    embedded: Option<usize>,
}

impl Field {
    fn opted_out(&self) -> bool {
        self.optional || self.pattern_optional
    }

    fn unreachable(&self, external_pkg: bool) -> bool {
        self.shadowed || (external_pkg && !self.exported)
    }
}

struct Fields {
    package_path: String,
    items: Vec<Field>,
}

/// `structure.Struct`. The field tree is flattened into `levels`, the root
/// first; an embedded field points at its level by index.
struct StructMeta {
    name: String,
    full_path: String,
    package_name: String,
    levels: Vec<Fields>,
    paths: HashMap<String, Vec<usize>>,
    enforced: bool,
    ignored: bool,
    optional: bool,
    pattern_enforced: bool,
    pattern_ignored: bool,
    pattern_optional: bool,
    allow_empty_decl: bool,
}

impl StructMeta {
    fn package_path(&self) -> &str {
        match self.full_path.rfind('.') {
            Some(i) => &self.full_path[..i],
            None => &self.full_path,
        }
    }

    fn is_enforced(&self) -> bool {
        self.enforced || self.pattern_enforced
    }

    fn is_ignored(&self) -> bool {
        self.ignored || self.pattern_ignored
    }

    fn is_optional(&self) -> bool {
        self.optional || self.pattern_optional
    }

    fn is_field_required(&self, f: &Field, external_pkg: bool) -> bool {
        if f.unreachable(external_pkg) {
            return false;
        }
        if f.enforced {
            return true;
        }
        if f.optional {
            return false;
        }
        if f.pattern_enforced {
            return true;
        }
        if f.pattern_optional {
            return false;
        }
        !self.is_optional()
    }

    /// `Struct.SkippedFields`.
    fn skipped_fields(&self, lit: &CompositeLit, caller_pkg: &str, can_name_promoted: bool) -> Vec<String> {
        if is_named_literal(lit) || lit.elts.is_empty() {
            let groups = self.group_keys(lit, caller_pkg, can_name_promoted);
            let mut missing = Vec::new();
            self.skipped_in(0, Some(&groups), caller_pkg, can_name_promoted, &mut missing);
            return missing;
        }
        let root = &self.levels[0];
        let external = root.package_path != caller_pkg;
        root.items
            .iter()
            .skip(lit.elts.len())
            .filter(|f| self.is_field_required(f, external))
            .map(|f| f.name.clone())
            .collect()
    }

    fn skipped_in(
        &self,
        level: usize,
        groups: Option<&KeyGroups>,
        caller_pkg: &str,
        can_name_promoted: bool,
        missing: &mut Vec<String>,
    ) {
        let fs = &self.levels[level];
        let external = fs.package_path != caller_pkg;
        for (i, f) in fs.items.iter().enumerate() {
            if groups.is_some_and(|g| g.named.contains(&i)) {
                continue;
            }
            if f.name == BLANK_FIELD_NAME {
                continue;
            }
            let child = groups.and_then(|g| g.children.get(&i));
            if !self.is_field_required(f, external) {
                self.skipped_under(f, child, caller_pkg, external, can_name_promoted, missing);
                continue;
            }
            if let (Some(child), Some(e)) = (child, f.embedded) {
                self.skipped_in(e, Some(child), caller_pkg, can_name_promoted, missing);
                continue;
            }
            missing.push(f.name.clone());
        }
    }

    fn skipped_under(
        &self,
        f: &Field,
        groups: Option<&KeyGroups>,
        caller_pkg: &str,
        external_pkg: bool,
        can_name_promoted: bool,
        missing: &mut Vec<String>,
    ) {
        let Some(e) = f.embedded else {
            return;
        };
        if f.opted_out() {
            return;
        }
        if can_name_promoted {
            self.skipped_in(e, groups, caller_pkg, can_name_promoted, missing);
            return;
        }
        if !f.unreachable(external_pkg) && self.requires_any_below(e, caller_pkg) {
            missing.push(f.name.clone());
        }
    }

    fn requires_any_below(&self, level: usize, caller_pkg: &str) -> bool {
        let fs = &self.levels[level];
        let external = fs.package_path != caller_pkg;
        for f in &fs.items {
            if f.name == BLANK_FIELD_NAME || f.unreachable(external) {
                continue;
            }
            if self.is_field_required(f, external) {
                return true;
            }
            if let Some(e) = f.embedded {
                if !f.opted_out() && self.requires_any_below(e, caller_pkg) {
                    return true;
                }
            }
        }
        false
    }

    /// `Fields.groupKeys`.
    fn group_keys(&self, lit: &CompositeLit, caller_pkg: &str, can_name_promoted: bool) -> KeyGroups {
        let mut groups = KeyGroups::default();
        for elt in &lit.elts {
            let Expr::KeyValueExpr(kv) = elt else {
                continue;
            };
            let Expr::Ident(key) = kv.key.as_ref() else {
                continue;
            };
            let Some(path) = self.paths.get(&promotion_key(caller_pkg, &key.name)) else {
                continue;
            };
            if path.len() == 1 || (can_name_promoted && path.len() > 1) {
                groups.add(path);
            }
        }
        groups
    }
}

#[derive(Default)]
struct KeyGroups {
    named: HashSet<usize>,
    children: HashMap<usize, KeyGroups>,
}

impl KeyGroups {
    fn add(&mut self, path: &[usize]) {
        let mut g = self;
        for &i in &path[..path.len() - 1] {
            g = g.children.entry(i).or_default();
        }
        g.named.insert(path[path.len() - 1]);
    }
}

fn is_named_literal(lit: &CompositeLit) -> bool {
    matches!(lit.elts.first(), Some(Expr::KeyValueExpr(_)))
}

/// `promotionKey`: an unexported name belongs to the package that wrote it.
fn promotion_key(pkg_path: &str, name: &str) -> String {
    if is_exported(name) {
        name.to_string()
    } else {
        format!("{pkg_path}.{name}")
    }
}

/// `indexPromotion`: the chain of field indices reaching every name a literal
/// of the outermost type may write; marks every field no chain reaches.
fn index_promotion(levels: &mut [Fields]) -> HashMap<String, Vec<usize>> {
    let mut paths: HashMap<String, Vec<usize>> = HashMap::new();
    let mut blocked: HashSet<String> = HashSet::new();
    let mut level: Vec<(usize, Vec<usize>)> = vec![(0, Vec::new())];
    while !level.is_empty() {
        let mut at_depth: HashMap<String, usize> = HashMap::new();
        for (l, _) in &level {
            let fs = &levels[*l];
            for item in &fs.items {
                *at_depth.entry(promotion_key(&fs.package_path, &item.name)).or_default() += 1;
            }
        }
        let mut next = Vec::new();
        for (l, path) in &level {
            let pkg = levels[*l].package_path.clone();
            for i in 0..levels[*l].items.len() {
                let mut p = path.clone();
                p.push(i);
                let item = &mut levels[*l].items[i];
                let key = promotion_key(&pkg, &item.name);
                item.shadowed = blocked.contains(&key) || at_depth.get(&key).copied().unwrap_or(0) > 1;
                if !item.shadowed {
                    paths.insert(key, p.clone());
                }
                if let Some(e) = item.embedded {
                    next.push((e, p));
                }
            }
        }
        blocked.extend(at_depth.into_keys());
        level = next;
    }
    paths
}

/// `structure.Processor`, one per pass.
struct Processor {
    enforce: PatternList,
    ignore: PatternList,
    optional: PatternList,
    allow_empty: PatternList,
    fields_cache: HashMap<TypeId, Rc<RawFields>>,
    struct_cache: HashMap<(Option<ObjectId>, TypeId), Rc<StructMeta>>,
}

impl Processor {
    /// `Processor.ResolveStruct`.
    fn resolve_struct(
        &mut self,
        cx: &mut Cx<'_, '_>,
        type_name: Option<ObjectId>,
        strct: TypeId,
        pos: u32,
        has_pos: bool,
    ) -> Rc<StructMeta> {
        if let Some(s) = self.struct_cache.get(&(type_name, strct)) {
            return s.clone();
        }
        let artifacts = cx.artifacts();
        let (name, full_path, package_name) = match type_name {
            Some(tn) => {
                let name = tn.name(artifacts.objects).to_string();
                let (pkg_name, pkg_path) = match tn.pkg(artifacts.objects) {
                    Some(pid) => {
                        let pkg = artifacts.packages.get(pid);
                        (pkg.name().to_string(), pkg.path().to_string())
                    }
                    None => (String::new(), String::new()),
                };
                (name.clone(), format!("{pkg_path}.{name}"), pkg_name)
            }
            None => (
                ANONYMOUS_NAME.to_string(),
                format!("{}.{ANONYMOUS_NAME}", cx.pass.pkg().pkg_path),
                cx.pass.pkg().name.to_string(),
            ),
        };
        let mut s = StructMeta {
            name,
            full_path,
            package_name,
            levels: Vec::new(),
            paths: HashMap::new(),
            enforced: false,
            ignored: false,
            optional: false,
            pattern_enforced: false,
            pattern_ignored: false,
            pattern_optional: false,
            allow_empty_decl: false,
        };

        let resolved = self.struct_fields(cx, strct);
        self.build_fields(&mut s, &resolved);
        s.paths = index_promotion(&mut s.levels);

        // resolveStructDirectives
        let at = if pos != 0 {
            let p = cx.pass.fset().position_for(Pos(pos as i64), false);
            p.is_valid().then_some((p.filename, p.line))
        } else {
            // A named type of a dependency, which guff holds without a position.
            type_name.filter(|_| has_pos).and_then(|tn| cx.dep_type_name_line(tn))
        };
        if let Some((filename, line)) = at {
            let dirs = cx.scanner.lookup(&filename, line);
            s.enforced = dirs.contains(&Directive::Enforce);
            s.ignored = dirs.contains(&Directive::Ignore);
            s.optional = dirs.contains(&Directive::Optional);
        }

        // matchStructPatterns
        s.pattern_enforced = self.enforce.match_full(&s.full_path);
        s.pattern_ignored = self.ignore.match_full(&s.full_path);
        s.pattern_optional = self.optional.match_full(&s.full_path);
        s.allow_empty_decl = self.allow_empty.match_full(&s.full_path);

        let s = Rc::new(s);
        self.struct_cache.insert((type_name, strct), s.clone());
        s
    }

    /// `Processor.getStructFields` / `resolveStructFields`.
    fn struct_fields(&mut self, cx: &mut Cx<'_, '_>, strct: TypeId) -> Rc<RawFields> {
        if let Some(f) = self.fields_cache.get(&strct) {
            return f.clone();
        }
        let artifacts = cx.artifacts();
        let mut result = RawFields {
            strct,
            package_path: String::new(),
            fields: Vec::new(),
        };
        let field_ids: Vec<ObjectId> = match artifacts.types.get(strct) {
            TypeData::Struct(s) => (0..s.num_fields()).map(|i| s.field(i)).collect(),
            _ => Vec::new(),
        };
        let dep_lines = match field_ids.first() {
            Some(&f) if f.pos(artifacts.objects) == 0 => f
                .pkg(artifacts.objects)
                .and_then(|pkg| cx.dep_struct_lines(strct, pkg)),
            _ => None,
        };
        for (index, f) in field_ids.into_iter().enumerate() {
            let artifacts = cx.artifacts();
            if result.package_path.is_empty() {
                if let Some(pid) = f.pkg(artifacts.objects) {
                    result.package_path = artifacts.packages.get(pid).path().to_string();
                }
            }
            let name = f.name(artifacts.objects).to_string();
            let exported = f.exported(artifacts.objects);
            let (embedded_flag, typ) = match artifacts.objects.get(f) {
                ObjectData::Var(v) => (v.embedded(), Some(v.typ())),
                _ => (false, None),
            };
            let embedded_strct = if embedded_flag {
                typ.and_then(|t| {
                    let under = unalias_readonly(artifacts.types, t).underlying(artifacts.types);
                    matches!(artifacts.types.get(under), TypeData::Struct(_)).then_some(under)
                })
            } else {
                None
            };
            let fpos = f.pos(artifacts.objects);
            let embedded = embedded_strct.map(|e| self.struct_fields(cx, e));
            let o = match &dep_lines {
                Some((file, lines)) if fpos == 0 => optionality(&cx.scanner.lookup(file, lines[index])),
                _ => optionality(&cx.scanner.lookup_pos(cx.pass.fset(), fpos)),
            };
            result.fields.push(RawField {
                name,
                exported,
                enforced: o.enforced,
                optional: o.optional,
                embedded,
            });
        }
        let result = Rc::new(result);
        self.fields_cache.insert(strct, result.clone());
        result
    }

    /// `Processor.buildFields`: level by level, each struct opened once, at the
    /// depth it is first reached and only where it is reached once.
    fn build_fields(&self, s: &mut StructMeta, resolved: &Rc<RawFields>) {
        let (root, pending) = self.level_fields(s, resolved);
        s.levels.push(root);
        let mut opened: HashSet<TypeId> = HashSet::new();
        opened.insert(resolved.strct);
        let mut level: Vec<(usize, usize, Rc<RawFields>)> =
            pending.into_iter().map(|(i, r)| (0, i, r)).collect();
        while !level.is_empty() {
            let mut occurrences: HashMap<TypeId, usize> = HashMap::new();
            for (_, _, r) in &level {
                *occurrences.entry(r.strct).or_default() += 1;
            }
            let mut next = Vec::new();
            for (owner, index, r) in &level {
                if opened.contains(&r.strct) || occurrences[&r.strct] > 1 {
                    continue;
                }
                opened.insert(r.strct);
                let (sub, below) = self.level_fields(s, r);
                let id = s.levels.len();
                s.levels.push(sub);
                s.levels[*owner].items[*index].embedded = Some(id);
                next.extend(below.into_iter().map(|(i, r)| (id, i, r)));
            }
            opened.extend(occurrences.into_keys());
            level = next;
        }
    }

    /// `Processor.levelFields`.
    fn level_fields(&self, s: &StructMeta, resolved: &RawFields) -> (Fields, Vec<(usize, Rc<RawFields>)>) {
        let external = resolved.package_path != s.package_path();
        let mut fields = Fields {
            package_path: resolved.package_path.clone(),
            items: Vec::with_capacity(resolved.fields.len()),
        };
        let mut pending = Vec::new();
        for sf in &resolved.fields {
            if external && !sf.exported && sf.embedded.is_none() {
                continue;
            }
            let field_path = format!("{}#{}", s.full_path, sf.name);
            fields.items.push(Field {
                name: sf.name.clone(),
                exported: sf.exported,
                enforced: sf.enforced,
                optional: sf.optional,
                pattern_enforced: self.enforce.match_full_except(&field_path, &s.full_path),
                pattern_optional: self.optional.match_full_except(&field_path, &s.full_path),
                shadowed: false,
                embedded: None,
            });
            if let Some(e) = &sf.embedded {
                pending.push((fields.items.len() - 1, e.clone()));
            }
        }
        (fields, pending)
    }
}

// ============================================================================
// analyzer — shared pass state
// ============================================================================

/// The type arenas of the pass's package.
#[derive(Clone, Copy)]
struct Arts<'a> {
    types: &'a TypeArena,
    objects: &'a ObjectArena,
    packages: &'a PackageArena,
    scopes: &'a ScopeArena,
}

struct Cx<'a, 'p> {
    pass: &'a Pass<'p>,
    arts: Arts<'a>,
    scanner: Scanner,
    /// A writable copy of the type arena, made on first need: identity and
    /// `Implements` cache interface type sets as they go.
    types_mut: Option<TypeArena>,
    /// The universe `error` type, looked up on first need.
    error_type: Option<Option<TypeId>>,
    /// Source files of dependency packages, by import path.
    dep_files: HashMap<String, Vec<String>>,
    /// Where a dependency's struct type is spelled: its file and field lines.
    dep_structs: HashMap<TypeId, Option<(String, Vec<i64>)>>,
}

impl<'a, 'p> Cx<'a, 'p> {
    fn artifacts(&self) -> Arts<'a> {
        self.arts
    }

    fn types_mut(&mut self) -> &mut TypeArena {
        let types = self.arts.types;
        self.types_mut.get_or_insert_with(|| types.clone())
    }

    fn identical(&mut self, a: TypeId, b: TypeId) -> bool {
        let arts = self.arts;
        let types = self.types_mut();
        api_identical(types, arts.objects, arts.packages, a, b)
    }

    fn implements_error(&mut self, typ: TypeId) -> bool {
        let arts = self.arts;
        let err = *self.error_type.get_or_insert_with(|| universe_error(arts));
        let Some(err) = err else {
            return false;
        };
        let types = self.types_mut();
        api_implements(types, arts.objects, arts.packages, typ, err)
    }
}

impl<'a, 'p> Cx<'a, 'p> {
    /// The source files of the package at `path`, found through the import
    /// graph of the pass's package. Standard-library files are left out, as
    /// upstream never reads them.
    fn package_files(&mut self, path: &str) -> Vec<String> {
        if let Some(files) = self.dep_files.get(path) {
            return files.clone();
        }
        let mut seen: HashSet<String> = HashSet::new();
        let mut queue: Vec<&guff_analysis::Package> = vec![self.pass.pkg()];
        let mut found: Vec<String> = Vec::new();
        while let Some(pkg) = queue.pop() {
            if pkg.pkg_path == path && !std::ptr::eq(pkg, self.pass.pkg()) {
                let files = if pkg.compiled_go_files.is_empty() {
                    &pkg.go_files
                } else {
                    &pkg.compiled_go_files
                };
                found = files
                    .iter()
                    .filter_map(|p| p.to_str().map(str::to_string))
                    .filter(|f| !is_goroot_file(f))
                    .collect();
                break;
            }
            for imp in pkg.imports.values() {
                if seen.insert(imp.id.clone()) {
                    queue.push(imp);
                }
            }
        }
        self.dep_files.insert(path.to_string(), found.clone());
        found
    }

    /// Where `strct`, a struct of the package `pkg`, is spelled in source: the
    /// file and the line of each field. guff's dependency objects carry no
    /// position (their type information outlives the `FileSet`), so the type
    /// declaring the struct is found by name in the package's files instead.
    fn dep_struct_lines(&mut self, strct: TypeId, pkg: PackageId) -> Option<(String, Vec<i64>)> {
        if let Some(found) = self.dep_structs.get(&strct) {
            return found.clone();
        }
        let arts = self.arts;
        let path = arts.packages.get(pkg).path().to_string();
        let scope = arts.packages.get(pkg).scope();
        let num_fields = match arts.types.get(strct) {
            TypeData::Struct(s) => s.num_fields(),
            _ => 0,
        };
        let mut found = None;
        'files: for file in self.package_files(&path) {
            let (_, decls) = load_file(&file);
            for (name, decl) in decls.iter() {
                let Some(lines) = &decl.struct_fields else {
                    continue;
                };
                if lines.len() != num_fields {
                    continue;
                }
                let Some(obj) = guff_types::scope::lookup(arts.scopes, scope, name) else {
                    continue;
                };
                let Some(typ) = obj.typ(arts.objects) else {
                    continue;
                };
                let under = match arts.types.get(typ) {
                    TypeData::Named(_) => named_origin(arts.types, typ).underlying(arts.types),
                    _ => unalias_readonly(arts.types, typ).underlying(arts.types),
                };
                if under == strct {
                    found = Some((file.clone(), lines.clone()));
                    break 'files;
                }
            }
        }
        self.dep_structs.insert(strct, found.clone());
        found
    }

    /// The physical position of a dependency type name with no position of
    /// its own, found by name among its package's declarations.
    fn dep_type_name_line(&mut self, obj: ObjectId) -> Option<(String, i64)> {
        let arts = self.arts;
        let pkg = obj.pkg(arts.objects)?;
        let path = arts.packages.get(pkg).path().to_string();
        let name = obj.name(arts.objects).to_string();
        for file in self.package_files(&path) {
            let (_, decls) = load_file(&file);
            if let Some(decl) = decls.get(&name) {
                return Some((file, decl.name_line));
            }
        }
        None
    }
}

fn universe_error(arts: Arts<'_>) -> Option<TypeId> {
    for oid in arts.objects.ids() {
        let ObjectData::TypeName(tn) = arts.objects.get(oid) else {
            continue;
        };
        if tn.name() != "error" || oid.pkg(arts.objects).is_some() {
            continue;
        }
        return tn.typ();
    }
    None
}

// ============================================================================
// analyzer/missing-fields-visitor.go
// ============================================================================

struct LiteralVisitor<'o> {
    options: &'o ExhaustructV5Options,
    processor: Processor,
}

fn type_of(pass: &Pass<'_>, id: u32) -> Option<TypeId> {
    if id == 0 {
        return None;
    }
    Some(pass.types_info()?.types.get(&id)?.typ)
}

/// `typeNameOf`.
fn type_name_of(types: &TypeArena, typ: TypeId) -> Option<ObjectId> {
    match types.get(typ) {
        TypeData::Alias(a) => Some(a.obj()),
        TypeData::Named(n) => Some(n.obj()),
        TypeData::TypeParam(tp) => Some(tp.obj()),
        _ => None,
    }
}

/// Whether `node` is an `ast.Expr`, `ast.Spec`, `ast.Decl` or `ast.Stmt`, and
/// whether it is a statement.
fn use_site_kind(node: NodeRef<'_>) -> Option<bool> {
    use NodeRef as N;
    match node {
        N::BadExpr(_)
        | N::Ident(_)
        | N::BasicLit(_)
        | N::Ellipsis(_)
        | N::FuncLit(_)
        | N::CompositeLit(_)
        | N::ParenExpr(_)
        | N::SelectorExpr(_)
        | N::IndexExpr(_)
        | N::IndexListExpr(_)
        | N::SliceExpr(_)
        | N::TypeAssertExpr(_)
        | N::CallExpr(_)
        | N::StarExpr(_)
        | N::UnaryExpr(_)
        | N::BinaryExpr(_)
        | N::KeyValueExpr(_)
        | N::ArrayType(_)
        | N::StructType(_)
        | N::FuncType(_)
        | N::InterfaceType(_)
        | N::MapType(_)
        | N::ChanType(_)
        | N::ImportSpec(_)
        | N::ValueSpec(_)
        | N::TypeSpec(_)
        | N::BadDecl(_)
        | N::GenDecl(_)
        | N::FuncDecl(_) => Some(false),
        N::BadStmt(_)
        | N::DeclStmt(_)
        | N::EmptyStmt(_)
        | N::LabeledStmt(_)
        | N::ExprStmt(_)
        | N::SendStmt(_)
        | N::IncDecStmt(_)
        | N::AssignStmt(_)
        | N::GoStmt(_)
        | N::DeferStmt(_)
        | N::ReturnStmt(_)
        | N::BranchStmt(_)
        | N::BlockStmt(_)
        | N::IfStmt(_)
        | N::CaseClause(_)
        | N::SwitchStmt(_)
        | N::TypeSwitchStmt(_)
        | N::CommClause(_)
        | N::SelectStmt(_)
        | N::ForStmt(_)
        | N::RangeStmt(_) => Some(true),
        _ => None,
    }
}

fn unparen(mut e: &Expr) -> &Expr {
    while let Expr::ParenExpr(p) = e {
        e = &p.x;
    }
    e
}

fn same_node(a: NodeRef<'_>, b: NodeRef<'_>) -> bool {
    a.kind() == b.kind() && a.erased_ptr() == b.erased_ptr()
}

fn is_blank(e: &Expr) -> bool {
    matches!(unparen(e), Expr::Ident(id) if id.name == "_")
}

impl<'o> LiteralVisitor<'o> {
    fn process(&mut self, cx: &mut Cx<'_, '_>, lit: &CompositeLit, stack: &[NodeRef<'_>]) -> Option<(u32, String)> {
        let (type_name, strct, pos, has_pos) = self.resolve_literal_type(cx, lit, stack)?;
        let s = self.processor.resolve_struct(cx, type_name, strct, pos, has_pos);
        let dirs = use_site_directives(cx, stack);
        let ignored = dirs.contains(&Directive::Ignore);
        let enforced = dirs.contains(&Directive::Enforce);

        if lit.elts.is_empty() && self.check_empty_allowed(cx, &s, lit, stack) {
            return None;
        }

        // checkLiteral / shouldCheck
        let should_check = if ignored {
            false
        } else if enforced {
            true
        } else if s.is_ignored() {
            false
        } else if s.is_enforced() {
            true
        } else {
            !self.options.explicit_mode
        };
        if !should_check {
            return None;
        }

        let pos = lit
            .ty
            .as_ref()
            .map(|t| t.pos().0 as u32)
            .unwrap_or(lit.lbrace.0 as u32);
        let version = effective_file_go_version(cx.pass, pos);
        let can_name_promoted = version_compare(&version, "go1.27") >= 0;
        let missing = s.skipped_fields(lit, &cx.pass.pkg().pkg_path, can_name_promoted);
        if missing.is_empty() {
            return None;
        }
        let display = format!("{}.{}", s.package_name, s.name);
        let msg = if missing.len() == 1 {
            format!("{display} is missing field {}", missing[0])
        } else {
            format!("{display} is missing fields {}", missing.join(", "))
        };
        Some((pos, msg))
    }

    /// `resolveLiteralType`.
    fn resolve_literal_type(
        &mut self,
        cx: &mut Cx<'_, '_>,
        lit: &CompositeLit,
        stack: &[NodeRef<'_>],
    ) -> Option<(Option<ObjectId>, TypeId, u32, bool)> {
        let artifacts = cx.artifacts();
        let types = artifacts.types;
        let mut typ = type_of(cx.pass, lit.id)?;
        let mut name = type_name_of(types, typ);

        let unaliased = unalias_readonly(types, typ);
        if !matches!(types.get(unaliased), TypeData::TypeParam(_)) {
            let under = unaliased.underlying(types);
            if let TypeData::Pointer(p) = types.get(under) {
                typ = p.elem();
                if name.is_none() {
                    name = type_name_of(types, typ);
                }
            }
        }

        let typ = unalias_readonly(types, typ);
        let name_pos = |n: Option<ObjectId>| n.map(|o| o.pos(artifacts.objects)).unwrap_or(0);
        match types.get(typ) {
            TypeData::Named(_) => {
                let origin = named_origin(types, typ);
                let under = origin.underlying(types);
                if !matches!(types.get(under), TypeData::Struct(_)) {
                    return None;
                }
                Some((name, under, name_pos(name), true))
            }
            TypeData::TypeParam(_) => {
                let strct = core_struct(cx, typ)?;
                Some((name, strct, 0, false))
            }
            TypeData::Struct(_) => {
                if name.is_some() {
                    return Some((name, typ, name_pos(name), true));
                }
                Some((None, typ, find_anonymous_struct_pos(lit, stack), false))
            }
            _ => None,
        }
    }

    /// `checkEmptyAllowed`.
    fn check_empty_allowed(
        &mut self,
        cx: &mut Cx<'_, '_>,
        s: &StructMeta,
        lit: &CompositeLit,
        stack: &[NodeRef<'_>],
    ) -> bool {
        if self.options.allow_empty || s.allow_empty_decl {
            return true;
        }
        let (parent, child) = match enclosing_of_literal(stack) {
            Some((p, c)) => (Some(p), Some(c)),
            None => (None, None),
        };
        if let Some(NodeRef::ReturnStmt(ret)) = parent {
            if self.options.allow_empty_returns {
                return true;
            }
            if is_error_return_statement(cx, ret, lit) {
                return true;
            }
        }
        if self.options.allow_empty_declarations {
            match parent {
                Some(NodeRef::AssignStmt(a)) if a.tok == Some(Token::DEFINE) => return true,
                Some(NodeRef::ValueSpec(_)) => return true,
                _ => {}
            }
        }
        if self.options.allow_empty_blank_assignments {
            if let (Some(parent), Some(child)) = (parent, child) {
                let blank = match parent {
                    NodeRef::ValueSpec(vs) => vs
                        .values
                        .iter()
                        .position(|v| same_node(expr_ref(v), child))
                        .is_some_and(|i| i < vs.names.len() && vs.names[i].name == "_"),
                    NodeRef::AssignStmt(a) => a
                        .rhs
                        .iter()
                        .position(|v| same_node(expr_ref(v), child))
                        .is_some_and(|i| i < a.lhs.len() && is_blank(&a.lhs[i])),
                    _ => false,
                };
                if blank {
                    return true;
                }
            }
        }
        false
    }
}

/// `useSiteDirectives`: the directives on the lines of the literal and of the
/// expressions holding it, up to and including the first statement.
fn use_site_directives(cx: &mut Cx<'_, '_>, stack: &[NodeRef<'_>]) -> Vec<Directive> {
    let mut dirs = Vec::new();
    for node in stack.iter().rev() {
        let Some(is_stmt) = use_site_kind(*node) else {
            break;
        };
        for d in cx.scanner.lookup_pos(cx.pass.fset(), go_pos(*node).0 as u32) {
            if !dirs.contains(&d) {
                dirs.push(d);
            }
        }
        if is_stmt {
            break;
        }
    }
    dirs
}

/// `enclosingOfLiteral`: the node holding the literal, through parentheses and
/// `&`, and the expression of it that holds the literal.
fn enclosing_of_literal<'a>(stack: &[NodeRef<'a>]) -> Option<(NodeRef<'a>, NodeRef<'a>)> {
    let mut child = *stack.last()?;
    for i in (1..stack.len()).rev() {
        match stack[i - 1] {
            p @ NodeRef::ParenExpr(_) => child = p,
            p @ NodeRef::UnaryExpr(u) => {
                if u.op == Token::AND {
                    child = p;
                    continue;
                }
                return None;
            }
            p => return Some((p, child)),
        }
    }
    None
}

/// `isErrorReturnStatement`.
fn is_error_return_statement(cx: &mut Cx<'_, '_>, ret: &ReturnStmt, lit: &CompositeLit) -> bool {
    let is_lit = |e: &Expr| matches!(e, Expr::CompositeLit(c) if std::ptr::eq(c, lit));
    for ri in ret.results.iter().rev() {
        let ri = unparen(ri);
        if is_lit(ri) {
            continue;
        }
        match ri {
            Expr::Ident(id) if id.name == "nil" => continue,
            Expr::UnaryExpr(u) if is_lit(unparen(&u.x)) => continue,
            _ => {}
        }
        let Some(t) = type_of(cx.pass, ri.id()) else {
            continue;
        };
        if cx.implements_error(t) {
            return true;
        }
    }
    false
}

/// `findAnonymousStructPos`.
fn find_anonymous_struct_pos(lit: &CompositeLit, stack: &[NodeRef<'_>]) -> u32 {
    if let Some(t) = &lit.ty {
        if let Expr::StructType(st) = t.as_ref() {
            return st.struct_.0 as u32;
        }
        return 0;
    }
    let mut map_key = false;
    let mut i = stack.len() as isize - 2;
    while i >= 0 {
        match stack[i as usize] {
            NodeRef::KeyValueExpr(kv) => {
                map_key = same_node(expr_ref(&kv.key), stack[i as usize + 1]);
                i -= 1;
            }
            NodeRef::CompositeLit(parent) => {
                return struct_pos_from_type(parent.ty.as_deref(), map_key);
            }
            _ => return 0,
        }
    }
    0
}

fn struct_pos_from_type(typ: Option<&Expr>, map_key: bool) -> u32 {
    match typ {
        Some(Expr::ArrayType(a)) => struct_pos_from_expr(&a.elt),
        Some(Expr::MapType(m)) => {
            if map_key {
                struct_pos_from_expr(&m.key)
            } else {
                struct_pos_from_expr(&m.value)
            }
        }
        _ => 0,
    }
}

fn struct_pos_from_expr(expr: &Expr) -> u32 {
    let expr = match expr {
        Expr::StarExpr(s) => s.x.as_ref(),
        e => e,
    };
    match expr {
        Expr::StructType(st) => st.struct_.0 as u32,
        _ => 0,
    }
}

/// `coreStruct`: the one struct every term of a type parameter's constraint
/// shares, or `None`.
fn core_struct(cx: &mut Cx<'_, '_>, tp: TypeId) -> Option<TypeId> {
    let artifacts = cx.artifacts();
    let TypeData::TypeParam(t) = artifacts.types.get(tp) else {
        return None;
    };
    let constraint = t.constraint()?;
    let mut r = CoreResolver {
        walking: HashSet::new(),
        done: HashMap::new(),
    };
    r.constraint_core(cx, constraint).0
}

struct CoreResolver {
    walking: HashSet<TypeId>,
    done: HashMap<TypeId, (Option<TypeId>, bool)>,
}

impl CoreResolver {
    fn constraint_core(&mut self, cx: &mut Cx<'_, '_>, typ: TypeId) -> (Option<TypeId>, bool) {
        let artifacts = cx.artifacts();
        let iface = typ.underlying(artifacts.types);
        let TypeData::Interface(i) = artifacts.types.get(iface) else {
            return (None, false);
        };
        if self.walking.contains(&iface) {
            return (None, true);
        }
        if let Some(&res) = self.done.get(&iface) {
            return res;
        }
        self.walking.insert(iface);
        let embeddeds: Vec<TypeId> = (0..i.num_embeddeds()).map(|k| i.embedded_type(k)).collect();
        let res = self.embeddeds_core(cx, &embeddeds);
        self.walking.remove(&iface);
        self.done.insert(iface, res);
        res
    }

    fn embeddeds_core(&mut self, cx: &mut Cx<'_, '_>, embeddeds: &[TypeId]) -> (Option<TypeId>, bool) {
        let mut core: Option<TypeId> = None;
        for &embedded in embeddeds {
            for term in union_terms(cx.artifacts().types.get(embedded), embedded) {
                let (strct, ok) = self.term_core(cx, term);
                if !ok {
                    return (None, false);
                }
                let Some(strct) = strct else {
                    continue;
                };
                if let Some(c) = core {
                    if !same_core(cx, c, strct) {
                        return (None, false);
                    }
                }
                core = Some(strct);
            }
        }
        (core, true)
    }

    fn term_core(&mut self, cx: &mut Cx<'_, '_>, term: TypeId) -> (Option<TypeId>, bool) {
        let artifacts = cx.artifacts();
        let under = term.underlying(artifacts.types);
        match artifacts.types.get(under) {
            TypeData::Interface(_) => self.constraint_core(cx, term),
            TypeData::Struct(_) => (Some(under), true),
            _ => (None, false),
        }
    }
}

fn union_terms(data: &TypeData, typ: TypeId) -> Vec<TypeId> {
    match data {
        TypeData::Union(u) => (0..u.len()).map(|i| u.term(i).typ()).collect(),
        _ => vec![typ],
    }
}

/// `sameCore`: identical structs whose fields also agree on optionality.
fn same_core(cx: &mut Cx<'_, '_>, a: TypeId, b: TypeId) -> bool {
    if a == b {
        return true;
    }
    if !cx.identical(a, b) {
        return false;
    }
    let artifacts = cx.artifacts();
    let (TypeData::Struct(sa), TypeData::Struct(sb)) = (artifacts.types.get(a), artifacts.types.get(b)) else {
        return false;
    };
    let pairs: Vec<(u32, u32)> = (0..sa.num_fields())
        .map(|i| {
            (
                sa.field(i).pos(artifacts.objects),
                sb.field(i).pos(artifacts.objects),
            )
        })
        .collect();
    for (pa, pb) in pairs {
        let oa = optionality(&cx.scanner.lookup_pos(cx.pass.fset(), pa));
        let ob = optionality(&cx.scanner.lookup_pos(cx.pass.fset(), pb));
        if oa != ob {
            return false;
        }
    }
    true
}

// ============================================================================
// analyzer/tag-migration-visitor.go
// ============================================================================

const TAG_KEY: &str = "exhaustruct";
const OPTIONAL_TAG_VALUE: &str = "optional";
const OPTIONAL_DIRECTIVE: &str = "//exhaustruct:optional";
const FIX_MESSAGE: &str = "fix";
const TAG_MESSAGE: &str = r#"struct tag "exhaustruct" is not supported anymore, use comment directives"#;

/// `reflect.StructTag.Lookup`.
fn struct_tag_lookup(tag: &str, key: &str) -> Option<String> {
    let mut tag = tag.as_bytes();
    while !tag.is_empty() {
        let mut i = 0;
        while i < tag.len() && tag[i] == b' ' {
            i += 1;
        }
        tag = &tag[i..];
        if tag.is_empty() {
            break;
        }
        i = 0;
        while i < tag.len() && is_tag_key_byte(tag[i]) {
            i += 1;
        }
        if i == 0 || i + 1 >= tag.len() || tag[i] != b':' || tag[i + 1] != b'"' {
            break;
        }
        let name = &tag[..i];
        tag = &tag[i + 1..];
        i = 1;
        while i < tag.len() && tag[i] != b'"' {
            if tag[i] == b'\\' {
                i += 1;
            }
            i += 1;
        }
        if i >= tag.len() {
            break;
        }
        let qvalue = &tag[..i + 1];
        tag = &tag[i + 1..];
        if name == key.as_bytes() {
            let q = std::str::from_utf8(qvalue).ok()?;
            return strconv::unquote(q).ok();
        }
    }
    None
}

/// `parseExhaustructTag`.
fn parse_exhaustruct_tag(literal: &str) -> Option<String> {
    let tag = strconv::unquote(literal).ok()?;
    struct_tag_lookup(&tag, TAG_KEY)
}

fn is_tag_key_byte(b: u8) -> bool {
    b > b' ' && b != b':' && b != b'"' && b != 0x7f
}

/// `removeExhaustructFromTag`.
fn remove_exhaustruct_from_tag(literal: &str) -> String {
    let Ok(tag) = strconv::unquote(literal) else {
        return literal.to_string();
    };
    let cut = cut_tag_entries(&tag, TAG_KEY);
    let tag = cut.trim_matches(' ');
    if tag.is_empty() {
        return String::new();
    }
    if strconv::can_backquote(tag) {
        return format!("`{tag}`");
    }
    strconv::quote(tag)
}

/// `cutTagEntries`.
fn cut_tag_entries(tag: &str, key: &str) -> String {
    let b = tag.as_bytes();
    let mut kept: Vec<u8> = Vec::new();
    let mut written = 0;
    let mut i = 0;
    while i < b.len() {
        let entry_start = i;
        let Some((entry_key, quote)) = scan_tag_key(b, i) else {
            break;
        };
        let Some(end) = scan_tag_value(b, quote) else {
            break;
        };
        i = end;
        if entry_key == key.as_bytes() {
            kept.extend_from_slice(&b[written..entry_start]);
            written = i;
        }
    }
    if written == 0 {
        return tag.to_string();
    }
    kept.extend_from_slice(&b[written..]);
    String::from_utf8(kept).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

fn scan_tag_key(tag: &[u8], mut i: usize) -> Option<(&[u8], usize)> {
    while i < tag.len() && tag[i] == b' ' {
        i += 1;
    }
    let start = i;
    while i < tag.len() && is_tag_key_byte(tag[i]) {
        i += 1;
    }
    if i == start || i + 1 >= tag.len() || tag[i] != b':' || tag[i + 1] != b'"' {
        return None;
    }
    Some((&tag[start..i], i + 1))
}

fn scan_tag_value(tag: &[u8], quote: usize) -> Option<usize> {
    let mut i = quote + 1;
    while i < tag.len() {
        if tag[i] == b'"' {
            return Some(i + 1);
        }
        if tag[i] == b'\\' {
            i += 1;
        }
        i += 1;
    }
    None
}

struct NameAnchor {
    pos: u32,
    starts_line: bool,
    optionality_decided: bool,
    shares_line_directive: bool,
}

struct TagPlacement {
    can_append: bool,
    anchors: Vec<NameAnchor>,
    gap_before_tag: bool,
}

impl TagPlacement {
    fn moves_a_line_directive(&self) -> bool {
        self.anchors
            .iter()
            .any(|a| a.shares_line_directive && !a.optionality_decided)
    }
}

/// The bytes of the files tags are read from, by physical filename.
struct Sources(HashMap<String, Option<Vec<u8>>>);

impl Sources {
    fn get(&mut self, pass: &Pass<'_>, pos: Pos) -> Option<&[u8]> {
        let name = pass.fset().position_for(pos, false).filename;
        self.0
            .entry(name.clone())
            .or_insert_with(|| file_source(pass, &name))
            .as_deref()
    }
}

fn is_go_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

fn span_blank(fset: &FileSet, src: Option<&[u8]>, from: Pos, to: Pos) -> bool {
    let (Some(f), Some(src)) = (fset.file(from), src) else {
        return false;
    };
    if to.0 < from.0 {
        return false;
    }
    let (start, end) = (f.offset(from) as usize, f.offset(to) as usize);
    if end > src.len() || start > end {
        return false;
    }
    src[start..end].iter().all(|&b| is_go_space(b))
}

fn rest_of_line_blank(fset: &FileSet, src: Option<&[u8]>, pos: Pos) -> bool {
    let Some(f) = fset.file(pos) else {
        return false;
    };
    let line = physical_line(fset, pos);
    if line >= 1 && (line as usize) < f.line_count() {
        return span_blank(fset, src, pos, f.line_start(line as usize + 1));
    }
    span_blank(fset, src, pos, f.pos(f.size()))
}

fn is_line_directive(line: &[u8]) -> bool {
    line.starts_with(b"//line ") || line.starts_with(b"/*line ")
}

fn only_line_directive(prefix: &[u8]) -> bool {
    let start = prefix
        .iter()
        .position(|&b| b != b' ' && b != b'\t')
        .unwrap_or(prefix.len());
    let trimmed = &prefix[start..];
    let Some(rest) = trimmed.strip_prefix(b"/*line ") else {
        return false;
    };
    let Some(close) = rest.windows(2).position(|w| w == b"*/") else {
        return false;
    };
    rest[close + 2..].iter().all(|&b| b == b' ' || b == b'\t')
}

struct TagVisitor<'s> {
    scanner: &'s mut Scanner,
    sources: Sources,
}

impl TagVisitor<'_> {
    /// `aboveLineDirective`.
    fn above_line_directive(&mut self, pass: &Pass<'_>, pos: Pos) -> (Pos, bool) {
        let fset = pass.fset();
        let Some(f) = fset.file(pos) else {
            return (pos, false);
        };
        let Some(src) = self.sources.get(pass, pos) else {
            return (pos, false);
        };
        let line = physical_line(fset, pos);
        if line < 1 || line as usize > f.line_count() {
            return (pos, false);
        }
        let line_start = f.line_start(line as usize);
        let (start, end) = (f.offset(line_start) as usize, f.offset(pos) as usize);
        if start <= end && end <= src.len() && only_line_directive(&src[start..end]) {
            return (line_start, true);
        }
        if line <= 1 {
            return (pos, false);
        }
        let above = f.line_start(line as usize - 1);
        let (start, end) = (f.offset(above) as usize, f.offset(line_start) as usize);
        if start > end || end > src.len() {
            return (pos, false);
        }
        if !is_line_directive(&src[start..end]) {
            return (pos, false);
        }
        (above, false)
    }

    /// `anchorAt`.
    fn anchor_at(&mut self, pass: &Pass<'_>, prev: Pos, pos: Pos) -> NameAnchor {
        let fset = pass.fset();
        let o = optionality(&self.scanner.lookup_pos(fset, pos.0 as u32));
        let (anchor, shares_line) = self.above_line_directive(pass, pos);
        NameAnchor {
            pos: anchor.0 as u32,
            starts_line: physical_line(fset, prev) != physical_line(fset, pos),
            optionality_decided: o.optional || o.enforced,
            shares_line_directive: shares_line,
        }
    }

    /// `nameAnchors`.
    fn name_anchors(&mut self, pass: &Pass<'_>, before: Pos, field: &AstField) -> Vec<NameAnchor> {
        if field.names.is_empty() {
            return vec![self.anchor_at(pass, before, field.pos())];
        }
        let mut anchors = Vec::new();
        let mut anchored = 0;
        let mut prev = before;
        for name in &field.names {
            let line = physical_line(pass.fset(), name.pos());
            if line != anchored {
                anchors.push(self.anchor_at(pass, prev, name.pos()));
                anchored = line;
            }
            prev = name.end();
        }
        anchors
    }

    /// `placeTag`.
    fn place_tag(&mut self, pass: &Pass<'_>, st: &StructType, i: usize) -> TagPlacement {
        let field = &st.fields.list[i];
        let fset = pass.fset();
        let before = if i > 0 { st.fields.list[i - 1].end() } else { st.fields.opening };
        let tag = field.tag.as_ref().expect("tagged field");
        let field_line = physical_line(fset, field.pos());
        let ty_end = field.ty.as_ref().map(|t| t.end()).unwrap_or(NO_POS);
        let can_append = physical_line(fset, tag.end()) == field_line && {
            let src = self.sources.get(pass, tag.pos());
            rest_of_line_blank(fset, src, tag.end())
        };
        let anchors = self.name_anchors(pass, before, field);
        let gap_before_tag = {
            let src = self.sources.get(pass, tag.pos());
            !span_blank(fset, src, ty_end, tag.pos())
        };
        TagPlacement {
            can_append,
            anchors,
            gap_before_tag,
        }
    }
}

/// `buildTagDiagnostic` / `buildTagFix`.
fn build_tag_diagnostic(field: &AstField, placement: &TagPlacement, tag_value: &str) -> Diagnostic {
    let tag = field.tag.as_ref().expect("tagged field");
    let mut fixes = Vec::new();
    if placement.can_append || !placement.moves_a_line_directive() {
        let new_tag = remove_exhaustruct_from_tag(&tag.value);
        let ty_end = field.ty.as_ref().map(|t| t.end()).unwrap_or(NO_POS);
        let mut start = tag.pos();
        if new_tag.is_empty() && !placement.gap_before_tag {
            start = ty_end;
        }
        let mut edits = vec![TextEdit {
            pos: start.0 as u32,
            end: tag.end().0 as u32,
            new_text: new_tag.clone(),
        }];
        if tag_value == OPTIONAL_TAG_VALUE {
            if placement.can_append {
                if !placement.anchors[0].optionality_decided {
                    edits[0].new_text = appended_directive(&new_tag, start == ty_end);
                }
            } else {
                for a in &placement.anchors {
                    if a.optionality_decided {
                        continue;
                    }
                    let mut line = format!("{OPTIONAL_DIRECTIVE}\n");
                    if !a.starts_line {
                        line.insert(0, '\n');
                    }
                    edits.push(TextEdit {
                        pos: a.pos,
                        end: a.pos,
                        new_text: line,
                    });
                }
            }
        }
        fixes.push(SuggestedFix {
            message: FIX_MESSAGE.to_string(),
            text_edits: edits,
        });
    }
    Diagnostic {
        pos: tag.pos().0 as u32,
        message: TAG_MESSAGE.to_string(),
        suggested_fixes: fixes,
        ..Diagnostic::default()
    }
}

fn appended_directive(new_tag: &str, after_type: bool) -> String {
    if !new_tag.is_empty() || after_type {
        format!("{new_tag} {OPTIONAL_DIRECTIVE}")
    } else {
        OPTIONAL_DIRECTIVE.to_string()
    }
}

/// `tagMigrationVisitor.visitStructType`.
fn visit_struct_type(pass: &Pass<'_>, tv: &mut TagVisitor<'_>, st: &StructType, out: &mut Vec<Diagnostic>) {
    for (i, field) in st.fields.list.iter().enumerate() {
        let Some(tag) = &field.tag else {
            continue;
        };
        let Some(value) = parse_exhaustruct_tag(&tag.value) else {
            continue;
        };
        let placement = tv.place_tag(pass, st, i);
        out.push(build_tag_diagnostic(field, &placement, &value));
    }
}

// ============================================================================
// analyzer.go
// ============================================================================

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "exhaustruct_v5 requires inspect analyzer".to_string())?;
    if pass.pkg().type_artifacts.is_none() {
        return Ok(None);
    }

    let options = pass
        .settings::<ExhaustructV5Options>("exhaustruct_v5")
        .cloned()
        .unwrap_or_default();
    let processor = Processor {
        enforce: PatternList::new(&options.enforce_patterns, "enforce")?,
        ignore: PatternList::new(&options.ignore_patterns, "ignore")?,
        optional: PatternList::new(&options.optional_patterns, "optional")?,
        allow_empty: PatternList::new(&options.allow_empty_patterns, "allow-empty")?,
        fields_cache: HashMap::new(),
        struct_cache: HashMap::new(),
    };

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    {
        let pass_ref: &Pass<'_> = pass;
        let (scanner, directive_diags) = Scanner::process_files(pass_ref);
        for (pos, msg) in directive_diags {
            diagnostics.push(Diagnostic {
                pos,
                message: msg,
                ..Diagnostic::default()
            });
        }

        let artifacts = pass_ref.pkg().type_artifacts.as_ref().expect("checked above");
        let mut cx = Cx {
            pass: pass_ref,
            arts: Arts {
                types: &artifacts.types,
                objects: &artifacts.objects,
                packages: &artifacts.packages,
                scopes: &artifacts.scopes,
            },
            scanner,
            types_mut: None,
            error_type: None,
            dep_files: HashMap::new(),
            dep_structs: HashMap::new(),
        };
        let mut visitor = LiteralVisitor {
            options: &options,
            processor,
        };
        for file in pass_ref.files() {
            let mut stack: Vec<NodeRef<'_>> = Vec::new();
            walk::inspect(NodeRef::File(file), |n| {
                match n {
                    Some(node) => {
                        stack.push(node);
                        if let NodeRef::CompositeLit(lit) = node {
                            if let Some((pos, msg)) = visitor.process(&mut cx, lit, &stack) {
                                diagnostics.push(Diagnostic {
                                    pos,
                                    message: msg,
                                    ..Diagnostic::default()
                                });
                            }
                        }
                    }
                    None => {
                        stack.pop();
                    }
                }
                true
            });
        }

        let mut tv = TagVisitor {
            scanner: &mut cx.scanner,
            sources: Sources(HashMap::new()),
        };
        for file in pass_ref.files() {
            walk::preorder_prune(NodeRef::File(file), |n| {
                if let NodeRef::StructType(st) = n {
                    visit_struct_type(pass_ref, &mut tv, st, &mut diagnostics);
                }
                true
            });
        }
    }

    for d in diagnostics {
        pass.report(d);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "exhaustruct_v5",
        doc: "Checks if all structure fields are initialized",
        url: "https://github.com/GaijinEntertainment/go-exhaustruct",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directive_parse() {
        assert_eq!(parse_directive("// not one"), None);
        assert_eq!(
            parse_directive("//exhaustruct:optional"),
            Some((vec![Directive::Optional], vec![]))
        );
        assert_eq!(
            parse_directive("//exhaustruct:optional,enforce trailing prose"),
            Some((vec![Directive::Optional, Directive::Enforce], vec![]))
        );
        assert_eq!(
            parse_directive("//exhaustruct:"),
            Some((vec![], vec!["empty directive".to_string()]))
        );
        assert_eq!(
            parse_directive("//exhaustruct:optionall"),
            Some((vec![], vec!["unknown directive (directive=optionall)".to_string()]))
        );
        assert_eq!(
            parse_directive("//exhaustruct:optional,optional"),
            Some((vec![Directive::Optional], vec!["duplicate directives".to_string()]))
        );
        assert_eq!(
            parse_directive("//exhaustruct:optional, enforce"),
            Some((
                vec![Directive::Optional],
                vec!["directive after a space is not read (directive=enforce)".to_string()]
            ))
        );
        assert_eq!(
            parse_directive("/*exhaustruct:ignore*/"),
            Some((vec![Directive::Ignore], vec![]))
        );
    }

    #[test]
    fn pattern_is_a_full_match() {
        let l = PatternList::new(&[r".*\.Test".to_string(), r"a|ab".to_string()], "enforce").unwrap();
        assert!(l.match_full("example.com/p.Test"));
        assert!(!l.match_full("example.com/p.Test2"));
        // Leftmost-longest: `a|ab` spans "ab" in Go, where leftmost-first stops at "a".
        assert!(l.match_full("ab"));
        assert!(PatternList::new(&[String::new()], "enforce").is_err());
    }

    #[test]
    fn tag_lookup_and_removal() {
        assert_eq!(parse_exhaustruct_tag(r#"`exhaustruct:"optional"`"#).as_deref(), Some("optional"));
        assert_eq!(parse_exhaustruct_tag(r#"`json:"a"`"#), None);
        assert_eq!(remove_exhaustruct_from_tag(r#"`exhaustruct:"optional"`"#), "");
        assert_eq!(
            remove_exhaustruct_from_tag(r#"`json:"a" exhaustruct:"optional" yaml:"b"`"#),
            r#"`json:"a" yaml:"b"`"#
        );
    }
}
