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
//! directives, SuggestedFix (TextEdit).

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, File, ImportSpec};
use guff::token::Token;
use guff::walk::{preorder, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn, Diagnostic, SuggestedFix, TextEdit};

use crate::options::GinkgolinterOptions;

const MSG_LEN: &str = "ginkgo-linter: wrong length assertion";
const MSG_NIL: &str = "ginkgo-linter: wrong nil assertion";
const MSG_BOOL: &str = "ginkgo-linter: wrong boolean assertion";
const MSG_FOCUS: &str =
    "ginkgo-linter: Focus container found. This is used only for local debug and should not be part of the actual source code";

fn with_suggestion(base: &str, suggestion: &str) -> String {
    format!("{base}. Consider using `{suggestion}` instead")
}

/// A finding whose suggestion is also its fix: upstream replaces the whole
/// assertion with the rewritten text, and withholds the fix when the rewrite
/// comes out identical to what is already written.
fn push_suggestion(
    assertion: &ParsedAssertion<'_>,
    base: &str,
    sug: &str,
    pending: &mut Vec<Finding>,
) {
    let edit = (sug != assertion.old_expr).then(|| TextEdit {
        pos: assertion.pos,
        end: assertion.end,
        new_text: sug.to_string(),
    });
    pending.push((assertion.pos, with_suggestion(base, sug), edit));
}

/// A reported position, its message, and the edit when there is one.
type Finding = (u32, String, Option<TextEdit>);

fn missing_assertion_msg(actual_func: &str) -> String {
    // Upstream wording (nunnatsa/ginkgolinter) — lists To/ToNot/NotTo even when
    // Should/ShouldNot would also be valid assertion methods.
    format!(
        "ginkgo-linter: \"{actual_func}\": missing assertion method. Expected \"To()\", \"ToNot()\" or \"NotTo()\""
    )
}

/// ginkgolinter renders with `GoFmtFormatter`
/// (`internal/formatter/formatter.go`), which is `printer.Fprint` — go/printer,
/// not an approximation. The walker this replaced always put blanks around a
/// binary operator and answered `"<expr>"` for everything outside nine arms.
fn expr_string(pass: &Pass<'_>, e: &Expr) -> String {
    code::node_text(pass, e).unwrap_or_default()
}

fn suggest_assert(actual_func: &str, subject: &str, assert_method: &str, matcher: &str) -> String {
    format!("{actual_func}({subject}).{assert_method}({matcher})")
}

fn len_inner(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::CallExpr(c)
            if matches!(c.fun.as_ref(), Expr::Ident(id) if id.name == "len") =>
        {
            c.args.first()
        }
        _ => None,
    }
}

fn lit_int_value(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::BasicLit(lit) if lit.kind == Some(Token::INT) => Some(lit.value.as_str()),
        _ => None,
    }
}
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
    /// The whole assertion's span and its current text — upstream's
    /// `NewBuilder(oldExpr, …)` keeps `oldExpr.Pos()`, `oldExpr.End()` and the
    /// formatted original, and offers a fix only when the rewrite differs from
    /// that original.
    end: u32,
    old_expr: String,
}

/// Peel `Expect(...).WithOffset(1).Should(Equal(...))` into parts.
fn parse_assertion<'a>(pass: &Pass<'_>, call: &'a CallExpr) -> Option<ParsedAssertion<'a>> {
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
        end: call.end().0 as u32,
        old_expr: expr_string(pass, &Expr::CallExpr(call.clone())),
    })
}

fn check_focus(call: &CallExpr, imports: ImportInfo<'_>, pending: &mut Vec<Finding>) {
    let Some(name) = call_func_name(call.fun.as_ref()) else {
        return;
    };
    if !is_focus_container(name) {
        return;
    }
    if matches_imported(call.fun.as_ref(), imports.ginkgo, name) {
        pending.push((call.pos().0 as u32, MSG_FOCUS.to_string(), None));
    }
}

