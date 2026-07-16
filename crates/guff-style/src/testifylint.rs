//! Port of [`github.com/Antonboom/testifylint`](https://github.com/Antonboom/testifylint)
//! (golangci-lint wrapper in `pkg/golinters/testifylint`).
//!
//! Checks usage of `github.com/stretchr/testify` assert/require helpers.
//!
//! Implemented checkers (defaults match upstream except noted):
//! `blank-import`, `bool-compare`, `compares`, `empty`, `error-nil`,
//! `float-compare`, `len`, `nil-compare`.
//!
//! DEFERRED: remaining checkers (contains, encoded-compare, equal-values,
//! error-is-as, expected-actual, formatter, go-require, mock-expect,
//! negative-positive, regexp, require-error, suite-*, useless-assert, zero,
//! time-compare), SuggestedFix / TextEdit, bool-compare custom-type casting
//! in messages, compares time.Time helpers.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{BasicLit, BinaryExpr, CallExpr, Expr, File, ImportSpec};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::{BasicKind, IS_FLOAT, IS_STRING};
use guff_types::named::named_obj;
use guff_types::TypeId;

use crate::options::TestifylintOptions;

const ASSERT_PKG: &str = "github.com/stretchr/testify/assert";
const REQUIRE_PKG: &str = "github.com/stretchr/testify/require";
const TESTIFY_ROOT: &str = "github.com/stretchr/testify";
const HTTP_PKG: &str = "github.com/stretchr/testify/http";
const MOCK_PKG: &str = "github.com/stretchr/testify/mock";
const SUITE_PKG: &str = "github.com/stretchr/testify/suite";

const IMPLEMENTED: &[&str] = &[
    "blank-import",
    "bool-compare",
    "compares",
    "empty",
    "error-nil",
    "float-compare",
    "len",
    "nil-compare",
];

/// Default-on checkers in upstream (suite-thelper is off by default).
fn default_enabled() -> HashSet<String> {
    IMPLEMENTED.iter().map(|s| (*s).to_string()).collect()
}

fn enabled_checkers(opts: &TestifylintOptions) -> HashSet<String> {
    let mut set = if opts.disable_all {
        HashSet::new()
    } else if opts.enable_all {
        IMPLEMENTED.iter().map(|s| (*s).to_string()).collect()
    } else {
        default_enabled()
    };
    for name in &opts.enable {
        if IMPLEMENTED.contains(&name.as_str()) {
            set.insert(name.clone());
        }
    }
    for name in &opts.disable {
        set.remove(name);
    }
    set
}

struct CallMeta<'a> {
    call: &'a CallExpr,
    #[allow(dead_code)]
    is_assert: bool,
    selector_x: String,
    #[allow(dead_code)]
    fn_name: String,
    fn_name_trimmed: String,
    is_fmt: bool,
    args: &'a [Expr],
}

fn unquote_import(path: &str) -> &str {
    path.trim_matches('"').trim_matches('`')
}

fn selector_x_str(expr: &Expr) -> String {
    match expr {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", selector_x_str(&sel.x), sel.sel.name),
        Expr::CallExpr(c) => format!("{}(...)", selector_x_str(&c.fun)),
        Expr::ParenExpr(p) => selector_x_str(&p.x),
        _ => "?".into(),
    }
}

fn parse_testify_callee(name: &str) -> Option<(bool, bool, String)> {
    // Returns (is_assert, is_pkg_level, fn_name).
    let try_pkg = |pkg: &str, is_assert: bool| -> Option<(bool, bool, String)> {
        let prefix = format!("{pkg}.");
        if let Some(rest) = name.strip_prefix(&prefix) {
            if !rest.contains('.') && !rest.contains(')') {
                return Some((is_assert, true, rest.to_string()));
            }
        }
        let method = format!("(*{pkg}.Assertions).");
        if let Some(rest) = name.strip_prefix(&method) {
            return Some((is_assert, false, rest.to_string()));
        }
        None
    };
    try_pkg(ASSERT_PKG, true).or_else(|| try_pkg(REQUIRE_PKG, false))
}

