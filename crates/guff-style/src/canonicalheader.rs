//! Port of [`github.com/golangci/canonicalheader`](https://github.com/golangci/canonicalheader),
//! the fork golangci-lint 2.14.0 moved to from lasiar/canonicalheader
//! (golangci-lint wrapper in `pkg/golinters/canonicalheader`).
//!
//! Reports non-canonical header keys passed to `net/http.Header` methods
//! (`Get` / `Set` / `Add` / `Del` / `Values`). The well-known initialisms
//! (ETag, WWW-Authenticate, X-Request-ID, …) and any configured `exclusions`
//! are the canonical spelling. SuggestedFix for string literals only.
//!
//! DEFERRED (see DEVELOPMENT.md R13): method-value calls (`f := h.Get`),
//! nested type-cast unwrapping of the key arg.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{BasicLit, CallExpr, Expr};
use guff::token::Token;
use guff::walk::{preorder, NodeRef};
use guff_analysis::code::{expr_to_string, type_func_name};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::ObjectData;

use crate::options::CanonicalheaderOptions;

const METHODS: &[&str] = &["Get", "Set", "Add", "Del", "Values"];

/// Port of Go's `net/textproto.CanonicalMIMEHeaderKey` /
/// `net/http.CanonicalHeaderKey` (without the special-case table).
fn canonical_mime_header_key(s: &str) -> String {
    let mut upper = true;
    let mut out = Vec::with_capacity(s.len());
    for &c in s.as_bytes() {
        if !valid_header_field_byte(c) {
            return s.to_string();
        }
        let mut c = c;
        if upper && c.is_ascii_lowercase() {
            c -= b'a' - b'A';
        } else if !upper && c.is_ascii_uppercase() {
            c += b'a' - b'A';
        }
        upper = c == b'-';
        out.push(c);
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

fn valid_header_field_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric()
        || c == b'!'
        || c == b'#'
        || c == b'$'
        || c == b'%'
        || c == b'&'
        || c == b'\''
        || c == b'*'
        || c == b'+'
        || c == b'-'
        || c == b'.'
        || c == b'^'
        || c == b'_'
        || c == b'`'
        || c == b'|'
        || c == b'~'
}

/// Upstream `initialism()` well-known non-MIME-canonical spellings.
fn initialism() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            ("A-Im", "A-IM"),
            ("Accept-Ch", "Accept-CH"),
            ("Alpn", "ALPN"),
            ("Amp-Cache-Transform", "AMP-Cache-Transform"),
            ("C-Pep", "C-PEP"),
            ("C-Pep-Info", "C-PEP-Info"),
            ("Cal-Managed-Id", "Cal-Managed-ID"),
            ("Caldav-Timezones", "CalDAV-Timezones"),
            ("Cdn-Cache-Control", "CDN-Cache-Control"),
            ("Cdn-Loop", "CDN-Loop"),
            ("Content-Id", "Content-ID"),
            ("Content-Md5", "Content-MD5"),
            ("Dasl", "DASL"),
            ("Dav", "DAV"),
            ("Differential-Id", "Differential-ID"),
            ("Dnt", "DNT"),
            ("Dpop", "DPoP"),
            ("Dpop-Nonce", "DPoP-Nonce"),
            ("Ediint-Features", "EDIINT-Features"),
            ("Etag", "ETag"),
            ("Expect-Ct", "Expect-CT"),
            ("Getprofile", "GetProfile"),
            ("Http2-Settings", "HTTP2-Settings"),
            ("Im", "IM"),
            (
                "Include-Referred-Token-Binding-Id",
                "Include-Referred-Token-Binding-ID",
            ),
            ("Last-Event-Id", "Last-Event-ID"),
            ("Mime-Version", "MIME-Version"),
            ("Nel", "NEL"),
            ("Odata-Entityid", "OData-EntityId"),
            ("Odata-Isolation", "OData-Isolation"),
            ("Odata-Maxversion", "OData-MaxVersion"),
            ("Odata-Version", "OData-Version"),
            ("Optional-Www-Authenticate", "Optional-WWW-Authenticate"),
            ("Oscore", "OSCORE"),
            ("Oslc-Core-Version", "OSLC-Core-Version"),
            ("P3p", "P3P"),
            ("Pep", "PEP"),
            ("Pep-Info", "PEP-Info"),
            ("Pics-Label", "PICS-Label"),
            ("Profileobject", "ProfileObject"),
            ("Repeatability-Client-Id", "Repeatability-Client-ID"),
            ("Repeatability-Request-Id", "Repeatability-Request-ID"),
            ("Sec-Gpc", "Sec-GPC"),
            ("Sec-Websocket-Accept", "Sec-WebSocket-Accept"),
            ("Sec-Websocket-Extensions", "Sec-WebSocket-Extensions"),
            ("Sec-Websocket-Key", "Sec-WebSocket-Key"),
            ("Sec-Websocket-Protocol", "Sec-WebSocket-Protocol"),
            ("Sec-Websocket-Version", "Sec-WebSocket-Version"),
            ("Setprofile", "SetProfile"),
            ("Slug", "SLUG"),
            ("Soapaction", "SoapAction"),
            ("Status-Uri", "Status-URI"),
            ("Tcn", "TCN"),
            ("Te", "TE"),
            ("Ttl", "TTL"),
            ("Uri", "URI"),
            ("Www-Authenticate", "WWW-Authenticate"),
            ("X-Correlation-Id", "X-Correlation-ID"),
            ("X-Dns-Prefetch-Control", "X-DNS-Prefetch-Control"),
            ("X-Real-Ip", "X-Real-IP"),
            ("X-Request-Id", "X-Request-ID"),
            ("X-Ua-Compatible", "X-UA-Compatible"),
            ("X-Webkit-Csp", "X-WebKit-CSP"),
            ("X-Xss", "X-XSS"),
            ("X-Xss-Protection", "X-XSS-Protection"),
        ])
    })
}