fn check_len_rule(
    pass: &Pass<'_>,
    assertion: &ParsedAssertion<'_>,
    actual: &Expr,
    matcher: &Expr,
    opts: &GinkgolinterOptions,
    pending: &mut Vec<Finding>,
) -> bool {
    if opts.suppress_len_assertion {
        return false;
    }

    let assert_method = assertion.assert_method.unwrap_or("Should");
    let matcher = unwrap_not(matcher);
    let mname = matcher_name(matcher).unwrap_or("");

    // Expect(len(x)).To(Equal(...)) / BeZero() / BeNumerically(...)
    if let Some(inner) = len_inner(actual) {
        let subject = expr_string(pass, inner);
        let push_len = |pending: &mut Vec<Finding>, matcher_sug: &str| {
            let sug = suggest_assert(assertion.actual_func, &subject, assert_method, matcher_sug);
            push_suggestion(assertion, MSG_LEN, &sug, pending);
        };
        match mname {
            "Equal" => {
                if let Some(c) = matcher_call(matcher) {
                    if let Some(arg) = c.args.first() {
                        if is_zero_lit(arg) {
                            push_len(pending, "BeEmpty()");
                        } else {
                            push_len(pending, &format!("HaveLen({})", expr_string(pass, arg)));
                        }
                        return true;
                    }
                }
                pending.push((assertion.pos, MSG_LEN.to_string(), None));
                return true;
            }
            "BeZero" => {
                push_len(pending, "BeEmpty()");
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
                        if op == "==" && rhs_zero {
                            push_len(pending, "BeEmpty()");
                            return true;
                        }
                        if matches!(op, ">" | ">=") && rhs_zero {
                            let method = match assert_method {
                                "To" => "ToNot",
                                "Should" => "ShouldNot",
                                other => other,
                            };
                            let sug = suggest_assert(
                                assertion.actual_func,
                                &subject,
                                method,
                                "BeEmpty()",
                            );
                            push_suggestion(assertion, MSG_LEN, &sug, pending);
                            return true;
                        }
                        if op == "!=" && rhs_zero {
                            let method = match assert_method {
                                "To" => "ToNot",
                                "Should" => "ShouldNot",
                                other => other,
                            };
                            let sug = suggest_assert(
                                assertion.actual_func,
                                &subject,
                                method,
                                "BeEmpty()",
                            );
                            push_suggestion(assertion, MSG_LEN, &sug, pending);
                            return true;
                        }
                        if matches!(op, "==" | "!=") {
                            if let Some(n) = c.args.get(1).and_then(lit_int_value) {
                                if op == "==" {
                                    push_len(pending, &format!("HaveLen({n})"));
                                } else {
                                    let method = match assert_method {
                                        "To" => "ToNot",
                                        "Should" => "ShouldNot",
                                        other => other,
                                    };
                                    let sug = suggest_assert(
                                        assertion.actual_func,
                                        &subject,
                                        method,
                                        &format!("HaveLen({n})"),
                                    );
                                    push_suggestion(assertion, MSG_LEN, &sug, pending);
                                }
                                return true;
                            }
                        }
                        // `>= 1` is also treated as non-empty by upstream.
                        if op == ">=" {
                            if let Some(Expr::BasicLit(lit)) = c.args.get(1) {
                                if lit.kind == Some(Token::INT) && lit.value == "1" {
                                    let method = match assert_method {
                                        "To" => "ToNot",
                                        "Should" => "ShouldNot",
                                        other => other,
                                    };
                                    let sug = suggest_assert(
                                        assertion.actual_func,
                                        &subject,
                                        method,
                                        "BeEmpty()",
                                    );
                                    push_suggestion(assertion, MSG_LEN, &sug, pending);
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
            let (len_side, other) = if is_len_call(bin.x.as_ref()) {
                (bin.x.as_ref(), bin.y.as_ref())
            } else {
                (bin.y.as_ref(), bin.x.as_ref())
            };
            if let Some(inner) = len_inner(len_side) {
                let subject = expr_string(pass, inner);
                let matcher_sug = if is_zero_lit(other) {
                    "BeEmpty()".to_string()
                } else {
                    format!("HaveLen({})", expr_string(pass, other))
                };
                let use_neg = matches!(bin.op, Token::NEQ)
                    ^ matches!(mname, "BeFalse")
                    ^ (matches!(mname, "Equal")
                        && matcher_call(matcher)
                            .and_then(|c| c.args.first())
                            .map(|a| is_bool_lit(a, false))
                            .unwrap_or(false));
                let method = if use_neg {
                    match assert_method {
                        "To" => "ToNot",
                        "Should" => "ShouldNot",
                        other => other,
                    }
                } else {
                    assert_method
                };
                let sug = suggest_assert(assertion.actual_func, &subject, method, &matcher_sug);
                push_suggestion(assertion, MSG_LEN, &sug, pending);
                return true;
            }
            pending.push((assertion.pos, MSG_LEN.to_string(), None));
            return true;
        }
    }

    false
}

fn check_havelen0(
    pass: &Pass<'_>,
    assertion: &ParsedAssertion<'_>,
    matcher: &Expr,
    opts: &GinkgolinterOptions,
    pending: &mut Vec<Finding>,
) -> bool {
    if opts.allow_havelen_zero {
        return false;
    }
    let assert_method = assertion.assert_method.unwrap_or("Should");
    let matcher = unwrap_not(matcher);
    if matcher_name(matcher) != Some("HaveLen") {
        return false;
    }
    let Some(c) = matcher_call(matcher) else {
        return false;
    };
    if c.args.first().map(is_zero_lit).unwrap_or(false) {
        let subject = assertion
            .actual_arg
            .map(|e| expr_string(pass, e))
            .unwrap_or_else(|| "<expr>".into());
        let sug = suggest_assert(assertion.actual_func, &subject, assert_method, "BeEmpty()");
        push_suggestion(assertion, MSG_LEN, &sug, pending);
        return true;
    }
    false
}

fn check_equal_nil(
    pass: &Pass<'_>,
    assertion: &ParsedAssertion<'_>,
    matcher: &Expr,
    opts: &GinkgolinterOptions,
    pending: &mut Vec<Finding>,
) -> bool {
    if opts.suppress_nil_assertion {
        return false;
    }
    let assert_method = assertion.assert_method.unwrap_or("Should");
    let matcher = unwrap_not(matcher);
    if matcher_name(matcher) != Some("Equal") {
        return false;
    }
    let Some(c) = matcher_call(matcher) else {
        return false;
    };
    if c.args.first().map(is_nil_ident).unwrap_or(false) {
        let subject = assertion
            .actual_arg
            .map(|e| expr_string(pass, e))
            .unwrap_or_else(|| "<expr>".into());
        let sug = suggest_assert(assertion.actual_func, &subject, assert_method, "BeNil()");
        push_suggestion(assertion, MSG_NIL, &sug, pending);
        return true;
    }
    false
}

fn check_equal_bool(
    pass: &Pass<'_>,
    assertion: &ParsedAssertion<'_>,
    matcher: &Expr,
    pending: &mut Vec<Finding>,
) -> bool {
    let assert_method = assertion.assert_method.unwrap_or("Should");
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
    let bool_matcher = if is_bool_lit(arg, true) {
        "BeTrue()"
    } else if is_bool_lit(arg, false) {
        "BeFalse()"
    } else {
        return false;
    };
    let subject = assertion
        .actual_arg
        .map(|e| expr_string(pass, e))
        .unwrap_or_else(|| "<expr>".into());
    let sug = suggest_assert(assertion.actual_func, &subject, assert_method, bool_matcher);
    push_suggestion(assertion, MSG_BOOL, &sug, pending);
    true
}

fn check_nil_compare(
    pass: &Pass<'_>,
    assertion: &ParsedAssertion<'_>,
    actual: &Expr,
    matcher: &Expr,
    opts: &GinkgolinterOptions,
    pending: &mut Vec<Finding>,
) -> bool {
    if opts.suppress_nil_assertion {
        return false;
    }
    let assert_method = assertion.assert_method.unwrap_or("Should");
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
    let subject = if is_nil_ident(bin.x.as_ref()) {
        expr_string(pass, bin.y.as_ref())
    } else {
        expr_string(pass, bin.x.as_ref())
    };
    let use_neg = matches!(bin.op, Token::NEQ)
        ^ matches!(mname, "BeFalse")
        ^ (matches!(mname, "Equal")
            && matcher_call(matcher)
                .and_then(|c| c.args.first())
                .map(|a| is_bool_lit(a, false))
                .unwrap_or(false));
    let method = if use_neg {
        match assert_method {
            "To" => "ToNot",
            "Should" => "ShouldNot",
            other => other,
        }
    } else {
        assert_method
    };
    let sug = suggest_assert(assertion.actual_func, &subject, method, "BeNil()");
    push_suggestion(assertion, MSG_NIL, &sug, pending);
    true
}

fn check_assertion(
    pass: &Pass<'_>,
    assertion: &ParsedAssertion<'_>,
    opts: &GinkgolinterOptions,
    pending: &mut Vec<Finding>,
) {
    // Missing assertion: Expect(x) as a statement.
    if assertion.assert_method.is_none() {
        pending.push((assertion.pos, missing_assertion_msg(assertion.actual_func), None));
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
            None,
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
    if check_len_rule(pass, assertion, actual, matcher, opts, pending) {
        return;
    }
    if check_nil_compare(pass, assertion, actual, matcher, opts, pending) {
        return;
    }
    if check_havelen0(pass, assertion, matcher, opts, pending) {
        return;
    }
    if check_equal_bool(pass, assertion, matcher, pending) {
        return;
    }
    let _ = check_equal_nil(pass, assertion, matcher, opts, pending);
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ginkgolinter requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<GinkgolinterOptions>("ginkgolinter")
        .cloned()
        .unwrap_or_default();

    let mut pending: Vec<Finding> = Vec::new();

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
                    let Some(assertion) = parse_assertion(pass, call) else {
                        return true;
                    };
                    if !matches_imported(assertion.actual_fun, imports.gomega, assertion.actual_func)
                    {
                        return true;
                    }
                    check_assertion(pass, &assertion, &opts, &mut pending);
                }
                _ => {}
            }
            true
        });
    }

    for (pos, message, edit) in pending {
        let Some(edit) = edit else {
            pass.reportf(pos, &message);
            continue;
        };
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: String::new(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
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
