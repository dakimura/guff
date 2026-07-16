//! Port of [`github.com/Antonboom/testifylint`](https://github.com/Antonboom/testifylint)
//! (golangci-lint wrapper in `pkg/golinters/testifylint`).
//!
//! Checks usage of `github.com/stretchr/testify` assert/require helpers.
//!
//! Implemented checkers (defaults match upstream except noted):
//! `blank-import`, `bool-compare`, `compares`, `contains`, `empty`,
//! `encoded-compare`, `equal-values`, `error-is-as`, `error-nil`, `expected-actual`,
//! `float-compare`, `formatter`, `len`, `negative-positive`, `nil-compare`, `regexp`,
//! `suite-dont-use-pkg`, `suite-extra-assert-call`, `suite-subtest-run`,
//! `time-compare`, `useless-assert`, `zero`.
//!
//! DEFERRED: remaining checkers (go-require, mock-expect, require-error,
//! suite-broken-parallel / suite-method-signature / suite-thelper),
//! SuggestedFix / TextEdit, formatter full printf CheckPrintf / require-f-funcs
//! object lookup parity, bool-compare custom-type casting in messages, compares
//! time.Time helpers, encoded-compare autofix text edits, error-is-as CollectT
//! special-case / full ErrorAs pointer diagnostics edge cases.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{
    ArrayType, BasicLit, BinaryExpr, CallExpr, CompositeLit, Expr, File, Ident, ImportSpec,
    SelectorExpr, StarExpr, UnaryExpr,
};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::api_predicates::api_implements;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::{BasicKind, IS_FLOAT, IS_STRING, IS_UNSIGNED, IS_UNTYPED};
use guff_types::named::named_obj;
use guff_types::predicates::identical;
use guff_types::scope::lookup as scope_lookup;
use guff_types::signature::{signature_params, signature_variadic};
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::{new_pointer, TypeId};
use regex::Regex;

use crate::options::{SuiteExtraAssertCallMode, TestifylintOptions};

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
    "contains",
    "empty",
    "encoded-compare",
    "equal-values",
    "error-is-as",
    "error-nil",
    "expected-actual",
    "float-compare",
    "formatter",
    "len",
    "negative-positive",
    "nil-compare",
    "regexp",
    "suite-dont-use-pkg",
    "suite-extra-assert-call",
    "suite-subtest-run",
    "time-compare",
    "useless-assert",
    "zero",
];

/// Upstream `DefaultExpectedVarPattern`.
const DEFAULT_EXPECTED_ACTUAL_PATTERN: &str =
    r"(^(exp(ected)?|want(ed)?)([A-Z]\w*)?$)|(^(\w*[a-z])?(Exp(ected)?|Want(ed)?)$)";

/// Upstream `DefaultTimeCompareSuppressCallsPattern`.
const DEFAULT_TIME_COMPARE_SUPPRESS: &str = r"Add|AddDate|Date|In|Local|Round|Truncate|UTC";

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
    selector: &'a SelectorExpr,
    #[allow(dead_code)]
    is_assert: bool,
    is_pkg: bool,
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

fn cut_vendor(path: &str) -> &str {
    if let Some(idx) = path.rfind("/vendor/") {
        &path[idx + "/vendor/".len()..]
    } else if let Some(rest) = path.strip_prefix("vendor/") {
        rest
    } else {
        path
    }
}

fn selector_x_str(expr: &Expr) -> String {
    match expr {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", selector_x_str(&sel.x), sel.sel.name),
        Expr::CallExpr(c) => format!("{}()", selector_x_str(&c.fun)),
        Expr::ParenExpr(p) => selector_x_str(&p.x),
        _ => "?".into(),
    }
}

fn expr_string(expr: &Expr) -> String {
    match expr {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", expr_string(&sel.x), sel.sel.name),
        Expr::CallExpr(c) => format!("{}()", expr_string(&c.fun)),
        Expr::ParenExpr(p) => expr_string(&p.x),
        Expr::StarExpr(s) => format!("*{}", expr_string(&s.x)),
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
    let info = pass.types_info()?;
    let Expr::SelectorExpr(sel) = &*call.fun else {
        return None;
    };
    let obj_id = info.uses.get(&sel.sel.id).copied()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if !matches!(artifacts.objects.get(obj_id), ObjectData::Func(_)) {
        return None;
    }
    // Prefer type_func_name so methods become `(*pkg.Assertions).Fn`
    // (call_name collapses them to `pkg.Fn` and mis-classifies as package calls).
    let name = code::type_func_name(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        obj_id,
    );
    let (is_assert, is_pkg, fn_name) = parse_testify_callee(&name)?;
    let is_fmt = fn_name.ends_with('f');
    let trimmed = fn_name.trim_end_matches('f').to_string();
    let args = if is_pkg && !call.args.is_empty() {
        &call.args[1..]
    } else {
        call.args.as_slice()
    };
    Some(CallMeta {
        call,
        selector: sel,
        is_assert,
        is_pkg,
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
    is_empty_interface_type(pass, typ)
}

fn types_identical(pass: &Pass<'_>, a: TypeId, b: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    identical(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        a,
        b,
    )
}

fn is_func(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    matches!(artifacts.types.get(typ), TypeData::Signature(_))
}

fn is_pkg_fn_call(pass: &Pass<'_>, ce: &CallExpr, pkg: &str, fn_name: &str) -> bool {
    code::call_name(pass, &ce.fun).as_deref() == Some(&format!("{pkg}.{fn_name}"))
}

fn is_strings_contains_call(pass: &Pass<'_>, ce: &CallExpr) -> bool {
    is_pkg_fn_call(pass, ce, "strings", "Contains")
}

fn is_regexp_must_compile_call(pass: &Pass<'_>, ce: &CallExpr) -> bool {
    is_pkg_fn_call(pass, ce, "regexp", "MustCompile")
}

fn call_fn_string(call: &CallMeta<'_>) -> String {
    format!("{}.{}", call.selector_x, call.fn_name)
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

fn has_bytes_type(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let under = typ.underlying(&artifacts.types);
    let TypeData::Slice(s) = artifacts.types.get(under) else {
        return false;
    };
    match artifacts.types.get(s.elem()) {
        TypeData::Basic(b) => matches!(b.kind(), BasicKind::Uint8),
        _ => false,
    }
}

fn is_string_or_bytes(pass: &Pass<'_>, expr: &Expr) -> bool {
    has_string_type(pass, expr) || has_bytes_type(pass, expr)
}

fn is_ident_with_name(expr: &Expr, name: &str) -> bool {
    matches!(unparen(expr), Expr::Ident(Ident { name: n, .. }) if n == name)
}

fn is_byte_array(expr: &Expr) -> bool {
    matches!(
        unparen(expr),
        Expr::ArrayType(ArrayType { len: None, elt, .. }) if is_ident_with_name(elt, "byte")
    )
}

fn is_errors_is_call(pass: &Pass<'_>, ce: &CallExpr) -> bool {
    is_pkg_fn_call(pass, ce, "errors", "Is")
}

fn is_errors_as_call(pass: &Pass<'_>, ce: &CallExpr) -> bool {
    is_pkg_fn_call(pass, ce, "errors", "As")
}

fn is_fmt_sprintf_call<'a>(pass: &Pass<'_>, expr: &'a Expr) -> Option<&'a [Expr]> {
    let Expr::CallExpr(ce) = unparen(expr) else {
        return None;
    };
    if is_pkg_fn_call(pass, ce, "fmt", "Sprintf") {
        Some(ce.args.as_slice())
    } else {
        None
    }
}

fn is_json_raw_message_cast(pass: &Pass<'_>, ce: &CallExpr) -> bool {
    is_pkg_fn_call(pass, ce, "encoding/json", "RawMessage")
}

fn is_json_object_or_array(s: &str) -> bool {
    let mut s = s.trim().to_string();
    // Match Go `strconv.Unquote` best-effort for double-quoted inputs.
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        if let Ok(v) = serde_json::from_str::<String>(&s) {
            s = v;
        }
    } else if s.len() >= 2 && s.starts_with('`') && s.ends_with('`') {
        s = s[1..s.len() - 1].to_string();
    }
    let s = s.trim();
    if s.is_empty() || !(s.starts_with('{') || s.starts_with('[')) {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(s).is_ok()
}

fn words_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[A-Z]+(?:[a-z]*|$)|[a-z]+").expect("words regex"))
}

fn json_ident_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"json|JSON|Json").expect("json ident regex"))
}

