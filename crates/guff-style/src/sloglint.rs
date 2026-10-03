//! Port of [`go-simpler.org/sloglint`](https://go-simpler.org/sloglint)
//! (golangci-lint wrapper in `pkg/golinters/sloglint`).
//!
//! Enforces consistent `log/slog` style: no mixed key-value/attr args (default),
//! plus optional checks for global loggers, context-only calls, static messages,
//! key naming, and argument layout.
//!
//! Defaults match golangci-lint `linters.settings.sloglint`
//! (`no-mixed-args: true`; other checks off).
//!
//! Fixes: `key-naming-case` re-quotes the re-cased key with a Go-exact
//! `strconv.Quote`, and `context: scope` rewrites `Info(` into
//! `InfoContext(ctx, `. `context: all` reports **without** a fix — upstream
//! says why in one line: "we don't know whether there is a context in the
//! scope" (`function_checks.go:62`).
//!
//! DEFERRED: SuggestedFix for discard-handler, together with its Go 1.24
//! version gate. `slog.DiscardHandler` landed in Go 1.24 and this case's module
//! is `go 1.22`, so the arm is unreachable from the fixture — shipping the fix
//! would be shipping code no gate measures.

use std::sync::OnceLock;

/// A finding, and the edit that fixes it when there is one.
type Pending = Vec<(u32, String, Option<(u32, u32, String)>)>;

use guff::ast::{BasicLit, CallExpr, CompositeLit, Expr, FieldList, FuncType, Ident};
use guff::scope::{ObjDecl, ObjKind};
use guff::token::Token;
use guff::walk::{self, NodeRef, Visitor};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::alias::unalias_readonly;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::BasicKind;
use guff_types::TypeId;

use crate::options::SloglintOptions;

#[derive(Clone, Debug)]
struct Func {
    full_name: String,
    message_pos: i32,
    arguments_pos: i32,
}

fn slog_funcs() -> &'static [Func] {
    static F: OnceLock<Vec<Func>> = OnceLock::new();
    F.get_or_init(|| {
        [
            ("log/slog.Log", 2, 3),
            ("log/slog.LogAttrs", 2, 3),
            ("log/slog.Debug", 0, 1),
            ("log/slog.Info", 0, 1),
            ("log/slog.Warn", 0, 1),
            ("log/slog.Error", 0, 1),
            ("log/slog.DebugContext", 1, 2),
            ("log/slog.InfoContext", 1, 2),
            ("log/slog.WarnContext", 1, 2),
            ("log/slog.ErrorContext", 1, 2),
            ("log/slog.With", -1, 0),
            ("log/slog.Group", -1, 1),
            ("log/slog.GroupAttrs", -1, 1),
            ("log/slog.NewTextHandler", -1, -1),
            ("log/slog.NewJSONHandler", -1, -1),
            ("(*log/slog.Logger).Log", 2, 3),
            ("(*log/slog.Logger).LogAttrs", 2, 3),
            ("(*log/slog.Logger).Debug", 0, 1),
            ("(*log/slog.Logger).Info", 0, 1),
            ("(*log/slog.Logger).Warn", 0, 1),
            ("(*log/slog.Logger).Error", 0, 1),
            ("(*log/slog.Logger).DebugContext", 1, 2),
            ("(*log/slog.Logger).InfoContext", 1, 2),
            ("(*log/slog.Logger).WarnContext", 1, 2),
            ("(*log/slog.Logger).ErrorContext", 1, 2),
            ("(*log/slog.Logger).With", -1, 0),
        ]
        .into_iter()
        .map(|(n, m, a)| Func {
            full_name: n.into(),
            message_pos: m,
            arguments_pos: a,
        })
        .collect()
    })
}

fn cut_vendor(path: &str) -> String {
    if let Some(i) = path.rfind("/vendor/") {
        return path[i + "/vendor/".len()..].to_string();
    }
    if let Some(r) = path.strip_prefix("vendor/") {
        return r.to_string();
    }
    path.to_string()
}

