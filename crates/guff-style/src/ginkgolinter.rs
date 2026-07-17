//! Port of [`github.com/nunnatsa/ginkgolinter`](https://github.com/nunnatsa/ginkgolinter)
//! (golangci-lint wrapper in `pkg/golinters/ginkgolinter`).
//!
//! Enforces idiomatic Ginkgo / Gomega assertion style. This first batch covers
//! the most common AST-detectable rules:
//!
//! - wrong length (`Expect(len(x)).To(Equal(n))`, `HaveLen(0)`, …)
//! - wrong nil (`Equal(nil)`, `x == nil` with `BeTrue`, …)
//! - wrong boolean (`Equal(true)` / `Equal(false)`)
//! - focus containers (`FDescribe` / `FIt` / …) when `forbid-focus-container`
//! - `Expect` + `Should`/`ShouldNot` when `force-expect-to`
//! - missing assertion (`Expect(x)` as a bare statement)
//!
//! DEFERRED (see DEVELOPMENT.md R13/R14): comparison rewrite, async
//! (`Eventually` func-call / intervals), error/`Succeed`/`HaveOccurred`
//! parity, `MatchError`, cap, type-compare, pointer compare, double-negation
//! / `force-tonot`, assertion description, spec pollution, comment suppress
//! directives, SuggestedFix.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, File, ImportSpec};
use guff::token::Token;
use guff::walk::{preorder, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::GinkgolinterOptions;

const MSG_LEN: &str = "ginkgo-linter: wrong length assertion";
const MSG_NIL: &str = "ginkgo-linter: wrong nil assertion";
const MSG_BOOL: &str = "ginkgo-linter: wrong boolean assertion";
const MSG_MISSING: &str = "ginkgo-linter: missing assertion method";
const MSG_FOCUS: &str =
    "ginkgo-linter: Focus container found. This is used only for local debug and should not be part of the actual source code";

fn is_ginkgo_path(path: &str) -> bool {
    matches!(
        path.trim_matches('"'),
        "github.com/onsi/ginkgo" | "github.com/onsi/ginkgo/v2"
    )
}

fn is_gomega_path(path: &str) -> bool {
    path.trim_matches('"') == "github.com/onsi/gomega"
}

fn import_local_name(imp: &ImportSpec) -> Option<&str> {
    imp.name.as_ref().map(|n| n.name.as_str())
}

#[derive(Clone, Copy)]
struct ImportInfo<'a> {
    /// `None` = not imported; `Some(".")` = dot; `Some(alias)` = named/default.
    ginkgo: Option<&'a str>,
    gomega: Option<&'a str>,
}

fn file_imports(file: &File) -> ImportInfo<'_> {
    let mut ginkgo = None;
    let mut gomega = None;
    for imp in &file.imports {
        let path = &imp.path.value;
        if is_ginkgo_path(path) {
            ginkgo = Some(match import_local_name(imp) {
                Some(".") => ".",
                Some(n) => n,
                None => "ginkgo",
            });
        } else if is_gomega_path(path) {
            gomega = Some(match import_local_name(imp) {
                Some(".") => ".",
                Some(n) => n,
                None => "gomega",
            });
        }
    }
    ImportInfo { ginkgo, gomega }
}

fn call_func_name(fun: &Expr) -> Option<&str> {
    match fun {
        Expr::Ident(id) => Some(id.name.as_str()),
        Expr::SelectorExpr(sel) => Some(sel.sel.name.as_str()),
        _ => None,
    }
}