fn json_negative_word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^(invalid|bad|malformed|broken|corrupt|wrong)$")
            .expect("json negative regex")
    })
}

fn yaml_word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"yaml|YAML|Yaml|^(yml|YML|Yml)$").expect("yaml word regex")
    })
}

fn split_into_words(s: &str) -> Vec<&str> {
    words_re().find_iter(s).map(|m| m.as_str()).collect()
}

fn has_word_matching(s: &str, re: &Regex) -> bool {
    split_into_words(s).into_iter().any(|w| re.is_match(w))
}

fn is_json_style_expr(pass: &Pass<'_>, expr: &Expr) -> bool {
    if let Some(s) = code::expr_to_string(pass, expr) {
        if is_json_object_or_array(&s) {
            return true;
        }
    }
    if let Expr::Ident(id) = unparen(expr) {
        if json_ident_re().is_match(&id.name)
            && !has_word_matching(&id.name, json_negative_word_re())
            && is_string_or_bytes(pass, expr)
        {
            return true;
        }
    }
    if let Some(args) = is_fmt_sprintf_call(pass, expr) {
        if let Some(first) = args.first() {
            return is_json_style_expr(pass, first);
        }
    }
    false
}

fn is_yaml_style_expr(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Expr::Ident(id) = unparen(expr) else {
        return false;
    };
    is_string_or_bytes(pass, expr) && has_word_matching(&id.name, yaml_word_re())
}

fn encoded_unwrap<'a>(pass: &Pass<'_>, expr: &'a Expr) -> (&'a Expr, bool) {
    let Expr::CallExpr(ce) = unparen(expr) else {
        return (expr, false);
    };
    if ce.args.is_empty() {
        return (expr, false);
    }
    if is_json_raw_message_cast(pass, ce) {
        if is_nil(pass, &ce.args[0]) {
            return encoded_unwrap(pass, &ce.args[0]);
        }
        let (inner, _) = encoded_unwrap(pass, &ce.args[0]);
        return (inner, true);
    }
    if is_ident_with_name(&ce.fun, "string")
        || is_byte_array(&ce.fun)
        || is_pkg_fn_call(pass, ce, "strings", "Replace")
        || is_pkg_fn_call(pass, ce, "strings", "ReplaceAll")
        || is_pkg_fn_call(pass, ce, "strings", "Trim")
        || is_pkg_fn_call(pass, ce, "strings", "TrimSpace")
    {
        return encoded_unwrap(pass, &ce.args[0]);
    }
    (expr, false)
}

fn is_basic_lit(expr: &Expr) -> bool {
    match unparen(expr) {
        Expr::UnaryExpr(UnaryExpr { op: Token::SUB, x, .. }) => is_basic_lit(x),
        Expr::BasicLit(_) => true,
        _ => false,
    }
}

fn is_untyped_const(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let under = typ.underlying(&artifacts.types);
    matches!(
        artifacts.types.get(under),
        TypeData::Basic(b) if b.info().contains(IS_UNTYPED)
    )
}

fn is_typed_const(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(tav) = info.types.get(&expr.id()) else {
        return false;
    };
    tav.val.is_some()
}

fn is_ident_named_after_pattern(pattern: &Regex, expr: &Expr) -> bool {
    matches!(unparen(expr), Expr::Ident(Ident { name, .. }) if pattern.is_match(name))
}

fn is_struct_var_named_after_pattern(pattern: &Regex, expr: &Expr) -> bool {
    matches!(
        unparen(expr),
        Expr::SelectorExpr(SelectorExpr { x, .. }) if is_ident_named_after_pattern(pattern, x)
    )
}

fn is_struct_field_named_after_pattern(pattern: &Regex, expr: &Expr) -> bool {
    matches!(
        unparen(expr),
        Expr::SelectorExpr(SelectorExpr { sel, .. }) if pattern.is_match(&sel.name)
    )
}

fn is_casted_basic_lit_or_expected(ce: &CallExpr, pattern: &Regex) -> bool {
    if ce.args.len() != 1 {
        return false;
    }
    let Expr::Ident(fn_id) = unparen(&ce.fun) else {
        return false;
    };
    match fn_id.name.as_str() {
        "complex64" | "complex128" => true,
        "uint" | "uint8" | "uint16" | "uint32" | "uint64" | "int" | "int8" | "int16" | "int32"
        | "int64" | "float32" | "float64" | "rune" | "string" => {
            is_basic_lit(&ce.args[0]) || is_ident_named_after_pattern(pattern, &ce.args[0])
        }
        _ => false,
    }
}

fn is_expected_value_factory(pass: &Pass<'_>, ce: &CallExpr, pattern: &Regex) -> bool {
    match unparen(&ce.fun) {
        Expr::Ident(id) => pattern.is_match(&id.name),
        Expr::SelectorExpr(sel) => {
            if is_pkg_fn_call(pass, ce, "time", "Date") {
                return true;
            }
            pattern.is_match(&sel.sel.name)
        }
        _ => false,
    }
}

fn expected_actual_pattern(opts: &TestifylintOptions) -> Regex {
    let pat = opts
        .expected_actual_pattern
        .as_deref()
        .unwrap_or(DEFAULT_EXPECTED_ACTUAL_PATTERN);
    Regex::new(pat).unwrap_or_else(|_| {
        Regex::new(DEFAULT_EXPECTED_ACTUAL_PATTERN).expect("default expected-actual pattern")
    })
}