fn callee_name(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    let info = pass.types_info()?;
    let obj_id = match &*call.fun {
        Expr::Ident(id) => info.uses.get(&id.id).copied()?,
        Expr::SelectorExpr(sel) => info.uses.get(&sel.sel.id).copied()?,
        _ => return None,
    };
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if !matches!(artifacts.objects.get(obj_id), ObjectData::Func(_)) {
        return None;
    }
    let mut name = code::type_func_name(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        obj_id,
    );
    if !name.contains('.') && !name.starts_with('(') {
        if let Some(pkg) = obj_id.pkg(&artifacts.objects) {
            if artifacts.packages.get(pkg).path().is_empty() && !pass.pkg().pkg_path.is_empty() {
                name = format!("{}.{}", pass.pkg().pkg_path, name);
            }
        }
    }
    Some(cut_vendor(&name))
}

fn type_of_expr(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn type_of_node_id(pass: &Pass<'_>, id: u32) -> Option<TypeId> {
    if id == 0 {
        return None;
    }
    let info = pass.types_info()?;
    Some(info.types.get(&id)?.typ)
}

fn type_name_of(pass: &Pass<'_>, typ: TypeId) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    Some(guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    ))
}

fn is_string_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Basic(b) => {
            matches!(b.kind(), BasicKind::String | BasicKind::UntypedString)
        }
        _ => false,
    }
}

fn is_attr_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(name) = type_name_of(pass, typ) else {
        return false;
    };
    name == "log/slog.Attr" || name.ends_with("/slog.Attr") || name == "Attr"
}

fn is_skip_slice(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(name) = type_name_of(pass, typ) else {
        return false;
    };
    matches!(
        name.as_str(),
        "[]any" | "[]interface{}" | "[]log/slog.Attr" | "[]Attr"
    ) || (name.starts_with("[]") && name.ends_with("/slog.Attr"))
}

fn is_group_call(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Expr::CallExpr(call) = expr else {
        return false;
    };
    matches!(
        callee_name(pass, call).as_deref(),
        Some("log/slog.Group" | "log/slog.GroupAttrs")
    )
}

/// Resolve a log key name the way upstream sloglint does: only string
/// literals and same-package const idents (via AST `Ident.obj`). Cross-package
/// selectors like `slogs.ColName` intentionally yield `None`, so
/// `key-naming-case` / allowed / forbidden checks skip them.
fn key_name(pass: &Pass<'_>, key: &Expr) -> Option<String> {
    match key {
        Expr::BasicLit(_) => code::expr_to_string(pass, key),
        Expr::Ident(id) => {
            let obj = id.obj.lock().ok()?.clone()?;
            if obj.kind != ObjKind::Con {
                return None;
            }
            let ObjDecl::ValueSpec(vs) = &obj.decl else {
                return None;
            };
            // Upstream always takes Values[0] (TODO for multi-value specs).
            vs.values
                .first()
                .and_then(|v| code::expr_to_string(pass, v))
        }
        _ => None,
    }
}

fn is_const_key(pass: &Pass<'_>, key: &Expr) -> bool {
    let id = match key {
        Expr::SelectorExpr(sel) => &sel.sel,
        Expr::Ident(id) => id,
        _ => return false,
    };
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(obj) = info.uses.get(&id.id).copied() else {
        return false;
    };
    matches!(artifacts.objects.get(obj), ObjectData::Const(_))
}

/// Port of `github.com/ettle/strcase` v0.2.0, the four functions sloglint
/// calls: `convertWithoutInitialisms` driven by `defaultSplitFn`.
///
/// The case functions are also applied to the *message*: upstream builds it as
/// `caseFn(caseName + " case")`, so `snake case` has to come out `snake_case`.
///
/// guff had a hand-written approximation that split only on `_`, `-` and
/// whitespace. `.` is a delimiter upstream too, so dotted keys —
/// `slog.String("http.method", …)`, the OpenTelemetry spelling — were never
/// reported as not being snake_case.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WordCase {
    Lower,
    Title,
    Camel,
}

#[derive(PartialEq, Eq)]
enum SplitAction {
    Noop,
    Split,
    SkipSplit,
}

