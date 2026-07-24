//! Port of [`github.com/jjti/spancheck`](https://github.com/jjti/spancheck)
//! (golangci-lint wrapper in `pkg/golinters/spancheck`).
//!
//! Checks OpenTelemetry / OpenCensus span `Start` / `End` pairing and (when
//! enabled) error-path `SetStatus` / `RecordError` coverage.
//!
//! Path analysis is an **AST approximation** (no x/tools ctrlflow):
//! - `defer span.End()` after assignment ⇒ OK
//! - otherwise any `span.End()` in the function after assignment ⇒ OK (may miss
//!   branch gaps — full CFG is DEFERRED)
//!
//! DEFERRED: full ctrlflow CFG parity; nested `FuncLit` / closure defer bodies;
//! OpenCensus-only `RecordError` gating nuance on mixed span types.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{AssignStmt, BlockStmt, CallExpr, Expr, FuncDecl, FuncType, SelectorExpr};
use guff::walk::{inspect, preorder, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect as inspect_pass;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::api_predicates::api_implements;
use guff_types::arena::ObjectData;
use guff_types::TypeId;
use regex::Regex;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpancheckOptions {
    /// Enabled checks: `"end"`, `"set-status"`, `"record-error"`.
    pub checks: Vec<String>,
    /// Function signatures that satisfy SetStatus / RecordError checks on error returns.
    pub ignore_check_signatures: Vec<String>,
    /// Extra `regex:telemetry-type` start-span signatures (see upstream config).
    pub extra_start_span_signatures: Vec<String>,
}

impl SpancheckOptions {
    fn enabled_checks(&self) -> HashSet<&str> {
        let checks = if self.checks.is_empty() {
            ["end"].into_iter().collect()
        } else {
            self.checks.iter().map(String::as_str).collect()
        };
        checks
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpanType {
    OpenTelemetry,
    OpenCensus,
}

struct SpanStartMatcher {
    signature: Regex,
    span_type: SpanType,
}

struct Config {
    end_check: bool,
    set_status_check: bool,
    record_error_check: bool,
    ignore_check_sigs: Option<Regex>,
    start_matchers: Vec<SpanStartMatcher>,
    custom_starter_sigs: Option<Regex>,
}

#[derive(Clone)]
struct SpanVar {
    name: String,
    pos: u32,
    span_type: SpanType,
    assign_pos: u32,
}

fn default_start_span_signatures() -> Vec<(&'static str, SpanType)> {
    vec![
        (
            r"\(go\.opentelemetry\.io/otel/trace\.Tracer\)\.Start",
            SpanType::OpenTelemetry,
        ),
        (
            r"go\.opencensus\.io/trace\.StartSpan",
            SpanType::OpenCensus,
        ),
        (
            r"go\.opencensus\.io/trace\.StartSpanWithRemoteParent",
            SpanType::OpenCensus,
        ),
    ]
}

fn parse_start_matchers(options: &SpancheckOptions) -> Vec<SpanStartMatcher> {
    let mut matchers = Vec::new();
    let mut custom = Vec::new();

    let mut sigs: Vec<(String, SpanType)> = default_start_span_signatures()
        .into_iter()
        .map(|(s, t)| (s.to_string(), t))
        .collect();

    for raw in &options.extra_start_span_signatures {
        let parts: Vec<&str> = raw.splitn(2, ':').collect();
        if parts.len() != 2 {
            continue;
        }
        let span_type = match parts[1] {
            "opentelemetry" => SpanType::OpenTelemetry,
            "opencensus" => SpanType::OpenCensus,
            _ => continue,
        };
        custom.push(parts[0].to_string());
        sigs.push((parts[0].to_string(), span_type));
    }

    for (pat, span_type) in sigs {
        if let Ok(re) = Regex::new(&pat) {
            matchers.push(SpanStartMatcher {
                signature: re,
                span_type,
            });
        }
    }

    matchers
}

fn build_config(options: &SpancheckOptions) -> Config {
    let enabled = options.enabled_checks();
    let start_matchers = parse_start_matchers(options);
    let custom_starter_sigs = if options.extra_start_span_signatures.is_empty() {
        None
    } else {
        options
            .extra_start_span_signatures
            .iter()
            .filter_map(|s| s.split(':').next())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .first()
            .and_then(|_| {
                let pats: Vec<String> = options
                    .extra_start_span_signatures
                    .iter()
                    .filter_map(|s| s.split(':').next().map(str::to_string))
                    .collect();
                if pats.is_empty() {
                    None
                } else {
                    Regex::new(&format!("({})", pats.join("|"))).ok()
                }
            })
    };

    Config {
        end_check: enabled.contains("end"),
        set_status_check: enabled.contains("set-status"),
        record_error_check: enabled.contains("record-error"),
        ignore_check_sigs: options.ignore_check_signatures.first().map(|_| {
            let joined = options.ignore_check_signatures.join("|");
            Regex::new(&format!("({joined})")).unwrap_or_else(|_| Regex::new("$^").unwrap())
        }),
        start_matchers,
        custom_starter_sigs,
    }
}

fn object_sig(pass: &Pass<'_>, expr: &Expr) -> Option<String> {
    let info = pass.types_info()?;
    let obj = match expr {
        Expr::Ident(id) => info.uses.get(&id.id).copied(),
        Expr::SelectorExpr(sel) => info.uses.get(&sel.sel.id).copied(),
        _ => None,
    }?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    Some(code::type_func_name(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        obj,
    ))
}

fn is_span_start_on_call(pass: &Pass<'_>, call: &CallExpr, cfg: &Config) -> Option<SpanType> {
    match call.fun.as_ref() {
        Expr::SelectorExpr(sel) if sel.sel.name == "Start" => {
            let sig = object_sig(pass, call.fun.as_ref())?;
            for m in &cfg.start_matchers {
                if m.signature.is_match(&sig) {
                    return Some(m.span_type);
                }
            }
            if sig.contains("Tracer).Start") {
                return Some(SpanType::OpenTelemetry);
            }
            if let Some(recv) = selector_recv_type_string(pass, &sel.x) {
                if recv.ends_with("trace.Tracer") {
                    return Some(SpanType::OpenTelemetry);
                }
            }
            None
        }
        _ => {
            if let Some(name) = code::call_name(pass, &call.fun) {
                if name.contains("StartSpan") {
                    return Some(SpanType::OpenCensus);
                }
            }
            object_sig(pass, &call.fun).and_then(|sig| {
                if sig.contains("StartSpan") {
                    Some(SpanType::OpenCensus)
                } else {
                    None
                }
            })
        }
    }
}

fn selector_recv_type_string(pass: &Pass<'_>, expr: &Expr) -> Option<String> {
    let typ = type_of(pass, expr)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    Some(guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    ))
}

fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn get_span_ident(node: NodeRef<'_>) -> Option<&str> {
    match node {
        NodeRef::AssignStmt(assign) => {
            if assign.lhs.len() > 1 {
                ident_name(&assign.lhs[1])
            } else if assign.lhs.len() == 1 {
                ident_name(&assign.lhs[0])
            } else {
                None
            }
        }
        NodeRef::ValueSpec(spec) => {
            if spec.names.len() > 1 {
                Some(spec.names[1].name.as_str())
            } else if spec.names.len() == 1 {
                Some(spec.names[0].name.as_str())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn ident_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn assign_pos(assign: &AssignStmt, lhs_index: usize) -> u32 {
    if assign.rhs.len() == 1 {
        if let Expr::CallExpr(call) = &assign.rhs[0] {
            return call.pos().0 as u32;
        }
        return assign.rhs[0].pos().0 as u32;
    }
    assign
        .rhs
        .get(lhs_index)
        .map(|e| e.pos().0 as u32)
        .unwrap_or(assign.tok_pos.0 as u32)
}

fn func_returns_error(pass: &Pass<'_>, ty: &FuncType) -> bool {
    let Some(results) = ty.results.as_ref() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let error_typ = universe_error(pass);
    let Some(error_typ) = error_typ else {
        return false;
    };
    for field in &results.list {
        let Some(ty) = &field.ty else {
            continue;
        };
        let Some(result_typ) = type_of(pass, ty) else {
            continue;
        };
        if api_implements(
            &mut artifacts.types.clone(),
            &artifacts.objects,
            &artifacts.packages,
            result_typ,
            error_typ,
        ) {
            return true;
        }
    }
    false
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

fn span_end_var(call: &CallExpr) -> Option<&str> {
    let Expr::SelectorExpr(close_sel) = call.fun.as_ref() else {
        return None;
    };
    if close_sel.sel.name != "End" {
        return None;
    }
    ident_name(&close_sel.x)
}

fn span_method_var<'a>(sel: &'a SelectorExpr, method: &str) -> Option<&'a str> {
    if sel.sel.name != method {
        return None;
    }
    ident_name(&sel.x)
}

fn ignore_sig_matches(pass: &Pass<'_>, expr: &Expr, re: &Regex) -> bool {
    object_sig(pass, expr).is_some_and(|s| re.is_match(&s))
}

struct SpanUsage {
    ended: bool,
    set_status: bool,
    record_error: bool,
}

fn check_body(
    pass: &Pass<'_>,
    body: &BlockStmt,
    cfg: &Config,
    has_error_ret: bool,
    pending: &mut Vec<(u32, String)>,
) {
    let mut spans: HashMap<String, SpanVar> = HashMap::new();
    let mut usage: HashMap<String, SpanUsage> = HashMap::new();

    // Pass 1: discover span assignments.
    inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        if matches!(n, NodeRef::FuncLit(_)) {
            return false;
        }
        match n {
            NodeRef::AssignStmt(assign) => {
                for (i, lhs) in assign.lhs.iter().enumerate() {
                    if assign.rhs.len() == 1 && assign.lhs.len() > 1 && i != 1 {
                        continue;
                    }
                    let Some(rhs) = assign.rhs.get(if assign.rhs.len() == 1 { 0 } else { i }) else {
                        continue;
                    };
                    let Expr::CallExpr(call) = rhs else {
                        continue;
                    };
                    let Some(span_type) = is_span_start_on_call(pass, call, cfg) else {
                        continue;
                    };
                    let Some(name) = ident_name(lhs) else {
                        pending.push((
                            rhs.pos().0 as u32,
                            "span is unassigned, probable memory leak".into(),
                        ));
                        continue;
                    };
                    if name == "_" {
                        pending.push((
                            lhs.pos().0 as u32,
                            "span is unassigned, probable memory leak".into(),
                        ));
                        continue;
                    }
                    let pos = assign_pos(assign, i);
                    spans.insert(
                        name.to_string(),
                        SpanVar {
                            name: name.to_string(),
                            pos,
                            span_type,
                            assign_pos: pos,
                        },
                    );
                    usage.insert(
                        name.to_string(),
                        SpanUsage {
                            ended: false,
                            set_status: false,
                            record_error: false,
                        },
                    );
                }
            }
            NodeRef::ValueSpec(spec) if !spec.values.is_empty() => {
                for (i, name_id) in spec.names.iter().enumerate() {
                    let rhs_idx = if spec.values.len() == 1 { 0 } else { i };
                    let Some(rhs) = spec.values.get(rhs_idx) else {
                        continue;
                    };
                    let Expr::CallExpr(call) = rhs else {
                        continue;
                    };
                    let Some(span_type) = is_span_start_on_call(pass, call, cfg) else {
                        continue;
                    };
                    let name = name_id.name.as_str();
                    if name == "_" {
                        pending.push((
                            name_id.pos().0 as u32,
                            "span is unassigned, probable memory leak".into(),
                        ));
                        continue;
                    }
                    let pos = rhs.pos().0 as u32;
                    spans.insert(
                        name.to_string(),
                        SpanVar {
                            name: name.to_string(),
                            pos,
                            span_type,
                            assign_pos: pos,
                        },
                    );
                    usage.insert(
                        name.to_string(),
                        SpanUsage {
                            ended: false,
                            set_status: false,
                            record_error: false,
                        },
                    );
                }
            }
            _ => {}
        }
        true
    });

    if spans.is_empty() {
        return;
    }

    // Pass 2: track End / SetStatus / RecordError / defer.
    inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        if matches!(n, NodeRef::FuncLit(_)) {
            return false;
        }
        match n {
            NodeRef::CallExpr(call) => {
                if let Some(var) = span_end_var(call) {
                    if let Some(u) = usage.get_mut(var) {
                        u.ended = true;
                    }
                }
                if let Expr::SelectorExpr(sel) = call.fun.as_ref() {
                    if let Some(var) = span_method_var(sel, "SetStatus") {
                        if let Some(u) = usage.get_mut(var) {
                            u.set_status = true;
                        }
                    }
                    if let Some(var) = span_method_var(sel, "RecordError") {
                        if let Some(u) = usage.get_mut(var) {
                            u.record_error = true;
                        }
                    }
                    if let Some(re) = &cfg.ignore_check_sigs {
                        if ignore_sig_matches(pass, &Expr::SelectorExpr(sel.clone()), re) {
                            for u in usage.values_mut() {
                                u.set_status = true;
                                u.record_error = true;
                            }
                        }
                    }
                }
            }
            NodeRef::DeferStmt(d) => {
                if let Some(var) = span_end_var(&d.call) {
                    if let Some(u) = usage.get_mut(var) {
                        u.ended = true;
                    }
                }
                if let Expr::FuncLit(fun) = d.call.fun.as_ref() {
                    if !fun.ty.params.as_ref().is_some_and(|p| !p.list.is_empty()) {
                        inspect(NodeRef::BlockStmt(&fun.body), |inner| {
                            let Some(inner) = inner else {
                                return true;
                            };
                            if let NodeRef::CallExpr(c) = inner {
                                if let Some(var) = span_end_var(c) {
                                    if let Some(u) = usage.get_mut(var) {
                                        u.ended = true;
                                    }
                                }
                            }
                            true
                        });
                    }
                }
            }
            _ => {}
        }
        true
    });

    for (name, sv) in &spans {
        let Some(u) = usage.get(name) else {
            continue;
        };
        if cfg.end_check && !u.ended {
            pending.push((
                sv.pos,
                format!(
                    "{}.End is not called on all paths, possible memory leak",
                    sv.name
                ),
            ));
        }
        if has_error_ret && cfg.set_status_check && !u.set_status {
            pending.push((
                sv.pos,
                format!("{}.SetStatus is not called on all paths", sv.name),
            ));
        }
        if has_error_ret
            && cfg.record_error_check
            && sv.span_type == SpanType::OpenTelemetry
            && !u.record_error
        {
            pending.push((
                sv.pos,
                format!("{}.RecordError is not called on all paths", sv.name),
            ));
        }
    }
}

fn skip_custom_starter(pass: &Pass<'_>, fd: &FuncDecl, cfg: &Config) -> bool {
    let Some(re) = &cfg.custom_starter_sigs else {
        return false;
    };
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(obj) = info.defs.get(&fd.name.id).and_then(|o| *o) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let sig = code::type_func_name(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        obj,
    );
    re.is_match(&sig)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect_pass::InspectResult>(inspect_pass::analyzer())
        .ok_or_else(|| "spancheck requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<SpancheckOptions>("spancheck")
        .cloned()
        .unwrap_or_default();
    let cfg = build_config(&options);
    let mut pending = Vec::new();

    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::FuncDecl(fd) => {
                    if skip_custom_starter(pass, fd, &cfg) {
                        return true;
                    }
                    let has_error_ret = func_returns_error(pass, &fd.ty);
                    if let Some(body) = &fd.body {
                        check_body(pass, body, &cfg, has_error_ret, &mut pending);
                    }
                }
                NodeRef::FuncLit(fl) => {
                    let has_error_ret = func_returns_error(pass, &fl.ty);
                    check_body(pass, &fl.body, &cfg, has_error_ret, &mut pending);
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
        name: "spancheck",
        doc: "Checks for mistakes with OpenTelemetry/Census spans.",
        url: "https://github.com/jjti/spancheck",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect_pass::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_enable_end_only() {
        let opts = SpancheckOptions::default();
        assert!(opts.enabled_checks().contains("end"));
        assert!(!opts.enabled_checks().contains("set-status"));
    }
}