fn time_compare_suppress_pattern(opts: &TestifylintOptions) -> Regex {
    let pat = opts
        .time_compare_suppress_calls_pattern
        .as_deref()
        .unwrap_or(DEFAULT_TIME_COMPARE_SUPPRESS);
    Regex::new(pat).unwrap_or_else(|_| {
        Regex::new(DEFAULT_TIME_COMPARE_SUPPRESS).expect("default time-compare suppress")
    })
}

fn expr_source_approx(expr: &Expr) -> String {
    match unparen(expr) {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => {
            format!("{}.{}", expr_source_approx(&sel.x), sel.sel.name)
        }
        Expr::CallExpr(ce) => format!("{}(...)", expr_source_approx(&ce.fun)),
        Expr::ParenExpr(p) => expr_source_approx(&p.x),
        Expr::StarExpr(s) => format!("*{}", expr_source_approx(&s.x)),
        Expr::UnaryExpr(u) => format!("{:?}{}", u.op, expr_source_approx(&u.x)),
        Expr::IndexExpr(ix) => {
            format!("{}[{}]", expr_source_approx(&ix.x), expr_source_approx(&ix.index))
        }
        Expr::CompositeLit(cl) => {
            if let Some(ty) = &cl.ty {
                format!("{}{{...}}", expr_source_approx(ty))
            } else {
                "{...}".into()
            }
        }
        Expr::BasicLit(lit) => lit.value.clone(),
        _ => "?".into(),
    }
}

fn need_suppress_time_call(expr: &Expr, pattern: &Regex) -> bool {
    pattern.is_match(&expr_source_approx(expr))
}

fn is_expected_value_candidate(pass: &Pass<'_>, expr: &Expr, pattern: &Regex) -> bool {
    match unparen(expr) {
        Expr::StarExpr(StarExpr { x, .. }) => is_expected_value_candidate(pass, x, pattern),
        Expr::UnaryExpr(UnaryExpr {
            op: Token::AND | Token::SUB,
            x,
            ..
        }) => is_expected_value_candidate(pass, x, pattern),
        Expr::CompositeLit(_) => true,
        Expr::CallExpr(ce) => {
            if let Some(lv) = is_builtin_len_call(pass, expr) {
                return is_ident_named_after_pattern(pattern, lv);
            }
            matches!(unparen(&ce.fun), Expr::ParenExpr(_))
                || is_casted_basic_lit_or_expected(ce, pattern)
                || is_expected_value_factory(pass, ce, pattern)
        }
        _ => {
            is_basic_lit(expr)
                || is_untyped_const(pass, expr)
                || is_typed_const(pass, expr)
                || is_ident_named_after_pattern(pattern, expr)
                || is_struct_var_named_after_pattern(pattern, expr)
                || is_struct_field_named_after_pattern(pattern, expr)
        }
    }
}

fn implements_error(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(err) = universe_error(pass) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        err,
    )
}

fn is_interface_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let under = typ.underlying(&artifacts.types);
    matches!(artifacts.types.get(under), TypeData::Interface(_))
}

