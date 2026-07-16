//! Port of [`github.com/timonwong/loggercheck`](https://github.com/timonwong/loggercheck)
//! (golangci-lint wrapper in `pkg/golinters/loggercheck`).
//!
//! Checks odd key-value pairs for common logger libraries (kitlog / klog /
//! logr / slog / zap), plus optional string-key and printf-like checks.
//!
//! Defaults match golangci-lint `linters.settings.loggercheck` (all checkers
//! enabled; `require-string-key` / `no-printf-like` off). Custom rules come
//! from `linters.settings.loggercheck.rules`.
//!
//! DEFERRED: rulefile path loading; full printf verb parser parity with
//! upstream `internal/checkers/printf` (simplified `%verb` scan used).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::named::named_obj;
use guff_types::signature::{signature_params, signature_variadic};
use guff_types::slice::slice_elem;
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::TypeId;

use crate::options::LoggercheckOptions;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheckerKind {
    General,
    Zap,
    Slog,
}

#[derive(Clone, Debug)]
struct RuleEntry {
    fqn: String,
    kind: CheckerKind,
}

fn static_rules() -> &'static [RuleEntry] {
    static R: OnceLock<Vec<RuleEntry>> = OnceLock::new();
    R.get_or_init(|| {
        let mut out = Vec::new();
        let push = |out: &mut Vec<RuleEntry>, kind: CheckerKind, names: &[&str]| {
            for n in names {
                out.push(RuleEntry {
                    fqn: (*n).to_string(),
                    kind,
                });
            }
        };
        push(
            &mut out,
            CheckerKind::General,
            &[
                "(github.com/go-logr/logr.Logger).Error",
                "(github.com/go-logr/logr.Logger).Info",
                "(github.com/go-logr/logr.Logger).WithValues",
            ],
        );
        push(
            &mut out,
            CheckerKind::General,
            &[
                "k8s.io/klog/v2.InfoS",
                "k8s.io/klog/v2.InfoSDepth",
                "k8s.io/klog/v2.ErrorS",
                "(k8s.io/klog/v2.Verbose).InfoS",
                "(k8s.io/klog/v2.Verbose).InfoSDepth",
                "(k8s.io/klog/v2.Verbose).ErrorS",
            ],
        );
        push(
            &mut out,
            CheckerKind::Zap,
            &[
                "(*go.uber.org/zap.SugaredLogger).With",
                "(*go.uber.org/zap.SugaredLogger).Debugw",
                "(*go.uber.org/zap.SugaredLogger).Infow",
                "(*go.uber.org/zap.SugaredLogger).Warnw",
                "(*go.uber.org/zap.SugaredLogger).Errorw",
                "(*go.uber.org/zap.SugaredLogger).DPanicw",
                "(*go.uber.org/zap.SugaredLogger).Panicw",
                "(*go.uber.org/zap.SugaredLogger).Fatalw",
            ],
        );
        push(
            &mut out,
            CheckerKind::General,
            &[
                "github.com/go-kit/log.With",
                "github.com/go-kit/log.WithPrefix",
                "github.com/go-kit/log.WithSuffix",
                "(github.com/go-kit/log.Logger).Log",
            ],
        );
        push(
            &mut out,
            CheckerKind::Slog,
            &[
                "log/slog.Group",
                "log/slog.With",
                "log/slog.Debug",
                "log/slog.Info",
                "log/slog.Warn",
                "log/slog.Error",
                "log/slog.DebugContext",
                "log/slog.InfoContext",
                "log/slog.WarnContext",
                "log/slog.ErrorContext",
                "(*log/slog.Logger).With",
                "(*log/slog.Logger).Debug",
                "(*log/slog.Logger).Info",
                "(*log/slog.Logger).Warn",
                "(*log/slog.Logger).Error",
                "(*log/slog.Logger).DebugContext",
                "(*log/slog.Logger).InfoContext",
                "(*log/slog.Logger).WarnContext",
                "(*log/slog.Logger).ErrorContext",
            ],
        );
        out
    })
}

fn ruleset_name(fqn: &str) -> &'static str {
    if fqn.contains("go-logr/logr") {
        "logr"
    } else if fqn.contains("k8s.io/klog") {
        "klog"
    } else if fqn.contains("go.uber.org/zap") {
        "zap"
    } else if fqn.contains("go-kit/log") {
        "kitlog"
    } else if fqn.contains("log/slog") {
        "slog"
    } else {
        "custom"
    }
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

fn enabled_set(opts: &LoggercheckOptions) -> HashSet<&'static str> {
    let mut s = HashSet::new();
    if opts.kitlog {
        s.insert("kitlog");
    }
    if opts.klog {
        s.insert("klog");
    }
    if opts.logr {
        s.insert("logr");
    }
    if opts.slog {
        s.insert("slog");
    }
    if opts.zap {
        s.insert("zap");
    }
    s.insert("custom");
    s
}

fn build_rule_index(opts: &LoggercheckOptions) -> HashMap<String, CheckerKind> {
    let enabled = enabled_set(opts);
    let mut map = HashMap::new();
    for r in static_rules() {
        let name = ruleset_name(&r.fqn);
        if !enabled.contains(name) {
            continue;
        }
        map.insert(r.fqn.clone(), r.kind);
    }
    for line in &opts.rules {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        map.insert(line.to_string(), CheckerKind::General);
    }
    map
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
    // Fixture typechecks leave Package.path empty → bare "Func".
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

fn is_empty_interface(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Interface(i) => i.num_explicit_methods() == 0 && i.num_embeddeds() == 0,
        _ => false,
    }
}

