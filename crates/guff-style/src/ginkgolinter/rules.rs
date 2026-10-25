//! `internal/rules`. Each rule answers whether it handled the expression; the
//! first that does ends the list, as `Rules.Apply` does.

use guff::token::Token;
use guff_types::arena::{TypeData, TypeId};

use super::expression::{arg, mt, ArgPayload, Duration, GomegaExpression, Info, MatchErrKind, Matcher};
use super::report::Builder;
use super::types::Tc;
use super::Config;

const WRONG_LENGTH: &str = "wrong length assertion";
const WRONG_CAP: &str = "wrong cap assertion";
const WRONG_NIL: &str = "wrong nil assertion";
const WRONG_ERR: &str = "wrong error assertion";
const WRONG_BOOL: &str = "wrong boolean assertion";
const WRONG_COMPARE: &str = "wrong comparison assertion";

type Ctx<'a, 'b, 'o> = (&'a Tc<'b>, &'a mut GomegaExpression<'o>, &'a Config, &'a mut Builder);

#[derive(Clone, Copy)]
enum Rule {
    ForceExpectTo,
    Len,
    Cap,
    Comparison,
    NilCompare,
    ComparePointer,
    ErrorEqualNil,
    MatchError,
    MatcherOnly,
    EqualDifferentTypes,
    HaveOccurred,
    Succeed,
    AssertionDescription,
    AsyncFuncCall,
    AsyncTimeIntervals,
    AsyncSucceed,
    HaveLen0,
    EqualBool,
    EqualNil,
    DoubleNegative,
    SimplifyNot,
}

const RULES: &[Rule] = &[
    Rule::ForceExpectTo,
    Rule::Len,
    Rule::Cap,
    Rule::Comparison,
    Rule::NilCompare,
    Rule::ComparePointer,
    Rule::ErrorEqualNil,
    Rule::MatchError,
    Rule::MatcherOnly,
    Rule::EqualDifferentTypes,
    Rule::HaveOccurred,
    Rule::Succeed,
    Rule::AssertionDescription,
];

const ASYNC_RULES: &[Rule] = &[
    Rule::AsyncFuncCall,
    Rule::AsyncTimeIntervals,
    Rule::ErrorEqualNil,
    Rule::MatchError,
    Rule::AsyncSucceed,
    Rule::AssertionDescription,
    Rule::MatcherOnly,
];

const MATCHER_ONLY_RULES: &[Rule] = &[
    Rule::HaveLen0,
    Rule::EqualBool,
    Rule::EqualNil,
    Rule::DoubleNegative,
    // must be the last rule in the list
    Rule::SimplifyNot,
];

fn apply_all(rules: &[Rule], tc: &Tc<'_>, g: &mut GomegaExpression<'_>, c: &Config, b: &mut Builder) -> bool {
    rules.iter().any(|r| apply(*r, (tc, g, c, b)))
}

pub(super) fn apply_sync(tc: &Tc<'_>, g: &mut GomegaExpression<'_>, c: &Config, b: &mut Builder) {
    apply_all(RULES, tc, g, c, b);
}

pub(super) fn apply_async(tc: &Tc<'_>, g: &mut GomegaExpression<'_>, c: &Config, b: &mut Builder) {
    apply_all(ASYNC_RULES, tc, g, c, b);
}

/// `MissingAssertionRule`.
pub(super) fn apply_missing_assertion(g: &GomegaExpression<'_>, b: &mut Builder) -> bool {
    if !g.is_missing_assertion() {
        return false;
    }
    let name = &g.actual_func_name;
    b.add_issue(
        false,
        format!(
            "{}: missing assertion method. Expected {}",
            go_quote(name),
            allowed_assertion_methods(name)
        ),
    );
    true
}

/// `gomegainfo.GetAllowedAssertionMethods`.
fn allowed_assertion_methods(name: &str) -> &'static str {
    match name {
        "Expect" | "ExpectWithOffset" => r#""To()", "ToNot()" or "NotTo()""#,
        "Eventually" | "EventuallyWithOffset" | "Consistently" | "ConsistentlyWithOffset" => {
            r#""Should()" or "ShouldNot()""#
        }
        "Ω" => r#""Should()", "To()", "ShouldNot()", "ToNot()" or "NotTo()""#,
        _ => r#"one of "To/NotTo/ToNot" (for Expect assertions) or "Should/ShouldNot" (for Eventually/Consistently assertions)"#,
    }
}

