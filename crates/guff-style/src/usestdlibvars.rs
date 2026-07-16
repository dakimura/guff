//! Port of [`github.com/sashamelentyev/usestdlibvars`](https://github.com/sashamelentyev/usestdlibvars).
//!
//! Implements the default-on HTTP checks (`http-method`, `http-status-code`).
//! `linters.settings.usestdlibvars` toggles for those checks are wired.
//!
//! DEFERRED: optional tables/flags (`time-weekday`, `time-month`, `time-layout`,
//! `crypto-hash`, `default-rpc-path`, `sql-isolation-level`, `tls-signature-scheme`,
//! `constant-kind`, `time-date-month`).

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{BasicLit, CallExpr, Expr, Stmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::options::UsestdlibvarsOptions;

fn http_method() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            ("GET", "http.MethodGet"),
            ("HEAD", "http.MethodHead"),
            ("POST", "http.MethodPost"),
            ("PUT", "http.MethodPut"),
            ("PATCH", "http.MethodPatch"),
            ("DELETE", "http.MethodDelete"),
            ("CONNECT", "http.MethodConnect"),
            ("OPTIONS", "http.MethodOptions"),
            ("TRACE", "http.MethodTrace"),
        ])
    })
}

fn http_status_code() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            ("100", "http.StatusContinue"),
            ("101", "http.StatusSwitchingProtocols"),
            ("102", "http.StatusProcessing"),
            ("103", "http.StatusEarlyHints"),
            ("200", "http.StatusOK"),
            ("201", "http.StatusCreated"),
            ("202", "http.StatusAccepted"),
            ("203", "http.StatusNonAuthoritativeInfo"),
            ("204", "http.StatusNoContent"),
            ("205", "http.StatusResetContent"),
            ("206", "http.StatusPartialContent"),
            ("207", "http.StatusMultiStatus"),
            ("208", "http.StatusAlreadyReported"),
            ("226", "http.StatusIMUsed"),
            ("300", "http.StatusMultipleChoices"),
            ("301", "http.StatusMovedPermanently"),
            ("302", "http.StatusFound"),
            ("303", "http.StatusSeeOther"),
            ("304", "http.StatusNotModified"),
            ("305", "http.StatusUseProxy"),
            ("307", "http.StatusTemporaryRedirect"),
            ("308", "http.StatusPermanentRedirect"),
            ("400", "http.StatusBadRequest"),
            ("401", "http.StatusUnauthorized"),
            ("402", "http.StatusPaymentRequired"),
            ("403", "http.StatusForbidden"),
            ("404", "http.StatusNotFound"),
            ("405", "http.StatusMethodNotAllowed"),
            ("406", "http.StatusNotAcceptable"),
            ("407", "http.StatusProxyAuthRequired"),
            ("408", "http.StatusRequestTimeout"),
            ("409", "http.StatusConflict"),
            ("410", "http.StatusGone"),
            ("411", "http.StatusLengthRequired"),
            ("412", "http.StatusPreconditionFailed"),
            ("413", "http.StatusRequestEntityTooLarge"),
            ("414", "http.StatusRequestURITooLong"),
            ("415", "http.StatusUnsupportedMediaType"),
            ("416", "http.StatusRequestedRangeNotSatisfiable"),
            ("417", "http.StatusExpectationFailed"),
            ("418", "http.StatusTeapot"),
            ("421", "http.StatusMisdirectedRequest"),
            ("422", "http.StatusUnprocessableEntity"),
            ("423", "http.StatusLocked"),
            ("424", "http.StatusFailedDependency"),
            ("425", "http.StatusTooEarly"),
            ("426", "http.StatusUpgradeRequired"),
            ("428", "http.StatusPreconditionRequired"),
            ("429", "http.StatusTooManyRequests"),
            ("431", "http.StatusRequestHeaderFieldsTooLarge"),
            ("451", "http.StatusUnavailableForLegalReasons"),
            ("500", "http.StatusInternalServerError"),
            ("501", "http.StatusNotImplemented"),
            ("502", "http.StatusBadGateway"),
            ("503", "http.StatusServiceUnavailable"),
            ("504", "http.StatusGatewayTimeout"),
            ("505", "http.StatusHTTPVersionNotSupported"),
            ("506", "http.StatusVariantAlsoNegotiates"),
            ("507", "http.StatusInsufficientStorage"),
            ("508", "http.StatusLoopDetected"),
            ("510", "http.StatusNotExtended"),
            ("511", "http.StatusNetworkAuthenticationRequired"),
        ])
    })
}