fn check_error_as_target(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    if call.args.len() < 2 {
        return;
    }
    let target = &call.args[1];
    if is_empty_interface(pass, target) {
        return;
    }
    let Some(typ) = type_of(pass, target) else {
        return;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let TypeData::Pointer(pt) = artifacts.types.get(typ) else {
        report_msg(
            "error-is-as",
            call,
            &format!(
                "second argument to {} must be a non-nil pointer to either a type that implements error, or to any interface type",
                call_fn_string(call)
            ),
            pending,
        );
        return;
    };
    let elem = pt.elem();
    if let Some(err) = universe_error(pass) {
        if elem == err {
            report_msg(
                "error-is-as",
                call,
                &format!("second argument to {} should not be *error", call_fn_string(call)),
                pending,
            );
            return;
        }
    }
    if !is_interface_type(pass, elem) && !implements_error(pass, elem) {
        report_msg(
            "error-is-as",
            call,
            &format!(
                "second argument to {} must be a non-nil pointer to either a type that implements error, or to any interface type",
                call_fn_string(call)
            ),
            pending,
        );
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

fn unparen<'a>(expr: &'a Expr) -> &'a Expr {
    match expr {
        Expr::ParenExpr(p) => unparen(&p.x),
        other => other,
    }
}

fn is_pkg_dot_type(expr: &Expr, pkg: &str, name: &str) -> bool {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(expr) else {
        return false;
    };
    matches!(unparen(x), Expr::Ident(Ident { name: pkg_name, .. }) if pkg_name == pkg)
        && sel.name == name
}

fn type_string(pass: &Pass<'_>, typ: TypeId) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    Some(guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    ))
}

fn is_time_instance(pass: &Pass<'_>, expr: &Expr) -> bool {
    type_of(pass, expr).is_some_and(|t| type_string(pass, t).as_deref() == Some("time.Time"))
}

fn is_ident_named_zero_prefix(expr: &Expr) -> bool {
    matches!(unparen(expr), Expr::Ident(Ident { name, .. }) if name.starts_with("zero"))
}

fn is_zero_time_instance(pass: &Pass<'_>, expr: &Expr) -> bool {
    if is_time_instance(pass, expr) && is_ident_named_zero_prefix(expr) {
        return true;
    }
    let Expr::CompositeLit(CompositeLit { ty: Some(ty), elts, .. }) = unparen(expr) else {
        return false;
    };
    is_pkg_dot_type(ty, "time", "Time") && elts.is_empty()
}

fn is_time_is_zero_call<'a>(pass: &Pass<'_>, expr: &'a Expr) -> Option<&'a Expr> {
    let Expr::CallExpr(ce) = unparen(expr) else {
        return None;
    };
    if !ce.args.is_empty() {
        return None;
    }
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(&ce.fun) else {
        return None;
    };
    if sel.name == "IsZero" && is_time_instance(pass, x) {
        Some(x.as_ref())
    } else {
        None
    }
}

fn is_time_equal_zero_call<'a>(pass: &Pass<'_>, expr: &'a Expr) -> Option<&'a Expr> {
    let Expr::CallExpr(ce) = unparen(expr) else {
        return None;
    };
    if ce.args.len() != 1 {
        return None;
    }
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(&ce.fun) else {
        return None;
    };
    if sel.name == "Equal"
        && is_time_instance(pass, x)
        && is_zero_time_instance(pass, &ce.args[0])
    {
        Some(x.as_ref())
    } else {
        None
    }
}

fn is_unsigned(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let under = typ.underlying(&artifacts.types);
    matches!(
        artifacts.types.get(under),
        TypeData::Basic(b) if b.info().contains(IS_UNSIGNED)
    )
}

fn is_typed_int_number(expr: &Expr, go_types: &[&str]) -> bool {
    let Expr::CallExpr(ce) = unparen(expr) else {
        return false;
    };
    if ce.args.len() != 1 {
        return false;
    }
    let Expr::Ident(id) = unparen(&ce.fun) else {
        return false;
    };
    go_types.contains(&id.name.as_str()) && is_int_basic_lit(&ce.args[0]).is_some()
}

const SIGNED_INT_TYPES: &[&str] = &["int", "int8", "int16", "int32", "int64"];
const UNSIGNED_INT_TYPES: &[&str] = &["uint", "uint8", "uint16", "uint32", "uint64"];

fn is_any_zero(expr: &Expr) -> bool {
    is_zero(expr)
        || is_typed_int_number(expr, SIGNED_INT_TYPES)
        || is_typed_int_number(expr, UNSIGNED_INT_TYPES)
}

fn is_not_any_zero(expr: &Expr) -> bool {
    !is_any_zero(expr)
}

fn is_zero_or_signed_zero(expr: &Expr) -> bool {
    is_zero(expr) || is_typed_int_number(expr, SIGNED_INT_TYPES)
}

fn is_signed_not_zero(pass: &Pass<'_>, expr: &Expr) -> bool {
    !is_unsigned(pass, expr) && !is_zero_or_signed_zero(expr)
}

fn can_be_negative(pass: &Pass<'_>, expr: &Expr) -> bool {
    is_builtin_len_call(pass, expr).is_none() && is_signed_not_zero(pass, expr)
}

fn can_not_be_negative(pass: &Pass<'_>, expr: &Expr) -> bool {
    is_builtin_len_call(pass, expr).is_some() || is_unsigned(pass, expr)
}

fn is_string_lit(expr: &Expr) -> bool {
    matches!(
        unparen(expr),
        Expr::BasicLit(BasicLit {
            kind: Some(Token::STRING),
            ..
        })
    )
}

fn is_strict_comparison_with<'a, FL, FR>(
    pass: &Pass<'_>,
    expr: &'a Expr,
    lhs_pred: FL,
    op: Token,
    rhs_pred: FR,
) -> Option<(&'a Expr, &'a Expr)>
where
    FL: Fn(&Pass<'_>, &Expr) -> bool,
    FR: Fn(&Pass<'_>, &Expr) -> bool,
{
    let Expr::BinaryExpr(be) = unparen(expr) else {
        return None;
    };
    if be.op == op && lhs_pred(pass, &be.x) && rhs_pred(pass, &be.y) {
        Some((&be.x, &be.y))
    } else {
        None
    }
}

fn expr_equal(a: &Expr, b: &Expr) -> bool {
    match (unparen(a), unparen(b)) {
        (Expr::Ident(Ident { name: na, .. }), Expr::Ident(Ident { name: nb, .. })) => na == nb,
        (
            Expr::BasicLit(BasicLit {
                value: va,
                kind: ka,
                ..
            }),
            Expr::BasicLit(BasicLit {
                value: vb,
                kind: kb,
                ..
            }),
        ) => va == vb && ka == kb,
        (
            Expr::SelectorExpr(SelectorExpr { x: xa, sel: sa, .. }),
            Expr::SelectorExpr(SelectorExpr { x: xb, sel: sb, .. }),
        ) => sa.name == sb.name && expr_equal(xa, xb),
        (Expr::UnaryExpr(u1), Expr::UnaryExpr(u2)) if u1.op == u2.op => expr_equal(&u1.x, &u2.x),
        (
            Expr::BinaryExpr(BinaryExpr {
                op: oa,
                x: xa,
                y: ya,
                ..
            }),
            Expr::BinaryExpr(BinaryExpr {
                op: ob,
                x: xb,
                y: yb,
                ..
            }),
        ) => oa == ob && expr_equal(xa, xb) && expr_equal(ya, yb),
        (
            Expr::CallExpr(CallExpr {
                fun: fa,
                args: aa,
                ..
            }),
            Expr::CallExpr(CallExpr {
                fun: fb,
                args: ab,
                ..
            }),
        ) => aa.len() == ab.len()
            && expr_equal(fa, fb)
            && aa.iter().zip(ab).all(|(x, y)| expr_equal(x, y)),
        _ => false,
    }
}

fn is_empty_interface_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let under = typ.underlying(&artifacts.types);
    match artifacts.types.get(under) {
        TypeData::Interface(i) => i.num_explicit_methods() == 0 && i.num_embeddeds() == 0,
        _ => false,
    }
}

fn pointer_elem_type(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let typ = type_of(pass, expr)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Pointer(p) => Some(p.elem()),
        _ => None,
    }
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

fn check_contains(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    if check_contains_string(pass, call, pending) {
        return;
    }
    check_contains_subset(pass, call, pending);
}

fn check_contains_string(
    pass: &Pass<'_>,
    call: &CallMeta<'_>,
    pending: &mut Vec<(u32, String)>,
) -> bool {
    if call.args.is_empty() {
        return false;
    }
    let mut expr = &call.args[0];
    let is_neg = is_negation(expr).is_some();
    if let Some(inner) = is_negation(expr) {
        expr = inner;
    }
    let Expr::CallExpr(ce) = unparen(expr) else {
        return false;
    };
    if ce.args.len() != 2 || !is_strings_contains_call(pass, ce) {
        return false;
    }
    let proposed = match call.fn_name_trimmed.as_str() {
        "True" => {
            if is_neg {
                "NotContains"
            } else {
                "Contains"
            }
        }
        "False" => {
            if is_neg {
                "Contains"
            } else {
                "NotContains"
            }
        }
        _ => return false,
    };
    report_use("contains", call, proposed, pending);
    true
}

fn check_contains_subset(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    if call.call.ellipsis.is_valid() {
        return;
    }
    if call.args.len() < 3 {
        return;
    }
    if has_string_type(pass, &call.args[2]) {
        // Possible false positives because of format string.
        return;
    }
    let proposed = match call.fn_name_trimmed.as_str() {
        "Contains" => {
            if call.is_fmt {
                "Subsetf"
            } else {
                "Subset"
            }
        }
        "NotContains" => {
            if call.is_fmt {
                "NotSubsetf"
            } else {
                "NotSubset"
            }
        }
        _ => return,
    };
    report_msg(
        "contains",
        call,
        &format!(
            "invalid usage of {}, use {}.{} for multi elements assertion",
            call_fn_string(call),
            call.selector_x,
            proposed
        ),
        pending,
    );
}

fn check_equal_values(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    let proposed = match call.fn_name_trimmed.as_str() {
        "EqualValues" => "Equal",
        "NotEqualValues" => "NotEqual",
        _ => return,
    };
    if call.args.len() < 2 {
        return;
    }
    let (first, second) = (&call.args[0], &call.args[1]);
    if is_func(pass, first) || is_func(pass, second) {
        // EqualValues for funcs is ok (testify#1524); Equal is not.
        return;
    }
    let Some(ft) = type_of(pass, first) else {
        return;
    };
    let Some(st) = type_of(pass, second) else {
        return;
    };
    if !types_identical(pass, ft, st) {
        return;
    }
    if is_empty_interface_type(pass, ft) || is_empty_interface_type(pass, st) {
        // Equal would compare dynamic types and fail; EqualValues is fine.
        return;
    }
    report_use("equal-values", call, proposed, pending);
}

fn check_regexp(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    match call.fn_name_trimmed.as_str() {
        "Regexp" | "NotRegexp" => {}
        _ => return,
    }
    if call.args.is_empty() {
        return;
    }
    let Expr::CallExpr(ce) = unparen(&call.args[0]) else {
        return;
    };
    if ce.args.len() == 1 && is_regexp_must_compile_call(pass, ce) {
        report_msg(
            "regexp",
            call,
            "remove unnecessary regexp.MustCompile",
            pending,
        );
    }
}

fn check_error_is_as(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    match call.fn_name_trimmed.as_str() {
        "Error" => {
            if call.args.len() >= 2 && is_error(pass, &call.args[1]) {
                // DEFERRED: skip when selector is *assert.CollectT.
                report_msg(
                    "error-is-as",
                    call,
                    &format!(
                        "invalid usage of {}.Error, use {}.ErrorIs instead",
                        call.selector_x, call.selector_x
                    ),
                    pending,
                );
            }
        }
        "NoError" => {
            if call.args.len() >= 2 && is_error(pass, &call.args[1]) {
                report_msg(
                    "error-is-as",
                    call,
                    &format!(
                        "invalid usage of {}.NoError, use {}.NotErrorIs instead",
                        call.selector_x, call.selector_x
                    ),
                    pending,
                );
            }
        }
        "IsType" => {
            if call.args.len() >= 2
                && (is_error(pass, &call.args[0]) || is_error(pass, &call.args[1]))
            {
                report_msg(
                    "error-is-as",
                    call,
                    &format!(
                        "use {}.ErrorIs or {}.ErrorAs depending on the case",
                        call.selector_x, call.selector_x
                    ),
                    pending,
                );
            }
        }
        "IsNotType" => {
            if call.args.len() >= 2
                && (is_error(pass, &call.args[0]) || is_error(pass, &call.args[1]))
            {
                report_msg(
                    "error-is-as",
                    call,
                    &format!(
                        "use {}.NotErrorIs or {}.NotErrorAs depending on the case",
                        call.selector_x, call.selector_x
                    ),
                    pending,
                );
            }
        }
        "True" => {
            if call.args.is_empty() {
                return;
            }
            let Expr::CallExpr(ce) = unparen(&call.args[0]) else {
                return;
            };
            if ce.args.len() != 2 {
                return;
            }
            let proposed = if is_errors_is_call(pass, ce) {
                "ErrorIs"
            } else if is_errors_as_call(pass, ce) {
                "ErrorAs"
            } else {
                return;
            };
            report_use("error-is-as", call, proposed, pending);
        }
        "False" => {
            if call.args.is_empty() {
                return;
            }
            let Expr::CallExpr(ce) = unparen(&call.args[0]) else {
                return;
            };
            if ce.args.len() != 2 {
                return;
            }
            let proposed = if is_errors_is_call(pass, ce) {
                "NotErrorIs"
            } else if is_errors_as_call(pass, ce) {
                "NotErrorAs"
            } else {
                return;
            };
            report_use("error-is-as", call, proposed, pending);
        }
        "ErrorAs" | "NotErrorAs" => check_error_as_target(pass, call, pending),
        _ => {}
    }
}

fn check_encoded_compare(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    match call.fn_name_trimmed.as_str() {
        "Equal" | "EqualValues" | "Exactly" => {}
        _ => return,
    }
    if call.args.len() < 2 {
        return;
    }
    let (a, a_explicit_json) = encoded_unwrap(pass, &call.args[0]);
    let (b, b_explicit_json) = encoded_unwrap(pass, &call.args[1]);
    if !is_string_or_bytes(pass, a) || !is_string_or_bytes(pass, b) {
        return;
    }
    let proposed = if a_explicit_json
        || b_explicit_json
        || is_json_style_expr(pass, a)
        || is_json_style_expr(pass, b)
    {
        "JSONEq"
    } else if is_yaml_style_expr(pass, a) || is_yaml_style_expr(pass, b) {
        "YAMLEq"
    } else {
        return;
    };
    report_use("encoded-compare", call, proposed, pending);
}

fn check_expected_actual(
    pass: &Pass<'_>,
    call: &CallMeta<'_>,
    opts: &TestifylintOptions,
    pending: &mut Vec<(u32, String)>,
) {
    match call.fn_name_trimmed.as_str() {
        "Equal"
        | "EqualExportedValues"
        | "EqualValues"
        | "Exactly"
        | "InDelta"
        | "InDeltaMapValues"
        | "InDeltaSlice"
        | "InEpsilon"
        | "InEpsilonSlice"
        | "IsNotType"
        | "IsType"
        | "JSONEq"
        | "NotEqual"
        | "NotEqualValues"
        | "NotSame"
        | "Same"
        | "WithinDuration"
        | "YAMLEq" => {}
        _ => return,
    }
    if call.args.len() < 2 {
        return;
    }
    let pattern = expected_actual_pattern(opts);
    let first = &call.args[0];
    let second = &call.args[1];
    let left = is_expected_value_candidate(pass, first, &pattern);
    let right = is_expected_value_candidate(pass, second, &pattern);
    if right && !left {
        report_msg(
            "expected-actual",
            call,
            "need to reverse actual and expected values",
            pending,
        );
    }
}

fn check_zero(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    match call.fn_name_trimmed.as_str() {
        "Equal" | "EqualValues" | "Exactly" => {
            if call.args.len() < 2 {
                return;
            }
            let (a, b) = (&call.args[0], &call.args[1]);
            let f1 = is_zero_time_instance(pass, a);
            let f2 = is_zero_time_instance(pass, b);
            if f1 != f2 {
                report_use("zero", call, "Zero", pending);
            }
        }
        "NotEqual" | "NotEqualValues" => {
            if call.args.len() < 2 {
                return;
            }
            let (a, b) = (&call.args[0], &call.args[1]);
            let f1 = is_zero_time_instance(pass, a);
            let f2 = is_zero_time_instance(pass, b);
            if f1 != f2 {
                report_use("zero", call, "NotZero", pending);
            }
        }
        "True" => {
            if call.args.is_empty() {
                return;
            }
            if is_time_is_zero_call(pass, &call.args[0]).is_some()
                || is_time_equal_zero_call(pass, &call.args[0]).is_some()
            {
                report_use("zero", call, "Zero", pending);
            }
        }
        "False" => {
            if call.args.is_empty() {
                return;
            }
            if is_time_is_zero_call(pass, &call.args[0]).is_some()
                || is_time_equal_zero_call(pass, &call.args[0]).is_some()
            {
                report_use("zero", call, "NotZero", pending);
            }
        }
        _ => {}
    }
}

fn check_negative_positive(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    if check_negative(pass, call, pending) {
        return;
    }
    check_positive(pass, call, pending);
}

fn check_negative(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) -> bool {
    match call.fn_name_trimmed.as_str() {
        "Less" => {
            if call.args.len() < 2 {
                return false;
            }
            let (a, b) = (&call.args[0], &call.args[1]);
            if can_be_negative(pass, a) && is_zero_or_signed_zero(b) {
                report_use("negative-positive", call, "Negative", pending);
                return true;
            }
        }
        "Greater" => {
            if call.args.len() < 2 {
                return false;
            }
            let (a, b) = (&call.args[0], &call.args[1]);
            if is_zero_or_signed_zero(a) && can_be_negative(pass, b) {
                report_use("negative-positive", call, "Negative", pending);
                return true;
            }
        }
        "True" => {
            if call.args.is_empty() {
                return false;
            }
            let expr = &call.args[0];
            let surviving = is_strict_comparison_with(
                pass,
                expr,
                |p, e| can_be_negative(p, e),
                Token::LSS,
                |_, e| is_zero_or_signed_zero(e),
            )
            .map(|(a, _)| a)
            .or_else(|| {
                is_strict_comparison_with(
                    pass,
                    expr,
                    |_, e| is_zero_or_signed_zero(e),
                    Token::GTR,
                    |p, e| can_be_negative(p, e),
                )
                .map(|(_, b)| b)
            });
            if surviving.is_some() {
                report_use("negative-positive", call, "Negative", pending);
                return true;
            }
        }
        "False" => {
            if call.args.is_empty() {
                return false;
            }
            let expr = &call.args[0];
            let has = is_strict_comparison_with(
                pass,
                expr,
                |p, e| can_be_negative(p, e),
                Token::GEQ,
                |_, e| is_zero_or_signed_zero(e),
            )
            .is_some()
                || is_strict_comparison_with(
                    pass,
                    expr,
                    |_, e| is_zero_or_signed_zero(e),
                    Token::LEQ,
                    |p, e| can_be_negative(p, e),
                )
                .is_some();
            if has {
                report_use("negative-positive", call, "Negative", pending);
                return true;
            }
        }
        _ => {}
    }
    false
}

fn check_positive(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    match call.fn_name_trimmed.as_str() {
        "Greater" => {
            if call.args.len() < 2 {
                return;
            }
            let (a, b) = (&call.args[0], &call.args[1]);
            if is_not_any_zero(a) && is_any_zero(b) {
                report_use("negative-positive", call, "Positive", pending);
            }
        }
        "Less" => {
            if call.args.len() < 2 {
                return;
            }
            let (a, b) = (&call.args[0], &call.args[1]);
            if is_any_zero(a) && is_not_any_zero(b) {
                report_use("negative-positive", call, "Positive", pending);
            }
        }
        "True" => {
            if call.args.is_empty() {
                return;
            }
            let expr = &call.args[0];
            let has = is_strict_comparison_with(
                pass,
                expr,
                |_, e| is_not_any_zero(e),
                Token::GTR,
                |_, e| is_any_zero(e),
            )
            .is_some()
                || is_strict_comparison_with(
                    pass,
                    expr,
                    |_, e| is_any_zero(e),
                    Token::LSS,
                    |_, e| is_not_any_zero(e),
                )
                .is_some();
            if has {
                report_use("negative-positive", call, "Positive", pending);
            }
        }
        "False" => {
            if call.args.is_empty() {
                return;
            }
            let expr = &call.args[0];
            let has = is_strict_comparison_with(
                pass,
                expr,
                |_, e| is_not_any_zero(e),
                Token::LEQ,
                |_, e| is_any_zero(e),
            )
            .is_some()
                || is_strict_comparison_with(
                    pass,
                    expr,
                    |_, e| is_any_zero(e),
                    Token::GEQ,
                    |_, e| is_not_any_zero(e),
                )
                .is_some();
            if has {
                report_use("negative-positive", call, "Positive", pending);
            }
        }
        _ => {}
    }
}

fn check_useless_assert_same_vars(call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) -> bool {
    let (first, second) = match call.fn_name_trimmed.as_str() {
        "Contains"
        | "ElementsMatch"
        | "Equal"
        | "EqualExportedValues"
        | "EqualValues"
        | "ErrorAs"
        | "ErrorIs"
        | "Exactly"
        | "Greater"
        | "GreaterOrEqual"
        | "Implements"
        | "InDelta"
        | "InDeltaMapValues"
        | "InDeltaSlice"
        | "InEpsilon"
        | "InEpsilonSlice"
        | "IsNotType"
        | "IsType"
        | "JSONEq"
        | "Less"
        | "LessOrEqual"
        | "NotElementsMatch"
        | "NotEqual"
        | "NotEqualValues"
        | "NotErrorAs"
        | "NotErrorIs"
        | "NotRegexp"
        | "NotSame"
        | "NotSubset"
        | "Regexp"
        | "Same"
        | "Subset"
        | "WithinDuration"
        | "YAMLEq" => {
            if call.args.len() < 2 {
                return false;
            }
            (&call.args[0], &call.args[1])
        }
        "True" | "False" => {
            if call.args.is_empty() {
                return false;
            }
            let Expr::BinaryExpr(be) = unparen(&call.args[0]) else {
                return false;
            };
            (be.x.as_ref(), be.y.as_ref())
        }
        _ => return false,
    };
    if expr_equal(first, second) {
        report_msg(
            "useless-assert",
            call,
            "asserting of the same variable",
            pending,
        );
        true
    } else {
        false
    }
}

fn check_useless_assert(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    if check_useless_assert_same_vars(call, pending) {
        return;
    }

    let meaningless = match call.fn_name_trimmed.as_str() {
        "False" | "True" => !call.args.is_empty()
            && (is_untyped_true(pass, &call.args[0]) || is_untyped_false(pass, &call.args[0])),
        "GreaterOrEqual" | "Less" => {
            call.args.len() >= 2
                && is_any_zero(&call.args[1])
                && can_not_be_negative(pass, &call.args[0])
        }
        "Implements" | "NotImplements" => {
            if call.args.is_empty() {
                false
            } else {
                pointer_elem_type(pass, &call.args[0])
                    .is_some_and(|elem| is_empty_interface_type(pass, elem))
            }
        }
        "LessOrEqual" | "Greater" => {
            call.args.len() >= 2
                && is_any_zero(&call.args[0])
                && can_not_be_negative(pass, &call.args[1])
        }
        "Positive" => call
            .args
            .first()
            .is_some_and(|a| is_int_basic_lit(a).is_some()),
        "Negative" => call.args.first().is_some_and(|a| {
            is_int_basic_lit(a).is_some() || can_not_be_negative(pass, a)
        }),
        "Error" | "Nil" | "NoError" | "NotNil" => {
            !call.args.is_empty() && is_nil(pass, &call.args[0])
        }
        "Empty" | "NotEmpty" => !call.args.is_empty() && is_string_lit(&call.args[0]),
        "NotZero" | "Zero" => {
            if call.args.is_empty() {
                false
            } else {
                let a = &call.args[0];
                is_int_basic_lit(a).is_some()
                    || is_string_lit(a)
                    || is_nil(pass, a)
                    || is_untyped_true(pass, a)
                    || is_untyped_false(pass, a)
                    || is_zero_time_instance(pass, a)
            }
        }
        _ => false,
    };

    if meaningless {
        report_msg("useless-assert", call, "meaningless assertion", pending);
    }
}

fn check_time_compare(
    pass: &Pass<'_>,
    call: &CallMeta<'_>,
    opts: &TestifylintOptions,
    pending: &mut Vec<(u32, String)>,
) {
    match call.fn_name_trimmed.as_str() {
        "Equal" | "EqualValues" | "Exactly" | "NotEqual" | "NotEqualValues" => {}
        _ => return,
    }
    if call.args.len() < 2 {
        return;
    }
    let lhs = &call.args[0];
    let rhs = &call.args[1];
    if !is_time_instance(pass, lhs) && !is_time_instance(pass, rhs) {
        return;
    }
    let pattern = time_compare_suppress_pattern(opts);
    if need_suppress_time_call(lhs, &pattern) || need_suppress_time_call(rhs, &pattern) {
        return;
    }
    report_msg(
        "time-compare",
        call,
        "equality-based assertion on time.Time can be flaky",
        pending,
    );
}

fn callee_func_obj(pass: &Pass<'_>, call: &CallMeta<'_>) -> Option<guff_types::arena::ObjectId> {
    let info = pass.types_info()?;
    match &*call.call.fun {
        Expr::Ident(id) => info.uses.get(&id.id).copied(),
        Expr::SelectorExpr(sel) => info.uses.get(&sel.sel.id).copied(),
        _ => None,
    }
}

fn signature_of_call(pass: &Pass<'_>, call: &CallMeta<'_>) -> Option<TypeId> {
    let obj = callee_func_obj(pass, call)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = obj.typ(&artifacts.objects)?;
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Signature(_) => Some(typ),
        _ => None,
    }
}