/// `%q` of a Go string.
pub(super) fn go_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn apply(rule: Rule, (tc, g, c, b): Ctx<'_, '_, '_>) -> bool {
    match rule {
        Rule::ForceExpectTo => force_expect_to(g, c, b),
        Rule::Len => len_rule(g, c, b),
        Rule::Cap => cap_rule(g, c, b),
        Rule::Comparison => comparison_rule(tc, g, c, b),
        Rule::NilCompare => nil_compare_rule(g, c, b),
        Rule::ComparePointer => compare_pointer_rule(tc, g, c, b),
        Rule::ErrorEqualNil => error_equal_nil_rule(g, c, b),
        Rule::MatchError => match_error_rule(tc, g, b),
        Rule::MatcherOnly => apply_all(MATCHER_ONLY_RULES, tc, g, c, b),
        Rule::EqualDifferentTypes => !c.suppress_type_compare && {
            let m = g.matcher.take().expect("matcher");
            let r = check_equal_different_types(tc, g, &m, false, b);
            g.matcher = Some(m);
            r
        },
        Rule::HaveOccurred => have_occurred_rule(g, c, b),
        Rule::Succeed => succeed_rule(g, c, b),
        Rule::AssertionDescription => {
            if c.force_assertion_description && !g.has_description() {
                b.add_issue(false, "missing assertion description".to_string());
                true
            } else {
                false
            }
        }
        Rule::AsyncFuncCall => {
            async_func_call_rule(g, c, b);
            false
        }
        Rule::AsyncTimeIntervals => {
            async_time_intervals_rule(g, c, b);
            false
        }
        Rule::AsyncSucceed => {
            async_succeed_rule(g, b);
            false
        }
        Rule::HaveLen0 => {
            if g.matcher_type_is(mt::HAVE_LEN_ZERO) && !c.allow_have_len0 {
                g.set_matcher_be_empty();
                b.add_issue(true, WRONG_LENGTH.to_string());
                true
            } else {
                false
            }
        }
        Rule::EqualBool => equal_bool_rule(g, b),
        Rule::EqualNil => {
            if !c.suppress_nil && g.matcher_type_is(mt::EQUAL_VALUE) {
                g.set_matcher_be_nil();
                b.add_issue(true, WRONG_NIL.to_string());
                true
            } else {
                false
            }
        }
        Rule::DoubleNegative => {
            if g.matcher_type_is(mt::BE_FALSE) && g.is_negative_assertion() {
                g.reverse_assertion_func_logic();
                g.set_matcher_be_true();
                b.add_issue(true, "avoid double negative assertion".to_string());
                true
            } else {
                false
            }
        }
        Rule::SimplifyNot => {
            if c.force_tonot && g.matcher.as_ref().is_some_and(|m| m.has_not) {
                b.add_issue(true, "simplify negation by removing the 'Not' matcher".to_string());
                true
            } else {
                false
            }
        }
    }
}

/// `ForceExpectToRule` — always answers `false` so the other rules still run.
fn force_expect_to(g: &mut GomegaExpression<'_>, c: &Config, b: &mut Builder) -> bool {
    if !c.force_expect_to {
        return false;
    }
    if g.actual_func_name != "Expect" && g.actual_func_name != "ExpectWithOffset" {
        return false;
    }
    let new_name = match g.assertion_func_name.as_str() {
        "Should" => "To",
        "ShouldNot" => "ToNot",
        _ => return false,
    };
    g.replace_assertion_method(new_name);
    b.add_issue(
        true,
        format!("must not use {} with {}", g.actual_func_name, g.orig_assertion_func_name),
    );
    false
}

fn be_numerically_op(g: &GomegaExpression<'_>) -> Option<Token> {
    match g.info() {
        Info::BeNumerically { op, .. } => Some(*op),
        _ => None,
    }
}

