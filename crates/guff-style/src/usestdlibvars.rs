//! Port of [`github.com/sashamelentyev/usestdlibvars`](https://github.com/sashamelentyev/usestdlibvars).
//!
//! Default-on: `http-method`, `http-status-code`.
//! Optional (default off): `time-weekday`, `time-month`, `time-layout`, `crypto-hash`,
//! `default-rpc-path`, `sql-isolation-level`, `tls-signature-scheme`, `constant-kind`,
//! `time-date-month`.
//!
//! Deprecated / noop upstream flags (`os-dev-null`, `syslog-priority`) are not exposed.

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

fn time_weekday() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            ("Sunday", "time.Sunday.String()"),
            ("Monday", "time.Monday.String()"),
            ("Tuesday", "time.Tuesday.String()"),
            ("Wednesday", "time.Wednesday.String()"),
            ("Thursday", "time.Thursday.String()"),
            ("Friday", "time.Friday.String()"),
            ("Saturday", "time.Saturday.String()"),
        ])
    })
}

fn time_month() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            ("January", "time.January.String()"),
            ("February", "time.February.String()"),
            ("March", "time.March.String()"),
            ("April", "time.April.String()"),
            ("May", "time.May.String()"),
            ("June", "time.June.String()"),
            ("July", "time.July.String()"),
            ("August", "time.August.String()"),
            ("September", "time.September.String()"),
            ("October", "time.October.String()"),
            ("November", "time.November.String()"),
            ("December", "time.December.String()"),
        ])
    })
}

fn time_layout() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            ("01/02 03:04:05PM '06 -0700", "time.Layout"),
            ("Mon Jan _2 15:04:05 2006", "time.ANSIC"),
            ("Mon Jan _2 15:04:05 MST 2006", "time.UnixDate"),
            ("Mon Jan 02 15:04:05 -0700 2006", "time.RubyDate"),
            ("02 Jan 06 15:04 MST", "time.RFC822"),
            ("02 Jan 06 15:04 -0700", "time.RFC822Z"),
            ("Monday, 02-Jan-06 15:04:05 MST", "time.RFC850"),
            ("Mon, 02 Jan 2006 15:04:05 MST", "time.RFC1123"),
            ("Mon, 02 Jan 2006 15:04:05 -0700", "time.RFC1123Z"),
            ("2006-01-02T15:04:05Z07:00", "time.RFC3339"),
            ("2006-01-02T15:04:05.999999999Z07:00", "time.RFC3339Nano"),
            ("3:04PM", "time.Kitchen"),
            ("Jan _2 15:04:05", "time.Stamp"),
            ("Jan _2 15:04:05.000", "time.StampMilli"),
            ("Jan _2 15:04:05.000000", "time.StampMicro"),
            ("Jan _2 15:04:05.000000000", "time.StampNano"),
            ("2006-01-02 15:04:05", "time.DateTime"),
            ("2006-01-02", "time.DateOnly"),
            ("15:04:05", "time.TimeOnly"),
        ])
    })
}

fn crypto_hash() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            ("MD4", "crypto.MD4.String()"),
            ("MD5", "crypto.MD5.String()"),
            ("SHA-1", "crypto.SHA1.String()"),
            ("SHA-224", "crypto.SHA224.String()"),
            ("SHA-256", "crypto.SHA256.String()"),
            ("SHA-384", "crypto.SHA384.String()"),
            ("SHA-512", "crypto.SHA512.String()"),
            ("MD5+SHA1", "crypto.MD5SHA1.String()"),
            ("RIPEMD-160", "crypto.RIPEMD160.String()"),
            ("SHA3-224", "crypto.SHA3_224.String()"),
            ("SHA3-256", "crypto.SHA3_256.String()"),
            ("SHA3-384", "crypto.SHA3_384.String()"),
            ("SHA3-512", "crypto.SHA3_512.String()"),
            ("SHA-512/224", "crypto.SHA512_224.String()"),
            ("SHA-512/256", "crypto.SHA512_256.String()"),
            ("BLAKE2s-256", "crypto.BLAKE2s_256.String()"),
            ("BLAKE2b-256", "crypto.BLAKE2b_256.String()"),
            ("BLAKE2b-384", "crypto.BLAKE2b_384.String()"),
            ("BLAKE2b-512", "crypto.BLAKE2b_512.String()"),
        ])
    })
}

