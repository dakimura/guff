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
    reparse_with_comments, Reparsed,
};

/// `disabledChecks` plus `isRepetitiveMsg` — upstream's whole configuration
/// for this rule.
///
/// ```go
/// r.disabledChecks = disabledChecks{PrivateReceivers: true, PublicInterfaces: true}
/// r.isRepetitiveMsg = "stutters"
/// ```
///
/// Two of the flags *enable* something (`checkPrivateReceivers`,
/// `checkPublicInterface` clear a default-on skip); five turn a kind of
/// declaration off; one changes a word in the message. guff read only the
/// first and `disableStutteringCheck`, so telegraf's
/// `disable-checks-on-types` — 312 findings golangci-lint does not make —
/// went nowhere.
#[derive(Clone, Copy, Debug)]
pub struct DisabledChecks {
    pub const_: bool,
    pub function: bool,
    pub method: bool,
    /// Default **true**: methods on unexported receivers are skipped.
    pub private_receivers: bool,
    /// Default **true**: exported interfaces' methods are not checked.
    pub public_interfaces: bool,
    pub repetitive_names: bool,
    pub type_: bool,
    pub var: bool,
}

impl Default for DisabledChecks {
    fn default() -> Self {
        Self {
            const_: false,
            function: false,
            method: false,
            private_receivers: true,
            public_interfaces: true,
            repetitive_names: false,
            type_: false,
            var: false,
        }
    }
}

impl DisabledChecks {
    /// `disabledChecks.isDisabled(kind)` for the kinds the doc checks pass in.
    fn is_disabled(&self, kind: &str) -> bool {
        match kind {
            "var" => self.var,
            "const" => self.const_,
            "function" => self.function,
            "method" => self.method,
            "type" => self.type_,
            _ => false,
        }
    }
}

/// `Configure`. An unknown flag is a configuration *error* upstream; guff
/// ignores it, because refusing a config is `compat/reject`'s business and a
/// linter that stops the run over one word would lose every other finding.
fn configure(pass: &Pass<'_>) -> (DisabledChecks, &'static str) {
    let mut dc = DisabledChecks::default();
    let mut msg = "stutters";
    let has = |o: &str| crate::config::rule_has_string_option(pass, "exported", o);
    if has("checkPrivateReceivers") {
        dc.private_receivers = false;
    }
    if has("disableStutteringCheck") {
        dc.repetitive_names = true;
    }
    if has("sayRepetitiveInsteadOfStutters") {
        msg = "is repetitive";
    }
    if has("checkPublicInterface") {
        dc.public_interfaces = false;
    }
    if has("disableChecksOnConstants") {
        dc.const_ = true;
    }
    if has("disableChecksOnFunctions") {
        dc.function = true;
    }
    if has("disableChecksOnMethods") {
        dc.method = true;
    }
    if has("disableChecksOnTypes") {
        dc.type_ = true;
    }
    if has("disableChecksOnVariables") {
        dc.var = true;
    }
    (dc, msg)
}

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
    gen_decl_missing: HashMap<usize, bool>,
    skip_file: bool,
    disabled: DisabledChecks,
    repetitive_msg: &'static str,
    /// Receiver type keys that implement `sort.Interface` (Len+Less+Swap).
    sortable: HashSet<String>,
    /// `PARSE_COMMENTS` reparse for the current file (docs + private FileSet).
    comments: Option<Arc<Reparsed>>,
}

