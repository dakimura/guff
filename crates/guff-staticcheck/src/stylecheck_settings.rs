//! Pass-time options for stylecheck ST* checks (ST1001 / ST1003 / ST1013).
//!
//! Wired from `linters.settings.staticcheck` / `linters.settings.stylecheck`
//! (golangci-lint `StaticCheckSettings`).

use std::collections::HashSet;

/// Upstream default initialisms (`honnef.co/go/tools/config.DefaultConfig`).
pub const DEFAULT_INITIALISMS: &[&str] = &[
    "ACL", "API", "ASCII", "CPU", "CSS", "DNS", "EOF", "GUID", "HTML", "HTTP", "HTTPS", "ID",
    "IP", "JSON", "QPS", "RAM", "RPC", "SLA", "SMTP", "SQL", "SSH", "TCP", "TLS", "TTL", "UDP",
    "UI", "GID", "UID", "UUID", "URI", "URL", "UTF8", "VM", "XML", "XMPP", "XSRF", "XSS", "SIP",
    "RTP", "AMQP", "DB", "TS",
];

/// Upstream default HTTP status codes that ST1013 does not flag.
pub const DEFAULT_HTTP_STATUS_CODE_WHITELIST: &[&str] = &["200", "400", "404", "500"];

/// Pass-time options from `linters.settings.staticcheck` / `stylecheck`.
///
/// `None` or empty list for each field means “use upstream defaults” (golangci
/// `staticCheckConfig` behaviour).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StylecheckOptions {
    /// ST1003 known initialisms. Empty/`None` → [`DEFAULT_INITIALISMS`].
    pub initialisms: Option<Vec<String>>,
    /// ST1001 packages allowed to be dot-imported. Empty/`None` → none.
    pub dot_import_whitelist: Option<Vec<String>>,
    /// ST1013 numeric codes that are not reported. Empty/`None` →
    /// [`DEFAULT_HTTP_STATUS_CODE_WHITELIST`].
    pub http_status_code_whitelist: Option<Vec<String>>,
}

impl StylecheckOptions {
    /// Effective initialism set for ST1003.
    pub fn effective_initialisms(&self) -> HashSet<String> {
        match &self.initialisms {
            Some(list) if !list.is_empty() => list.iter().cloned().collect(),
            _ => DEFAULT_INITIALISMS.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    /// Effective package paths allowed as dot imports (ST1001).
    pub fn effective_dot_import_whitelist(&self) -> HashSet<String> {
        match &self.dot_import_whitelist {
            Some(list) => list.iter().cloned().collect(),
            None => HashSet::new(),
        }
    }

    /// Whether `code` is on the ST1013 whitelist.
    pub fn http_status_whitelisted(&self, code: i64) -> bool {
        let codes: Vec<i64> = match &self.http_status_code_whitelist {
            Some(list) if !list.is_empty() => list
                .iter()
                .filter_map(|s| s.parse::<i64>().ok())
                .collect(),
            _ => DEFAULT_HTTP_STATUS_CODE_WHITELIST
                .iter()
                .filter_map(|s| s.parse::<i64>().ok())
                .collect(),
        };
        codes.contains(&code)
    }
}