/// The `isApplied` LenRule and CapRule share.
fn len_or_cap_applies(g: &GomegaExpression<'_>, func_arg: u64, cmp_arg: u64) -> bool {
    if g.actual_arg_type_is(func_arg) {
        if g.matcher_type_is(mt::EQUAL | mt::BE_ZERO) {
            return true;
        }
        if g.matcher_type_is(mt::BE_NUMERICALLY) {
            let op = be_numerically_op(g);
            return op == Some(Token::EQL)
                || op == Some(Token::NEQ)
                || g.matcher_type_is(mt::EQUAL_ZERO | mt::GREATER_THAN_ZERO);
        }
    }
    g.actual_arg_type_is(cmp_arg) && g.matcher_type_is(mt::BE_TRUE | mt::BE_FALSE | mt::EQUAL_BOOL_VALUE)
}

/// `LenRule`.
fn len_rule(g: &mut GomegaExpression<'_>, c: &Config, b: &mut Builder) -> bool {
    if c.suppress_len || !len_or_cap_applies(g, arg::LEN_FUNC, arg::LEN_COMPARISON) {
        return false;
    }
    let fixed = if g.actual_arg_type_is(arg::LEN_FUNC) {
        len_fix_equal(g)
    } else if g.actual_arg_type_is(arg::LEN_COMPARISON) {
        len_fix_comparison(g)
    } else {
        false
    };
    if fixed {
        b.add_issue(true, WRONG_LENGTH.to_string());
    }
    fixed
}

fn len_fix_equal(g: &mut GomegaExpression<'_>) -> bool {
    if g.matcher_type_is(mt::EQUAL) {
        if !g.set_len_numeric_matcher() {
            return false;
        }
    } else if g.matcher_type_is(mt::BE_ZERO) {
        g.set_matcher_be_empty();
    } else if g.matcher_type_is(mt::BE_NUMERICALLY) {
        let op = be_numerically_op(g);
        if op == Some(Token::EQL) {
            if !g.set_len_numeric_matcher() {
                return false;
            }
        } else if op == Some(Token::NEQ) {
            g.reverse_assertion_func_logic();
            if !g.set_len_numeric_matcher() {
                return false;
            }
        } else if g.matcher_type_is(mt::GREATER_THAN_ZERO) {
            g.reverse_assertion_func_logic();
            g.set_matcher_be_empty();
        } else {
            return false;
        }
    } else {
        return false;
    }
    g.replace_actual_with_its_first_arg();
    true
}

fn len_fix_comparison(g: &mut GomegaExpression<'_>) -> bool {
    let ArgPayload::FuncComparison { op, val, arg, .. } = g.arg() else {
        return false;
    };
    let (op, val, func_arg) = (*op, val.clone(), arg.clone());
    if op == Token::NEQ {
        g.reverse_assertion_func_logic();
    } else if op != Token::EQL {
        return false;
    }
    if g.matcher_type_is(mt::BOOL_VALUE_FALSE) {
        g.reverse_assertion_func_logic();
    }
    if val.is_value_zero() {
        g.set_matcher_be_empty();
    } else if let Some(e) = val.value_expr() {
        g.set_matcher_len(e);
    }
    g.replace_actual(func_arg);
    true
}

/// `CapRule`.
fn cap_rule(g: &mut GomegaExpression<'_>, c: &Config, b: &mut Builder) -> bool {
    if c.suppress_len || !len_or_cap_applies(g, arg::CAP_FUNC, arg::CAP_COMPARISON) {
        return false;
    }
    let fixed = if g.actual_arg_type_is(arg::CAP_FUNC) {
        cap_fix_equal(g)
    } else if g.actual_arg_type_is(arg::CAP_COMPARISON) {
        cap_fix_comparison(g)
    } else {
        false
    };
    if fixed {
        b.add_issue(true, WRONG_CAP.to_string());
    }
    fixed
}

