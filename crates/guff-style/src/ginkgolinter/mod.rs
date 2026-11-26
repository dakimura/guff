//! Port of [`github.com/nunnatsa/ginkgolinter`](https://github.com/nunnatsa/ginkgolinter)
//! v0.24.0 (golangci-lint 2.14.0's pin; wrapper in `pkg/golinters/ginkgolinter`).
//!
//! Enforces idiomatic Ginkgo / Gomega assertion style. The layout follows
//! upstream's packages: [`gomega`] finds the root of an assertion chain by the
//! Gomega interfaces its calls return, [`expression`] parses the actual and the
//! matcher into a copy the rules rewrite, [`rules`] holds the rule lists in
//! upstream's order, and [`report`] renders the message with the rewritten
//! copy as the suggestion. The ginkgo half (focus containers, spec pollution)
//! is name-based upstream too and lives here.

mod expression;
mod gomega;
mod report;
mod rules;
mod types;

use std::sync::OnceLock;

use guff::ast::{CallExpr, CommentGroup, Decl, Expr, File, Ident, Stmt};
use guff::commentmap::new_comment_map;
use guff::token::Token;
use guff::walk::{preorder_prune, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit};

use crate::options::GinkgolinterOptions;
use expression::GomegaExpression;
use gomega::{Handler, Interfaces};
use report::Builder;
use types::Tc;

/// `config.Config`.
#[derive(Clone, Default)]
pub(crate) struct Config {
    suppress_len: bool,
    suppress_nil: bool,
    suppress_err: bool,
    suppress_compare: bool,
    suppress_async: bool,
    forbid_focus: bool,
    suppress_type_compare: bool,
    allow_have_len0: bool,
    force_expect_to: bool,
    validate_async_intervals: bool,
    forbid_spec_pollution: bool,
    force_succeed: bool,
    force_assertion_description: bool,
    force_tonot: bool,
}

impl Config {
    fn from_options(o: &GinkgolinterOptions) -> Config {
        Config {
            suppress_len: o.suppress_len_assertion,
            suppress_nil: o.suppress_nil_assertion,
            suppress_err: o.suppress_err_assertion,
            suppress_compare: o.suppress_compare_assertion,
            suppress_async: o.suppress_async_assertion,
            forbid_focus: o.forbid_focus_container,
            suppress_type_compare: o.suppress_type_compare_assertion,
            allow_have_len0: o.allow_havelen_zero,
            force_expect_to: o.force_expect_to,
            validate_async_intervals: o.validate_async_intervals,
            forbid_spec_pollution: o.forbid_spec_pollution,
            force_succeed: o.force_succeed,
            force_assertion_description: o.force_assertion_description,
            force_tonot: o.force_tonot,
        }
    }

    fn all_true(&self) -> bool {
        self.suppress_len
            && self.suppress_nil
            && self.suppress_err
            && self.suppress_compare
            && self.suppress_async
            && !self.forbid_focus
    }

