//! ST1013 — should use constants for HTTP error codes, not magic numbers.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1013`.
//! Default whitelist matches staticcheck: 200, 400, 404, 500.
//! `http_status_code_whitelist` settings wiring is DEFERRED (R16).

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::Expr;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::expr_to_int;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

fn http_status_codes() -> &'static HashMap<i64, &'static str> {
    static M: OnceLock<HashMap<i64, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            (100, "StatusContinue"),
            (101, "StatusSwitchingProtocols"),
            (102, "StatusProcessing"),
            (200, "StatusOK"),
            (201, "StatusCreated"),
            (202, "StatusAccepted"),
            (203, "StatusNonAuthoritativeInfo"),
            (204, "StatusNoContent"),
            (205, "StatusResetContent"),
            (206, "StatusPartialContent"),
            (207, "StatusMultiStatus"),
            (208, "StatusAlreadyReported"),
            (226, "StatusIMUsed"),
            (300, "StatusMultipleChoices"),
            (301, "StatusMovedPermanently"),
            (302, "StatusFound"),
            (303, "StatusSeeOther"),
            (304, "StatusNotModified"),
            (305, "StatusUseProxy"),
            (307, "StatusTemporaryRedirect"),
            (308, "StatusPermanentRedirect"),
            (400, "StatusBadRequest"),
            (401, "StatusUnauthorized"),
            (402, "StatusPaymentRequired"),
            (403, "StatusForbidden"),
            (404, "StatusNotFound"),
            (405, "StatusMethodNotAllowed"),
            (406, "StatusNotAcceptable"),
            (407, "StatusProxyAuthRequired"),
            (408, "StatusRequestTimeout"),
            (409, "StatusConflict"),
            (410, "StatusGone"),
            (411, "StatusLengthRequired"),
            (412, "StatusPreconditionFailed"),
            (413, "StatusRequestEntityTooLarge"),
            (414, "StatusRequestURITooLong"),
            (415, "StatusUnsupportedMediaType"),
            (416, "StatusRequestedRangeNotSatisfiable"),
            (417, "StatusExpectationFailed"),
            (418, "StatusTeapot"),
            (422, "StatusUnprocessableEntity"),
            (423, "StatusLocked"),
            (424, "StatusFailedDependency"),
            (426, "StatusUpgradeRequired"),
            (428, "StatusPreconditionRequired"),
            (429, "StatusTooManyRequests"),
            (431, "StatusRequestHeaderFieldsTooLarge"),
            (451, "StatusUnavailableForLegalReasons"),
            (500, "StatusInternalServerError"),
            (501, "StatusNotImplemented"),
            (502, "StatusBadGateway"),
            (503, "StatusServiceUnavailable"),
            (504, "StatusGatewayTimeout"),
            (505, "StatusHTTPVersionNotSupported"),
            (506, "StatusVariantAlsoNegotiates"),
            (507, "StatusInsufficientStorage"),
            (508, "StatusLoopDetected"),
            (510, "StatusNotExtended"),
            (511, "StatusNetworkAuthenticationRequired"),
        ])
    })
}

fn default_whitelist(code: i64) -> bool {
    matches!(code, 200 | 400 | 404 | 500)
}

fn arg_index_for(name: &str) -> Option<usize> {
    match name {
        "net/http.Error" => Some(2),
        "net/http.Redirect" => Some(3),
        "net/http.StatusText" => Some(0),
        "net/http.RedirectHandler" => Some(1),
        _ => None,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ST1013 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, String, String)> = Vec::new();
    {
        let files = pass.files();
        inspect.preorder(files, |n| {
            let NodeRef::CallExpr(call) = n else {
                return;
            };
            let Some(name) = guff_analysis::code::call_name(pass, &call.fun) else {
                return;
            };
            let Some(arg_i) = arg_index_for(&name) else {
                return;
            };
            let Some(arg) = call.args.get(arg_i) else {
                return;
            };
            let Expr::BasicLit(lit) = arg else {
                return;
            };
            if lit.kind != Some(Token::INT) {
                return;
            }
            let Some(n) = expr_to_int(pass, arg) else {
                return;
            };
            if default_whitelist(n) {
                return;
            }
            let Some(status) = http_status_codes().get(&n) else {
                return;
            };
            let replacement = format!("http.{status}");
            pending.push((
                lit.value_pos.0 as u32,
                lit.end().0 as u32,
                format!("should use constant {replacement} instead of numeric literal {n}"),
                replacement,
            ));
        });
    }

    for (pos, end, message, replacement) in pending {
        pass.report(Diagnostic {
            pos,
            end,
            message: message.clone(),
            suggested_fixes: vec![SuggestedFix {
                message: format!("Use {replacement} instead of the numeric literal"),
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

fn st1013_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1013",
        doc: "should use constants for HTTP error codes, not magic numbers",
        url: "https://staticcheck.dev/docs/checks/#ST1013",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1013_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1013_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn whitelist_defaults() {
        assert!(default_whitelist(200));
        assert!(default_whitelist(404));
        assert!(!default_whitelist(506));
    }
}