/// `isSpace`: ASCII whitespace, then `unicode.IsSpace` above 127.
fn strcase_is_space(r: char) -> bool {
    matches!(r, ' ' | '\t' | '\n' | '\r') || (r as u32 >= 128 && r.is_whitespace())
}

/// `defaultSplitFn`. `prev` and `next` are `'\0'` past either end, as the
/// zero rune is upstream.
fn default_split_fn(prev: char, curr: char, next: char) -> SplitAction {
    if curr.is_lowercase() {
        return SplitAction::Noop;
    }
    if curr == '_' || curr == '-' || strcase_is_space(curr) {
        return SplitAction::SkipSplit;
    }
    if curr.is_uppercase() && (prev.is_lowercase() || (prev.is_uppercase() && next.is_lowercase())) {
        return SplitAction::Split;
    }
    // `unicode.IsNumber` is the N category, which is what `is_numeric` tests.
    if prev.is_numeric() {
        // `v4.3` is not split.
        if (curr == '.' || curr == ',') && next.is_numeric() {
            return SplitAction::Noop;
        }
        if !curr.is_numeric() && curr != '.' {
            return SplitAction::Split;
        }
    }
    if curr == '.' {
        return SplitAction::SkipSplit;
    }
    SplitAction::Noop
}

/// Go's `unicode.ToUpper` / `ToLower` map one rune to one rune. Rust's can
/// expand (`ß` → `SS`); where they do, Go has no simple mapping and keeps the
/// rune — except `İ`, whose simple lowercase is `i`.
fn strcase_to_upper(r: char) -> char {
    let mut it = r.to_uppercase();
    match (it.next(), it.next()) {
        (Some(u), None) => u,
        _ => r,
    }
}

fn strcase_to_lower(r: char) -> char {
    let mut it = r.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        (Some('i'), Some(_)) => 'i',
        _ => r,
    }
}

/// `convertWithoutInitialisms(input, delimiter, wordCase)`; `None` is the zero
/// delimiter (no separator written).
fn convert_without_initialisms(input: &str, delimiter: Option<char>, word_case: WordCase) -> String {
    // `strings.TrimSpace` trims Unicode whitespace, as `str::trim` does.
    let runes: Vec<char> = input.trim().chars().collect();
    let mut b = String::with_capacity(input.len() + 4);
    let mut curr = '\0';
    let mut in_word = false;
    let mut first_word = true;
    for i in 0..runes.len() {
        let prev = curr;
        curr = runes[i];
        let next = runes.get(i + 1).copied().unwrap_or('\0');
        match default_split_fn(prev, curr, next) {
            SplitAction::SkipSplit => {
                if in_word {
                    b.extend(delimiter);
                }
                in_word = false;
                continue;
            }
            SplitAction::Split => {
                if in_word {
                    b.extend(delimiter);
                }
                in_word = false;
            }
            SplitAction::Noop => {}
        }
        let out = match word_case {
            WordCase::Lower => strcase_to_lower(curr),
            WordCase::Title if in_word => strcase_to_lower(curr),
            WordCase::Title => strcase_to_upper(curr),
            WordCase::Camel if in_word => strcase_to_lower(curr),
            WordCase::Camel if first_word => {
                first_word = false;
                strcase_to_lower(curr)
            }
            WordCase::Camel => strcase_to_upper(curr),
        };
        b.push(out);
        in_word = true;
    }
    b
}

fn to_snake(s: &str) -> String {
    convert_without_initialisms(s, Some('_'), WordCase::Lower)
}

fn to_kebab(s: &str) -> String {
    convert_without_initialisms(s, Some('-'), WordCase::Lower)
}

fn to_pascal(s: &str) -> String {
    convert_without_initialisms(s, None, WordCase::Title)
}

fn to_camel(s: &str) -> String {
    convert_without_initialisms(s, None, WordCase::Camel)
}

