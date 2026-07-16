//! Port of [`github.com/ashanbrown/forbidigo`](https://github.com/ashanbrown/forbidigo)
//! (golangci-lint wrapper in `pkg/golinters/forbidigo`).
//!
//! Forbids identifiers matching configured regexp patterns. Default patterns
//! match `fmt.Print*` / builtin `print` / `println`.
//!
//! golangci-lint always ignores `//permit` (prefer `//nolint`). `analyze-types`
//! expansion is DEFERRED (see DEVELOPMENT.md R13) — patterns match literal
//! source text only.

use std::sync::OnceLock;

use guff::ast::{
    Decl, Expr, Field, FieldList, File, FuncDecl, FuncType, GenDecl, Spec, Stmt, TypeSpec,
    ValueSpec,
};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use regex::Regex;

use crate::options::{ForbidigoOptions, ForbidigoPattern};

/// Upstream default when `forbid` is empty.
const DEFAULT_PATTERN: &str = r"^(fmt\.Print(|f|ln)|print|println)$";

/// Optional trailing `(# message)?` group used by forbidigo for custom msgs.
fn extract_msg_from_pattern(pat: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"\(#\s*([^)]+?)\)\s*\??\s*$").expect("forbidigo msg extract")
    });
    re.captures(pat)
        .map(|c| c.get(1).unwrap().as_str().trim().to_string())
        .filter(|s| !s.is_empty())
}

struct CompiledPattern {
    re: Regex,
    pkg_re: Option<Regex>,
    pattern: String,
    msg: String,
}

impl CompiledPattern {
    fn compile(p: &ForbidigoPattern) -> Option<Self> {
        let re = Regex::new(&p.pattern).ok()?;
        let pkg_re = if p.pkg.is_empty() {
            None
        } else {
            Some(Regex::new(&p.pkg).ok()?)
        };
        let mut msg = p.msg.clone();
        if msg.is_empty() {
            if let Some(extracted) = extract_msg_from_pattern(&p.pattern) {
                msg = extracted;
            }
        }
        Some(Self {
            re,
            pkg_re,
            pattern: p.pattern.clone(),
            msg,
        })
    }

    fn matches(&self, texts: &[&str]) -> bool {
        texts.iter().any(|t| self.re.is_match(t))
    }
}

fn compile_patterns(opts: &ForbidigoOptions) -> Vec<CompiledPattern> {
    let patterns: Vec<ForbidigoPattern> = if opts.forbid.is_empty() {
        vec![ForbidigoPattern {
            pattern: DEFAULT_PATTERN.into(),
            pkg: String::new(),
            msg: String::new(),
        }]
    } else {
        opts.forbid.clone()
    };
    patterns
        .iter()
        .filter_map(CompiledPattern::compile)
        .collect()
}

fn expr_text(expr: &Expr) -> String {
    match expr {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", expr_text(&sel.x), sel.sel.name),
        Expr::ParenExpr(p) => expr_text(&p.x),
        Expr::StarExpr(s) => format!("*{}", expr_text(&s.x)),
        Expr::CallExpr(c) => {
            let mut s = expr_text(&c.fun);
            s.push('(');
            for (i, a) in c.args.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&expr_text(a));
            }
            if c.ellipsis.is_valid() {
                if !c.args.is_empty() {
                    s.push_str(", ");
                }
                s.push_str("...");
            }
            s.push(')');
            s
        }
        Expr::IndexExpr(i) => format!("{}[{}]", expr_text(&i.x), expr_text(&i.index)),
        Expr::UnaryExpr(u) => format!("{}{}", u.op, expr_text(&u.x)),
        Expr::BinaryExpr(b) => format!("{} {} {}", expr_text(&b.x), b.op, expr_text(&b.y)),
        Expr::BasicLit(lit) => lit.value.clone(),
        _ => String::new(),
    }
}

fn check_ident_or_selector(
    expr: &Expr,
    patterns: &[CompiledPattern],
    pending: &mut Vec<(u32, String)>,
) {
    match expr {
        Expr::Ident(_) | Expr::SelectorExpr(_) => {}
        _ => return,
    }
    let src = expr_text(expr);
    if src.is_empty() {
        return;
    }
    let texts = [src.as_str()];
    for p in patterns {
        // DEFERRED: analyze-types package path matching (pkg_re unused until then).
        let _ = &p.pkg_re;
        if p.matches(&texts) {
            let explanation = if p.msg.is_empty() {
                format!(" by pattern `{}`", p.pattern)
            } else {
                format!(" because `{}`", p.msg)
            };
            pending.push((
                expr.pos().0 as u32,
                format!("use of `{src}` forbidden{explanation}"),
            ));
        }
    }
}

