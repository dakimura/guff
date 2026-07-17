//! Port of [`github.com/yeya24/promlinter`](https://github.com/yeya24/promlinter)
//! (golangci-lint wrapper in `pkg/golinters/promlinter`).
//!
//! Extracts Prometheus metric constructors (`NewCounter` / `NewGauge` / … /
//! `NewCounterFunc` / `NewGaugeFunc`, including `promauto` / method-value
//! forms) and runs prometheus `promlint` naming checks.
//!
//! DEFERRED: `MustNewConstMetric` / channel-send paths, kube-state-metrics
//! `NewFamilyGenerator`, `metrics.NewDesc` (k8s component-base), resolving
//! Opts via `Ident`/`AssignStmt`, `BuildFQName` inside Name values, and
//! `strict` parse-failure diagnostics (strict currently only reserved).

use std::sync::OnceLock;

use guff::ast::{CallExpr, CompositeLit, Expr};
use guff::token::Token;
use guff::walk::{preorder, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use regex::Regex;

use crate::options::PromlinterOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetricType {
    Counter,
    Gauge,
    Histogram,
    Summary,
    Untyped,
}

impl MetricType {
    fn as_promlint_name(self) -> &'static str {
        match self {
            MetricType::Counter => "counter",
            MetricType::Gauge => "gauge",
            MetricType::Histogram => "histogram",
            MetricType::Summary => "summary",
            MetricType::Untyped => "untyped",
        }
    }
}

#[derive(Debug, Clone)]
struct MetricFamily {
    name: String,
    /// `None` → Help field missing (lint "no help text"); `Some("")` is fine.
    help: Option<String>,
    metric_type: MetricType,
    labels: Vec<String>,
    pos: u32,
}

#[derive(Debug, Default)]
struct Opts {
    namespace: String,
    subsystem: String,
    name: String,
    help: Option<String>,
    labels: Vec<String>,
}

fn metrics_ctor_type(name: &str) -> Option<MetricType> {
    match name {
        "Counter" | "NewCounter" | "NewCounterVec" | "NewCounterFunc" => Some(MetricType::Counter),
        "Gauge" | "NewGauge" | "NewGaugeVec" | "NewGaugeFunc" => Some(MetricType::Gauge),
        "NewHistogram" | "NewHistogramVec" => Some(MetricType::Histogram),
        "NewSummary" | "NewSummaryVec" => Some(MetricType::Summary),
        _ => None,
    }
}

fn build_fq_name(namespace: &str, subsystem: &str, name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    match (namespace.is_empty(), subsystem.is_empty()) {
        (false, false) => format!("{namespace}_{subsystem}_{name}"),
        (false, true) => format!("{namespace}_{name}"),
        (true, false) => format!("{subsystem}_{name}"),
        (true, true) => name.to_string(),
    }
}

fn unquote_string_lit(value: &str) -> Option<String> {
    let v = value.trim();
    if (v.starts_with('"') && v.ends_with('"')) || (v.starts_with('`') && v.ends_with('`')) {
        Some(v[1..v.len() - 1].to_string())
    } else {
        None
    }
}

fn parse_string_value(expr: &Expr) -> Option<String> {
    match expr {
        Expr::BasicLit(lit) if lit.kind == Some(Token::STRING) => unquote_string_lit(&lit.value),
        Expr::BinaryExpr(bin) if bin.op == Token::ADD => {
            let x = parse_string_value(bin.x.as_ref())?;
            let y = parse_string_value(bin.y.as_ref())?;
            Some(x + &y)
        }
        Expr::UnaryExpr(u) => parse_string_value(u.x.as_ref()),
        _ => None,
    }
}