fn new_call_meta<'a>(pass: &Pass<'_>, call: &'a CallExpr) -> Option<CallMeta<'a>> {
    let name = code::call_name(pass, &call.fun)?;
    let (is_assert, is_pkg, fn_name) = parse_testify_callee(&name)?;
    let Expr::SelectorExpr(sel) = &*call.fun else {
        return None;
    };
    let is_fmt = fn_name.ends_with('f');
    let trimmed = fn_name.trim_end_matches('f').to_string();
    let args = if is_pkg && !call.args.is_empty() {
        &call.args[1..]
    } else {
        call.args.as_slice()
    };
    Some(CallMeta {
        call,
        is_assert,
        selector_x: selector_x_str(&sel.x),
        fn_name,
        fn_name_trimmed: trimmed,
        is_fmt,
        args,
    })
}

fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn underlying_basic(pass: &Pass<'_>, typ: TypeId) -> Option<BasicKind> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = unalias_readonly(&artifacts.types, typ);
    let under = typ.underlying(&artifacts.types);
    match artifacts.types.get(under) {
        TypeData::Basic(b) => Some(b.kind()),
        _ => None,
    }
}

fn is_empty_interface(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Interface(i) => i.num_explicit_methods() == 0 && i.num_embeddeds() == 0,
        TypeData::Named(_) => {
            let under = typ.underlying(&artifacts.types);
            match artifacts.types.get(under) {
                TypeData::Interface(i) => i.num_explicit_methods() == 0 && i.num_embeddeds() == 0,
                _ => false,
            }
        }
        _ => false,
    }
}

fn has_bool_type(pass: &Pass<'_>, expr: &Expr) -> bool {
    matches!(
        type_of(pass, expr).and_then(|t| underlying_basic(pass, t)),
        Some(BasicKind::Bool)
    )
}

fn is_bool_override(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Named(_) => named_obj(&artifacts.types, typ).name(&artifacts.objects) == "bool",
        _ => false,
    }
}

fn is_untyped_true(pass: &Pass<'_>, expr: &Expr) -> bool {
    code::is_bool_const(pass, expr) && code::bool_const(pass, expr)
}

fn is_untyped_false(pass: &Pass<'_>, expr: &Expr) -> bool {
    code::is_bool_const(pass, expr) && !code::bool_const(pass, expr)
}

fn is_nil(pass: &Pass<'_>, expr: &Expr) -> bool {
    code::is_nil(pass, expr)
}

fn universe_error(pass: &Pass<'_>) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    for oid in artifacts.objects.ids() {
        let ObjectData::TypeName(tn) = artifacts.objects.get(oid) else {
            continue;
        };
        if tn.name() != "error" {
            continue;
        }
        if oid.pkg(&artifacts.objects).is_some() {
            continue;
        }
        return tn.typ();
    }
    None
}

fn is_error(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(err) = universe_error(pass) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    typ == err
}

fn has_string_type(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let under = typ.underlying(&artifacts.types);
    match artifacts.types.get(under) {
        TypeData::Basic(b) => b.info().contains(IS_STRING),
        _ => false,
    }
}

fn is_float(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let under = typ.underlying(&artifacts.types);
    match artifacts.types.get(under) {
        TypeData::Basic(b) => b.info().contains(IS_FLOAT),
        _ => false,
    }
}

fn is_pointer(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    matches!(artifacts.types.get(typ), TypeData::Pointer(_))
}

fn is_int_basic_lit(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::UnaryExpr(u) if u.op == Token::SUB => {
            is_int_basic_lit(&u.x).map(|v| -v)
        }
        Expr::BasicLit(lit) if lit.kind == Some(Token::INT) => lit.value.parse().ok(),
        _ => None,
    }
}

fn is_zero(expr: &Expr) -> bool {
    is_int_basic_lit(expr) == Some(0)
}

fn is_one(expr: &Expr) -> bool {
    is_int_basic_lit(expr) == Some(1)
}

fn is_empty_string_lit(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::BasicLit(BasicLit {
            kind: Some(Token::STRING),
            value,
            ..
        }) if value == "\"\"" || value == "``"
    )
}

fn is_builtin_len_call<'a>(pass: &Pass<'_>, expr: &'a Expr) -> Option<&'a Expr> {
    let Expr::CallExpr(ce) = expr else {
        return None;
    };
    if ce.args.len() != 1 {
        return None;
    }
    if code::call_name(pass, &ce.fun).as_deref() == Some("len") {
        return Some(&ce.args[0]);
    }
    None
}