fn named_type_name(pass: &Pass<'_>, typ: TypeId) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Named(_) => {
            let obj = named_obj(&artifacts.types, typ);
            Some(obj.name(&artifacts.objects).to_string())
        }
        _ => None,
    }
}

fn filter_key_values<'a>(
    pass: &Pass<'_>,
    args: &'a [Expr],
    skip_named: Option<&str>,
) -> Vec<&'a Expr> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        if let Some(want) = skip_named {
            let skip = matches!(arg, Expr::CallExpr(_) | Expr::Ident(_))
                && type_of_expr(pass, arg)
                    .and_then(|t| named_type_name(pass, t))
                    .as_deref()
                    == Some(want);
            if skip {
                continue;
            }
        }
        out.push(arg);
    }
    out
}

fn is_ascii(s: &str) -> bool {
    s.bytes().all(|b| b < 0x80)
}

/// Simplified printf-like scan (upstream has a fuller verb parser).
fn first_printf_like(format: &str) -> Option<&str> {
    let bytes = format.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        if i < bytes.len() && bytes[i] == b'%' {
            i += 1;
            continue;
        }
        while i < bytes.len() && matches!(bytes[i], b'#' | b'0' | b'+' | b'-' | b' ') {
            i += 1;
        }
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
        if i < bytes.len() && bytes[i].is_ascii_alphabetic() {
            return Some(&format[start..=i]);
        }
        return None;
    }
    None
}

fn check_call(
    pass: &Pass<'_>,
    call: &CallExpr,
    rules: &HashMap<String, CheckerKind>,
    opts: &LoggercheckOptions,
    pending: &mut Vec<(u32, String)>,
) {
    if call.ellipsis.is_valid() {
        return;
    }

    let Some(name) = callee_name(pass, call) else {
        return;
    };
    let Some(&kind) = rules.get(&name) else {
        return;
    };

    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let info = match pass.types_info() {
        Some(i) => i,
        None => return,
    };
    let obj_id = match &*call.fun {
        Expr::Ident(id) => info.uses.get(&id.id).copied(),
        Expr::SelectorExpr(sel) => info.uses.get(&sel.sel.id).copied(),
        _ => None,
    };
    let Some(obj_id) = obj_id else {
        return;
    };
    let Some(sig_id) = obj_id.typ(&artifacts.objects) else {
        return;
    };
    if !signature_variadic(&artifacts.types, sig_id) {
        return;
    }
    let Some(params) = signature_params(&artifacts.types, sig_id) else {
        return;
    };
    let nparams = tuple_len(&artifacts.types, Some(params));
    if nparams == 0 {
        return;
    }
    let last = tuple_at(&artifacts.types, params, nparams - 1);
    let Some(last_ty) = last.typ(&artifacts.objects) else {
        return;
    };
    let last_ty = unalias_readonly(&artifacts.types, last_ty);
    let elem = match artifacts.types.get(last_ty) {
        TypeData::Slice(_) => slice_elem(&artifacts.types, last_ty),
        _ => return,
    };
    if !is_empty_interface(pass, elem) {
        return;
    }

    let start_index = nparams - 1;
    if call.args.len() < start_index {
        return;
    }
    let kv_args = &call.args[start_index..];
    let skip = match kind {
        CheckerKind::Zap => Some("Field"),
        CheckerKind::Slog => Some("Attr"),
        CheckerKind::General => None,
    };
    let filtered = filter_key_values(pass, kv_args, skip);

    if filtered.len() % 2 != 0 {
        let first = filtered[0];
        pending.push((
            first.pos().0 as u32,
            "odd number of arguments passed as key-value pairs for logging".into(),
        ));
    }

    if opts.require_string_key {
        for i in (0..filtered.len()).step_by(2) {
            let arg = filtered[i];
            if let Some(value) = code::expr_to_string(pass, arg) {
                if !is_ascii(&value) {
                    pending.push((
                        arg.pos().0 as u32,
                        format!(
                            "logging keys are expected to be alphanumeric strings, please remove any non-latin characters from {value:?}"
                        ),
                    ));
                }
            } else {
                pending.push((
                    arg.pos().0 as u32,
                    format!(
                        "logging keys are expected to be inlined constant strings, please replace \"…\" provided with string"
                    ),
                ));
            }
        }
    }

    if opts.no_printf_like {
        for arg in &call.args {
            if let Some(format) = code::expr_to_string(pass, arg) {
                if let Some(spec) = first_printf_like(&format) {
                    pending.push((
                        arg.pos().0 as u32,
                        format!("logging message should not use format specifier {spec:?}"),
                    ));
                    break;
                }
            }
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "loggercheck requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<LoggercheckOptions>("loggercheck")
        .cloned()
        .unwrap_or_default();
    let rules = build_rule_index(&options);

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::preorder(NodeRef::File(file), |n| {
            if let NodeRef::CallExpr(call) = n {
                check_call(pass, call, &rules, &options, &mut pending);
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
        name: "loggercheck",
        doc: "checks key value pairs for common logger libraries (kitlog,klog,logr,slog,zap)",
        url: "https://github.com/timonwong/loggercheck",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: Vec::new(),
    })
}