fn cap_fix_equal(g: &mut GomegaExpression<'_>) -> bool {
    match g.info() {
        Info::Equal(v) => {
            let Some(e) = v.value_expr() else {
                return false;
            };
            g.set_matcher_cap(e);
        }
        Info::BeZero => g.set_matcher_cap_zero(),
        Info::BeNumerically { op, value, .. } => {
            let (op, e) = (*op, value.value_expr());
            if op == Token::EQL {
                let Some(e) = e else { return false };
                g.set_matcher_cap(e);
            } else if op == Token::NEQ {
                g.reverse_assertion_func_logic();
                let Some(e) = e else { return false };
                g.set_matcher_cap(e);
            } else if g.matcher_type_is(mt::GREATER_THAN_ZERO) {
                g.reverse_assertion_func_logic();
                g.set_matcher_cap_zero();
            } else {
                return false;
            }
        }
        _ => return false,
    }
    g.replace_actual_with_its_first_arg();
    true
}

fn cap_fix_comparison(g: &mut GomegaExpression<'_>) -> bool {
    let ArgPayload::FuncComparison { op, val, arg, .. } = g.arg() else {
        return false;
    };
    let (op, val, func_arg) = (*op, val.clone(), arg.clone());
    if op == Token::NEQ {
        g.reverse_assertion_func_logic();
    } else if op != Token::EQL {
        return false;
    }
    if let Some(e) = val.value_expr() {
        g.set_matcher_cap(e);
    }
    g.replace_actual(func_arg);
    if g.matcher_type_is(mt::BOOL_VALUE_FALSE) {
        g.reverse_assertion_func_logic();
    }
    true
}

/// `ComparisonRule`.
fn comparison_rule(tc: &Tc<'_>, g: &mut GomegaExpression<'_>, c: &Config, b: &mut Builder) -> bool {
    if c.suppress_compare || !g.actual_arg_type_is(arg::COMPARISON) {
        return false;
    }
    let Some((op, left, right)) = g.arg().as_comparison() else {
        return false;
    };
    let (left, right) = (left.clone(), right.clone());
    let equal = |g: &mut GomegaExpression<'_>| {
        if right.is_value_zero() {
            g.set_matcher_be_zero();
        } else if let Some(e) = right.value_expr() {
            if left.is_interface(tc) || left.is_pointer(tc) {
                g.set_matcher_be_identical_to(e);
            } else {
                g.set_matcher_equal(e);
            }
        }
    };
    match op {
        Token::EQL => equal(g),
        Token::NEQ => {
            g.reverse_assertion_func_logic();
            equal(g);
        }
        Token::GTR | Token::GEQ | Token::LSS | Token::LEQ => {
            if !right.is_value_numeric() {
                return false;
            }
            let Some(e) = right.value_expr() else {
                return false;
            };
            g.set_matcher_be_numerically(op, e);
        }
        _ => return false,
    }
    if g.matcher_type_is(mt::BOOL_VALUE_FALSE) {
        g.reverse_assertion_func_logic();
    }
    if let Some(e) = left.value_expr() {
        g.replace_actual(e);
    }
    b.add_issue(true, WRONG_COMPARE.to_string());
    true
}

/// `NilCompareRule`.
fn nil_compare_rule(g: &mut GomegaExpression<'_>, c: &Config, b: &mut Builder) -> bool {
    if !g.matcher_type_is(mt::EQUAL_BOOL_VALUE | mt::BE_TRUE | mt::BE_FALSE) {
        return false;
    }
    let ArgPayload::NilComparison { val, op, .. } = g.arg() else {
        return false;
    };
    let (val, op) = (val.clone(), *op);
    let is_err = val.is_error() && !c.suppress_err;
    if !is_err && c.suppress_nil {
        return false;
    }
    if g.matcher_type_is(mt::BOOL_VALUE_FALSE) {
        g.reverse_assertion_func_logic();
    }
    let template = if is_err {
        if val.is_func() {
            g.set_matcher_succeed();
        } else {
            g.reverse_assertion_func_logic();
            g.set_matcher_have_occurred();
        }
        WRONG_ERR
    } else {
        g.set_matcher_be_nil();
        WRONG_NIL
    };
    if let Some(e) = val.value_expr() {
        g.replace_actual(e);
    }
    if op == Token::NEQ {
        g.reverse_assertion_func_logic();
    }
    b.add_issue(true, template.to_string());
    true
}