fn get_msg_and_args_position(pass: &Pass<'_>, call: &CallMeta<'_>) -> Option<usize> {
    let sig = signature_of_call(pass, call)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if !signature_variadic(&artifacts.types, sig) {
        return None;
    }
    let params = signature_params(&artifacts.types, sig)?;
    let n = tuple_len(&artifacts.types, Some(params));
    if n == 0 {
        return None;
    }
    let last = tuple_at(&artifacts.types, params, n - 1);
    if last.name(&artifacts.objects) != "msgAndArgs" {
        return None;
    }
    let Some(last_ty) = last.typ(&artifacts.objects) else {
        return None;
    };
    let last_ty = unalias_readonly(&artifacts.types, last_ty);
    if !matches!(artifacts.types.get(last_ty), TypeData::Slice(_)) {
        return None;
    }
    Some(n - 1)
}

fn get_msg_position(pass: &Pass<'_>, call: &CallMeta<'_>) -> Option<usize> {
    let sig = signature_of_call(pass, call)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let params = signature_params(&artifacts.types, sig)?;
    let n = tuple_len(&artifacts.types, Some(params));
    for i in 0..n {
        let p = tuple_at(&artifacts.types, params, i);
        let name = p.name(&artifacts.objects);
        if name != "msg" && name != "format" {
            continue;
        }
        let Some(ty) = p.typ(&artifacts.objects) else {
            continue;
        };
        let ty = unalias_readonly(&artifacts.types, ty);
        let under = ty.underlying(&artifacts.types);
        if matches!(
            artifacts.types.get(under),
            TypeData::Basic(b) if b.info().contains(IS_STRING)
        ) {
            return Some(i);
        }
    }
    None
}