    /// `Config.UpdateFromComment`.
    fn update_from_comment(&mut self, groups: &[CommentGroup]) {
        for group in groups {
            if self.all_true() {
                break;
            }
            for c in &group.list {
                for line in c.text.split('\n') {
                    let line = line.strip_prefix("//").unwrap_or(line);
                    let line = line.strip_prefix("/*").unwrap_or(line);
                    let line = line.strip_suffix("*/").unwrap_or(line);
                    match line.trim() {
                        "ginkgo-linter:ignore-len-assert-warning" => self.suppress_len = true,
                        "ginkgo-linter:ignore-nil-assert-warning" => self.suppress_nil = true,
                        "ginkgo-linter:ignore-err-assert-warning" => self.suppress_err = true,
                        "ginkgo-linter:ignore-compare-assert-warning" => self.suppress_compare = true,
                        "ginkgo-linter:ignore-async-assert-warning" => self.suppress_async = true,
                        "ginkgo-linter:ignore-focus-container-warning" => self.forbid_focus = false,
                        "ginkgo-linter:ignore-type-compare-warning" => self.suppress_type_compare = true,
                        _ => {}
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ginkgohandler

/// `ginkgohandler.Handler`: `None` name is the dot import.
struct GinkgoHandler {
    name: Option<String>,
}

const FOCUS_CONTAINER_FOUND: &str = "ginkgo-linter: Focus container found. This is used only for local debug and should not be part of the actual source code. Consider to replace with ";
const FOCUS_SPEC_FOUND: &str = "ginkgo-linter: Focus spec found. This is used only for local debug and should not be part of the actual source code. Consider to remove it";

impl GinkgoHandler {
    /// `GetGinkgoHandler`.
    fn for_file(file: &File) -> Option<GinkgoHandler> {
        for imp in &file.imports {
            if imp.path.value != "\"github.com/onsi/ginkgo\"" && imp.path.value != "\"github.com/onsi/ginkgo/v2\"" {
                continue;
            }
            return Some(match imp.name.as_ref().map(|n| n.name.as_str()) {
                Some(".") => GinkgoHandler { name: None },
                None => GinkgoHandler {
                    name: Some("ginkgo".to_string()),
                },
                Some(n) => GinkgoHandler {
                    name: Some(n.to_string()),
                },
            });
        }
        None
    }

    /// The container function's identifier, when `call` names one through this
    /// import.
    fn func_ident<'a>(&self, call: &'a CallExpr) -> Option<&'a Ident> {
        match (&self.name, call.fun.as_ref()) {
            (None, Expr::Ident(id)) => Some(id),
            (Some(name), Expr::SelectorExpr(sel)) => match sel.x.as_ref() {
                Expr::Ident(x) if &x.name == name => Some(&sel.sel),
                _ => None,
            },
            _ => None,
        }
    }

    fn is_focus_spec(&self, e: &Expr) -> bool {
        match (&self.name, e) {
            (None, Expr::Ident(id)) => id.name == "Focus",
            (Some(name), Expr::SelectorExpr(sel)) => {
                matches!(sel.x.as_ref(), Expr::Ident(x) if &x.name == name) && sel.sel.name == "Focus"
            }
            _ => false,
        }
    }

    /// `HandleGinkgoSpecs`.
    fn handle_specs(&self, e: &Expr, config: &Config, out: &mut Vec<Diagnostic>) -> bool {
        let Expr::CallExpr(call) = e else {
            return false;
        };
        let mut go_deeper = false;
        if config.forbid_focus && self.check_focus_container(call, out) {
            go_deeper = true;
        }
        if config.forbid_spec_pollution && self.check_assignments_in_container(call, out) {
            go_deeper = true;
        }
        go_deeper
    }

    /// `checkFocusContainer`.
    fn check_focus_container(&self, call: &CallExpr, out: &mut Vec<Diagnostic>) -> bool {
        let mut found = false;
        let id = self.func_ident(call);
        if let Some(id) = id {
            if is_focus_container(&id.name) {
                let new_name = &id.name[1..];
                let pos = id.name_pos.0 as u32;
                out.push(Diagnostic {
                    pos,
                    message: format!("{FOCUS_CONTAINER_FOUND}{}", rules::go_quote(new_name)),
                    suggested_fixes: vec![SuggestedFix {
                        message: format!("should replace {} with {new_name}", id.name),
                        text_edits: vec![TextEdit {
                            pos,
                            end: pos + id.name.len() as u32,
                            new_text: new_name.to_string(),
                        }],
                    }],
                    ..Diagnostic::default()
                });
                found = true;
            }
        }
        if id.is_some_and(|id| is_container(&id.name)) {
            for a in &call.args {
                if self.is_focus_spec(a) {
                    out.push(Diagnostic {
                        pos: a.pos().0 as u32,
                        message: FOCUS_SPEC_FOUND.to_string(),
                        ..Diagnostic::default()
                    });
                    found = true;
                } else if let Expr::CallExpr(inner) = a {
                    if self.check_focus_container(inner, out) {
                        found = true;
                    }
                }
            }
        }
        found
    }

    /// `checkAssignmentsInContainer`.
    fn check_assignments_in_container(&self, call: &CallExpr, out: &mut Vec<Diagnostic>) -> bool {
        if !self.func_ident(call).is_some_and(|id| is_wrap_container(&id.name)) {
            return false;
        }
        for a in &call.args {
            if let Expr::FuncLit(f) = a {
                return check_assignments(&f.body.list, out);
            }
        }
        false
    }
}

fn is_focus_container(name: &str) -> bool {
    matches!(name, "FDescribe" | "FContext" | "FWhen" | "FIt" | "FDescribeTable" | "FEntry")
}

fn is_container(name: &str) -> bool {
    matches!(
        name,
        "It" | "When"
            | "Context"
            | "Describe"
            | "DescribeTable"
            | "Entry"
            | "PIt"
            | "PWhen"
            | "PContext"
            | "PDescribe"
            | "PDescribeTable"
            | "PEntry"
            | "XIt"
            | "XWhen"
            | "XContext"
            | "XDescribe"
            | "XDescribeTable"
            | "XEntry"
    ) || is_focus_container(name)
}

fn is_wrap_container(name: &str) -> bool {
    matches!(
        name,
        "When" | "Context"
            | "Describe"
            | "FWhen"
            | "FContext"
            | "FDescribe"
            | "PWhen"
            | "PContext"
            | "PDescribe"
            | "XWhen"
            | "XContext"
            | "XDescribe"
    )
}

fn use_before_each(out: &mut Vec<Diagnostic>, id: &Ident) {
    out.push(Diagnostic {
        pos: id.name_pos.0 as u32,
        message: format!("use BeforeEach() to assign variable {}", id.name),
        ..Diagnostic::default()
    });
}

/// `checkAssignments`.
fn check_assignments(list: &[Stmt], out: &mut Vec<Diagnostic>) -> bool {
    let mut found = false;
    for stmt in list {
        match stmt {
            Stmt::DeclStmt(ds) => {
                if let Decl::GenDecl(gen) = &ds.decl {
                    if gen.tok != Some(Token::VAR) {
                        continue;
                    }
                    for spec in &gen.specs {
                        if let guff::ast::Spec::ValueSpec(vs) = spec {
                            for (i, val) in vs.values.iter().enumerate() {
                                if !matches!(val, Expr::FuncLit(_)) {
                                    if let Some(name) = vs.names.get(i) {
                                        use_before_each(out, name);
                                        found = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Stmt::AssignStmt(a) => {
                for (i, val) in a.rhs.iter().enumerate() {
                    if matches!(val, Expr::FuncLit(_)) {
                        continue;
                    }
                    if let Some(Expr::Ident(id)) = a.lhs.get(i) {
                        if id.name != "_" {
                            use_before_each(out, id);
                            found = true;
                        }
                    }
                }
            }
            Stmt::IfStmt(s) => {
                if check_assignments(&s.body.list, out) {
                    found = true;
                }
                if let Some(Stmt::BlockStmt(b)) = s.else_.as_deref() {
                    if check_assignments(&b.list, out) {
                        found = true;
                    }
                }
            }
            _ => {}
        }
    }
    found
}

// ---------------------------------------------------------------------------
// linter

/// `getTimePkg`.
fn time_pkg(file: &File) -> String {
    let mut name = "time".to_string();
    for imp in &file.imports {
        if imp.path.value == "\"time\"" {
            if let Some(n) = &imp.name {
                name = n.name.clone();
            }
        }
    }
    name
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ginkgolinter requires inspect analyzer".to_string())?;

    let base = Config::from_options(
        &pass
            .settings::<GinkgolinterOptions>("ginkgolinter")
            .cloned()
            .unwrap_or_default(),
    );

    let mut out: Vec<Diagnostic> = Vec::new();
    {
        let Some(tc) = Tc::new(pass) else {
            return Ok(None);
        };
        let ifaces = Interfaces::find(&tc);
        for file in pass.files() {
            // The analysis AST carries no comments. Every directive this
            // linter reads starts with `ginkgo-linter:`, so a file without that
            // text needs no reparse: its comment map would change nothing.
            let comments = if guff_analysis::comments::file_source_contains(pass, file, b"ginkgo-linter:") {
                guff_analysis::comments::file_comments(pass, file)
            } else {
                Vec::new()
            };
            lint_file(&tc, ifaces.as_ref(), file, &comments, &base, &mut out);
        }
    }
    for d in out {
        pass.report(d);
    }
    Ok(None)
}

fn lint_file(
    tc: &Tc<'_>,
    ifaces: Option<&Interfaces>,
    file: &File,
    comments: &[CommentGroup],
    base: &Config,
    out: &mut Vec<Diagnostic>,
) {
    let cm = new_comment_map(tc.fset(), NodeRef::File(file), comments);
    let mut file_config = base.clone();
    for (node, groups) in cm.iter() {
        if file_config.all_true() {
            break;
        }
        if let NodeRef::GenDecl(_) = node {
            file_config.update_from_comment(groups);
        }
    }

    let gomega = ifaces.map(|i| Handler::new(i, file));
    let ginkgo = GinkgoHandler::for_file(file);
    if gomega.is_none() && ginkgo.is_none() {
        return;
    }
    let time_pkg = time_pkg(file);

    preorder_prune(NodeRef::File(file), |n| {
        if let Some(ginkgo) = &ginkgo {
            if let NodeRef::ValueSpec(spec) = n {
                let mut go_deeper = false;
                for val in &spec.values {
                    go_deeper = ginkgo.handle_specs(val, &file_config, out) || go_deeper;
                }
                if go_deeper {
                    return true;
                }
            }
        }
        let NodeRef::ExprStmt(stmt) = n else {
            return true;
        };
        let Expr::CallExpr(call) = &stmt.x else {
            return true;
        };
        let mut config = file_config.clone();
        if let Some(groups) = cm.get(n) {
            config.update_from_comment(groups);
        }
        if let Some(ginkgo) = &ginkgo {
            if ginkgo.handle_specs(&stmt.x, &config, out) {
                return true;
            }
        }
        let Some(gomega) = &gomega else {
            return true;
        };
        let Some(mut gexp) = GomegaExpression::new(tc, gomega, call, &time_pkg) else {
            return true;
        };
        let mut builder = Builder::new(tc, call);
        check_gomega_expression(tc, &mut gexp, &config, &mut builder, out)
    });
}

/// `checkGomegaExpression`.
fn check_gomega_expression(
    tc: &Tc<'_>,
    gexp: &mut GomegaExpression<'_>,
    config: &Config,
    builder: &mut Builder,
    out: &mut Vec<Diagnostic>,
) -> bool {
    let go_nested = if rules::apply_missing_assertion(gexp, builder) {
        true
    } else if gexp.is_async {
        rules::apply_async(tc, gexp, config, builder);
        true
    } else {
        rules::apply_sync(tc, gexp, config, builder);
        false
    };
    if builder.has_report() {
        builder.set_fix_offer(tc, gexp.clone.as_ref());
        out.push(builder.build());
    }
    go_nested
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "ginkgolinter",
        doc: "Enforces standards of using ginkgo and gomega.",
        url: "https://github.com/nunnatsa/ginkgolinter",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