fn parse_composite_opts(lit: &CompositeLit) -> Option<Opts> {
    let mut opts = Opts::default();
    for elt in &lit.elts {
        // Label slice elements: `[]string{"foo", "bar"}`
        if let Expr::BasicLit(bl) = elt {
            if bl.kind == Some(Token::STRING) {
                if let Some(s) = unquote_string_lit(&bl.value) {
                    opts.labels.push(s);
                }
            }
            continue;
        }

        let Expr::KeyValueExpr(kv) = elt else {
            continue;
        };
        let Expr::Ident(key) = kv.key.as_ref() else {
            continue;
        };
        match key.name.as_str() {
            "Namespace" => {
                opts.namespace = parse_string_value(kv.value.as_ref())?;
            }
            "Subsystem" => {
                opts.subsystem = parse_string_value(kv.value.as_ref())?;
            }
            "Name" => {
                opts.name = parse_string_value(kv.value.as_ref())?;
            }
            "Help" => {
                opts.help = Some(parse_string_value(kv.value.as_ref())?);
            }
            _ => {}
        }
    }
    Some(opts)
}

fn parse_opts_expr(expr: &Expr) -> Option<Opts> {
    match expr {
        Expr::CompositeLit(lit) => parse_composite_opts(lit),
        Expr::UnaryExpr(u) if u.op == Token::AND => parse_opts_expr(u.x.as_ref()),
        _ => None,
    }
}

fn call_method_name(call: &CallExpr) -> Option<String> {
    match call.fun.as_ref() {
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::SelectorExpr(sel) => Some(sel.sel.name.clone()),
        _ => None,
    }
}

fn camel_case_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[a-z][A-Z]").expect("camelCase"))
}

fn unit_abbreviations() -> &'static [&'static str] {
    &[
        "s", "ms", "us", "ns", "sec", "b", "kb", "mb", "gb", "tb", "pb", "m", "h", "d",
    ]
}

fn units_map() -> &'static [(&'static str, &'static str)] {
    &[
        ("amperes", "amperes"),
        ("bytes", "bytes"),
        ("celsius", "celsius"),
        ("grams", "grams"),
        ("joules", "joules"),
        ("kelvin", "kelvin"),
        ("meters", "meters"),
        ("metres", "metres"),
        ("seconds", "seconds"),
        ("volts", "volts"),
        ("minutes", "seconds"),
        ("hours", "seconds"),
        ("days", "seconds"),
        ("weeks", "seconds"),
        ("kelvins", "kelvin"),
        ("fahrenheit", "celsius"),
        ("rankine", "celsius"),
        ("inches", "meters"),
        ("yards", "meters"),
        ("miles", "meters"),
        ("bits", "bytes"),
        ("calories", "joules"),
        ("pounds", "grams"),
        ("ounces", "grams"),
    ]
}

fn unit_prefixes() -> &'static [&'static str] {
    &[
        "pico", "nano", "micro", "milli", "centi", "deci", "deca", "hecto", "kilo", "kibi",
        "mega", "mibi", "giga", "gibi", "tera", "tebi", "peta", "pebi",
    ]
}

fn metric_units(name: &str) -> Option<(&str, &str)> {
    for part in name.split('_') {
        for &(unit, base) in units_map() {
            if part == unit {
                return Some((unit, base));
            }
        }
        for prefix in unit_prefixes() {
            if let Some(rest) = part.strip_prefix(prefix) {
                for &(unit, base) in units_map() {
                    if rest == unit {
                        return Some((part, base));
                    }
                }
            }
        }
    }
    None
}

/// Patterns used by upstream `DisabledLintFuncs` matching (`lintFuncText`).
fn lint_func_patterns(name: &str) -> &'static [&'static str] {
    match name {
        "Help" => &["no help text"],
        "MetricUnits" => &["use base unit"],
        "Counter" => &["counter metrics should"],
        "HistogramSummaryReserved" => &["non-histogram", "non-summary"],
        "MetricTypeInName" => &["metric name should not include type"],
        "ReservedChars" => &["metric names should not contain ':'"],
        "CamelCase" => &["'snake_case' not 'camelCase'"],
        // Upstream map key is `lintUnitAbbreviations`; golangci docs use
        // `UnitAbbreviations`. Accept both disable names.
        "UnitAbbreviations" | "lintUnitAbbreviations" => {
            &["metric names should not contain abbreviated units"]
        }
        _ => &[],
    }
}