fn assert_has_formatted_analogue(call: &CallMeta<'_>) -> bool {
    // DEFERRED: look up `Fn+"f"` in assert/require packages / receiver methods.
    // Stubs and real testify expose f-analogues for the assertions we care about.
    !call.fn_name.ends_with('f')
}

fn is_printf_like_call(pass: &Pass<'_>, call: &CallMeta<'_>) -> Option<usize> {
    if call.call.ellipsis.is_valid() {
        return None;
    }
    let msg_and_args_pos = get_msg_and_args_position(pass, call)?;
    if msg_and_args_pos >= call.call.args.len() {
        return None;
    }
    if !assert_has_formatted_analogue(call) {
        return None;
    }
    Some(msg_and_args_pos)
}

fn string_lit_value(expr: &Expr) -> Option<String> {
    let Expr::BasicLit(BasicLit {
        kind: Some(Token::STRING),
        value,
        ..
    }) = unparen(expr)
    else {
        return None;
    };
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        serde_json::from_str(value).ok()
    } else if value.len() >= 2 && value.starts_with('`') && value.ends_with('`') {
        Some(value[1..value.len() - 1].to_string())
    } else {
        None
    }
}

fn check_formatter(
    pass: &Pass<'_>,
    call: &CallMeta<'_>,
    opts: &TestifylintOptions,
    pending: &mut Vec<(u32, String)>,
) {
    if call.is_fmt {
        check_formatter_fmt(pass, call, pending);
    } else {
        check_formatter_not_fmt(pass, call, opts, pending);
    }
}