fn visit_expr(expr: &Expr, patterns: &[CompiledPattern], pending: &mut Vec<(u32, String)>) {
    match expr {
        Expr::Ident(_) => {
            check_ident_or_selector(expr, patterns, pending);
        }
        Expr::SelectorExpr(sel) => {
            check_ident_or_selector(expr, patterns, pending);
            // Upstream: only descend into X when it is not a bare Ident.
            if !matches!(sel.x.as_ref(), Expr::Ident(_)) {
                visit_expr(&sel.x, patterns, pending);
            }
        }
        Expr::ParenExpr(p) => visit_expr(&p.x, patterns, pending),
        Expr::StarExpr(s) => visit_expr(&s.x, patterns, pending),
        Expr::UnaryExpr(u) => visit_expr(&u.x, patterns, pending),
        Expr::BinaryExpr(b) => {
            visit_expr(&b.x, patterns, pending);
            visit_expr(&b.y, patterns, pending);
        }
        Expr::CallExpr(c) => visit_call(c, patterns, pending),
        Expr::IndexExpr(i) => {
            visit_expr(&i.x, patterns, pending);
            visit_expr(&i.index, patterns, pending);
        }
        Expr::IndexListExpr(i) => {
            visit_expr(&i.x, patterns, pending);
            for idx in &i.indices {
                visit_expr(idx, patterns, pending);
            }
        }
        Expr::SliceExpr(s) => {
            visit_expr(&s.x, patterns, pending);
            if let Some(l) = &s.low {
                visit_expr(l, patterns, pending);
            }
            if let Some(h) = &s.high {
                visit_expr(h, patterns, pending);
            }
            if let Some(m) = &s.max {
                visit_expr(m, patterns, pending);
            }
        }
        Expr::TypeAssertExpr(t) => {
            visit_expr(&t.x, patterns, pending);
            if let Some(ty) = &t.ty {
                visit_expr(ty, patterns, pending);
            }
        }
        Expr::CompositeLit(c) => {
            if let Some(ty) = &c.ty {
                visit_expr(ty, patterns, pending);
            }
            for elt in &c.elts {
                visit_expr(elt, patterns, pending);
            }
        }
        Expr::KeyValueExpr(kv) => {
            visit_expr(&kv.key, patterns, pending);
            visit_expr(&kv.value, patterns, pending);
        }
        Expr::FuncLit(f) => {
            visit_func_type(&f.ty, patterns, pending);
            visit_block(&f.body.list, patterns, pending);
        }
        Expr::Ellipsis(e) => {
            if let Some(elt) = &e.elt {
                visit_expr(elt, patterns, pending);
            }
        }
        Expr::ArrayType(a) => {
            if let Some(len) = &a.len {
                visit_expr(len, patterns, pending);
            }
            visit_expr(&a.elt, patterns, pending);
        }
        Expr::StructType(s) => visit_field_list(Some(&s.fields), patterns, pending),
        Expr::FuncType(t) => visit_func_type(t, patterns, pending),
        Expr::InterfaceType(i) => visit_field_list(Some(&i.methods), patterns, pending),
        Expr::MapType(m) => {
            visit_expr(&m.key, patterns, pending);
            visit_expr(&m.value, patterns, pending);
        }
        Expr::ChanType(c) => visit_expr(&c.value, patterns, pending),
        Expr::BadExpr(_) | Expr::BasicLit(_) => {}
    }
}

fn visit_call(
    call: &guff::ast::CallExpr,
    patterns: &[CompiledPattern],
    pending: &mut Vec<(u32, String)>,
) {
    visit_expr(&call.fun, patterns, pending);
    for a in &call.args {
        visit_expr(a, patterns, pending);
    }
}

fn visit_field_list(
    fields: Option<&FieldList>,
    patterns: &[CompiledPattern],
    pending: &mut Vec<(u32, String)>,
) {
    let Some(fields) = fields else {
        return;
    };
    for f in &fields.list {
        visit_field(f, patterns, pending);
    }
}

fn visit_field(field: &Field, patterns: &[CompiledPattern], pending: &mut Vec<(u32, String)>) {
    // Upstream ignores field names; only walk the type.
    if let Some(ty) = &field.ty {
        visit_expr(ty, patterns, pending);
    }
}