fn case_fn(case_name: &str) -> Option<fn(&str) -> String> {
    match case_name {
        "snake" => Some(to_snake),
        "kebab" => Some(to_kebab),
        "camel" => Some(to_camel),
        "pascal" => Some(to_pascal),
        _ => None,
    }
}

fn all_funcs(opts: &SloglintOptions) -> Vec<Func> {
    let mut out: Vec<Func> = slog_funcs().to_vec();
    for c in &opts.custom_funcs {
        out.push(Func {
            full_name: c.name.clone(),
            message_pos: c.msg_pos,
            arguments_pos: c.args_pos,
        });
    }
    out
}

fn find_func<'a>(funcs: &'a [Func], name: &str) -> Option<(usize, &'a Func)> {
    funcs.iter().enumerate().find(|(_, f)| f.full_name == name)
}

struct CtxParam {
    /// The text to pass as the context argument: the parameter's own name, or
    /// `<name>.Context()` when it is an `*http.Request`. Upstream builds the
    /// same string as `ctxArg` (`function_checks.go:88`), and
    /// `collect_ctx_params` below already tells the two apart — it just kept
    /// the name.
    arg: String,
}

fn collect_ctx_params(pass: &Pass<'_>, params: &FieldList) -> Vec<CtxParam> {
    let mut out = Vec::new();
    for field in &params.list {
        if field.names.is_empty() {
            continue;
        }
        let Some(ty_expr) = field.ty.as_ref() else {
            continue;
        };
        let Some(typ) = type_of_expr(pass, ty_expr) else {
            continue;
        };
        let Some(name) = type_name_of(pass, typ) else {
            continue;
        };
        let ok = name == "context.Context"
            || name == "*net/http.Request"
            || (name.starts_with('*') && name.ends_with("/http.Request"));
        if ok {
            let param = field.names[0].name.clone();
            let arg = if name == "context.Context" {
                param
            } else {
                format!("{param}.Context()")
            };
            out.push(CtxParam { arg });
        }
    }
    out
}

fn params_from_func_type(pass: &Pass<'_>, ft: &FuncType) -> Vec<CtxParam> {
    ft.params
        .as_ref()
        .map(|p| collect_ctx_params(pass, p))
        .unwrap_or_default()
}

fn analyze_key(
    pass: &Pass<'_>,
    opts: &SloglintOptions,
    key: &Expr,
    pending: &mut Pending,
) {
    if opts.no_raw_keys && !is_const_key(pass, key) {
        let name = key_name(pass, key).unwrap_or_else(|| "…".into());
        pending.push((
            key.pos().0 as u32,
            format!("the {name:?} key should be a constant"),
            None,
        ));
    }
    if let Some(case_name) = opts.key_naming_case.as_deref() {
        if let (Some(name), Some(cf)) = (key_name(pass, key), case_fn(case_name)) {
            if name != cf(&name) {
                pending.push((
                    key.pos().0 as u32,
                    // Upstream builds this as `caseFn(caseName + " case")` —
                    // the naming function is applied to the *sentence*, so the
                    // message reads `snake_case`, `kebab-case`, `camelCase`,
                    // `PascalCase`. Printing the raw setting plus " case"
                    // agrees with none of them.
                    format!("keys should be written in {}", cf(&format!("{case_name} case"))),
                    // `strconv.AppendQuote(nil, caseFn(name))` — the re-cased
                    // key, re-quoted the way Go spells a string literal. The
                    // Go-exact quoter is in `guff-gostd` because `dupword`
                    // needed it (続き 74); this is the second caller.
                    Some((
                        key.pos().0 as u32,
                        key.end().0 as u32,
                        guff_gostd::strconv::quote(&cf(&name)),
                    )),
                ));
            }
        }
    }
    if !opts.allowed_keys.is_empty() {
        if let Some(name) = key_name(pass, key) {
            if !opts.allowed_keys.iter().any(|k| k == &name) {
                pending.push((
                    key.pos().0 as u32,
                    format!("the {name:?} key is not allowed and should not be used"),
                    None,
                ));
            }
        }
    }
    if !opts.forbidden_keys.is_empty() {
        if let Some(name) = key_name(pass, key) {
            if opts.forbidden_keys.iter().any(|k| k == &name) {
                pending.push((
                    key.pos().0 as u32,
                    format!("the {name:?} key is forbidden and should not be used"),
                    None,
                ));
            }
        }
    }
}