fn rpc_default_path() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            ("/_goRPC_", "rpc.DefaultRPCPath"),
            ("/debug/rpc", "rpc.DefaultDebugPath"),
        ])
    })
}

fn sql_isolation_level() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            ("Read Uncommitted", "sql.LevelReadUncommitted.String()"),
            ("Read Committed", "sql.LevelReadCommitted.String()"),
            ("Write Committed", "sql.LevelWriteCommitted.String()"),
            ("Repeatable Read", "sql.LevelRepeatableRead.String()"),
        ])
    })
}

fn tls_signature_scheme() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            ("PSSWithSHA256", "tls.PSSWithSHA256.String()"),
            ("ECDSAWithP256AndSHA256", "tls.ECDSAWithP256AndSHA256.String()"),
            ("Ed25519", "tls.Ed25519.String()"),
            ("PSSWithSHA384", "tls.PSSWithSHA384.String()"),
            ("PSSWithSHA512", "tls.PSSWithSHA512.String()"),
            ("PKCS1WithSHA256", "tls.PKCS1WithSHA256.String()"),
            ("PKCS1WithSHA384", "tls.PKCS1WithSHA384.String()"),
            ("PKCS1WithSHA512", "tls.PKCS1WithSHA512.String()"),
            ("ECDSAWithP384AndSHA384", "tls.ECDSAWithP384AndSHA384.String()"),
            ("ECDSAWithP521AndSHA512", "tls.ECDSAWithP521AndSHA512.String()"),
            ("PKCS1WithSHA1", "tls.PKCS1WithSHA1.String()"),
            ("ECDSAWithSHA1", "tls.ECDSAWithSHA1.String()"),
        ])
    })
}

fn constant_kind() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            ("Bool", "constant.Bool.String()"),
            ("String", "constant.String.String()"),
            ("Int", "constant.Int.String()"),
            ("Float", "constant.Float.String()"),
            ("Complex", "constant.Complex.String()"),
        ])
    })
}

fn time_date_month() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            ("1", "time.January"),
            ("2", "time.February"),
            ("3", "time.March"),
            ("4", "time.April"),
            ("5", "time.May"),
            ("6", "time.June"),
            ("7", "time.July"),
            ("8", "time.August"),
            ("9", "time.September"),
            ("10", "time.October"),
            ("11", "time.November"),
            ("12", "time.December"),
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

fn check_map(
    pending: &mut Vec<(u32, u32, String, String)>,
    lit: &BasicLit,
    map: &HashMap<&'static str, &'static str>,
) {
    let key = lit_value(lit);
    if let Some(replacement) = map.get(key.as_str()) {
        queue_replace(pending, lit, replacement);
    }
}

fn check_optional_literal(
    options: &UsestdlibvarsOptions,
    pending: &mut Vec<(u32, u32, String, String)>,
    lit: &BasicLit,
) {
    if lit.kind != Some(Token::STRING) {
        return;
    }
    if options.time_weekday {
        check_map(pending, lit, time_weekday());
    }
    if options.time_month {
        check_map(pending, lit, time_month());
    }
    if options.time_layout {
        check_map(pending, lit, time_layout());
    }
    if options.crypto_hash {
        check_map(pending, lit, crypto_hash());
    }
    if options.default_rpc_path {
        check_map(pending, lit, rpc_default_path());
    }
    if options.sql_isolation_level {
        check_map(pending, lit, sql_isolation_level());
    }
    if options.tls_signature_scheme {
        check_map(pending, lit, tls_signature_scheme());
    }
    if options.constant_kind {
        check_map(pending, lit, constant_kind());
    }
}

fn check_time_date_month(
    options: &UsestdlibvarsOptions,
    pending: &mut Vec<(u32, u32, String, String)>,
    lit: &BasicLit,
) {
    if !options.time_date_month {
        return;
    }
    check_map(pending, lit, time_date_month());
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
        ("time", "Date") => {
            if let Some(lit) = basic_lit_from_args(&call.args, 8, 1, Token::INT) {
                check_time_date_month(options, pending, lit);
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

    if !options.any_enabled() {
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
                NodeRef::BasicLit(lit) => {
                    if options.any_literal_table() {
                        check_optional_literal(&options, &mut pending, lit);
                    }
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