fn visit_func_type(ty: &FuncType, patterns: &[CompiledPattern], pending: &mut Vec<(u32, String)>) {
    visit_field_list(ty.type_params.as_ref(), patterns, pending);
    visit_field_list(ty.params.as_ref(), patterns, pending);
    visit_field_list(ty.results.as_ref(), patterns, pending);
}

fn visit_value_spec(spec: &ValueSpec, patterns: &[CompiledPattern], pending: &mut Vec<(u32, String)>) {
    // Upstream ignores names; walk type and values only.
    if let Some(ty) = &spec.ty {
        visit_expr(ty, patterns, pending);
    }
    for v in &spec.values {
        visit_expr(v, patterns, pending);
    }
}

fn visit_type_spec(spec: &TypeSpec, patterns: &[CompiledPattern], pending: &mut Vec<(u32, String)>) {
    visit_field_list(spec.type_params.as_ref(), patterns, pending);
    visit_expr(&spec.ty, patterns, pending);
}

fn visit_gen_decl(decl: &GenDecl, patterns: &[CompiledPattern], pending: &mut Vec<(u32, String)>) {
    for spec in &decl.specs {
        match spec {
            Spec::ValueSpec(vs) => visit_value_spec(vs, patterns, pending),
            Spec::TypeSpec(ts) => visit_type_spec(ts, patterns, pending),
            Spec::ImportSpec(_) => {} // ignore import alias names
        }
    }
}

fn visit_block(stmts: &[Stmt], patterns: &[CompiledPattern], pending: &mut Vec<(u32, String)>) {
    for s in stmts {
        visit_stmt(s, patterns, pending);
    }
}

fn visit_stmt(stmt: &Stmt, patterns: &[CompiledPattern], pending: &mut Vec<(u32, String)>) {
    match stmt {
        Stmt::DeclStmt(d) => match &d.decl {
            Decl::GenDecl(g) => visit_gen_decl(g, patterns, pending),
            Decl::FuncDecl(f) => visit_func_decl(f, false, false, patterns, pending),
            Decl::BadDecl(_) => {}
        },
        Stmt::ExprStmt(e) => visit_expr(&e.x, patterns, pending),
        Stmt::SendStmt(s) => {
            visit_expr(&s.chan_, patterns, pending);
            visit_expr(&s.value, patterns, pending);
        }
        Stmt::IncDecStmt(s) => visit_expr(&s.x, patterns, pending),
        Stmt::AssignStmt(a) => {
            for e in &a.lhs {
                visit_expr(e, patterns, pending);
            }
            for e in &a.rhs {
                visit_expr(e, patterns, pending);
            }
        }
        Stmt::GoStmt(g) => visit_call(&g.call, patterns, pending),
        Stmt::DeferStmt(d) => visit_call(&d.call, patterns, pending),
        Stmt::ReturnStmt(r) => {
            for e in &r.results {
                visit_expr(e, patterns, pending);
            }
        }
        Stmt::BlockStmt(b) => visit_block(&b.list, patterns, pending),
        Stmt::IfStmt(i) => {
            if let Some(init) = &i.init {
                visit_stmt(init, patterns, pending);
            }
            visit_expr(&i.cond, patterns, pending);
            visit_block(&i.body.list, patterns, pending);
            if let Some(els) = &i.else_ {
                visit_stmt(els, patterns, pending);
            }
        }
        Stmt::CaseClause(c) => {
            for e in &c.list {
                visit_expr(e, patterns, pending);
            }
            visit_block(&c.body, patterns, pending);
        }
        Stmt::SwitchStmt(s) => {
            if let Some(init) = &s.init {
                visit_stmt(init, patterns, pending);
            }
            if let Some(tag) = &s.tag {
                visit_expr(tag, patterns, pending);
            }
            visit_block(&s.body.list, patterns, pending);
        }
        Stmt::TypeSwitchStmt(s) => {
            if let Some(init) = &s.init {
                visit_stmt(init, patterns, pending);
            }
            visit_stmt(&s.assign, patterns, pending);
            visit_block(&s.body.list, patterns, pending);
        }
        Stmt::CommClause(c) => {
            if let Some(comm) = &c.comm {
                visit_stmt(comm, patterns, pending);
            }
            visit_block(&c.body, patterns, pending);
        }
        Stmt::SelectStmt(s) => visit_block(&s.body.list, patterns, pending),
        Stmt::ForStmt(f) => {
            if let Some(init) = &f.init {
                visit_stmt(init, patterns, pending);
            }
            if let Some(cond) = &f.cond {
                visit_expr(cond, patterns, pending);
            }
            if let Some(post) = &f.post {
                visit_stmt(post, patterns, pending);
            }
            visit_block(&f.body.list, patterns, pending);
        }
        Stmt::RangeStmt(r) => {
            if let Some(key) = &r.key {
                visit_expr(key, patterns, pending);
            }
            if let Some(val) = &r.value {
                visit_expr(val, patterns, pending);
            }
            visit_expr(&r.x, patterns, pending);
            visit_block(&r.body.list, patterns, pending);
        }
        Stmt::LabeledStmt(l) => visit_stmt(&l.stmt, patterns, pending),
        Stmt::BranchStmt(_) | Stmt::EmptyStmt(_) | Stmt::BadStmt(_) => {}
    }
}