fn check_formatter_not_fmt(
    pass: &Pass<'_>,
    call: &CallMeta<'_>,
    opts: &TestifylintOptions,
    pending: &mut Vec<(u32, String)>,
) {
    let Some(msg_and_args_pos) = is_printf_like_call(pass, call) else {
        return;
    };
    let last_arg_pos = call.call.args.len() - 1;
    let is_single = msg_and_args_pos == last_arg_pos;
    let msg_and_args = &call.call.args[msg_and_args_pos];

    if matches!(call.fn_name_trimmed.as_str(), "Fail" | "FailNow") {
        if let Some(failure_msg) = string_lit_value(&call.args[0]) {
            if failure_msg.contains('%') {
                report_msg(
                    "formatter",
                    call,
                    "failure message is not a format string, use msgAndArgs instead",
                    pending,
                );
                return;
            }
        }
    }

    if is_fmt_sprintf_call(pass, msg_and_args).is_some() && is_single {
        if opts.formatter_require_f_funcs {
            report_msg(
                "formatter",
                call,
                &format!("use {}.{}f", call.selector_x, call.fn_name),
                pending,
            );
        } else {
            report_msg(
                "formatter",
                call,
                "remove unnecessary fmt.Sprintf",
                pending,
            );
        }
        return;
    }

    if has_string_type(pass, msg_and_args) {
        if let Some(format) = string_lit_value(msg_and_args) {
            if format.is_empty() {
                report_msg("formatter", call, "empty message", pending);
                return;
            }
        }
        if opts.formatter_require_f_funcs {
            report_msg(
                "formatter",
                call,
                &format!("use {}.{}f", call.selector_x, call.fn_name),
                pending,
            );
        }
    } else if is_single {
        if opts.formatter_require_string_msg {
            report_msg(
                "formatter",
                call,
                "do not use non-string value as first element (msg) of msgAndArgs",
                pending,
            );
        }
    } else {
        report_msg(
            "formatter",
            call,
            "using msgAndArgs with non-string first element (msg) causes panic",
            pending,
        );
    }
}