fn lit_value(lit: &BasicLit) -> String {
    lit.value.trim_matches('"').to_string()
}

fn as_basic_lit(expr: &Expr, kind: Token) -> Option<&BasicLit> {
    match expr {
        Expr::BasicLit(lit) if lit.kind == Some(kind) => Some(lit),
        _ => None,
    }
}

fn basic_lit_from_args<'a>(
    args: &'a [Expr],
    count: usize,
    idx: usize,
    kind: Token,
) -> Option<&'a BasicLit> {
    if args.len() != count || idx >= count {
        return None;
    }
    as_basic_lit(&args[idx], kind)
}

fn basic_lit_from_elts<'a>(elts: &'a [Expr], key: &str) -> Option<&'a BasicLit> {
    for e in elts {
        let Expr::KeyValueExpr(kv) = e else {
            continue;
        };
        let Expr::Ident(id) = kv.key.as_ref() else {
            continue;
        };
        if id.name != key {
            continue;
        }
        if let Expr::BasicLit(lit) = kv.value.as_ref() {
            return Some(lit);
        }
    }
    None
}

fn sel_pkg_name(expr: &Expr) -> Option<(&str, &str)> {
    let Expr::SelectorExpr(se) = expr else {
        return None;
    };
    let Expr::Ident(pkg) = se.x.as_ref() else {
        return None;
    };
    Some((pkg.name.as_str(), se.sel.name.as_str()))
}

fn queue_replace(pending: &mut Vec<(u32, u32, String, String)>, lit: &BasicLit, replacement: &str) {
    let current = lit_value(lit);
    pending.push((
        lit.value_pos.0 as u32,
        lit.end().0 as u32,
        format!("\"{current}\" can be replaced by {replacement}"),
        replacement.to_string(),
    ));
}

fn check_http_method(
    options: &UsestdlibvarsOptions,
    pending: &mut Vec<(u32, u32, String, String)>,
    lit: &BasicLit,
) {
    if !options.http_method {
        return;
    }
    let key = lit_value(lit).to_uppercase();
    if let Some(replacement) = http_method().get(key.as_str()) {
        queue_replace(pending, lit, replacement);
    }
}

fn check_http_status(
    options: &UsestdlibvarsOptions,
    pending: &mut Vec<(u32, u32, String, String)>,
    lit: &BasicLit,
) {
    if !options.http_status_code {
        return;
    }
    let key = lit_value(lit);
    if let Some(replacement) = http_status_code().get(key.as_str()) {
        queue_replace(pending, lit, replacement);
    }
}

fn fun_args(
    options: &UsestdlibvarsOptions,
    pending: &mut Vec<(u32, u32, String, String)>,
    call: &CallExpr,
) {
    let Some((pkg, fun)) = sel_pkg_name(&call.fun) else {
        // (*ResponseWriter).WriteHeader(200)
        if let Expr::SelectorExpr(se) = call.fun.as_ref() {
            if se.sel.name == "WriteHeader" {
                if let Some(lit) = basic_lit_from_args(&call.args, 1, 0, Token::INT) {
                    check_http_status(options, pending, lit);
                }
            }
        }
        return;
    };

    match (pkg, fun) {
        ("http", "NewRequest") => {
            if let Some(lit) = basic_lit_from_args(&call.args, 3, 0, Token::STRING) {
                check_http_method(options, pending, lit);
            }
        }
        ("http", "NewRequestWithContext") => {
            if let Some(lit) = basic_lit_from_args(&call.args, 4, 1, Token::STRING) {
                check_http_method(options, pending, lit);
            }
        }
        ("httptest", "NewRequest") => {
            if let Some(lit) = basic_lit_from_args(&call.args, 3, 0, Token::STRING) {
                check_http_method(options, pending, lit);
            }
        }
        ("http", "Error") => {
            if let Some(lit) = basic_lit_from_args(&call.args, 3, 2, Token::INT) {
                check_http_status(options, pending, lit);
            }
        }
        ("http", "StatusText") => {
            if let Some(lit) = basic_lit_from_args(&call.args, 1, 0, Token::INT) {
                check_http_status(options, pending, lit);
            }
        }
        ("http", "Redirect") => {
            if let Some(lit) = basic_lit_from_args(&call.args, 4, 3, Token::INT) {
                check_http_status(options, pending, lit);
            }
        }
        ("http", "RedirectHandler") => {
            if let Some(lit) = basic_lit_from_args(&call.args, 2, 1, Token::INT) {
                check_http_status(options, pending, lit);
            }
        }
        _ => {
            if fun == "WriteHeader" {
                if let Some(lit) = basic_lit_from_args(&call.args, 1, 0, Token::INT) {
                    check_http_status(options, pending, lit);
                }
            }
        }
    }
}