/// `buildExclusions`: canonical form → accepted spelling. The initialism
/// table unless `use-default-exclusions` is off, then each configured
/// exclusion keyed by its own MIME-canonical form (overriding the table).
/// golangci-lint joins the list with commas and the fork's flag splits on
/// them again, trimming and dropping empties — reproduced here.
fn build_exclusions(opts: &CanonicalheaderOptions) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    if opts.use_default_exclusions {
        for (canonical, spelling) in initialism() {
            out.insert((*canonical).to_string(), (*spelling).to_string());
        }
    }
    for ex in opts.exclusions.iter().flat_map(|e| e.split(',')) {
        let ex = ex.trim();
        if ex.is_empty() {
            continue;
        }
        out.insert(canonical_mime_header_key(ex), ex.to_string());
    }
    out
}

/// The fork's `canonicalHeaderKey`: the table's spelling when the
/// MIME-canonical form is in it, else the MIME-canonical form.
///
/// lasiar's version (golangci-lint 2.12.2) also returned whether the key was
/// in the table and treated that as a reason to stay silent, and returned
/// early when the argument was already MIME-canonical: `x-request-id` and
/// `X-Request-Id` were both fine. The fork suggests `X-Request-ID` for both.
fn canonical_header_key(s: &str, exclusions: &HashMap<String, String>) -> String {
    let canonical = canonical_mime_header_key(s);
    match exclusions.get(&canonical) {
        Some(spelling) => spelling.clone(),
        None => canonical,
    }
}

fn is_header_method(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return false;
    };
    if !METHODS.contains(&sel.sel.name.as_str()) {
        return false;
    }
    let Some(obj_id) = info.uses.get(&sel.sel.id).copied() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    if !matches!(artifacts.objects.get(obj_id), ObjectData::Func(_)) {
        return false;
    }
    let name = type_func_name(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        obj_id,
    );
    name == format!("(net/http.Header).{}", sel.sel.name)
}

enum KeyArg {
    Literal {
        value: String,
        quote: char,
        pos: u32,
        end: u32,
    },
    Const {
        value: String,
        pos: u32,
        end: u32,
    },
}