fn analyze_attr_key_from_lit(
    pass: &Pass<'_>,
    opts: &SloglintOptions,
    lit: &CompositeLit,
    pending: &mut Pending,
) {
    match lit.elts.len() {
        1 => {
            if let Expr::KeyValueExpr(kv) = &lit.elts[0] {
                if let Expr::Ident(Ident { name, .. }) = &*kv.key {
                    if name == "Key" {
                        analyze_key(pass, opts, &kv.value, pending);
                    }
                }
            }
        }
        2 => {
            if let Expr::KeyValueExpr(kv) = &lit.elts[0] {
                if let Expr::Ident(Ident { name, .. }) = &*kv.key {
                    if name == "Key" {
                        analyze_key(pass, opts, &kv.value, pending);
                        return;
                    }
                }
            }
            if let Expr::KeyValueExpr(kv) = &lit.elts[1] {
                if let Expr::Ident(Ident { name, .. }) = &*kv.key {
                    if name == "Key" {
                        analyze_key(pass, opts, &kv.value, pending);
                        return;
                    }
                }
            }
            if !matches!(lit.elts[0], Expr::KeyValueExpr(_)) {
                analyze_key(pass, opts, &lit.elts[0], pending);
            }
        }
        _ => {}
    }
}

fn is_static_msg(pass: &Pass<'_>, msg: &Expr) -> bool {
    match msg {
        Expr::BasicLit(BasicLit { kind, .. }) => *kind == Some(Token::STRING),
        Expr::Ident(id) => {
            let Some(info) = pass.types_info() else {
                return false;
            };
            let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                return false;
            };
            let Some(obj) = info.uses.get(&id.id).copied() else {
                return false;
            };
            matches!(artifacts.objects.get(obj), ObjectData::Const(_))
        }
        Expr::BinaryExpr(b) if b.op == Token::ADD => {
            is_static_msg(pass, &b.x) && is_static_msg(pass, &b.y)
        }
        _ => false,
    }
}

fn analyze_message(
    pass: &Pass<'_>,
    opts: &SloglintOptions,
    msg: &Expr,
    pending: &mut Pending,
) {
    if opts.static_msg && !is_static_msg(pass, msg) {
        pending.push((
            msg.pos().0 as u32,
            "message should be a string literal or a constant".into(),
            None,
        ));
    }
    let Some(style) = opts.msg_style.as_deref() else {
        return;
    };
    let Some(s) = code::expr_to_string(pass, msg) else {
        return;
    };
    let trimmed: Vec<char> = s.trim().chars().collect();
    if trimmed.len() < 2 {
        return;
    }
    let first = trimmed[0];
    let second = trimmed[1];
    if !first.is_alphabetic() {
        return;
    }
    let bad = match style {
        "lowercased" => {
            first.is_uppercase() && !second.is_ascii_punctuation() && !second.is_uppercase()
        }
        "capitalized" => first.is_lowercase() && !second.is_uppercase(),
        _ => false,
    };
    if bad {
        pending.push((msg.pos().0 as u32, format!("message should be {style}"), None));
    }
}