fn is_disabled(text: &str, disabled: &[String]) -> bool {
    for name in disabled {
        for pattern in lint_func_patterns(name) {
            if text.contains(pattern) {
                return true;
            }
        }
    }
    false
}

fn lint_metric(mf: &MetricFamily) -> Vec<String> {
    let mut problems = Vec::new();
    let name = &mf.name;

    // Help
    if mf.help.is_none() {
        problems.push("no help text".to_string());
    }

    // MetricUnits
    if let Some((unit, base)) = metric_units(name) {
        if unit != base {
            problems.push(format!("use base unit {base:?} instead of {unit:?}"));
        }
    }

    // Counter
    let is_counter = mf.metric_type == MetricType::Counter;
    let is_untyped = mf.metric_type == MetricType::Untyped;
    let has_total = name.ends_with("_total");
    if is_counter && !has_total {
        problems.push("counter metrics should have \"_total\" suffix".to_string());
    } else if !is_untyped && !is_counter && has_total {
        problems.push("non-counter metrics should not have \"_total\" suffix".to_string());
    }

    // HistogramSummaryReserved
    if mf.metric_type != MetricType::Untyped {
        let is_histogram = mf.metric_type == MetricType::Histogram;
        let is_summary = mf.metric_type == MetricType::Summary;
        if !is_histogram && name.ends_with("_bucket") {
            problems.push("non-histogram metrics should not have \"_bucket\" suffix".to_string());
        }
        if !is_histogram && !is_summary && name.ends_with("_count") {
            problems.push(
                "non-histogram and non-summary metrics should not have \"_count\" suffix"
                    .to_string(),
            );
        }
        if !is_histogram && !is_summary && name.ends_with("_sum") {
            problems.push(
                "non-histogram and non-summary metrics should not have \"_sum\" suffix".to_string(),
            );
        }
        for label in &mf.labels {
            if !is_histogram && label == "le" {
                problems.push("non-histogram metrics should not have \"le\" label".to_string());
            }
            if !is_summary && label == "quantile" {
                problems.push("non-summary metrics should not have \"quantile\" label".to_string());
            }
        }
    }

    // MetricTypeInName
    if mf.metric_type != MetricType::Untyped {
        let n = name.to_lowercase();
        let typename = mf.metric_type.as_promlint_name();
        if n.contains(&format!("_{typename}_")) || n.ends_with(&format!("_{typename}")) {
            problems.push(format!("metric name should not include type '{typename}'"));
        }
    }

    // ReservedChars
    if name.contains(':') {
        problems.push("metric names should not contain ':'".to_string());
    }

    // CamelCase
    if camel_case_re().is_match(name) {
        problems.push("metric names should be written in 'snake_case' not 'camelCase'".to_string());
    }
    for label in &mf.labels {
        if camel_case_re().is_match(label) {
            problems
                .push("label names should be written in 'snake_case' not 'camelCase'".to_string());
        }
    }

    // UnitAbbreviations
    let n = name.to_lowercase();
    for abbr in unit_abbreviations() {
        if n.contains(&format!("_{abbr}_")) || n.ends_with(&format!("_{abbr}")) {
            problems.push("metric names should not contain abbreviated units".to_string());
            break;
        }
    }

    problems
}