/// `ComparePointerRule`.
fn compare_pointer_rule(tc: &Tc<'_>, g: &mut GomegaExpression<'_>, c: &Config, b: &mut Builder) -> bool {
    let ArgPayload::Regular(actual) = g.arg() else {
        return false;
    };
    if !actual.is_pointer(tc) {
        return false;
    }
    match g.info() {
        Info::Equal(v) => {
            if v.is_pointer(tc) || v.is_interface(tc) {
                return false;
            }
        }
        Info::BeEquivalentTo(v) | Info::BeIdenticalTo(v) => {
            if v.is_pointer(tc) || v.is_interface(tc) || v.is_nil() {
                return false;
            }
        }
        Info::EqualNil(_) => return false,
        Info::BeTrue | Info::BeFalse | Info::BeNumerically { .. } | Info::EqualTrue | Info::EqualFalse => {}
        _ => return false,
    }
    apply_all(MATCHER_ONLY_RULES, tc, g, c, b);
    g.set_matcher_have_value();
    b.add_issue(true, "comparing a pointer to a value will always fail".to_string());
    true
}

/// `ErrorEqualNilRule`.
fn error_equal_nil_rule(g: &mut GomegaExpression<'_>, c: &Config, b: &mut Builder) -> bool {
    if c.suppress_err {
        return false;
    }
    if !g.is_async && g.actual_arg_type_is(arg::FUNC_SIG) {
        return false;
    }
    if !(g.actual_arg_type_is(arg::ERROR_TYPE) && g.matcher_type_is(mt::BE_NIL | mt::EQUAL_NIL)) {
        return false;
    }
    let valuer_is_func = g.arg().as_valuer().is_some_and(|(_, is_func)| is_func);
    if valuer_is_func && !g.actual_arg_type_is(arg::ERROR_METHOD) || g.actual_arg_type_is(arg::ERR_FUNC) {
        g.set_matcher_succeed();
    } else {
        g.reverse_assertion_func_logic();
        g.set_matcher_have_occurred();
    }
    b.add_issue(true, WRONG_ERR.to_string());
    true
}

/// `MatchErrorRule`.
fn match_error_rule(tc: &Tc<'_>, g: &mut GomegaExpression<'_>, b: &mut Builder) -> bool {
    if !g.matcher_type_is(mt::MATCH_ERROR | mt::MULTIPLE) {
        return false;
    }
    match g.info() {
        Info::MatchError { kind, num_args } => {
            let (kind, num_args) = (*kind, *num_args);
            check_match_error(tc, g, kind, num_args, b)
        }
        Info::Multiple { matchers, .. } => {
            let nested: Vec<(MatchErrKind, usize)> = matchers
                .iter()
                .filter_map(|m| match m.info {
                    Info::MatchError { kind, num_args } => Some((kind, num_args)),
                    _ => None,
                })
                .collect();
            let mut res = false;
            for (kind, num_args) in nested {
                if check_match_error(tc, g, kind, num_args, b) {
                    res = true;
                }
            }
            res
        }
        _ => false,
    }
}

/// `checkMatchErrorMatcher`. The arguments it formats and trims are always the
/// top-level matcher's, even for a `MatchError` nested in `And` / `Or`.
fn check_match_error(tc: &Tc<'_>, g: &mut GomegaExpression<'_>, kind: MatchErrKind, num_args: usize, b: &mut Builder) -> bool {
    if !g.actual_arg_type_is(arg::ERROR_TYPE) {
        let actual = g.actual_arg_expr();
        b.add_issue(
            false,
            format!(
                "the MatchError matcher used to assert a non error type ({})",
                b.format_expr(tc, &actual)
            ),
        );
    }
    match kind {
        MatchErrKind::Invalid => {
            let first = g.matcher_clone_ref().args.first().cloned();
            let text = first.map(|e| b.format_expr(tc, &e)).unwrap_or_default();
            b.add_issue(
                false,
                format!("MatchError first parameter ({text}) must be error, string, GomegaMatcher or func(error)bool are allowed"),
            );
        }
        MatchErrKind::WithErrFunc { second_is_string } => {
            if num_args == Info::allowed_num_args(kind) {
                if !second_is_string {
                    b.add_issue(
                        false,
                        "The second parameter of MatchError must be the function description (string)".to_string(),
                    );
                }
                return true;
            }
            if num_args == 1 {
                b.add_issue(false, "missing function description as second parameter of MatchError".to_string());
                return true;
            }
        }
        MatchErrKind::WithErr | MatchErrKind::WithMatcher | MatchErrKind::WithString => {}
    }
    let allowed = Info::allowed_num_args(kind);
    if num_args == allowed {
        return true;
    }
    if num_args > allowed {
        let m = g.matcher_clone();
        m.args.truncate(allowed);
        b.add_issue(false, "redundant MatchError arguments; consider removing them".to_string());
        return true;
    }
    false
}