fn analyze_arguments(
    pass: &Pass<'_>,
    opts: &SloglintOptions,
    call: &CallExpr,
    args: &[Expr],
    pending: &mut Pending,
) {
    let mut keys = Vec::new();
    let mut attrs = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let Some(typ) = type_of_expr(pass, &args[i]) else {
            i += 1;
            continue;
        };
        if is_skip_slice(pass, typ) {
            i += 1;
            continue;
        }
        if is_string_type(pass, typ) {
            keys.push(&args[i]);
            analyze_key(pass, opts, &args[i], pending);
            i += 2;
            continue;
        }
        if is_attr_type(pass, typ) {
            attrs.push(&args[i]);
            i += 1;
            continue;
        }
        i += 1;
    }

    if opts.no_mixed_args && !keys.is_empty() {
        for attr in &attrs {
            if is_group_call(pass, attr) {
                continue;
            }
            pending.push((
                attr.pos().0 as u32,
                "key-value pairs and attributes should not be mixed".into(),
                None,
            ));
            break;
        }
    }

    if opts.kv_only {
        let name = callee_name(pass, call).unwrap_or_default();
        let replacement = match name.as_str() {
            "log/slog.GroupAttrs" => Some("slog.Group"),
            "log/slog.LogAttrs" => Some("slog.Log"),
            "(*log/slog.Logger).LogAttrs" => Some("slog.Logger.Log"),
            _ => None,
        };
        if let Some(r) = replacement {
            pending.push((
                call.pos().0 as u32,
                format!("use {r} with key-value pairs instead"),
                None,
            ));
        } else {
            for attr in &attrs {
                if is_group_call(pass, attr) {
                    continue;
                }
                pending.push((attr.pos().0 as u32, "attributes should not be used".into(), None));
                break;
            }
        }
    }

    if opts.attr_only {
        let name = callee_name(pass, call).unwrap_or_default();
        let replacement = match name.as_str() {
            "log/slog.Group" => Some("slog.GroupAttrs"),
            "log/slog.Log" => Some("slog.LogAttrs"),
            "(*log/slog.Logger).Log" => Some("slog.Logger.LogAttrs"),
            _ => None,
        };
        if let Some(r) = replacement {
            pending.push((
                call.pos().0 as u32,
                format!("use {r} with attributes instead"),
                None,
            ));
        } else if let Some(key) = keys.first() {
            pending.push((
                key.pos().0 as u32,
                "key-value pairs should not be used".into(),
                None,
            ));
        }
    }

    if opts.args_on_sep_lines {
        let mut all: Vec<&Expr> = Vec::new();
        all.extend(keys.iter().copied());
        all.extend(attrs.iter().copied());
        if all.len() > 1 {
            if let Some(fset) = pass.pkg().fset.as_ref() {
                let mut prev = fset.position(all[0].pos()).line;
                for arg in &all[1..] {
                    let curr = fset.position(arg.pos()).line;
                    if curr == prev {
                        pending.push((
                            arg.pos().0 as u32,
                            "arguments should be put on separate lines".into(),
                            None,
                        ));
                        break;
                    }
                    prev = curr;
                }
            }
        }
    }
}

fn method_base_name(full: &str) -> &str {
    full.rsplit(['.', ')']).next().unwrap_or(full)
}