fn collect_from_call(call: &CallExpr, out: &mut Vec<MetricFamily>) {
    let Some(method) = call_method_name(call) else {
        return;
    };
    let Some(metric_type) = metrics_ctor_type(&method) else {
        return;
    };
    if call.args.is_empty() {
        return;
    }

    let Some(mut opts) = parse_opts_expr(&call.args[0]) else {
        return;
    };

    // Vec forms take label names as the second argument.
    if method.ends_with("Vec") && call.args.len() > 1 {
        if let Expr::CompositeLit(labels) = &call.args[1] {
            if let Some(label_opts) = parse_composite_opts(labels) {
                if !label_opts.labels.is_empty() {
                    opts.labels = label_opts.labels;
                }
            }
        }
    }

    let fq = build_fq_name(&opts.namespace, &opts.subsystem, &opts.name);
    if fq.is_empty() {
        return;
    }

    out.push(MetricFamily {
        name: fq,
        help: opts.help,
        metric_type,
        labels: opts.labels,
        pos: call.args[0].pos().0 as u32,
    });
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "promlinter requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<PromlinterOptions>("promlinter")
        .cloned()
        .unwrap_or_default();

    let mut metrics: Vec<MetricFamily> = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::CallExpr(call) = n {
                collect_from_call(call, &mut metrics);
            }
            true
        });
    }

    // Dedup by (name, type, help, labels) like upstream MetricFamily.String().
    let mut seen = std::collections::HashSet::new();
    let mut pending: Vec<(u32, String)> = Vec::new();
    for mf in &metrics {
        let key = format!(
            "{}|{:?}|{:?}|{}",
            mf.name,
            mf.metric_type,
            mf.help,
            mf.labels.join(",")
        );
        if !seen.insert(key) {
            continue;
        }
        for text in lint_metric(mf) {
            if is_disabled(&text, &opts.disabled_linters) {
                continue;
            }
            pending.push((
                mf.pos,
                format!("Metric: {} Error: {}", mf.name, text),
            ));
        }
    }

    for (pos, msg) in pending {
        pass.reportf(pos, &msg);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "promlinter",
        doc: "Check Prometheus metrics naming via promlint",
        url: "https://github.com/yeya24/promlinter",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mf(name: &str, ty: MetricType, help: Option<&str>, labels: &[&str]) -> MetricFamily {
        MetricFamily {
            name: name.to_string(),
            help: help.map(str::to_string),
            metric_type: ty,
            labels: labels.iter().map(|s| (*s).to_string()).collect(),
            pos: 0,
        }
    }

    #[test]
    fn counter_needs_total_suffix() {
        let problems = lint_metric(&mf("http_requests", MetricType::Counter, Some("h"), &[]));
        assert!(
            problems.iter().any(|p| p.contains("_total")),
            "{problems:?}"
        );
    }

    #[test]
    fn missing_help_is_flagged() {
        let problems = lint_metric(&mf("http_requests_total", MetricType::Counter, None, &[]));
        assert!(problems.iter().any(|p| p == "no help text"), "{problems:?}");
    }

    #[test]
    fn empty_help_is_ok() {
        let problems = lint_metric(&mf(
            "http_requests_total",
            MetricType::Counter,
            Some(""),
            &[],
        ));
        assert!(
            !problems.iter().any(|p| p == "no help text"),
            "{problems:?}"
        );
    }

    #[test]
    fn camel_case_is_flagged() {
        let problems = lint_metric(&mf("httpRequests_total", MetricType::Counter, Some("h"), &[]));
        assert!(
            problems.iter().any(|p| p.contains("snake_case")),
            "{problems:?}"
        );
    }

    #[test]
    fn non_base_unit_is_flagged() {
        let problems = lint_metric(&mf(
            "job_duration_minutes",
            MetricType::Gauge,
            Some("h"),
            &[],
        ));
        assert!(
            problems.iter().any(|p| p.contains("use base unit")),
            "{problems:?}"
        );
    }

    #[test]
    fn disable_counter_suppresses() {
        let opts = PromlinterOptions {
            strict: false,
            disabled_linters: vec!["Counter".to_string()],
        };
        let problems = lint_metric(&mf("http_requests", MetricType::Counter, Some("h"), &[]));
        let remaining: Vec<_> = problems
            .into_iter()
            .filter(|t| !is_disabled(t, &opts.disabled_linters))
            .collect();
        assert!(
            !remaining.iter().any(|p| p.contains("_total")),
            "{remaining:?}"
        );
    }

    #[test]
    fn build_fq_name_joins_parts() {
        assert_eq!(build_fq_name("ns", "sub", "name"), "ns_sub_name");
        assert_eq!(build_fq_name("ns", "", "name"), "ns_name");
        assert_eq!(build_fq_name("", "", "name"), "name");
        assert_eq!(build_fq_name("ns", "sub", ""), "");
    }
}