fn is_len_call_and_zero<'a>(pass: &Pass<'_>, a: &'a Expr, b: &Expr) -> Option<&'a Expr> {
    let len_arg = is_builtin_len_call(pass, a)?;
    if is_zero(b) {
        Some(len_arg)
    } else {
        None
    }
}

fn xor_nil<'a>(pass: &Pass<'_>, a: &'a Expr, b: &'a Expr) -> Option<&'a Expr> {
    let an = is_nil(pass, a);
    let bn = is_nil(pass, b);
    if an != bn {
        if an {
            Some(b)
        } else {
            Some(a)
        }
    } else {
        None
    }
}

fn proposed_fn_name(call: &CallMeta<'_>, base: &str) -> String {
    if call.is_fmt {
        format!("{base}f")
    } else {
        base.to_string()
    }
}

fn report_use(checker: &str, call: &CallMeta<'_>, proposed: &str, pending: &mut Vec<(u32, String)>) {
    let msg = format!(
        "{checker}: use {}.{}",
        call.selector_x,
        proposed_fn_name(call, proposed)
    );
    pending.push((call.call.pos().0 as u32, msg));
}

fn report_msg(checker: &str, call: &CallMeta<'_>, body: &str, pending: &mut Vec<(u32, String)>) {
    pending.push((
        call.call.pos().0 as u32,
        format!("{checker}: {body}"),
    ));
}

fn is_negation(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::UnaryExpr(u) if u.op == Token::NOT => Some(&u.x),
        _ => None,
    }
}