/// `EqualDifferentTypesRule.checkEqualDifferentTypes`.
fn check_equal_different_types(tc: &Tc<'_>, g: &GomegaExpression<'_>, m: &Matcher, parent_pointer: bool, b: &mut Builder) -> bool {
    let mut actual_type = g.actual().arg_type;
    if parent_pointer {
        if let Some(elem) = actual_type.and_then(|t| tc.pointer_elem(t)) {
            actual_type = Some(elem);
        }
    }
    let (matcher_type, matcher_name) = match &m.info {
        Info::Equal(v) | Info::BeIdenticalTo(v) => (v.get_type(), m.info.name()),
        Info::HaveValue(nested) => return check_equal_different_types(tc, g, nested, true, b),
        Info::Multiple { matchers, .. } => {
            let mut found = false;
            for nested in matchers {
                if check_equal_different_types(tc, g, nested, parent_pointer, b) {
                    found = true;
                }
            }
            return found;
        }
        Info::EqualNil(t) => (*t, m.info.name()),
        Info::WithTransform { func_type, nested } => {
            let r = match &nested.info {
                Info::Equal(v) | Info::BeIdenticalTo(v) => (v.get_type(), nested.info.name()),
                _ => return false,
            };
            actual_type = *func_type;
            r
        }
        _ => return false,
    };
    if tc.identical(matcher_type, actual_type) {
        return false;
    }
    if is_implementing(tc, matcher_type, actual_type) || is_implementing(tc, actual_type, matcher_type) {
        return false;
    }
    b.add_issue(
        false,
        format!(
            "use {matcher_name} with different types: Comparing {} with {}; either change the expected value type if possible, or use the BeEquivalentTo() matcher, instead of {matcher_name}()",
            tc.type_string(actual_type),
            tc.type_string(matcher_type),
        ),
    );
    true
}

/// `EqualDifferentTypesRule.isImplementing`.
fn is_implementing(tc: &Tc<'_>, ifs: Option<TypeId>, implementer: Option<TypeId>) -> bool {
    let (Some(ifs), Some(implementer)) = (ifs, implementer) else {
        return false;
    };
    if !tc.is_interface(Some(ifs)) {
        return false;
    }
    let mut iface = ifs;
    while !matches!(tc.data(iface), TypeData::Interface(_)) {
        iface = tc.underlying(iface);
    }
    tc.implements(implementer, iface)
}

/// `HaveOccurredRule`.
fn have_occurred_rule(g: &mut GomegaExpression<'_>, c: &Config, b: &mut Builder) -> bool {
    if !g.matcher_type_is(mt::HAVE_OCCURRED) {
        return false;
    }
    if !g.actual_arg_type_is(arg::ERROR_TYPE) {
        b.add_issue(false, "asserting a non-error type with HaveOccurred matcher".to_string());
        return true;
    }
    if c.force_succeed {
        if let ArgPayload::Err(v) = g.arg() {
            if v.is_func() {
                g.reverse_assertion_func_logic();
                g.set_matcher_succeed();
                b.add_issue(true, "prefer using the Succeed matcher for error function, instead of HaveOccurred".to_string());
                return true;
            }
        }
    }
    false
}

/// `SucceedRule`.
fn succeed_rule(g: &mut GomegaExpression<'_>, c: &Config, b: &mut Builder) -> bool {
    if g.is_async || !g.matcher_type_is(mt::SUCCEED) {
        return false;
    }
    if !g.actual_arg_type_is(arg::ERROR_TYPE) {
        if g.actual().is_tuple && !g.actual_arg_type_is(arg::ERROR_METHOD_NO_ERR) {
            b.add_issue(false, "the Success matcher does not support multiple values".to_string());
        } else {
            b.add_issue(false, "asserting a non-error type with Succeed matcher".to_string());
        }
        return true;
    }
    if c.force_succeed {
        if let ArgPayload::Err(v) = g.arg() {
            if !v.is_func() {
                g.reverse_assertion_func_logic();
                g.set_matcher_have_occurred();
                b.add_issue(
                    true,
                    "prefer using the HaveOccurred matcher for non-function error value, instead of Succeed".to_string(),
                );
                return true;
            }
        }
    }
    false
}