impl KeyArg {
    fn value(&self) -> &str {
        match self {
            KeyArg::Literal { value, .. } | KeyArg::Const { value, .. } => value,
        }
    }

    fn pos(&self) -> u32 {
        match self {
            KeyArg::Literal { pos, .. } | KeyArg::Const { pos, .. } => *pos,
        }
    }

    fn end(&self) -> u32 {
        match self {
            KeyArg::Literal { end, .. } | KeyArg::Const { end, .. } => *end,
        }
    }
}

fn peel_string_casts<'a>(pass: &Pass<'_>, mut expr: &'a Expr) -> &'a Expr {
    // DEFERRED: full nested cast peel like upstream; stop at non-ident CallExpr.
    loop {
        let Expr::CallExpr(c) = expr else {
            break;
        };
        if c.args.is_empty() {
            break;
        }
        let Expr::Ident(fun) = c.fun.as_ref() else {
            break;
        };
        let Some(info) = pass.types_info() else {
            break;
        };
        let Some(obj_id) = info
            .uses
            .get(&fun.id)
            .copied()
            .or_else(|| info.defs.get(&fun.id).copied().flatten())
        else {
            break;
        };
        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
            break;
        };
        if matches!(artifacts.objects.get(obj_id), ObjectData::Func(_)) {
            break;
        }
        expr = &c.args[0];
    }
    expr
}

fn key_arg(pass: &Pass<'_>, expr: &Expr) -> Option<KeyArg> {
    let expr = peel_string_casts(pass, expr);
    match expr {
        Expr::BasicLit(BasicLit {
            kind: Some(Token::STRING),
            value,
            value_pos,
            ..
        }) => {
            if value.len() < 2 {
                return None;
            }
            let quote = value.chars().next()?;
            if quote != '"' && quote != '`' {
                return None;
            }
            let unquoted = expr_to_string(pass, expr)?;
            Some(KeyArg::Literal {
                value: unquoted,
                quote,
                pos: value_pos.0 as u32,
                end: expr.end().0 as u32,
            })
        }
        Expr::Ident(id) => {
            let value = expr_to_string(pass, expr)?;
            Some(KeyArg::Const {
                value,
                pos: id.name_pos.0 as u32,
                end: id.end().0 as u32,
            })
        }
        _ => None,
    }
}

struct Pending {
    diag: Diagnostic,
}

fn check_call(
    pass: &Pass<'_>,
    call: &CallExpr,
    exclusions: &HashMap<String, String>,
    pending: &mut Vec<Pending>,
) {
    if !is_header_method(pass, call) || call.args.is_empty() {
        return;
    }
    let Some(arg) = key_arg(pass, &call.args[0]) else {
        return;
    };
    let canonical = canonical_header_key(arg.value(), exclusions);
    if arg.value() == canonical {
        return;
    }
    // One message for literals and constants in the fork; lasiar had
    // "non-canonical header %q, instead use: %q" and a third for constants.
    let message = format!("use {canonical:?} instead of {:?}", arg.value());
    let suggested_fixes = match &arg {
        KeyArg::Literal { quote, .. } => {
            let new_text = format!("{quote}{canonical}{quote}");
            vec![SuggestedFix {
                message: format!("should be replaced {:?} with {canonical:?}", arg.value()),
                text_edits: vec![TextEdit {
                    pos: arg.pos(),
                    end: arg.end(),
                    new_text,
                }],
            }]
        }
        KeyArg::Const { .. } => Vec::new(),
    };
    pending.push(Pending {
        diag: Diagnostic {
            pos: arg.pos(),
            end: arg.end(),
            message,
            suggested_fixes,
            ..Diagnostic::default()
        },
    });
}