fn check_formatter_fmt(pass: &Pass<'_>, call: &CallMeta<'_>, pending: &mut Vec<(u32, String)>) {
    let Some(format_pos) = get_msg_position(pass, call) else {
        return;
    };
    if format_pos >= call.call.args.len() {
        return;
    }
    let last_arg_pos = call.call.args.len() - 1;
    let msg = &call.call.args[format_pos];
    let no_format_args = format_pos == last_arg_pos;

    if no_format_args {
        if is_fmt_sprintf_call(pass, msg).is_some() {
            report_msg(
                "formatter",
                call,
                "remove unnecessary fmt.Sprintf",
                pending,
            );
            return;
        }
    }

    if let Some(format) = string_lit_value(msg) {
        if format.is_empty() {
            report_msg("formatter", call, "empty message", pending);
        }
        // DEFERRED: check-format-string via full printf.CheckPrintf.
    }
}

fn lookup_named_type(pass: &Pass<'_>, pkg_path: &str, name: &str) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    for i in 0..artifacts.packages.len() {
        let pid = artifacts.packages.id_at(i);
        let pkg = artifacts.packages.get(pid);
        if cut_vendor(pkg.path()) != pkg_path {
            continue;
        }
        let oid = scope_lookup(&artifacts.scopes, pkg.scope(), name)?;
        let ObjectData::TypeName(tn) = artifacts.objects.get(oid) else {
            continue;
        };
        return tn.typ();
    }
    None
}

fn implements_iface(pass: &Pass<'_>, typ: TypeId, iface: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    if api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        iface,
    ) {
        return true;
    }
    let ptr = new_pointer(&mut types, typ);
    api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        ptr,
        iface,
    )
}

fn implements_testify_suite(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(iface) = lookup_named_type(pass, SUITE_PKG, "TestingSuite") else {
        return false;
    };
    implements_iface(pass, typ, iface)
}

fn implements_testing_t(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    if let Some(iface) = lookup_named_type(pass, ASSERT_PKG, "TestingT") {
        if implements_iface(pass, typ, iface) {
            return true;
        }
    }
    if let Some(iface) = lookup_named_type(pass, REQUIRE_PKG, "TestingT") {
        if implements_iface(pass, typ, iface) {
            return true;
        }
    }
    false
}

fn check_suite_dont_use_pkg(
    pass: &Pass<'_>,
    call: &CallMeta<'_>,
    pending: &mut Vec<(u32, String)>,
) {
    if !call.is_pkg {
        return;
    }
    // Raw first arg is `t` for package-level assertions.
    if call.call.args.len() < 2 {
        return;
    }
    let t = &call.call.args[0];
    let Expr::CallExpr(ce) = t else {
        return;
    };
    let Expr::SelectorExpr(se) = &*ce.fun else {
        return;
    };
    if !implements_testify_suite(pass, &se.x) {
        return;
    }
    if se.sel.name != "T" {
        return;
    }
    // Prefer Ident receiver (`s.T()`), matching upstream.
    let Expr::Ident(rcv) = &*se.x else {
        return;
    };
    let mut new_selector = rcv.name.clone();
    if !call.is_assert {
        new_selector.push_str(".Require()");
    }
    report_msg(
        "suite-dont-use-pkg",
        call,
        &format!("use {new_selector}.{}", call.fn_name),
        pending,
    );
}

fn check_suite_extra_assert_call(
    pass: &Pass<'_>,
    call: &CallMeta<'_>,
    opts: &TestifylintOptions,
    pending: &mut Vec<(u32, String)>,
) {
    if call.is_pkg {
        return;
    }
    match opts.suite_extra_assert_call_mode {
        SuiteExtraAssertCallMode::Require => {
            let Expr::Ident(x) = &*call.selector.x else {
                return;
            };
            if !implements_testify_suite(pass, &call.selector.x) {
                return;
            }
            report_msg(
                "suite-extra-assert-call",
                call,
                &format!("use an explicit {}.Assert().{}", x.name, call.fn_name),
                pending,
            );
        }
        SuiteExtraAssertCallMode::Remove => {
            let Expr::CallExpr(x) = &*call.selector.x else {
                return;
            };
            let Expr::SelectorExpr(se) = &*x.fun else {
                return;
            };
            if !implements_testify_suite(pass, &se.x) {
                return;
            }
            if se.sel.name != "Assert" {
                return;
            }
            report_msg(
                "suite-extra-assert-call",
                call,
                &format!(
                    "need to simplify the assertion to {}.{}",
                    expr_string(&se.x),
                    call.fn_name
                ),
                pending,
            );
        }
    }
}

fn check_suite_subtest_run(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Expr::SelectorExpr(se) = &*call.fun else {
        return;
    };
    if se.sel.name != "Run" {
        return;
    }
    let Expr::CallExpr(t_call) = &*se.x else {
        return;
    };
    let Expr::SelectorExpr(t_sel) = &*t_call.fun else {
        return;
    };
    if t_sel.sel.name != "T" {
        return;
    }
    if implements_testify_suite(pass, &t_sel.x) && implements_testing_t(pass, &se.x) {
        pending.push((
            call.pos().0 as u32,
            format!(
                "suite-subtest-run: use {}.Run to run subtest",
                expr_string(&t_sel.x)
            ),
        ));
    }
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
    if enabled.contains("zero") {
        check_zero(pass, call, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("time-compare") {
        check_time_compare(pass, call, opts, pending);
        if pending.len() > before {
            return;
        }
    }
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
    if enabled.contains("negative-positive") {
        check_negative_positive(pass, call, pending);
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
    if enabled.contains("contains") {
        check_contains(pass, call, pending);
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
    if enabled.contains("error-is-as") {
        check_error_is_as(pass, call, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("encoded-compare") {
        check_encoded_compare(pass, call, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("expected-actual") {
        check_expected_actual(pass, call, opts, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("len") {
        check_len(pass, call, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("equal-values") {
        check_equal_values(pass, call, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("regexp") {
        check_regexp(pass, call, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("suite-extra-assert-call") {
        check_suite_extra_assert_call(pass, call, opts, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("suite-dont-use-pkg") {
        check_suite_dont_use_pkg(pass, call, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("useless-assert") {
        check_useless_assert(pass, call, pending);
        if pending.len() > before {
            return;
        }
    }
    if enabled.contains("formatter") {
        check_formatter(pass, call, opts, pending);
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
                if enabled.contains("suite-subtest-run") {
                    check_suite_subtest_run(pass, ce, &mut pending);
                }
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