fn analyze_function(
    pass: &Pass<'_>,
    opts: &SloglintOptions,
    call: &CallExpr,
    name: &str,
    ctx_stack: &[Vec<CtxParam>],
    pending: &mut Pending,
) {
    if let Some(mode) = opts.no_global.as_deref() {
        let base = method_base_name(name);
        if matches!(
            base,
            "Log" | "LogAttrs"
                | "Debug"
                | "Info"
                | "Warn"
                | "Error"
                | "DebugContext"
                | "InfoContext"
                | "WarnContext"
                | "ErrorContext"
                | "With"
        ) {
            if let Expr::SelectorExpr(sel) = &*call.fun {
                if let Expr::Ident(id) = &*sel.x {
                    if id.name == "slog" {
                        pending.push((
                            id.pos().0 as u32,
                            "default logger should not be used".into(),
                            None,
                        ));
                    } else if mode == "all" {
                        if let (Some(info), Some(artifacts)) =
                            (pass.types_info(), pass.pkg().type_artifacts.as_ref())
                        {
                            if let Some(obj) = info.uses.get(&id.id).copied() {
                                if let Some(pkg) = obj.pkg(&artifacts.objects) {
                                    let pkg_scope = artifacts.packages.get(pkg).scope();
                                    if obj.parent(&artifacts.objects) == Some(pkg_scope) {
                                        pending.push((
                                            id.pos().0 as u32,
                                            "global logger should not be used".into(),
                                            None,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(mode) = opts.context.as_deref() {
        let base = method_base_name(name);
        if matches!(base, "Debug" | "Info" | "Warn" | "Error") {
            if let Expr::SelectorExpr(sel) = &*call.fun {
                if mode == "all" {
                    pending.push((
                        sel.sel.pos().0 as u32,
                        format!("{base}Context should be used instead"),
                        None,
                    ));
                } else if mode == "scope" {
                    for params in ctx_stack.iter().rev() {
                        if !params.is_empty() {
                            // The span runs from the method name to *past the
                            // opening paren* and the replacement carries its own
                            // `(`, so `Info(` becomes `InfoContext(ctx, ` in one
                            // edit (`function_checks.go:96`).
                            pending.push((
                                sel.sel.pos().0 as u32,
                                format!("{base}Context should be used instead"),
                                Some((
                                    sel.sel.pos().0 as u32,
                                    call.lparen.0 as u32 + 1,
                                    format!("{base}Context({}, ", params[0].arg),
                                )),
                            ));
                            break;
                        }
                    }
                }
            }
        }
    }

    if matches!(
        name,
        "log/slog.NewTextHandler" | "log/slog.NewJSONHandler"
    ) && !call.args.is_empty()
    {
        if let Expr::SelectorExpr(sel) = &call.args[0] {
            if let (Some(info), Some(artifacts)) =
                (pass.types_info(), pass.pkg().type_artifacts.as_ref())
            {
                if let Some(obj) = info.uses.get(&sel.sel.id).copied() {
                    let pkg_ok = obj
                        .pkg(&artifacts.objects)
                        .map(|p| {
                            let pkg = artifacts.packages.get(p);
                            pkg.name() == "io" || pkg.path() == "io"
                        })
                        .unwrap_or(false);
                    if pkg_ok && sel.sel.name == "Discard" {
                        pending.push((
                            call.pos().0 as u32,
                            "use slog.DiscardHandler instead".into(),
                            None,
                        ));
                    }
                }
            }
        }
    }
}

fn check_call(
    pass: &Pass<'_>,
    opts: &SloglintOptions,
    funcs: &[Func],
    call: &CallExpr,
    ctx_stack: &[Vec<CtxParam>],
    pending: &mut Pending,
) {
    let Some(name) = callee_name(pass, call) else {
        return;
    };

    if matches!(
        name.as_str(),
        "log/slog.Int"
            | "log/slog.Int64"
            | "log/slog.Uint64"
            | "log/slog.Float64"
            | "log/slog.String"
            | "log/slog.Bool"
            | "log/slog.Time"
            | "log/slog.Duration"
            | "log/slog.Any"
    ) {
        if !call.args.is_empty() {
            analyze_key(pass, opts, &call.args[0], pending);
        }
        return;
    }

    if matches!(name.as_str(), "log/slog.Group" | "log/slog.GroupAttrs")
        && !call.args.is_empty()
    {
        analyze_key(pass, opts, &call.args[0], pending);
    }

    let Some((idx, func)) = find_func(funcs, &name) else {
        return;
    };
    let standard = idx < slog_funcs().len();
    if standard {
        analyze_function(pass, opts, call, &name, ctx_stack, pending);
    }
    if func.message_pos >= 0 {
        let pos = func.message_pos as usize;
        if call.args.len() > pos {
            analyze_message(pass, opts, &call.args[pos], pending);
        }
    }
    if func.arguments_pos >= 0 {
        let pos = func.arguments_pos as usize;
        if call.args.len() > pos {
            analyze_arguments(pass, opts, call, &call.args[pos..], pending);
        }
    }
}

struct SlogVisitor<'a, 'p> {
    pass: &'a Pass<'p>,
    opts: &'a SloglintOptions,
    funcs: &'a [Func],
    ctx_stack: Vec<Vec<CtxParam>>,
    pending: &'a mut Pending,
}

impl<'a, 'p> Visitor<'a> for SlogVisitor<'a, 'p> {
    fn enter(&mut self, node: NodeRef<'a>) -> bool {
        match node {
            NodeRef::FuncDecl(fd) => {
                self.ctx_stack
                    .push(params_from_func_type(self.pass, &fd.ty));
            }
            NodeRef::FuncLit(fl) => {
                self.ctx_stack
                    .push(params_from_func_type(self.pass, &fl.ty));
            }
            NodeRef::CallExpr(call) => {
                check_call(
                    self.pass,
                    self.opts,
                    self.funcs,
                    call,
                    &self.ctx_stack,
                    self.pending,
                );
            }
            NodeRef::CompositeLit(lit) => {
                if let Some(typ) = type_of_node_id(self.pass, lit.id) {
                    if is_attr_type(self.pass, typ) {
                        analyze_attr_key_from_lit(self.pass, self.opts, lit, self.pending);
                    }
                }
            }
            _ => {}
        }
        true
    }

    fn leave(&mut self, node: NodeRef<'a>) {
        match node {
            NodeRef::FuncDecl(_) | NodeRef::FuncLit(_) => {
                self.ctx_stack.pop();
            }
            _ => {}
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "sloglint requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<SloglintOptions>("sloglint")
        .cloned()
        .unwrap_or_default();

    if options.kv_only && options.attr_only {
        return Err("sloglint: kv-only and attr-only are incompatible".into());
    }

    let funcs = all_funcs(&options);
    let mut pending: Pending = Vec::new();

    for file in pass.files() {
        let mut visitor = SlogVisitor {
            pass,
            opts: &options,
            funcs: &funcs,
            ctx_stack: Vec::new(),
            pending: &mut pending,
        };
        walk::walk(&mut visitor, NodeRef::File(file));
    }

    for (pos, msg, fix) in pending {
        let Some((from, to, new_text)) = fix else {
            pass.reportf(pos, &msg);
            continue;
        };
        pass.report(Diagnostic {
            pos,
            message: msg,
            suggested_fixes: vec![SuggestedFix {
                message: String::new(),
                text_edits: vec![TextEdit {
                    pos: from,
                    end: to,
                    new_text,
                }],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "sloglint",
        doc: "Ensures consistent code style when using log/slog",
        url: "https://go-simpler.org/sloglint",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(input, ToSnake, ToKebab, ToCamel, ToPascal)` as printed by
    /// `github.com/ettle/strcase` v0.2.0 itself. The golden cases only see
    /// whether a key is reported; the fix text is the converted key.
    #[test]
    fn strcase_matches_upstream() {
        let cases = [
            ("http.method", "http_method", "http-method", "httpMethod", "HttpMethod"),
            ("HTTPServer", "http_server", "http-server", "httpServer", "HttpServer"),
            ("FOOBar", "foo_bar", "foo-bar", "fooBar", "FooBar"),
            ("v4.3", "v4.3", "v4.3", "v4.3", "V4.3"),
            ("80port", "80port", "80port", "80port", "80port"),
            ("a..b", "a_b", "a-b", "aB", "AB"),
            ("trailing_", "trailing_", "trailing-", "trailing", "Trailing"),
            (" spaced key ", "spaced_key", "spaced-key", "spacedKey", "SpacedKey"),
            ("straße", "straße", "straße", "straße", "Straße"),
            ("İstanbul", "istanbul", "istanbul", "istanbul", "İstanbul"),
            ("ⅫRoman", "ⅻ_roman", "ⅻ-roman", "ⅻRoman", "ⅫRoman"),
            ("Mixed.Case_key-x", "mixed_case_key_x", "mixed-case-key-x", "mixedCaseKeyX", "MixedCaseKeyX"),
            ("snake case", "snake_case", "snake-case", "snakeCase", "SnakeCase"),
        ];
        for (input, snake, kebab, camel, pascal) in cases {
            assert_eq!(to_snake(input), snake, "ToSnake({input:?})");
            assert_eq!(to_kebab(input), kebab, "ToKebab({input:?})");
            assert_eq!(to_camel(input), camel, "ToCamel({input:?})");
            assert_eq!(to_pascal(input), pascal, "ToPascal({input:?})");
        }
    }
}