fn visit_func_decl(
    func: &FuncDecl,
    is_test_file: bool,
    exclude_examples: bool,
    patterns: &[CompiledPattern],
    pending: &mut Vec<(u32, String)>,
) {
    let is_example = is_test_file
        && func.recv.is_none()
        && func.name.name.starts_with("Example");
    if is_example && exclude_examples {
        return;
    }
    // Upstream walks Type + Body only (skips the function name Ident).
    visit_func_type(&func.ty, patterns, pending);
    if let Some(body) = &func.body {
        visit_block(&body.list, patterns, pending);
    }
}

fn is_whole_file_example(file: &File, is_test_file: bool) -> bool {
    if !is_test_file || file.decls.len() <= 1 {
        return false;
    }
    let mut num_examples = 0;
    let mut num_tests_and_benchmarks = 0;
    for decl in &file.decls {
        let Decl::FuncDecl(func) = decl else {
            continue;
        };
        if func.recv.is_some() {
            continue;
        }
        let name = func.name.name.as_str();
        if name.starts_with("Test") || name.starts_with("Benchmark") {
            num_tests_and_benchmarks += 1;
            break;
        }
        if name.starts_with("Example") {
            num_examples += 1;
        }
    }
    num_examples == 1 && num_tests_and_benchmarks == 0
}

fn visit_file(
    file: &File,
    filename: &str,
    exclude_examples: bool,
    patterns: &[CompiledPattern],
    pending: &mut Vec<(u32, String)>,
) {
    let is_test_file = filename.ends_with("_test.go");
    if exclude_examples && is_whole_file_example(file, is_test_file) {
        return;
    }
    for decl in &file.decls {
        match decl {
            Decl::GenDecl(g) => visit_gen_decl(g, patterns, pending),
            Decl::FuncDecl(f) => {
                visit_func_decl(f, is_test_file, exclude_examples, patterns, pending);
            }
            Decl::BadDecl(_) => {}
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "forbidigo requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<ForbidigoOptions>("forbidigo")
        .cloned()
        .unwrap_or_default();

    // DEFERRED: analyze-types (literal source matching only).
    let _analyze_types = options.analyze_types;

    let patterns = compile_patterns(&options);
    if patterns.is_empty() {
        return Ok(None);
    }

    let mut pending = Vec::new();
    let pkg = pass.pkg();
    let fset = pass.fset();
    for (i, file) in pass.files().iter().enumerate() {
        let fallback = fset.position(file.pos()).filename;
        let filename = pkg
            .compiled_go_files
            .get(i)
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or(fallback.as_str());
        visit_file(
            file,
            filename,
            options.exclude_godoc_examples,
            &patterns,
            &mut pending,
        );
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "forbidigo",
        doc: "Forbids identifiers",
        url: "https://github.com/ashanbrown/forbidigo",
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
    fn default_pattern_matches_fmt_println() {
        let opts = ForbidigoOptions::default();
        let pats = compile_patterns(&opts);
        assert!(pats[0].matches(&["fmt.Println"]));
        assert!(pats[0].matches(&["print"]));
        assert!(!pats[0].matches(&["fmt.Sprintf"]));
    }

    #[test]
    fn extracts_msg_from_hash_group() {
        let msg = extract_msg_from_pattern(r"\bioutil\b(# Use io and os packages instead of ioutil)?");
        assert_eq!(
            msg.as_deref(),
            Some("Use io and os packages instead of ioutil")
        );
    }
}