/// Upstream's opening move, and the reason a package can go completely silent:
///
/// ```go
/// var headerObject types.Object
/// for _, object := range pass.TypesInfo.Uses {
///     if object.Pkg() != nil && object.Pkg().Path() == pkgPath && object.Name() == name {
///         headerObject = object
///         break
///     }
/// }
/// if headerObject == nil { return nil, nil }
/// ...
/// if !types.Identical(gotType, headerObject.Type()) { return }
/// ```
///
/// `net/http` has **four** objects named `Header`: the type `http.Header`, the
/// fields `Request.Header` and `Response.Header` — whose type *is*
/// `http.Header` — and the method `ResponseWriter.Header`, whose type is
/// `func() http.Header`. The loop keeps whichever one a **map** iteration hands
/// it first, so when it lands on the method the identity test never holds and
/// the analyzer reports nothing in that package.
///
/// Measured against golangci-lint v2.12.2 with a fresh cache each run:
///
/// | package uses | upstream |
/// |---|---|
/// | only `w.Header().Set(…)` | **always silent** (the method is the only candidate) |
/// | only `r.Header.Get(…)` | **always reports** (the field is the only candidate) |
/// | both, or a written `http.Header` beside `w.Header()` | **0 or 2, at random** |
///
/// guff had no such gate and always reported. It now matches both deterministic
/// halves: silent when every candidate is the method, reporting when any
/// candidate carries the type itself. The mixed case is a coin flip upstream and
/// no port can match it — guff takes the reporting side, and syncthing's
/// `lib/api` is allowlisted for it.
fn package_has_header_typed_object(pass: &Pass<'_>) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let is_http_header_type = |typ| {
        let t = guff_types::unalias_readonly(&artifacts.types, typ);
        match artifacts.types.get(t) {
            guff_types::arena::TypeData::Named(_) => {
                let obj = guff_types::named::named_obj(&artifacts.types, t);
                obj.name(&artifacts.objects) == "Header"
                    && obj.pkg(&artifacts.objects).is_some_and(|p| {
                        artifacts.packages.get(p).path() == "net/http"
                    })
            }
            _ => false,
        }
    };
    info.uses.values().any(|obj| {
        obj.name(&artifacts.objects) == "Header"
            && obj
                .pkg(&artifacts.objects)
                .is_some_and(|p| artifacts.packages.get(p).path() == "net/http")
            && obj
                .typ(&artifacts.objects)
                .is_some_and(&is_http_header_type)
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "canonicalheader requires inspect analyzer".to_string())?;

    if !package_has_header_typed_object(pass) {
        return Ok(None);
    }

    let opts = pass
        .settings::<CanonicalheaderOptions>("canonicalheader")
        .cloned()
        .unwrap_or_default();
    let exclusions = build_exclusions(&opts);
    let mut pending = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::CallExpr(call) = n {
                check_call(pass, call, &exclusions, &mut pending);
            }
            true
        });
    }

    for p in pending {
        pass.report(p.diag);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "canonicalheader",
        doc: "canonicalheader checks whether net/http.Header uses canonical header",
        url: "https://github.com/lasiar/canonicalheader",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_mime_matches_go() {
        assert_eq!(canonical_mime_header_key("foo"), "Foo");
        assert_eq!(canonical_mime_header_key("etag"), "Etag");
        assert_eq!(canonical_mime_header_key("content-type"), "Content-Type");
        assert_eq!(canonical_mime_header_key("Test-HEader"), "Test-Header");
    }

    #[test]
    fn initialism_overrides_etag() {
        let ex = build_exclusions(&CanonicalheaderOptions::default());
        assert_eq!(canonical_header_key("etag", &ex), "ETag");
        assert_eq!(canonical_header_key("www-authenticate", &ex), "WWW-Authenticate");
        assert_eq!(canonical_header_key("Test-HEader", &ex), "Test-Header");
    }

    #[test]
    fn exclusions_override_and_split_like_the_fork() {
        let ex = build_exclusions(&CanonicalheaderOptions {
            exclusions: vec!["X-ID, x-custom-TOKEN".into(), "".into()],
            use_default_exclusions: false,
        });
        assert_eq!(canonical_header_key("x-id", &ex), "X-ID");
        assert_eq!(canonical_header_key("X-Custom-Token", &ex), "x-custom-TOKEN");
        // The table is off.
        assert_eq!(canonical_header_key("etag", &ex), "Etag");
    }
}