/// `EqualBoolRule`.
fn equal_bool_rule(g: &mut GomegaExpression<'_>, b: &mut Builder) -> bool {
    if !g.matcher_type_is(mt::EQUAL_BOOL_VALUE) {
        return false;
    }
    if g.matcher_type_is(mt::BOOL_VALUE_TRUE) {
        g.set_matcher_be_true();
    } else if g.is_negative_assertion() {
        g.reverse_assertion_func_logic();
        g.set_matcher_be_true();
    } else {
        g.set_matcher_be_false();
    }
    b.add_issue(true, WRONG_BOOL.to_string());
    true
}

/// `AsyncFuncCallRule`.
fn async_func_call_rule(g: &mut GomegaExpression<'_>, c: &Config, b: &mut Builder) {
    if c.suppress_async || !g.is_async {
        return;
    }
    let invalid = g.actual().async_arg.as_ref().is_some_and(|a| !a.valid);
    if !invalid {
        return;
    }
    g.append_with_args_to_actual();
    let name = g.actual_func_name.clone();
    b.add_issue(
        true,
        format!(
            "use a function call in {name}. This actually checks nothing, because {name} receives the function returned value, instead of function itself, and this value is never changed"
        ),
    );
}

/// `AsyncTimeIntervalsRule`.
fn async_time_intervals_rule(g: &mut GomegaExpression<'_>, c: &Config, b: &mut Builder) {
    if c.suppress_async || !c.validate_async_intervals || !g.is_async {
        return;
    }
    let Some(a) = g.actual.as_mut().and_then(|a| a.async_arg.take()) else {
        return;
    };
    if a.too_many_timeouts {
        b.add_issue(false, "timeout defined more than once".to_string());
    }
    if a.too_many_polling {
        b.add_issue(false, "polling defined more than once".to_string());
    }
    let timeout = check_interval(g, a.timeout.as_ref(), b);
    let polling = check_interval(g, a.polling.as_ref(), b);
    if timeout > 0 && polling > 0 && polling > timeout {
        b.add_issue(false, "timeout must not be shorter than the polling interval".to_string());
    }
    if let Some(actual) = g.actual.as_mut() {
        actual.async_arg = Some(a);
    }
}

const ONLY_USE_TIME_DURATION: &str = "only use time.Duration for timeout and polling in Eventually() or Consistently()";

/// `checkInterval`.
fn check_interval(g: &mut GomegaExpression<'_>, d: Option<&Duration>, b: &mut Builder) -> i64 {
    let Some(d) = d else {
        return 0;
    };
    match d {
        Duration::Real(_) | Duration::UnknownType => {}
        Duration::Numeric { .. } | Duration::Str { .. } => {
            if g.replace_interval(d) {
                b.add_issue(true, ONLY_USE_TIME_DURATION.to_string());
            }
        }
        Duration::Unknown => b.add_issue(true, ONLY_USE_TIME_DURATION.to_string()),
    }
    d.duration()
}

/// `AsyncSucceedRule`.
fn async_succeed_rule(g: &GomegaExpression<'_>, b: &mut Builder) {
    if !(g.is_async
        && g.matcher_type_is(mt::SUCCEED)
        && g.actual_arg_type_is(arg::FUNC_SIG)
        && !g.actual_arg_type_is(arg::ERROR_TYPE | arg::GOMEGA_PARAM | arg::TB_PARAM))
    {
        return;
    }
    if g.actual_arg_type_is(arg::MULTI_RETS) {
        b.add_issue(false, "Success matcher does not support multiple values".to_string());
    } else {
        b.add_issue(
            false,
            "Success matcher only support a single error value, or function with Gomega as its first parameter".to_string(),
        );
    }
}
