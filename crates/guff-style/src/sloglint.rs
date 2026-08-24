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
//! DEFERRED: SuggestedFix for `context: scope`, discard-handler, and
//! key-naming-case; Go 1.24 version gate for discard-handler.

use std::sync::OnceLock;

use guff::ast::{BasicLit, CallExpr, CompositeLit, Expr, FieldList, FuncType, Ident};
use guff::scope::{ObjDecl, ObjKind};
use guff::token::Token;
use guff::walk::{self, NodeRef, Visitor};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
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

/// Whitespace is a word separator for `github.com/ettle/strcase`, which is what
/// upstream sloglint uses. It matters beyond keys with spaces in them: the
/// message itself is built as `caseFn(caseName + " case")`, so the case
/// function has to turn `snake case` into `snake_case` — apply it to a string
/// that treats the space as content and the sentence comes out unchanged.
fn is_strcase_space(c: char) -> bool {
    c == ' ' || c == '\t' || c == '\n' || c == '\r' || (c as u32 >= 128 && c.is_whitespace())
}

fn to_snake(s: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == '-' || c == '_' || is_strcase_space(c) {
            if !out.ends_with('_') {
                out.push('_');
            }
            continue;
        }
        if c.is_uppercase() {
            if i > 0 && !out.ends_with('_') {
                let prev = chars[i - 1];
                let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
                if prev.is_lowercase() || prev.is_ascii_digit() || next_lower {
                    out.push('_');
                }
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out.trim_matches('_').to_string()
}

fn to_kebab(s: &str) -> String {
    to_snake(s).replace('_', "-")
}

fn to_pascal(s: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for c in s.chars() {
        if c == '_' || c == '-' || is_strcase_space(c) {
            upper = true;
            continue;
        }
        if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn to_camel(s: &str) -> String {
    let p = to_pascal(s);
    let mut chars = p.chars();
    match chars.next() {
        None => String::new(),
        Some(f) => f.to_lowercase().collect::<String>() + chars.as_str(),
    }
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
    name: String,
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
            out.push(CtxParam {
                name: field.names[0].name.clone(),
            });
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
    pending: &mut Vec<(u32, String)>,
) {
    if opts.no_raw_keys && !is_const_key(pass, key) {
        let name = key_name(pass, key).unwrap_or_else(|| "…".into());
        pending.push((
            key.pos().0 as u32,
            format!("the {name:?} key should be a constant"),
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
                ));
            }
        }
    }
}

fn analyze_attr_key_from_lit(
    pass: &Pass<'_>,
    opts: &SloglintOptions,
    lit: &CompositeLit,
    pending: &mut Vec<(u32, String)>,
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
    pending: &mut Vec<(u32, String)>,
) {
    if opts.static_msg && !is_static_msg(pass, msg) {
        pending.push((
            msg.pos().0 as u32,
            "message should be a string literal or a constant".into(),
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
        pending.push((msg.pos().0 as u32, format!("message should be {style}")));
    }
}

fn analyze_arguments(
    pass: &Pass<'_>,
    opts: &SloglintOptions,
    call: &CallExpr,
    args: &[Expr],
    pending: &mut Vec<(u32, String)>,
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
            ));
        } else {
            for attr in &attrs {
                if is_group_call(pass, attr) {
                    continue;
                }
                pending.push((attr.pos().0 as u32, "attributes should not be used".into()));
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
            ));
        } else if let Some(key) = keys.first() {
            pending.push((
                key.pos().0 as u32,
                "key-value pairs should not be used".into(),
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
    pending: &mut Vec<(u32, String)>,
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
                    ));
                } else if mode == "scope" {
                    for params in ctx_stack.iter().rev() {
                        if !params.is_empty() {
                            // DEFERRED: SuggestedFix inserting ctx arg
                            let _ = &params[0].name;
                            pending.push((
                                sel.sel.pos().0 as u32,
                                format!("{base}Context should be used instead"),
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
    pending: &mut Vec<(u32, String)>,
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
    pending: &'a mut Vec<(u32, String)>,
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
    let mut pending = Vec::new();

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

    for (pos, msg) in pending {
        pass.reportf(pos, &msg);
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