fn call_pkg_qualifier(fun: &Expr) -> Option<&str> {
    match fun {
        Expr::SelectorExpr(sel) => match sel.x.as_ref() {
            Expr::Ident(id) => Some(id.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

/// True when `fun` refers to `name` under the given import alias.
fn matches_imported(fun: &Expr, alias: Option<&str>, name: &str) -> bool {
    let Some(alias) = alias else {
        return false;
    };
    let Some(fn_name) = call_func_name(fun) else {
        return false;
    };
    if fn_name != name {
        return false;
    }
    if alias == "." {
        return matches!(fun, Expr::Ident(_));
    }
    call_pkg_qualifier(fun) == Some(alias)
}

fn is_focus_container(name: &str) -> bool {
    matches!(
        name,
        "FDescribe" | "FContext" | "FWhen" | "FIt" | "FDescribeTable" | "FEntry"
    )
}

fn is_assertion_method(name: &str) -> bool {
    matches!(name, "To" | "ToNot" | "NotTo" | "Should" | "ShouldNot")
}

fn is_actual_method(name: &str) -> bool {
    matches!(
        name,
        "Expect"
            | "ExpectWithOffset"
            | "Ω"
            | "Eventually"
            | "EventuallyWithOffset"
            | "Consistently"
            | "ConsistentlyWithOffset"
    )
}

fn is_chain_helper(name: &str) -> bool {
    matches!(
        name,
        "WithOffset"
            | "WithArgs"
            | "Within"
            | "ProbeEvery"
            | "WithTimeout"
            | "WithPolling"
            | "MustPassRepeatedly"
            | "StopTrying"
    )
}

fn is_len_call(expr: &Expr) -> bool {
    matches!(expr, Expr::CallExpr(c) if matches!(c.fun.as_ref(), Expr::Ident(id) if id.name == "len"))
}

fn is_nil_ident(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(id) if id.name == "nil")
}

fn is_bool_lit(expr: &Expr, want: bool) -> bool {
    matches!(
        expr,
        Expr::Ident(id) if matches!((want, id.name.as_str()), (true, "true") | (false, "false"))
    )
}

fn is_zero_lit(expr: &Expr) -> bool {
    match expr {
        Expr::BasicLit(lit) if lit.kind == Some(Token::INT) => {
            lit.value == "0" || lit.value == "00"
        }
        _ => false,
    }
}

fn matcher_name(matcher: &Expr) -> Option<&str> {
    match matcher {
        Expr::CallExpr(c) => call_func_name(c.fun.as_ref()),
        Expr::Ident(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn matcher_call(matcher: &Expr) -> Option<&CallExpr> {
    match matcher {
        Expr::CallExpr(c) => Some(c),
        _ => None,
    }
}

/// Unwrap `Not(matcher)` → inner matcher.
fn unwrap_not<'a>(matcher: &'a Expr) -> &'a Expr {
    if let Expr::CallExpr(c) = matcher {
        if matches!(call_func_name(c.fun.as_ref()), Some("Not")) {
            if let Some(inner) = c.args.first() {
                return inner;
            }
        }
    }
    matcher
}

struct ParsedAssertion<'a> {
    actual_func: &'a str,
    actual_fun: &'a Expr,
    actual_arg: Option<&'a Expr>,
    assert_method: Option<&'a str>,
    matcher: Option<&'a Expr>,
    pos: u32,
}

/// Peel `Expect(...).WithOffset(1).Should(Equal(...))` into parts.
fn parse_assertion<'a>(call: &'a CallExpr) -> Option<ParsedAssertion<'a>> {
    let mut assert_method = None;
    let mut matcher = None;
    let mut node: &CallExpr = call;

    if let Some(name) = call_func_name(node.fun.as_ref()) {
        if is_assertion_method(name) {
            assert_method = Some(name);
            matcher = node.args.first();
            match node.fun.as_ref() {
                Expr::SelectorExpr(sel) => match sel.x.as_ref() {
                    Expr::CallExpr(inner) => node = inner,
                    _ => return None,
                },
                _ => return None,
            }
        }
    }

    loop {
        let Some(name) = call_func_name(node.fun.as_ref()) else {
            break;
        };
        if !is_chain_helper(name) {
            break;
        }
        match node.fun.as_ref() {
            Expr::SelectorExpr(sel) => match sel.x.as_ref() {
                Expr::CallExpr(inner) => node = inner,
                _ => break,
            },
            _ => break,
        }
    }

    let actual_fun = node.fun.as_ref();
    let actual_func = call_func_name(actual_fun)?;
    if !is_actual_method(actual_func) {
        return None;
    }

    let offset = if actual_func.ends_with("WithOffset") {
        1
    } else {
        0
    };

    Some(ParsedAssertion {
        actual_func,
        actual_fun,
        actual_arg: node.args.get(offset),
        assert_method,
        matcher,
        pos: call.pos().0 as u32,
    })
}

fn check_focus(call: &CallExpr, imports: ImportInfo<'_>, pending: &mut Vec<(u32, String)>) {
    let Some(name) = call_func_name(call.fun.as_ref()) else {
        return;
    };
    if !is_focus_container(name) {
        return;
    }
    if matches_imported(call.fun.as_ref(), imports.ginkgo, name) {
        pending.push((call.pos().0 as u32, MSG_FOCUS.to_string()));
    }
}

fn check_len_rule(
    actual: &Expr,
    matcher: &Expr,
    opts: &GinkgolinterOptions,
    pending: &mut Vec<(u32, String)>,
    pos: u32,
) -> bool {
    if opts.suppress_len_assertion {
        return false;
    }

    let matcher = unwrap_not(matcher);
    let mname = matcher_name(matcher).unwrap_or("");

    // Expect(len(x)).To(Equal(...)) / BeZero() / BeNumerically(...)
    if is_len_call(actual) {
        match mname {
            "Equal" | "BeZero" => {
                pending.push((pos, MSG_LEN.to_string()));
                return true;
            }
            "BeNumerically" => {
                if let Some(c) = matcher_call(matcher) {
                    if let Some(op) = c.args.first().and_then(|e| match e {
                        Expr::BasicLit(lit) if lit.kind == Some(Token::STRING) => {
                            Some(lit.value.trim_matches('"'))
                        }
                        _ => None,
                    }) {
                        let rhs_zero = c.args.get(1).map(is_zero_lit).unwrap_or(false);
                        if matches!(op, "==" | "!=") || (matches!(op, ">" | ">=") && rhs_zero) {
                            pending.push((pos, MSG_LEN.to_string()));
                            return true;
                        }
                        // `>= 1` is also treated as non-empty by upstream.
                        if op == ">=" {
                            if let Some(Expr::BasicLit(lit)) = c.args.get(1) {
                                if lit.kind == Some(Token::INT) && lit.value == "1" {
                                    pending.push((pos, MSG_LEN.to_string()));
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Expect(len(x) == n).To(BeTrue()) / Equal(true)
    if let Expr::BinaryExpr(bin) = actual {
        if matches!(bin.op, Token::EQL | Token::NEQ)
            && (is_len_call(bin.x.as_ref()) || is_len_call(bin.y.as_ref()))
            && matches!(mname, "BeTrue" | "BeFalse" | "Equal")
        {
            pending.push((pos, MSG_LEN.to_string()));
            return true;
        }
    }

    false
}

fn check_havelen0(
    matcher: &Expr,
    opts: &GinkgolinterOptions,
    pending: &mut Vec<(u32, String)>,
    pos: u32,
) -> bool {
    if opts.allow_havelen_zero {
        return false;
    }
    let matcher = unwrap_not(matcher);
    if matcher_name(matcher) != Some("HaveLen") {
        return false;
    }
    let Some(c) = matcher_call(matcher) else {
        return false;
    };
    if c.args.first().map(is_zero_lit).unwrap_or(false) {
        pending.push((pos, MSG_LEN.to_string()));
        return true;
    }
    false
}

fn check_equal_nil(
    matcher: &Expr,
    opts: &GinkgolinterOptions,
    pending: &mut Vec<(u32, String)>,
    pos: u32,
) -> bool {
    if opts.suppress_nil_assertion {
        return false;
    }
    let matcher = unwrap_not(matcher);
    if matcher_name(matcher) != Some("Equal") {
        return false;
    }
    let Some(c) = matcher_call(matcher) else {
        return false;
    };
    if c.args.first().map(is_nil_ident).unwrap_or(false) {
        pending.push((pos, MSG_NIL.to_string()));
        return true;
    }
    false
}

fn check_equal_bool(matcher: &Expr, pending: &mut Vec<(u32, String)>, pos: u32) -> bool {
    let matcher = unwrap_not(matcher);
    if matcher_name(matcher) != Some("Equal") {
        return false;
    }
    let Some(c) = matcher_call(matcher) else {
        return false;
    };
    let Some(arg) = c.args.first() else {
        return false;
    };
    if is_bool_lit(arg, true) || is_bool_lit(arg, false) {
        pending.push((pos, MSG_BOOL.to_string()));
        return true;
    }
    false
}

fn check_nil_compare(
    actual: &Expr,
    matcher: &Expr,
    opts: &GinkgolinterOptions,
    pending: &mut Vec<(u32, String)>,
    pos: u32,
) -> bool {
    if opts.suppress_nil_assertion {
        return false;
    }
    let matcher = unwrap_not(matcher);
    let mname = matcher_name(matcher).unwrap_or("");
    if !matches!(mname, "BeTrue" | "BeFalse" | "Equal") {
        return false;
    }
    let Expr::BinaryExpr(bin) = actual else {
        return false;
    };
    if !matches!(bin.op, Token::EQL | Token::NEQ) {
        return false;
    }
    let has_nil = is_nil_ident(bin.x.as_ref()) || is_nil_ident(bin.y.as_ref());
    if !has_nil {
        return false;
    }
    // Prefer length rule if one side is len(...).
    if is_len_call(bin.x.as_ref()) || is_len_call(bin.y.as_ref()) {
        return false;
    }
    pending.push((pos, MSG_NIL.to_string()));
    true
}

fn check_assertion(
    assertion: &ParsedAssertion<'_>,
    opts: &GinkgolinterOptions,
    pending: &mut Vec<(u32, String)>,
) {
    // Missing assertion: Expect(x) as a statement.
    if assertion.assert_method.is_none() {
        pending.push((assertion.pos, MSG_MISSING.to_string()));
        return;
    }

    if opts.force_expect_to
        && matches!(assertion.actual_func, "Expect" | "ExpectWithOffset")
        && matches!(assertion.assert_method, Some("Should") | Some("ShouldNot"))
    {
        pending.push((
            assertion.pos,
            format!(
                "ginkgo-linter: must not use {} with {}",
                assertion.actual_func,
                assertion.assert_method.unwrap_or("")
            ),
        ));
        // Upstream continues applying other rules after this one.
    }

    let Some(matcher) = assertion.matcher else {
        return;
    };
    let Some(actual) = assertion.actual_arg else {
        return;
    };

    // Order mirrors upstream: len → nil-compare → matcher-only (HaveLen0 / EqualBool / EqualNil).
    if check_len_rule(actual, matcher, opts, pending, assertion.pos) {
        return;
    }
    if check_nil_compare(actual, matcher, opts, pending, assertion.pos) {
        return;
    }
    if check_havelen0(matcher, opts, pending, assertion.pos) {
        return;
    }
    if check_equal_bool(matcher, pending, assertion.pos) {
        return;
    }
    let _ = check_equal_nil(matcher, opts, pending, assertion.pos);
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ginkgolinter requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<GinkgolinterOptions>("ginkgolinter")
        .cloned()
        .unwrap_or_default();

    let mut pending: Vec<(u32, String)> = Vec::new();

    for file in pass.files() {
        let imports = file_imports(file);
        if imports.ginkgo.is_none() && imports.gomega.is_none() {
            continue;
        }

        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::CallExpr(call) => {
                    if opts.forbid_focus_container && imports.ginkgo.is_some() {
                        check_focus(call, imports, &mut pending);
                    }
                }
                NodeRef::ExprStmt(stmt) => {
                    if imports.gomega.is_none() {
                        return true;
                    }
                    let Expr::CallExpr(call) = &stmt.x else {
                        return true;
                    };
                    let Some(assertion) = parse_assertion(call) else {
                        return true;
                    };
                    if !matches_imported(assertion.actual_fun, imports.gomega, assertion.actual_func)
                    {
                        return true;
                    }
                    check_assertion(&assertion, &opts, &mut pending);
                }
                _ => {}
            }
            true
        });
    }

    for (pos, msg) in pending {
        pass.reportf(pos, &msg);
    }
    Ok(None)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_names() {
        assert!(is_focus_container("FDescribe"));
        assert!(is_focus_container("FIt"));
        assert!(!is_focus_container("Describe"));
    }

    #[test]
    fn assertion_and_actual_names() {
        assert!(is_assertion_method("Should"));
        assert!(is_actual_method("Expect"));
        assert!(!is_actual_method("Describe"));
    }
}