fn is_comparison_with<'a, F>(
    pass: &Pass<'_>,
    expr: &'a Expr,
    pred: F,
    op: Token,
) -> Option<&'a Expr>
where
    F: Fn(&Pass<'_>, &Expr) -> bool,
{
    let Expr::BinaryExpr(be) = expr else {
        return None;
    };
    if be.op != op {
        return None;
    }
    let t1 = pred(pass, &be.x);
    let t2 = pred(pass, &be.y);
    if t1 != t2 {
        if t1 {
            Some(&be.y)
        } else {
            Some(&be.x)
        }
    } else {
        None
    }
}

fn is_comparison_with_float(pass: &Pass<'_>, expr: &Expr, op: Token) -> bool {
    let Expr::BinaryExpr(be) = expr else {
        return false;
    };
    be.op == op && (is_float(pass, &be.x) || is_float(pass, &be.y))
}

fn check_bool_compare(
    pass: &Pass<'_>,
    call: &CallMeta<'_>,
    opts: &TestifylintOptions,
    pending: &mut Vec<(u32, String)>,
) {
    let ignore_custom = opts.bool_compare_ignore_custom_types;
    let allow = |surviving: &Expr| -> bool {
        if has_bool_type(pass, surviving) {
            return true;
        }
        !ignore_custom
    };

    match call.fn_name_trimmed.as_str() {
        "Equal" | "EqualValues" | "Exactly" => {
            if call.args.len() < 2 {
                return;
            }
            let (a, b) = (&call.args[0], &call.args[1]);
            if is_empty_interface(pass, a) || is_empty_interface(pass, b) {
                return;
            }
            if is_bool_override(pass, a) || is_bool_override(pass, b) {
                return;
            }
            let t1 = is_untyped_true(pass, a);
            let t2 = is_untyped_true(pass, b);
            let f1 = is_untyped_false(pass, a);
            let f2 = is_untyped_false(pass, b);
            if t1 != t2 {
                let surviving = if t1 { b } else { a };
                if call.fn_name_trimmed == "Exactly" && !has_bool_type(pass, surviving) {
                    return;
                }
                if allow(surviving) {
                    report_use("bool-compare", call, "True", pending);
                }
            } else if f1 != f2 {
                let surviving = if f1 { b } else { a };
                if call.fn_name_trimmed == "Exactly" && !has_bool_type(pass, surviving) {
                    return;
                }
                if allow(surviving) {
                    report_use("bool-compare", call, "False", pending);
                }
            }
        }
        "NotEqual" | "NotEqualValues" => {
            if call.args.len() < 2 {
                return;
            }
            let (a, b) = (&call.args[0], &call.args[1]);
            if is_empty_interface(pass, a) || is_empty_interface(pass, b) {
                return;
            }
            if is_bool_override(pass, a) || is_bool_override(pass, b) {
                return;
            }
            let t1 = is_untyped_true(pass, a);
            let t2 = is_untyped_true(pass, b);
            let f1 = is_untyped_false(pass, a);
            let f2 = is_untyped_false(pass, b);
            if t1 != t2 {
                let surviving = if t1 { b } else { a };
                if allow(surviving) {
                    report_use("bool-compare", call, "False", pending);
                }
            } else if f1 != f2 {
                let surviving = if f1 { b } else { a };
                if allow(surviving) {
                    report_use("bool-compare", call, "True", pending);
                }
            }
        }
        "True" => {
            if call.args.is_empty() {
                return;
            }
            let expr = &call.args[0];
            if let Some(surviving) = is_comparison_with(pass, expr, is_untyped_true, Token::EQL)
                .or_else(|| is_comparison_with(pass, expr, is_untyped_false, Token::NEQ))
            {
                if !is_empty_interface(pass, surviving) && allow(surviving) {
                    report_msg(
                        "bool-compare",
                        call,
                        "need to simplify the assertion",
                        pending,
                    );
                    return;
                }
            }
            if let Some(surviving) = is_comparison_with(pass, expr, is_untyped_true, Token::NEQ)
                .or_else(|| is_comparison_with(pass, expr, is_untyped_false, Token::EQL))
                .or_else(|| is_negation(expr))
            {
                if !is_empty_interface(pass, surviving) && allow(surviving) {
                    report_use("bool-compare", call, "False", pending);
                }
            }
        }
        "False" => {
            if call.args.is_empty() {
                return;
            }
            let expr = &call.args[0];
            if let Some(surviving) = is_comparison_with(pass, expr, is_untyped_true, Token::EQL)
                .or_else(|| is_comparison_with(pass, expr, is_untyped_false, Token::NEQ))
            {
                if !is_empty_interface(pass, surviving) && allow(surviving) {
                    report_msg(
                        "bool-compare",
                        call,
                        "need to simplify the assertion",
                        pending,
                    );
                    return;
                }
            }
            if let Some(surviving) = is_comparison_with(pass, expr, is_untyped_true, Token::NEQ)
                .or_else(|| is_comparison_with(pass, expr, is_untyped_false, Token::EQL))
                .or_else(|| is_negation(expr))
            {
                if !is_empty_interface(pass, surviving) && allow(surviving) {
                    report_use("bool-compare", call, "True", pending);
                }
            }
        }
        _ => {}
    }
}

fn check_empty(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    if call.args.is_empty() {
        return;
    }
    let a = &call.args[0];

    match call.fn_name_trimmed.as_str() {
        "Zero" => {
            if has_string_type(pass, a) || is_builtin_len_call(pass, a).is_some() {
                report_use("empty", call, "Empty", pending);
                return;
            }
        }
        "Empty" => {
            if is_builtin_len_call(pass, a).is_some() {
                report_msg("empty", call, "remove unnecessary len", pending);
                return;
            }
        }
        "Positive" => {
            if is_builtin_len_call(pass, a).is_some() {
                report_use("empty", call, "NotEmpty", pending);
                return;
            }
        }
        "NotZero" => {
            if has_string_type(pass, a) || is_builtin_len_call(pass, a).is_some() {
                report_use("empty", call, "NotEmpty", pending);
                return;
            }
        }
        "NotEmpty" => {
            if is_builtin_len_call(pass, a).is_some() {
                report_msg("empty", call, "remove unnecessary len", pending);
                return;
            }
        }
        _ => {}
    }

    if call.args.len() < 2 {
        return;
    }
    let b = &call.args[1];

    match call.fn_name_trimmed.as_str() {
        "Len" if is_zero(b) => report_use("empty", call, "Empty", pending),
        "Equal" | "EqualValues" | "Exactly" => {
            if is_empty_string_lit(a) {
                report_use("empty", call, "Empty", pending);
            } else if is_len_call_and_zero(pass, a, b).is_some()
                || is_len_call_and_zero(pass, b, a).is_some()
            {
                report_use("empty", call, "Empty", pending);
            }
        }
        "LessOrEqual" => {
            if is_builtin_len_call(pass, a).is_some() && is_zero(b) {
                report_use("empty", call, "Empty", pending);
            }
        }
        "GreaterOrEqual" => {
            if is_builtin_len_call(pass, b).is_some() && is_zero(a) {
                report_use("empty", call, "Empty", pending);
            }
        }
        "Less" => {
            if is_builtin_len_call(pass, a).is_some() && (is_one(b) || is_zero(b)) {
                report_use("empty", call, "Empty", pending);
            } else if is_builtin_len_call(pass, b).is_some() && is_zero(a) {
                report_use("empty", call, "NotEmpty", pending);
            }
        }
        "Greater" => {
            if is_builtin_len_call(pass, b).is_some() && (is_one(a) || is_zero(a)) {
                report_use("empty", call, "Empty", pending);
            } else if is_builtin_len_call(pass, a).is_some() && is_zero(b) {
                report_use("empty", call, "NotEmpty", pending);
            }
        }
        "NotEqual" | "NotEqualValues" => {
            if is_empty_string_lit(a) {
                report_use("empty", call, "NotEmpty", pending);
            } else if is_len_call_and_zero(pass, a, b).is_some()
                || is_len_call_and_zero(pass, b, a).is_some()
            {
                report_use("empty", call, "NotEmpty", pending);
            }
        }
        _ => {}
    }
}

fn check_error_nil(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    match call.fn_name_trimmed.as_str() {
        "Nil" | "Empty" | "Zero" => {
            if !call.args.is_empty() && is_error(pass, &call.args[0]) {
                report_use("error-nil", call, "NoError", pending);
            }
        }
        "NotNil" | "NotEmpty" | "NotZero" => {
            if !call.args.is_empty() && is_error(pass, &call.args[0]) {
                report_use("error-nil", call, "Error", pending);
            }
        }
        "Equal" | "EqualValues" | "Exactly" | "ErrorIs" | "IsType" => {
            if call.args.len() < 2 {
                return;
            }
            let (a, b) = (&call.args[0], &call.args[1]);
            if (is_error(pass, a) && is_nil(pass, b)) || (is_nil(pass, a) && is_error(pass, b)) {
                report_use("error-nil", call, "NoError", pending);
            }
        }
        "NotEqual" | "NotEqualValues" | "NotErrorIs" | "IsNotType" => {
            if call.args.len() < 2 {
                return;
            }
            let (a, b) = (&call.args[0], &call.args[1]);
            if (is_error(pass, a) && is_nil(pass, b)) || (is_nil(pass, a) && is_error(pass, b)) {
                report_use("error-nil", call, "Error", pending);
            }
        }
        _ => {}
    }
}

fn check_nil_compare(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    if call.args.len() < 2 {
        return;
    }
    if xor_nil(pass, &call.args[0], &call.args[1]).is_none() {
        return;
    }
    match call.fn_name_trimmed.as_str() {
        "Equal" | "EqualValues" | "Exactly" => {
            report_use("nil-compare", call, "Nil", pending);
        }
        "NotEqual" | "NotEqualValues" => {
            report_use("nil-compare", call, "NotNil", pending);
        }
        _ => {}
    }
}

fn check_len(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    let check_args = |a: &Expr, b: &Expr| -> bool {
        let first = is_builtin_len_call(pass, a);
        let second = is_builtin_len_call(pass, b);
        match (first, second) {
            (Some(_), Some(_)) => true,
            (Some(_), None) => is_int_basic_lit(b).is_some(),
            (None, Some(_)) => true,
            (None, None) => false,
        }
    };

    match call.fn_name_trimmed.as_str() {
        "Equal" | "EqualValues" | "Exactly" => {
            if call.args.len() >= 2 && check_args(&call.args[0], &call.args[1]) {
                // Prefer empty when comparing to 0 (empty runs earlier in registry).
                report_use("len", call, "Len", pending);
            }
        }
        "True" => {
            if call.args.is_empty() {
                return;
            }
            let Expr::BinaryExpr(BinaryExpr {
                x, y, op: Token::EQL, ..
            }) = &call.args[0]
            else {
                return;
            };
            // In True, actual is usually first → check y,x like upstream.
            if check_args(y, x) {
                report_use("len", call, "Len", pending);
            }
        }
        _ => {}
    }
}

fn check_float_compare(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    let invalid = match call.fn_name_trimmed.as_str() {
        "Equal" | "EqualValues" | "Exactly" => {
            call.args.len() > 1
                && (is_float(pass, &call.args[0]) || is_float(pass, &call.args[1]))
        }
        "True" => {
            !call.args.is_empty() && is_comparison_with_float(pass, &call.args[0], Token::EQL)
        }
        "False" => {
            !call.args.is_empty() && is_comparison_with_float(pass, &call.args[0], Token::NEQ)
        }
        _ => false,
    };
    if invalid {
        let suffix = if call.is_fmt { "f" } else { "" };
        report_msg(
            "float-compare",
            call,
            &format!(
                "use {}.InEpsilon{} (or InDelta{})",
                call.selector_x, suffix, suffix
            ),
            pending,
        );
    }
}

fn check_compares(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    if call.args.is_empty() {
        return;
    }
    let Expr::BinaryExpr(be) = &call.args[0] else {
        return;
    };
    let proposed = match call.fn_name_trimmed.as_str() {
        "True" => match be.op {
            Token::EQL => "Equal",
            Token::NEQ => "NotEqual",
            Token::GTR => "Greater",
            Token::GEQ => "GreaterOrEqual",
            Token::LSS => "Less",
            Token::LEQ => "LessOrEqual",
            _ => return,
        },
        "False" => match be.op {
            Token::EQL => "NotEqual",
            Token::NEQ => "Equal",
            Token::GTR => "LessOrEqual",
            Token::GEQ => "Less",
            Token::LSS => "GreaterOrEqual",
            Token::LEQ => "Greater",
            _ => return,
        },
        _ => return,
    };
    let mut proposed = proposed.to_string();
    if is_pointer(pass, &be.x) && is_pointer(pass, &be.y) {
        match proposed.as_str() {
            "Equal" => proposed = "Same".into(),
            "NotEqual" => proposed = "NotSame".into(),
            _ => {}
        }
    }
    report_use("compares", call, &proposed, pending);
}

fn check_blank_import(file: &File, pending: &mut Vec<(u32, String)>) {
    const BAD: &[&str] = &[
        TESTIFY_ROOT,
        ASSERT_PKG,
        HTTP_PKG,
        MOCK_PKG,
        REQUIRE_PKG,
        SUITE_PKG,
    ];
    for imp in &file.imports {
        let ImportSpec { name: Some(name), path, .. } = imp else {
            continue;
        };
        if name.name != "_" {
            continue;
        }
        let pkg = unquote_import(&path.value);
        if BAD.contains(&pkg) {
            pending.push((
                path.value_pos.0 as u32,
                format!("blank-import: avoid blank import of {pkg} as it does nothing"),
            ));
        }
    }
}

fn check_call(
    pass: &Pass<'_>,
    call: &CallMeta<'_>,
    enabled: &HashSet<String>,
    opts: &TestifylintOptions,
    pending: &mut Vec<(u32, String)>,
) {
    // Priority mirrors upstream registry order among implemented checkers.
    let before = pending.len();
    if enabled.contains("float-compare") {
        check_float_compare(pass, call, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("bool-compare") {
        check_bool_compare(pass, call, opts, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("empty") {
        check_empty(pass, call, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("compares") {
        check_compares(pass, call, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("error-nil") {
        check_error_nil(pass, call, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("nil-compare") {
        check_nil_compare(pass, call, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("len") {
        check_len(pass, call, pending);
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "testifylint requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<TestifylintOptions>("testifylint")
        .cloned()
        .unwrap_or_default();
    let enabled = enabled_checkers(&options);
    let mut pending = Vec::new();

    for file in pass.files() {
        if enabled.contains("blank-import") {
            check_blank_import(file, &mut pending);
        }
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::CallExpr(ce) = n {
                if let Some(meta) = new_call_meta(pass, ce) {
                    check_call(pass, &meta, &enabled, &options, &mut pending);
                }
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
        name: "testifylint",
        doc: "Checks usage of github.com/stretchr/testify",
        url: "https://github.com/Antonboom/testifylint",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: Vec::new(),
    })
}