impl<'a> Checker<'a> {
    pub fn try_new(pass: &'a Pass<'a>) -> Option<Self> {
        if !is_importable_package(&pass.pkg().name) {
            return None;
        }
        let (disabled, repetitive_msg) = configure(pass);
        Some(Self {
            pass,
            failures: Vec::new(),
            gen_decl_missing: HashMap::new(),
            skip_file: false,
            disabled,
            repetitive_msg,
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
        if let Some(rp) = &self.comments {
            check_file(
                &rp.file,
                pkg,
                self.disabled,
                self.repetitive_msg,
                &self.sortable,
                &mut self.gen_decl_missing,
                &mut batch,
            );
            for mut f in batch {
                f.pos = remap_pos(self.pass, report, &rp.fset, f.pos);
                self.failures.push(f);
            }
        } else {
            check_file(
                report,
                pkg,
                self.disabled,
                self.repetitive_msg,
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
    disabled: DisabledChecks,
    repetitive_msg: &str,
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
                            lint_type_doc(ts, g, disabled, failures);
                            // `checkRepetitiveNames` is *not* behind
                            // `isDisabled("type")`: turning the type doc check
                            // off leaves the naming one running.
                            if !disabled.repetitive_names {
                                check_repetitive(
                                    pkg,
                                    &ts.name.name,
                                    "type",
                                    ts.name.name_pos.0,
                                    repetitive_msg,
                                    failures,
                                );
                            }
                            if !disabled.public_interfaces {
                                if let Expr::InterfaceType(iface) = &ts.ty {
                                    if ts.name.is_exported() {
                                        for m in &iface.methods.list {
                                            lint_interface_method(&ts.name.name, m, failures);
                                        }
                                    }
                                }
                            }
                        }
                        Spec::ValueSpec(vs) => {
                            if let Some(gd) = last_gen {
                                lint_value_spec(vs, gd, disabled, gen_decl_missing, failures);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Decl::FuncDecl(f) => {
                lint_func_doc(f, disabled, sortable, failures);
                if f.recv.is_none() && !disabled.repetitive_names {
                    check_repetitive(
                        pkg,
                        &f.name.name,
                        "func",
                        f.name.name_pos.0,
                        repetitive_msg,
                        failures,
                    );
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
    if !guff::ast::ast_is_exported(&recv) && !check_private_receivers {
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
    disabled: DisabledChecks,
    sortable: &HashSet<String>,
    failures: &mut Vec<Failure>,
) {
    if !f.name.is_exported() {
        return;
    }
    let (kind, name) = if f.recv.is_some() {
        if !must_check_method(f, !disabled.private_receivers, sortable) {
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
    // `if w.disabledChecks.isDisabled(kind) { return }` — after the kind is
    // known, so `disableChecksOnMethods` and `disableChecksOnFunctions` are
    // separate switches.
    if disabled.is_disabled(kind) {
        return;
    }
    let first = first_comment_line(f.doc.as_ref());
    let status = check_go_doc_status(&first, &f.name.name);
    match status {
        GoDocStatus::Ok => return,
        GoDocStatus::Missing => {
            failures.push(Failure::with_confidence(
                "exported",
                f.ty.func.0 as u32,
                format!("exported {kind} {name} should have comment or be unexported"),
                status.confidence(),
            ));
            return;
        }
        _ => {}
    }
    failures.push(Failure::with_confidence(
        "exported",
        f.doc.as_ref().map(|d| d.pos().0).unwrap_or(f.name.name_pos.0) as u32,
        format!(
            r#"comment on exported {kind} {name} should be of the form "{} ..."{}"#,
            f.name.name,
            status.correction_hint(&first)
        ),
        status.confidence(),
    ));
}

/// How a doc comment relates to the name it documents (upstream
/// `exportedGoDocStatus`).
#[derive(Clone, Copy, PartialEq)]
enum GoDocStatus {
    Ok,
    Missing,
    CaseMismatch,
    FirstLetterMismatch,
    /// The comment simply does not begin with the name. guff used to treat
    /// this as fine, so a doc comment that talks about something else was
    /// never reported.
    Unexpected,
}

impl GoDocStatus {
    fn confidence(self) -> f64 {
        if self == GoDocStatus::Unexpected {
            0.8
        } else {
            1.0
        }
    }

    /// Suffix appended to the "should be of the form" message. Only the two
    /// mismatch statuses carry one.
    fn correction_hint(self, first_comment_line: &str) -> String {
        let first_word = first_comment_line.split(' ').next().unwrap_or("");
        match self {
            GoDocStatus::CaseMismatch => {
                format!(r#" by using its correct casing, not "{first_word} ...""#)
            }
            GoDocStatus::FirstLetterMismatch => {
                format!(r#" to match its exported status, not "{first_word} ...""#)
            }
            _ => String::new(),
        }
    }
}

fn strip_first_rune(s: &str) -> &str {
    match s.chars().next() {
        Some(c) => &s[c.len_utf8()..],
        None => s,
    }
}

fn check_go_doc_status(first_comment_line: &str, name: &str) -> GoDocStatus {
    if first_comment_line.is_empty() {
        return GoDocStatus::Missing;
    }
    let expected = format!("{} ", name.trim());
    if first_comment_line.starts_with(&expected) {
        return GoDocStatus::Ok;
    }
    if !has_prefix_insensitive(first_comment_line, &expected) {
        return GoDocStatus::Unexpected;
    }
    if strip_first_rune(first_comment_line).starts_with(strip_first_rune(&expected)) {
        // Only the first character differs: "sendJSON" became "SendJSON".
        return GoDocStatus::FirstLetterMismatch;
    }
    GoDocStatus::CaseMismatch
}

fn lint_type_doc(
    ts: &TypeSpec,
    gd: &GenDecl,
    disabled: DisabledChecks,
    failures: &mut Vec<Failure>,
) {
    if disabled.is_disabled("type") {
        return;
    }
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
            ..Failure::default()
        });
        return;
    }
    // A leading article is allowed: "A Foo is ..." documents Foo. When one is
    // present the expected prefix grows to include it, and the hint below
    // quotes the comment with the article already removed.
    const ARTICLES: [&str; 4] = ["A", "An", "The", "This"];
    let mut expected_prefix = ts.name.name.clone();
    let mut hint_line = first.clone();
    for article in ARTICLES {
        if ts.name.name == article {
            continue;
        }
        if let Some(rest) = hint_line.strip_prefix(&format!("{article} ")) {
            expected_prefix = format!("{article} {}", ts.name.name);
            hint_line = rest.to_string();
            break;
        }
    }
    // Upstream re-reads the doc here, so the match is against the *unstripped*
    // comment and the article-extended prefix.
    let status = check_go_doc_status(&first, &expected_prefix);
    if status == GoDocStatus::Ok {
        return;
    }
    failures.push(Failure::with_confidence(
        "exported",
        doc.map(|d| d.pos().0).unwrap_or(ts.name.name_pos.0) as u32,
        format!(
            r#"comment on exported type {} should be of the form "{} ..." (with optional leading article){}"#,
            ts.name.name,
            ts.name.name,
            status.correction_hint(&hint_line)
        ),
        status.confidence(),
    ));
}

fn lint_value_spec(
    vs: &ValueSpec,
    gd: &GenDecl,
    disabled: DisabledChecks,
    gen_decl_missing: &mut HashMap<usize, bool>,
    failures: &mut Vec<Failure>,
) {
    let kind = if gd.tok == Some(Token::CONST) { "const" } else { "var" };
    if disabled.is_disabled(kind) {
        return;
    }
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
                    ..Failure::default()
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
            ..Failure::default()
        });
        gen_decl_missing.insert(key, true);
        return;
    }
    if !gd_first.is_empty() && gd.lparen.is_valid() {
        return;
    }
    let (doc, doc_line) = if !vs_first.is_empty() {
        (vs.doc.as_ref(), vs_first)
    } else {
        (gd.doc.as_ref(), gd_first)
    };
    let status = check_go_doc_status(&doc_line, &name.name);
    if status == GoDocStatus::Ok {
        return;
    }
    failures.push(Failure::with_confidence(
        "exported",
        // Upstream blames the doc comment, not the name.
        doc.map(|d| d.pos().0).unwrap_or(name.name_pos.0) as u32,
        format!(
            r#"comment on exported {kind} {} should be of the form "{} ..."{}"#,
            name.name,
            name.name,
            status.correction_hint(&doc_line)
        ),
        status.confidence(),
    ));
}

/// `lintInterfaceMethod`, reached only when `checkPublicInterface` cleared the
/// default `PublicInterfaces` skip.
fn lint_interface_method(type_name: &str, m: &guff::ast::Field, failures: &mut Vec<Failure>) {
    let Some(name_ident) = m.names.first() else {
        return;
    };
    if !name_ident.is_exported() {
        return;
    }
    let name = &name_ident.name;
    let first = first_comment_line(m.doc.as_ref());
    let status = check_go_doc_status(&first, name);
    match status {
        GoDocStatus::Ok => (),
        GoDocStatus::Missing => {
            failures.push(Failure::with_confidence(
                "exported",
                name_ident.name_pos.0 as u32,
                format!("public interface method {type_name}.{name} should be commented"),
                status.confidence(),
            ));
        }
        _ => {
            let hint = status.correction_hint(&first);
            let pos = m
                .doc
                .as_ref()
                .and_then(|d| d.list.first())
                .map(|c| c.slash.0)
                .unwrap_or(name_ident.name_pos.0);
            failures.push(Failure::with_confidence(
                "exported",
                pos as u32,
                format!(
                    "comment on exported interface method {type_name}.{name} should be of the form \"{name} ...\"{hint}"
                ),
                status.confidence(),
            ));
        }
    }
}

fn check_repetitive(
    pkg: &str,
    name: &str,
    thing: &str,
    pos: i64,
    repetitive_msg: &str,
    failures: &mut Vec<Failure>,
) {
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
                "{thing} name will be used as {pkg}.{name} by other packages, and that {repetitive_msg}; consider calling this {rem}"
            ),
            ..Failure::default()
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