fn type_elts(
    options: &UsestdlibvarsOptions,
    pending: &mut Vec<(u32, u32, String, String)>,
    typ: &Expr,
    elts: &[Expr],
) {
    let Some((pkg, name)) = sel_pkg_name(typ) else {
        return;
    };
    match (pkg, name) {
        ("http", "Request") => {
            if let Some(lit) = basic_lit_from_elts(elts, "Method") {
                check_http_method(options, pending, lit);
            }
        }
        ("http", "Response") => {
            if let Some(lit) = basic_lit_from_elts(elts, "StatusCode") {
                check_http_status(options, pending, lit);
            }
        }
        ("httptest", "ResponseRecorder") => {
            if let Some(lit) = basic_lit_from_elts(elts, "Code") {
                check_http_status(options, pending, lit);
            }
        }
        _ => {}
    }
}

fn binary_expr(
    options: &UsestdlibvarsOptions,
    pending: &mut Vec<(u32, u32, String, String)>,
    x: &Expr,
    y: &Expr,
) {
    let Some((_, sel)) = sel_pkg_name(x) else {
        return;
    };
    match sel {
        "StatusCode" => {
            if let Some(lit) = as_basic_lit(y, Token::INT) {
                check_http_status(options, pending, lit);
            }
        }
        "Method" => {
            if let Some(lit) = as_basic_lit(y, Token::STRING) {
                check_http_method(options, pending, lit);
            }
        }
        _ => {}
    }
}

fn switch_stmt(
    options: &UsestdlibvarsOptions,
    pending: &mut Vec<(u32, u32, String, String)>,
    tag: &Expr,
    body: &[Stmt],
) {
    let Some((_, sel)) = sel_pkg_name(tag) else {
        return;
    };
    let check_method = sel == "Method";
    let check_status = sel == "StatusCode";
    if !check_method && !check_status {
        return;
    }
    for stmt in body {
        let Stmt::CaseClause(cc) = stmt else {
            continue;
        };
        for expr in &cc.list {
            if check_method {
                if let Some(lit) = as_basic_lit(expr, Token::STRING) {
                    check_http_method(options, pending, lit);
                }
            }
            if check_status {
                if let Some(lit) = as_basic_lit(expr, Token::INT) {
                    check_http_status(options, pending, lit);
                }
            }
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "usestdlibvars requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<UsestdlibvarsOptions>("usestdlibvars")
        .copied()
        .unwrap_or_default();

    if !options.http_method && !options.http_status_code {
        // DEFERRED: optional tables would still run here when implemented.
        return Ok(None);
    }

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::CallExpr(call) => {
                    fun_args(&options, &mut pending, call);
                    true
                }
                NodeRef::CompositeLit(cl) => {
                    if let Some(ty) = cl.ty.as_deref() {
                        type_elts(&options, &mut pending, ty, &cl.elts);
                    }
                    true
                }
                NodeRef::BinaryExpr(bin) => {
                    // Skip comparison/arithmetic ops that never carry const replacements.
                    if matches!(
                        bin.op,
                        Token::LSS
                            | Token::GTR
                            | Token::LEQ
                            | Token::GEQ
                            | Token::QUO
                            | Token::ADD
                            | Token::SUB
                            | Token::MUL
                    ) {
                        return true;
                    }
                    binary_expr(&options, &mut pending, &bin.x, &bin.y);
                    true
                }
                NodeRef::SwitchStmt(sw) => {
                    if let Some(tag) = &sw.tag {
                        switch_stmt(&options, &mut pending, tag, &sw.body.list);
                    }
                    true
                }
                _ => true,
            }
        });
    }

    for (pos, end, message, replacement) in pending {
        pass.report(Diagnostic {
            pos,
            end,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: format!("Use {replacement}"),
                text_edits: vec![TextEdit {
                    pos,
                    end,
                    new_text: replacement,
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
        name: "usestdlibvars",
        doc: "Detects the possibility to use variables/constants from the Go standard library.",
        url: "https://github.com/sashamelentyev/usestdlibvars",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
