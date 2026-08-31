//! Port of [`github.com/securego/gosec`](https://github.com/securego/gosec)
//! (golangci-lint wrapper in `pkg/golinters/gosec`).
//!
//! Implemented rules (AST / types-info only):
//! - **G101** — potential hardcoded credentials (name pattern + zxcvbn entropy /
//!   known secret regexes on AssignStmt / ValueSpec / BinaryExpr / CompositeLit)
//! - **G102** — bind to all interfaces (`net.Listen` / `crypto/tls.Listen` address)
//! - **G103** — `unsafe` calls (`Pointer` / `String` / `StringData` / `Slice` / `SliceData`)
//! - **G104** — unchecked errors
//! - **G106** — `ssh.InsecureIgnoreHostKey`
//! - **G107** — HTTP request with variable URL (SSRF; Ident-only like upstream
//!   `ResolveVar`; BasicLit/Const safe; full TryResolve DEFERRED)
//! - **G108** — blank import of `net/http/pprof`
//! - **G109** — `strconv.Atoi` result converted to `int16`/`int32`
//! - **G110** — potential decompression bomb (`io.Copy`/`CopyBuffer` from archive reader)
//! - **G111** — `http.Dir("/")` directory traversal
//! - **G112** — `http.Server` without `ReadHeaderTimeout`/`ReadTimeout` (Slowloris)
//! - **G114** — `net/http` serve helpers without timeouts
//! - **G115** — integer overflow conversion (SSA + range analysis; see
//!   `gosec_g115`)
//! - **G118** — context propagation failure: an uncalled `cancel`, a goroutine on
//!   `context.Background`/`TODO`, or a non-terminating loop with no `ctx.Done()`
//!   guard (SSA; see `gosec_g118`)
//! - **G122** — `filepath.Walk`/`WalkDir` callback path into race-prone `os` sinks (AST approx of SSA)
//! - **G123** — `tls.Config` sets `VerifyPeerCertificate` but leaves session
//!   resumption able to skip it (SSA; see `gosec_g123`)
//! - **G124** — `http.Cookie` missing Secure / HttpOnly / SameSite (AST approx of SSA rule)
//! - **G202** — SQL string concatenation
//! - **G203** — `html/template` non-escaping helpers with non-literal args
//! - **G204** — subprocess launched with non-literal args (`os/exec` / `syscall` / `execabs`;
//!   full `resolve.go` `TryResolve`, incl. the parameter/field exemption in the
//!   executable-name slot — see `FileDecls` for why resolution is file-local)
//! - **G301** — poor directory permissions (`os.Mkdir` / `MkdirAll`; default ≤ `0o750`)
//! - **G302** — poor file permissions (`os.OpenFile` / `Chmod`; default ≤ `0o600`)
//! - **G303** — tempfile creation under predictable shared `/tmp` paths
//! - **G306** — poor `WriteFile` permissions (default ≤ `0o600`)
//! - **G401** — weak hash (`crypto/md5` / `crypto/sha1` `New`/`Sum`)
//! - **G402** — `tls.Config` with `InsecureSkipVerify: true` (MinVersion / CipherSuites DEFERRED)
//! - **G403** — weak RSA key (`crypto/rsa.GenerateKey` bits < 2048)
//! - **G404** — weak RNG (`math/rand` / `math/rand/v2`)
//! - **G405** — weak encryption (`crypto/des` / `crypto/rc4`)
//! - **G406** — deprecated weak hash (`golang.org/x/crypto/{md4,ripemd160}`)
//! - **G501–G507** — blocklisted imports
//! - **G602** — slice index / bounds out of range (SSA; see `gosec_g602`)
//! - **G702 / G703 / G705 / G706 / G710** — command injection, path traversal,
//!   XSS, log injection and open redirect: one taint engine over five tables of
//!   sources, sinks and sanitizers (SSA; see `gosec_taint`)
//!
//! Message format matches golangci: `"Gxxx: <what>"`.
//!
//! DEFERRED: remaining rules (G113, G116–G117, G119–G121, G201, G304–G305, G307
//! config-gated, G402 MinVersion/CipherSuites, G601, and the taint rules the
//! engine has no table for — G701 SQL, G704 SSRF, G707–G709),
//! full `gosec:disable` block directives / per-rule
//! `config` map, G104 audit mode + config allowlist extensions, G107 local
//! string-lit TryResolve, G102 Ident const resolution, concurrency.
//!
//! Every rule above is gated against golangci-lint 2.12.2 at check level by
//! `compat/golden/cases/gosec` — including the **severity** golangci attaches
//! to gosec findings and to no other linter's.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, BinaryExpr, CallExpr, CompositeLit, Decl, Expr, File, Ident, ImportSpec, Spec,
    ValueSpec,
};
use guff::token::Token;
use guff::commentmap::node_end;
use guff::walk::{preorder, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn};
use guff_types::arena::{ObjectData, ObjectId, TypeData};
use guff_types::check_lookup::implements;
use guff_types::new_pointer;
use guff_types::scope::lookup;
use guff_types::typestring::type_string;
use guff_types::TypeId;
use guff_types::alias::unalias_readonly;
use regex::Regex;

use crate::options::GosecOptions;

struct RuleDef {
    id: &'static str,
    /// For call rules: `(pkg_path, func_name)`.
    calls: &'static [(&'static str, &'static str)],
    /// For import rules: `(import_path, description)`.
    imports: &'static [(&'static str, &'static str)],
    /// When true, import rule only matches blank imports (`import _ "…"`).
    blank_import_only: bool,
    /// Call-rule message body (after `"Gxxx: "`).
    call_what: &'static str,
}

const RULES: &[RuleDef] = &[
    RuleDef {
        id: "G103",
        calls: &[
            ("unsafe", "Pointer"),
            ("unsafe", "String"),
            ("unsafe", "StringData"),
            ("unsafe", "Slice"),
            ("unsafe", "SliceData"),
        ],
        imports: &[],
        blank_import_only: false,
        call_what: "Use of unsafe calls should be audited",
    },
    RuleDef {
        id: "G106",
        calls: &[("golang.org/x/crypto/ssh", "InsecureIgnoreHostKey")],
        imports: &[],
        blank_import_only: false,
        call_what: "Use of ssh InsecureIgnoreHostKey should be audited",
    },
    RuleDef {
        id: "G108",
        calls: &[],
        imports: &[(
            "net/http/pprof",
            "Profiling endpoint is automatically exposed on /debug/pprof",
        )],
        blank_import_only: true,
        call_what: "",
    },
    RuleDef {
        id: "G114",
        calls: &[
            ("net/http", "ListenAndServe"),
            ("net/http", "ListenAndServeTLS"),
            ("net/http", "Serve"),
            ("net/http", "ServeTLS"),
        ],
        imports: &[],
        blank_import_only: false,
        call_what: "Use of net/http serve function that has no support for setting timeouts",
    },
    RuleDef {
        id: "G401",
        calls: &[
            ("crypto/md5", "New"),
            ("crypto/md5", "Sum"),
            ("crypto/sha1", "New"),
            ("crypto/sha1", "Sum"),
        ],
        imports: &[],
        blank_import_only: false,
        call_what: "Use of weak cryptographic primitive",
    },
    RuleDef {
        id: "G404",
        // gosec v2.26.1 `rules/rand.go`, verbatim. `Perm`, `Shuffle` and
        // `ExpFloat64` are *not* on it — they were guff additions, and with
        // the package-qualified match in place they were the only remaining
        // way `rand.Perm(n)` could be a finding upstream is silent on.
        calls: &[
            ("math/rand", "New"),
            ("math/rand", "Read"),
            ("math/rand", "Float32"),
            ("math/rand", "Float64"),
            ("math/rand", "Int"),
            ("math/rand", "Int31"),
            ("math/rand", "Int31n"),
            ("math/rand", "Int63"),
            ("math/rand", "Int63n"),
            ("math/rand", "Intn"),
            ("math/rand", "NormFloat64"),
            ("math/rand", "Uint32"),
            ("math/rand", "Uint64"),
            ("math/rand/v2", "New"),
            ("math/rand/v2", "Float32"),
            ("math/rand/v2", "Float64"),
            ("math/rand/v2", "Int"),
            ("math/rand/v2", "Int32"),
            ("math/rand/v2", "Int32N"),
            ("math/rand/v2", "Int64"),
            ("math/rand/v2", "Int64N"),
            ("math/rand/v2", "IntN"),
            ("math/rand/v2", "N"),
            ("math/rand/v2", "NormFloat64"),
            ("math/rand/v2", "Uint32"),
            ("math/rand/v2", "Uint32N"),
            ("math/rand/v2", "Uint64"),
            ("math/rand/v2", "Uint64N"),
            ("math/rand/v2", "UintN"),
        ],
        imports: &[],
        blank_import_only: false,
        call_what: "Use of weak random number generator (math/rand or math/rand/v2 instead of crypto/rand)",
    },
    RuleDef {
        id: "G405",
        calls: &[
            ("crypto/des", "NewCipher"),
            ("crypto/des", "NewTripleDESCipher"),
            ("crypto/rc4", "NewCipher"),
        ],
        imports: &[],
        blank_import_only: false,
        call_what: "Use of weak cryptographic primitive",
    },
    RuleDef {
        id: "G406",
        calls: &[
            ("golang.org/x/crypto/md4", "New"),
            ("golang.org/x/crypto/ripemd160", "New"),
        ],
        imports: &[],
        blank_import_only: false,
        call_what: "Use of deprecated weak cryptographic primitive",
    },
    RuleDef {
        id: "G501",
        calls: &[],
        imports: &[(
            "crypto/md5",
            "Blocklisted import crypto/md5: weak cryptographic primitive",
        )],
        blank_import_only: false,
        call_what: "",
    },
    RuleDef {
        id: "G502",
        calls: &[],
        imports: &[(
            "crypto/des",
            "Blocklisted import crypto/des: weak cryptographic primitive",
        )],
        blank_import_only: false,
        call_what: "",
    },
    RuleDef {
        id: "G503",
        calls: &[],
        imports: &[(
            "crypto/rc4",
            "Blocklisted import crypto/rc4: weak cryptographic primitive",
        )],
        blank_import_only: false,
        call_what: "",
    },
    RuleDef {
        id: "G504",
        calls: &[],
        imports: &[(
            "net/http/cgi",
            "Blocklisted import net/http/cgi: Go versions < 1.6.3 are vulnerable to Httpoxy attack: (CVE-2016-5386)",
        )],
        blank_import_only: false,
        call_what: "",
    },
    RuleDef {
        id: "G505",
        calls: &[],
        imports: &[(
            "crypto/sha1",
            "Blocklisted import crypto/sha1: weak cryptographic primitive",
        )],
        blank_import_only: false,
        call_what: "",
    },
    RuleDef {
        id: "G506",
        calls: &[],
        imports: &[(
            "golang.org/x/crypto/md4",
            "Blocklisted import golang.org/x/crypto/md4: deprecated and weak cryptographic primitive",
        )],
        blank_import_only: false,
        call_what: "",
    },
    RuleDef {
        id: "G507",
        calls: &[],
        imports: &[(
            "golang.org/x/crypto/ripemd160",
            "Blocklisted import golang.org/x/crypto/ripemd160: deprecated and weak cryptographic primitive",
        )],
        blank_import_only: false,
        call_what: "",
    },
];

/// Synthetic rule ids handled outside [`RULES`] (arg-sensitive / AST-pattern).
const EXTRA_RULE_IDS: &[&str] = &[
    "G101", "G102", "G104", "G107", "G109", "G110", "G111", "G112", "G115", "G118", "G122", "G124",
    "G123", "G202",
    "G203",
    "G204", "G301", "G302", "G303", "G306", "G402", "G403", "G602",
    // The taint engine's five rules (`gosec_taint`), all SSA analyzers.
    "G702", "G703", "G705", "G706", "G710",
];

const G301_MODE: i64 = 0o750;
const G302_MODE: i64 = 0o600;
const G306_MODE: i64 = 0o600;
const G403_MIN_BITS: i64 = 2048;

const G301_CALLS: &[(&str, &str)] = &[("os", "Mkdir"), ("os", "MkdirAll")];
const G302_CALLS: &[(&str, &str)] = &[("os", "OpenFile"), ("os", "Chmod")];
const G306_CALLS: &[(&str, &str)] = &[("os", "WriteFile"), ("io/ioutil", "WriteFile")];
const G303_CALLS: &[(&str, &str)] = &[
    ("os", "Create"),
    ("os", "WriteFile"),
    ("io/ioutil", "WriteFile"),
];
const G403_CALLS: &[(&str, &str)] = &[("crypto/rsa", "GenerateKey")];
const G111_CALLS: &[(&str, &str)] = &[("net/http", "Dir")];
const G107_CALLS: &[(&str, &str)] = &[
    ("net/http", "Do"),
    ("net/http", "Get"),
    ("net/http", "Head"),
    ("net/http", "Post"),
    ("net/http", "PostForm"),
    ("net/http", "RoundTrip"),
];
const G203_CALLS: &[(&str, &str)] = &[
    ("html/template", "CSS"),
    ("html/template", "HTML"),
    ("html/template", "HTMLAttr"),
    ("html/template", "JS"),
    ("html/template", "JSStr"),
    ("html/template", "Srcset"),
    ("html/template", "URL"),
];
const G109_ATOI: (&str, &str) = ("strconv", "Atoi");
const G110_READER_CALLS: &[(&str, &str)] = &[
    ("compress/gzip", "NewReader"),
    ("compress/zlib", "NewReader"),
    ("compress/zlib", "NewReaderDict"),
    ("compress/bzip2", "NewReader"),
    ("compress/flate", "NewReader"),
    ("compress/flate", "NewReaderDict"),
    ("compress/lzw", "NewReader"),
    ("archive/tar", "NewReader"),
    ("archive/zip", "NewReader"),
];
const G110_COPY_CALLS: &[(&str, &str)] = &[("io", "Copy"), ("io", "CopyBuffer")];
/// Upstream: `^(/(usr|var))?/tmp(/.*)?$`
const G303_TMP_PATTERN: &str = r"^(/(usr|var))?/tmp(/.*)?$";
const G303_WHAT: &str = "File creation in shared tmp directory without using ioutil.Tempfile";
const G107_WHAT: &str = "Potential HTTP request made with variable url";
const G122_WHAT: &str = "Filesystem operation in filepath.Walk/WalkDir callback uses race-prone path; consider root-scoped APIs (e.g. os.Root) to prevent symlink TOCTOU traversal";
const G109_WHAT: &str =
    "Potential Integer overflow made by strconv.Atoi result conversion to int16/32";
const G110_WHAT: &str = "Potential DoS vulnerability via decompression bomb";
const G112_WHAT: &str =
    "Potential Slowloris Attack because ReadHeaderTimeout is not configured in the http.Server";
const G203_WHAT: &str = "The used method does not auto-escape HTML. This can potentially lead to 'Cross-site Scripting' vulnerabilities, in case the attacker controls the input.";

const G101_WHAT: &str = "Potential hardcoded credentials";
/// Upstream default: `(?i)passwd|pass|password|pwd|secret|token|pw|apiKey|bearer|cred`
const G101_NAME_PATTERN: &str = r"(?i)passwd|pass|password|pwd|secret|token|pw|apiKey|bearer|cred";

struct SecretPattern {
    name: &'static str,
    re: &'static str,
}

/// Subset of securego/gosec `secretsPatterns` (enough for common tokens / keys).
const G101_SECRET_PATTERNS: &[SecretPattern] = &[
    SecretPattern {
        name: "RSA private key",
        re: r"-----BEGIN RSA PRIVATE KEY-----",
    },
    SecretPattern {
        name: "SSH (DSA) private key",
        re: r"-----BEGIN DSA PRIVATE KEY-----",
    },
    SecretPattern {
        name: "SSH (EC) private key",
        re: r"-----BEGIN EC PRIVATE KEY-----",
    },
    SecretPattern {
        name: "PGP private key block",
        re: r"-----BEGIN PGP PRIVATE KEY BLOCK-----",
    },
    SecretPattern {
        name: "Slack Token",
        re: r"xox[pborsa]-[0-9]{12}-[0-9]{12}-[0-9]{12}-[a-z0-9]{32}",
    },
    SecretPattern {
        name: "AWS API Key",
        re: r"AKIA[0-9A-Z]{16}",
    },
    SecretPattern {
        name: "AWS Temporary Access Key",
        re: r"ASIA[0-9A-Z]{16}",
    },
    SecretPattern {
        name: "Amazon MWS Auth Token",
        re: r"amzn\.mws\.[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
    },
    SecretPattern {
        name: "AWS AppSync GraphQL Key",
        re: r"da2-[a-z0-9]{26}",
    },
    SecretPattern {
        name: "GitHub personal access token",
        re: r"ghp_[a-zA-Z0-9]{36}",
    },
    SecretPattern {
        name: "GitHub fine-grained access token",
        re: r"github_pat_[a-zA-Z0-9]{22}_[a-zA-Z0-9]{59}",
    },
    SecretPattern {
        name: "GitHub action temporary token",
        re: r"ghs_[a-zA-Z0-9]{36}",
    },
    SecretPattern {
        name: "Google API Key",
        re: r"AIza[0-9A-Za-z\-_]{35}",
    },
    SecretPattern {
        name: "Google Cloud Platform OAuth",
        re: r"[0-9]+-[0-9A-Za-z_]{32}\.apps\.googleusercontent\.com",
    },
    SecretPattern {
        name: "Google (GCP) Service-account",
        re: r#""type": "service_account""#,
    },
    SecretPattern {
        name: "Google OAuth Access Token",
        re: r"ya29\.[0-9A-Za-z\-_]+",
    },
    SecretPattern {
        name: "Generic API Key",
        re: r#"[aA][pP][iI]_?[kK][eE][yY].*[''|"][0-9a-zA-Z]{32,45}[''|"]"#,
    },
    SecretPattern {
        name: "Generic Secret",
        re: r#"[sS][eE][cC][rR][eE][tT].*[''|"][0-9a-zA-Z]{32,45}[''|"]"#,
    },
    SecretPattern {
        name: "Heroku API Key",
        re: r"[hH][eE][rR][oO][kK][uU].*[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}",
    },
    SecretPattern {
        name: "MailChimp API Key",
        re: r"[0-9a-f]{32}-us[0-9]{1,2}",
    },
    SecretPattern {
        name: "Mailgun API Key",
        re: r"key-[0-9a-zA-Z]{32}",
    },
    SecretPattern {
        name: "Password in URL",
        re: r#"[a-zA-Z]{3,10}://[a-zA-Z0-9.\-_+]{1,64}:[a-zA-Z0-9.\-_!$%&*+=^()]{1,128}@[a-zA-Z0-9.\-_]+(:[0-9]+)?(/[^"'\s]*)?(["'\s]|$)"#,
    },
    SecretPattern {
        name: "Slack Webhook",
        re: r"https://hooks\.slack\.com/services/T[a-zA-Z0-9_]{8}/B[a-zA-Z0-9_]{8}/[a-zA-Z0-9_]{24}",
    },
    SecretPattern {
        name: "Stripe API Key",
        re: r"sk_live_[0-9a-zA-Z]{24}",
    },
    SecretPattern {
        name: "Stripe Restricted API Key",
        re: r"rk_live_[0-9a-zA-Z]{24}",
    },
    SecretPattern {
        name: "Square Access Token",
        re: r"sq0atp-[0-9A-Za-z\-_]{22}",
    },
    SecretPattern {
        name: "Square OAuth Secret",
        re: r"sq0csp-[0-9A-Za-z\-_]{43}",
    },
    SecretPattern {
        name: "Telegram Bot API Key",
        re: r"[0-9]+:AA[0-9A-Za-z\-_]{33}",
    },
    SecretPattern {
        name: "Twilio API Key",
        re: r"SK[0-9a-fA-F]{32}",
    },
    SecretPattern {
        name: "Twitter Access Token",
        re: r"[tT][wW][iI][tT][tT][eE][rR].*[1-9][0-9]+-[0-9a-zA-Z]{40}",
    },
    SecretPattern {
        name: "Twitter OAuth",
        re: r#"[tT][wW][iI][tT][tT][eE][rR].*[''|"][0-9a-zA-Z]{35,44}[''|"]"#,
    },
];

const G204_CALLS: &[(&str, &str)] = &[
    ("os/exec", "Command"),
    ("os/exec", "CommandContext"),
    ("syscall", "Exec"),
    ("syscall", "ForkExec"),
    ("syscall", "StartProcess"),
    ("golang.org/x/sys/execabs", "Command"),
    ("golang.org/x/sys/execabs", "CommandContext"),
];

const G102_CALLS: &[(&str, &str)] = &[("net", "Listen"), ("crypto/tls", "Listen")];

/// gosec `issue.Score` (`Low = iota`, `Medium`, `High`) — ordered, so
/// golangci's `i.Severity >= severity` comparison is a plain `>=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Score {
    Low,
    Medium,
    High,
}

impl Score {
    /// golangci-lint `convertScoreToString`, which is what lands in the
    /// `Severity` field of a gosec issue. gosec is the only linter golangci
    /// grades this way — every other one leaves the field empty — so this is
    /// the only place guff sets `Diagnostic::severity`.
    fn as_str(self) -> &'static str {
        match self {
            Score::Low => "low",
            Score::Medium => "medium",
            Score::High => "high",
        }
    }
}

/// golangci-lint `convertToScore`: `""`/`"low"` → Low, and anything
/// unrecognized disables the filter (upstream returns `-1`, which every score
/// compares `>=` against).
fn threshold_score(s: &str) -> Option<Score> {
    match s.trim().to_ascii_lowercase().as_str() {
        "" | "low" => Some(Score::Low),
        "medium" => Some(Score::Medium),
        "high" => Some(Score::High),
        _ => None,
    }
}

/// `(severity, confidence)` per rule id, from gosec v2.26.1 rule metadata.
///
/// Only rules guff implements are listed; unknown ids fall back to `Low/Low`
/// so a new rule is never silently filtered out before it has a row here.
const RULE_SCORES: &[(&str, Score, Score)] = &[
    ("G101", Score::High, Score::Low),
    ("G102", Score::Medium, Score::High),
    ("G103", Score::Low, Score::High),
    ("G104", Score::Low, Score::High),
    ("G106", Score::Medium, Score::High),
    ("G107", Score::Medium, Score::Medium),
    ("G108", Score::High, Score::High),
    ("G109", Score::High, Score::Medium),
    ("G110", Score::Medium, Score::Medium),
    ("G111", Score::Medium, Score::Medium),
    ("G112", Score::Medium, Score::Low),
    ("G114", Score::Medium, Score::High),
    ("G115", Score::High, Score::Medium),
    // G118 grades each of its three checks separately; see `issue_scores`.
    ("G118", Score::Medium, Score::High),
    ("G123", Score::High, Score::High),
    ("G122", Score::High, Score::Medium),
    ("G124", Score::Medium, Score::High),
    ("G202", Score::Medium, Score::High),
    ("G203", Score::Medium, Score::Low),
    ("G204", Score::Medium, Score::High),
    ("G301", Score::Medium, Score::High),
    ("G302", Score::Medium, Score::High),
    ("G303", Score::Medium, Score::High),
    ("G306", Score::Medium, Score::High),
    ("G401", Score::Medium, Score::High),
    // G402 is message-dependent; see `issue_scores`.
    ("G402", Score::High, Score::High),
    ("G403", Score::Medium, Score::High),
    ("G404", Score::High, Score::Medium),
    ("G405", Score::Medium, Score::High),
    ("G406", Score::Medium, Score::High),
    ("G501", Score::Medium, Score::High),
    ("G502", Score::Medium, Score::High),
    ("G503", Score::Medium, Score::High),
    ("G504", Score::Medium, Score::High),
    ("G505", Score::Medium, Score::High),
    ("G506", Score::Medium, Score::High),
    ("G507", Score::Medium, Score::High),
    ("G602", Score::Low, Score::High),
    // taint/analyzer.go grades every taint finding `rule.Severity` /
    // `issue.High`; the severities are the four `taint.RuleInfo`s in
    // `analyzers/analyzerslist.go`. "CRITICAL" maps to High — gosec has no
    // fourth score.
    ("G702", Score::High, Score::High),
    ("G703", Score::High, Score::High),
    ("G705", Score::Medium, Score::High),
    ("G706", Score::Low, Score::High),
    ("G710", Score::Medium, Score::High),
];

/// Severity/confidence for one finding.
///
/// Upstream attaches these per `NewIssue` call, not per rule, so the few rules
/// that grade their own findings need the message to disambiguate. G402 is the
/// only such rule guff implements: a definite `InsecureSkipVerify set to true`
/// is High/High, while the `may be set to true` variant drops to High/Low.
fn issue_scores(rule: &str, msg: &str) -> (Score, Score) {
    if rule == "G402" && msg.contains("may be set to true") {
        return (Score::High, Score::Low);
    }
    if rule == "G118" {
        // One analyzer id, three checks, three grades — the table's entry is
        // the lost-cancel one, so only the other two need naming here.
        if msg == crate::gosec_g118::MSG_BACKGROUND {
            return (Score::High, Score::Medium);
        }
        if msg == crate::gosec_g118::MSG_LOOP_WITHOUT_DONE {
            return (Score::High, Score::Low);
        }
    }
    RULE_SCORES
        .iter()
        .find(|(id, _, _)| *id == rule)
        .map(|(_, sev, conf)| (*sev, *conf))
        .unwrap_or((Score::Low, Score::Low))
}

fn enabled_rules(opts: &GosecOptions) -> HashSet<&'static str> {
    let mut ids: HashSet<&'static str> = RULES.iter().map(|r| r.id).collect();
    for id in EXTRA_RULE_IDS {
        ids.insert(id);
    }
    if !opts.includes.is_empty() {
        let want: HashSet<&str> = opts.includes.iter().map(String::as_str).collect();
        ids.retain(|id| want.contains(id));
    }
    if !opts.excludes.is_empty() {
        let skip: HashSet<&str> = opts.excludes.iter().map(String::as_str).collect();
        ids.retain(|id| !skip.contains(id));
    }
    ids
}

fn unquote_import(lit: &str) -> &str {
    lit.trim().trim_matches('"').trim_matches('`')
}

fn unquote_string_lit(value: &str) -> Option<String> {
    let v = value.trim();
    if (v.starts_with('"') && v.ends_with('"')) || (v.starts_with('`') && v.ends_with('`')) {
        Some(v[1..v.len() - 1].to_string())
    } else {
        None
    }
}

fn split_fq_name(fq: &str) -> Option<(&str, &str)> {
    let idx = fq.rfind('.')?;
    if idx == 0 || idx + 1 >= fq.len() {
        return None;
    }
    Some((&fq[..idx], &fq[idx + 1..]))
}

fn cut_vendor(path: &str) -> &str {
    if let Some(i) = path.rfind("vendor/") {
        &path[i + "vendor/".len()..]
    } else {
        path
    }
}

fn imported_pkg_path(pass: &Pass<'_>, pkg_ident: &Ident) -> Option<String> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let &obj = info.uses.get(&pkg_ident.id)?;
    match artifacts.objects.get(obj) {
        ObjectData::PkgName(pn) => {
            let path = artifacts.packages.get(pn.imported()).path();
            Some(cut_vendor(path).to_string())
        }
        _ => None,
    }
}

/// AST approx of gosec G122 (SSA walk-symlink-race): race-prone `os`/`ioutil`
/// sinks fed by the callback path of `filepath.Walk` / `WalkDir` / `io/fs.WalkDir`.
///
/// Covers inline `FuncLit` callbacks (the common case). Named-function callbacks
/// and full SSA path-dependence remain DEFERRED.
fn check_g122_walk_call(
    pass: &Pass<'_>,
    call: &CallExpr,
    pkg: &str,
    name: &str,
    pending: &mut Vec<(u32, u32, String)>,
) {
    let cb_idx = match (pkg, name) {
        ("path/filepath", "Walk" | "WalkDir") => 1usize,
        ("io/fs", "WalkDir") => 2usize,
        _ => return,
    };
    let Some(cb_expr) = call.args.get(cb_idx) else {
        return;
    };
    let Expr::FuncLit(fl) = cb_expr else {
        return;
    };
    let Some(params) = fl.ty.params.as_ref() else {
        return;
    };
    let Some(path_param) = params
        .list
        .first()
        .and_then(|f| f.names.first())
        .map(|id| id.name.as_str())
    else {
        return;
    };
    if path_param.is_empty() || path_param == "_" {
        return;
    }
    // Upstream's `pathDependsOn` is a backward walk from the sink argument
    // through BinOp / Convert / UnOp / **call arguments**, so anything derived
    // from the callback's path is still the callback's path — `cleanPath :=
    // filepath.Clean(path)` most of all, which is what coredns
    // `plugin/auto/walk.go` writes before `os.Open(cleanPath)`. Matching the
    // parameter's name alone saw none of it.
    //
    // Forward propagation over the body in source order stands in for the
    // backward SSA walk: an assignment whose right side mentions a tainted name
    // taints what it defines.
    let mut tainted: HashSet<String> = HashSet::new();
    tainted.insert(path_param.to_string());
    let root = fl.body.pos();
    preorder(NodeRef::FuncLit(fl), |n| {
        match n {
            // Upstream scans the callback's *own* blocks
            // (`scanCallbackForRaceSinks` walks `fn.Blocks`). A nested function
            // literal is a separate `ssa.Function` there, and the callback's
            // path parameter reaches it as a free variable, not as
            // `cb.Params[0]` — so `pathDependsOn` never matches and the sink is
            // not reported. authelia's `templates/util_test.go` puts its
            // `os.ReadFile(path)` inside a `t.Run(…, func(t *testing.T){ … })`
            // for exactly this shape.
            NodeRef::FuncLit(inner) if inner.body.pos() != root => return false,
            NodeRef::AssignStmt(assign) => {
                if assign
                    .rhs
                    .iter()
                    .any(|e| {
                        tainted
                            .iter()
                            .any(|t| expr_mentions_ident_nonvariadic(pass, e, t))
                    })
                {
                    for lhs in &assign.lhs {
                        if let Expr::Ident(id) = lhs {
                            if id.name != "_" {
                                tainted.insert(id.name.clone());
                            }
                        }
                    }
                }
            }
            NodeRef::ValueSpec(spec) => {
                if spec
                    .values
                    .iter()
                    .any(|e| {
                        tainted
                            .iter()
                            .any(|t| expr_mentions_ident_nonvariadic(pass, e, t))
                    })
                {
                    for id in &spec.names {
                        if id.name != "_" {
                            tainted.insert(id.name.clone());
                        }
                    }
                }
            }
            NodeRef::CallExpr(inner) => {
                for name in &tainted {
                    if let Some(pos) = g122_sink_pos(pass, inner, name) {
                        pending.push((pos, pos, format!("G122: {G122_WHAT}")));
                        break;
                    }
                }
            }
            _ => {}
        }
        true
    });
}

const G202_WHAT: &str = "SQL string concatenation";

/// gosec's `sqlCallIdents`: the receiver's *type string* and method name, with
/// the index of the argument that carries the query.
///
/// `GetCallInfo` answers with the receiver's type for a method call, which is
/// why this is keyed the way it is rather than by the declaring package.
const SQL_CALL_IDENTS: &[(&str, &str, usize)] = &[
    ("*database/sql.Conn", "ExecContext", 1),
    ("*database/sql.Conn", "QueryContext", 1),
    ("*database/sql.Conn", "QueryRowContext", 1),
    ("*database/sql.Conn", "PrepareContext", 1),
    ("*database/sql.DB", "Exec", 0),
    ("*database/sql.DB", "ExecContext", 1),
    ("*database/sql.DB", "Query", 0),
    ("*database/sql.DB", "QueryContext", 1),
    ("*database/sql.DB", "QueryRow", 0),
    ("*database/sql.DB", "QueryRowContext", 1),
    ("*database/sql.DB", "Prepare", 0),
    ("*database/sql.DB", "PrepareContext", 1),
    ("*database/sql.Tx", "Exec", 0),
    ("*database/sql.Tx", "ExecContext", 1),
    ("*database/sql.Tx", "Query", 0),
    ("*database/sql.Tx", "QueryContext", 1),
    ("*database/sql.Tx", "QueryRow", 0),
    ("*database/sql.Tx", "QueryRowContext", 1),
    ("*database/sql.Tx", "Prepare", 0),
    ("*database/sql.Tx", "PrepareContext", 1),
];

fn sql_keyword_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(SELECT|DELETE|INSERT|UPDATE|INTO|FROM|WHERE)( |\n|\r|\t)").unwrap())
}

/// The receiver type string `GetCallInfo` would answer for a method call.
///
/// It is a small, specific set of receiver shapes, and the ones it leaves out
/// are the point: `s.GetConnection(t).QueryContext(…)` has a call for a
/// receiver whose own callee is a *selector*, which `getCallInfo` has no case
/// for, so it returns an error and the rule never runs. dapr writes four of
/// those, and asking the type checker for the receiver's type instead — which
/// answers perfectly well — reports all four.
fn g202_recv_type(pass: &Pass<'_>, file: &File, recv: &Expr) -> Option<String> {
    match recv {
        // `expr.Obj != nil && expr.Obj.Kind == ast.Var`: a variable the parser
        // resolved *in this file*. Anything else yields the identifier's name,
        // which never matches a `*database/sql.…` key.
        Expr::Ident(id) => {
            let obj = code::object_of(pass, id)?;
            let artifacts = pass.pkg().type_artifacts.as_ref()?;
            if !matches!(artifacts.objects.get(obj), ObjectData::Var(_)) {
                return None;
            }
            let pos = obj.pos(&artifacts.objects);
            if pos < file.file_start.0 as u32 || pos > file.file_end.0 as u32 {
                return None;
            }
            type_name_of(pass, recv)
        }
        // `ctx.Info.TypeOf(expr.Sel)` — the selected field or method.
        Expr::SelectorExpr(sel) => g202_object_type_name(pass, &sel.sel),
        Expr::CallExpr(inner) => match &*inner.fun {
            Expr::Ident(id) if id.name == "new" && !inner.args.is_empty() => {
                type_name_of(pass, &inner.args[0])
            }
            // `f().Method()` resolves through `f`'s declaration to its first
            // result; a *method* call there does not resolve at all.
            Expr::Ident(id) => g202_func_first_result_type(pass, file, id),
            _ => None,
        },
        _ => None,
    }
}

fn g202_object_type_name(pass: &Pass<'_>, id: &Ident) -> Option<String> {
    let obj = code::object_of(pass, id)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = obj.typ(&artifacts.objects)?;
    Some(type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    ))
}

fn g202_func_first_result_type(pass: &Pass<'_>, file: &File, id: &Ident) -> Option<String> {
    let obj = code::object_of(pass, id)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let decl_pos = obj.pos(&artifacts.objects);
    if decl_pos < file.file_start.0 as u32 || decl_pos > file.file_end.0 as u32 {
        return None;
    }
    for decl in &file.decls {
        let Decl::FuncDecl(fd) = decl else {
            continue;
        };
        if fd.name.pos().0 as u32 != decl_pos {
            continue;
        }
        let results = fd.ty.results.as_ref()?;
        let first = results.list.first()?;
        let ty = first.ty.as_ref()?;
        return type_name_of(pass, ty);
    }
    None
}

/// `findQueryArg`: the argument taking raw SQL, for a call this rule knows.
fn g202_query_arg<'a>(pass: &Pass<'_>, file: &File, call: &'a CallExpr) -> Option<&'a Expr> {
    let Expr::SelectorExpr(sel) = &*call.fun else {
        return None;
    };
    let recv = g202_recv_type(pass, file, &sel.x)?;
    let idx = SQL_CALL_IDENTS
        .iter()
        .find(|(t, m, _)| *t == recv && *m == sel.sel.name)
        .map(|(_, _, i)| *i)?;
    call.args.get(idx)
}

/// `GetBinaryExprOperands`: the leaves of a chain of binary expressions, in
/// source order. The operator is not consulted — upstream flattens any nesting.
fn binary_expr_operands<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::BinaryExpr(b) = e {
        binary_expr_operands(&b.x, out);
        binary_expr_operands(&b.y, out);
    } else {
        out.push(e);
    }
}

/// gosec's `TryResolve`: can every value in this subtree be pinned to a
/// constant?
///
/// `resolveIdent` is the load-bearing half and it is *syntactic*: an identifier
/// whose `ast.Object` is absent or is not a `Var` resolves to true, which covers
/// constants and — because Go's parser resolves objects per file — package-level
/// variables declared in another file. A `Var` is followed to its declaration,
/// and a parameter lands on an `*ast.Field`, which `TryResolve` has no case for
/// and therefore does not resolve. That is what makes `"… "+tableName` a
/// finding when `tableName` is a parameter.
fn g202_try_resolve(pass: &Pass<'_>, file: &File, e: &Expr) -> bool {
    match e {
        Expr::BasicLit(_) => true,
        Expr::BinaryExpr(b) => {
            g202_try_resolve(pass, file, &b.x) && g202_try_resolve(pass, file, &b.y)
        }
        Expr::KeyValueExpr(kv) => {
            g202_try_resolve(pass, file, &kv.key) && g202_try_resolve(pass, file, &kv.value)
        }
        Expr::IndexExpr(i) => g202_try_resolve(pass, file, &i.x),
        Expr::SliceExpr(sl) => g202_try_resolve(pass, file, &sl.x),
        Expr::CompositeLit(lit) => {
            !lit.elts.is_empty() && lit.elts.iter().all(|el| g202_try_resolve(pass, file, el))
        }
        Expr::CallExpr(_) => false,
        Expr::Ident(id) => g202_resolve_ident(pass, file, id),
        _ => false,
    }
}

fn g202_resolve_ident(pass: &Pass<'_>, file: &File, id: &guff::ast::Ident) -> bool {
    let Some(obj) = code::object_of(pass, id) else {
        return true;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return true;
    };
    if !matches!(artifacts.objects.get(obj), ObjectData::Var(_)) {
        return true;
    }
    // `ast.Object` is filled by the parser, per file: a variable declared in
    // another file of the package has none, and resolves.
    let decl_pos = obj.pos(&artifacts.objects);
    let file_start = file.file_start.0 as u32;
    let file_end = file.file_end.0 as u32;
    if decl_pos < file_start || decl_pos > file_end {
        return true;
    }
    match g202_find_decl(file, decl_pos) {
        Some(G202Decl::Assign(rhs)) => {
            !rhs.is_empty() && rhs.iter().all(|e| g202_try_resolve(pass, file, e))
        }
        Some(G202Decl::Values(values)) => {
            !values.is_empty() && values.iter().all(|e| g202_try_resolve(pass, file, e))
        }
        // A parameter, a receiver, a named result, a `range` clause: upstream
        // reaches an `*ast.Field` or an `*ast.RangeStmt`, neither of which
        // `TryResolve` answers for.
        Some(G202Decl::Unresolvable) | None => false,
    }
}

enum G202Decl<'a> {
    Assign(&'a [Expr]),
    Values(&'a [Expr]),
    Unresolvable,
}

/// The declaration `ast.Object.Decl` would point at, found by position.
fn g202_find_decl(file: &File, decl_pos: u32) -> Option<G202Decl<'_>> {
    let mut found = None;
    preorder(NodeRef::File(file), |n| {
        if found.is_some() {
            return false;
        }
        match n {
            NodeRef::AssignStmt(a) if a.tok == Some(Token::DEFINE) => {
                if a.lhs.iter().any(|l| ident_pos_is(l, decl_pos)) {
                    found = Some(G202Decl::Assign(a.rhs.as_slice()));
                }
            }
            NodeRef::ValueSpec(spec) => {
                if spec.names.iter().any(|n| n.pos().0 as u32 == decl_pos) {
                    found = Some(G202Decl::Values(spec.values.as_slice()));
                }
            }
            NodeRef::Field(f) => {
                if f.names.iter().any(|n| n.pos().0 as u32 == decl_pos) {
                    found = Some(G202Decl::Unresolvable);
                }
            }
            NodeRef::RangeStmt(r) => {
                for e in [r.key.as_ref(), r.value.as_ref()].into_iter().flatten() {
                    if ident_pos_is(e, decl_pos) {
                        found = Some(G202Decl::Unresolvable);
                    }
                }
            }
            _ => {}
        }
        true
    });
    found
}

fn ident_pos_is(e: &Expr, pos: u32) -> bool {
    matches!(e, Expr::Ident(id) if id.pos().0 as u32 == pos)
}

/// G202 — SQL string concatenation, the direct branch of upstream's
/// `sqlStrConcat.checkQuery`.
///
/// DEFERRED: the identifier branch, where the query is built up in a variable
/// (`q := "SELECT …"; q += tainted`) before the call.
fn check_g202_call(pass: &Pass<'_>, file: &File, call: &CallExpr, pending: &mut Vec<(u32, u32, String)>) {
    let Some(query) = g202_query_arg(pass, file, call) else {
        return;
    };
    if !matches!(query, Expr::BinaryExpr(_)) {
        return;
    }
    let mut operands = Vec::new();
    binary_expr_operands(query, &mut operands);
    let Some(Expr::BasicLit(first)) = operands.first().copied() else {
        return;
    };
    let Some(text) = string_lit_from_expr(&Expr::BasicLit(first.clone())) else {
        return;
    };
    if !sql_keyword_re().is_match(&text) {
        return;
    }
    for op in &operands[1..] {
        if !g202_try_resolve(pass, file, op) {
            pending.push((query.pos().0 as u32, query.end().0 as u32, format!("G202: {G202_WHAT}")));
            return;
        }
    }
}

fn g122_sink_arg_indexes(pkg: &str, name: &str) -> Option<&'static [usize]> {
    match (pkg, name) {
        (
            "os",
            "Open" | "OpenFile" | "Create" | "WriteFile" | "ReadFile" | "Remove" | "RemoveAll"
            | "Mkdir" | "MkdirAll" | "Chmod" | "Chown" | "Lchown" | "Chtimes",
        ) => Some(&[0]),
        ("os", "Rename" | "Symlink" | "Link") => Some(&[0, 1]),
        ("io/ioutil", "ReadFile" | "WriteFile") => Some(&[0]),
        _ => None,
    }
}

fn g122_sink_pos(pass: &Pass<'_>, call: &CallExpr, path_param: &str) -> Option<u32> {
    // Package funcs only — `root.Remove(path)` (*os.Root) is the safe alternative.
    if !is_pkg_sel_call(pass, call) {
        return None;
    }
    let (pkg, name) = resolve_pkg_call(pass, call)?;
    let indexes = g122_sink_arg_indexes(&pkg, &name)?;
    for &idx in indexes {
        let Some(arg) = call.args.get(idx) else {
            continue;
        };
        if expr_mentions_ident_nonvariadic(pass, arg, path_param) {
            // `s.addIssue(instr.Pos())` on an `ssa.CallInstruction`, and
            // go/ssa sets a call's pos to the CallExpr's **Lparen**, not to
            // the callee. Same for G703 (taint's `call.Pos()`); the AST rules
            // in this file report the node instead.
            return Some(call.lparen.0 as u32);
        }
    }
    None
}

/// [`expr_mentions_ident`], except that it does not look inside the arguments
/// of a **variadic** call.
///
/// Upstream's `pathDependsOn` walks a call's `ssa.Call.Args`, and go/ssa packs a
/// variadic call's arguments into a slice first — `t1 = new [2]string; *… =
/// cleanPath; t5 = slice t1[:]; t6 = filepath.Join(t5...)` — so the one argument
/// it sees is an `*ssa.Slice`, which `pathDependsOn` has no case for. The chain
/// ends there. `os.Remove(filepath.Join(path, "sub"))` is therefore not a G122
/// for gosec, and `filepath.Clean(path)` is.
fn expr_mentions_ident_nonvariadic(pass: &Pass<'_>, expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::CallExpr(c) if call_is_variadic(pass, c) => false,
        Expr::CallExpr(c) => c
            .args
            .iter()
            .any(|a| expr_mentions_ident_nonvariadic(pass, a, name)),
        Expr::ParenExpr(p) => expr_mentions_ident_nonvariadic(pass, &p.x, name),
        Expr::BinaryExpr(b) => {
            expr_mentions_ident_nonvariadic(pass, &b.x, name)
                || expr_mentions_ident_nonvariadic(pass, &b.y, name)
        }
        Expr::UnaryExpr(u) => expr_mentions_ident_nonvariadic(pass, &u.x, name),
        Expr::StarExpr(st) => expr_mentions_ident_nonvariadic(pass, &st.x, name),
        other => expr_mentions_ident(other, name),
    }
}

fn call_is_variadic(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(tv) = info.types.get(&call.fun.id()) else {
        return false;
    };
    guff_types::signature::signature_variadic(&artifacts.types, tv.typ)
}

fn expr_mentions_ident(expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::Ident(id) => id.name == name,
        Expr::ParenExpr(p) => expr_mentions_ident(&p.x, name),
        Expr::CallExpr(c) => c.args.iter().any(|a| expr_mentions_ident(a, name)),
        Expr::BinaryExpr(b) => {
            expr_mentions_ident(&b.x, name) || expr_mentions_ident(&b.y, name)
        }
        Expr::SelectorExpr(s) => expr_mentions_ident(&s.x, name),
        Expr::IndexExpr(i) => {
            expr_mentions_ident(&i.x, name) || expr_mentions_ident(&i.index, name)
        }
        Expr::UnaryExpr(u) => expr_mentions_ident(&u.x, name),
        Expr::StarExpr(s) => expr_mentions_ident(&s.x, name),
        Expr::SliceExpr(s) => {
            expr_mentions_ident(&s.x, name)
                || s.low.as_ref().is_some_and(|e| expr_mentions_ident(e, name))
                || s.high.as_ref().is_some_and(|e| expr_mentions_ident(e, name))
                || s.max.as_ref().is_some_and(|e| expr_mentions_ident(e, name))
        }
        _ => false,
    }
}

/// Resolve `(package_path, func_or_type_name)` for a call / conversion.
/// The `(selector, ident)` pair gosec's `CallList.ContainsPkgCallExpr` matches
/// a rule against — which is **not** the declaring package of the callee.
///
/// `GetCallInfo` answers with the *syntax*: a call whose receiver is an
/// identifier bound to a package gives `("rand", "Int")`, and everything else —
/// a variable, a field, a nested selector — gives the receiver's *type string*,
/// `"*math/rand.Rand"`. `ContainsPkgCallExpr` then resolves the first form
/// through the file's imports and looks it up; the second contains a `.`, so it
/// is used verbatim as the key and matches only a rule that registered a type
/// (only G110 does, through a different matcher).
///
/// So `rand.Int()` is a G404 finding and `r.r.Int()` — a method on a
/// `*rand.Rand` — is not, however clearly it is the same package's `Int`.
/// Resolving the callee's package instead reported four of those on coredns,
/// including the two in its own `plugin/pkg/rand` wrapper. `is_pkg_sel_call`
/// had already been bolted onto G107 and G114 for one observed false positive
/// each; this is the same rule, applied where upstream applies it.
fn resolve_pkg_qualified_call(pass: &Pass<'_>, call: &CallExpr) -> Option<(String, String)> {
    let Expr::SelectorExpr(sel) = &*call.fun else {
        return None;
    };
    let Expr::Ident(pkg_ident) = sel.x.as_ref() else {
        return None;
    };
    let path = imported_pkg_path(pass, pkg_ident)?;
    Some((path, sel.sel.name.clone()))
}

fn resolve_pkg_call(pass: &Pass<'_>, call: &CallExpr) -> Option<(String, String)> {
    if let Some(fq) = code::call_name(pass, &call.fun) {
        if let Some((pkg, name)) = split_fq_name(&fq) {
            return Some((cut_vendor(pkg).to_string(), name.to_string()));
        }
    }

    let Expr::SelectorExpr(sel) = &*call.fun else {
        return None;
    };

    // TypeName conversion (e.g. `unsafe.Pointer(x)`): Uses of Sel may be a type.
    if let (Some(info), Some(artifacts)) = (pass.types_info(), pass.pkg().type_artifacts.as_ref()) {
        if let Some(&obj) = info.uses.get(&sel.sel.id) {
            if let Some(pkg_id) = obj.pkg(&artifacts.objects) {
                let path = cut_vendor(artifacts.packages.get(pkg_id).path()).to_string();
                if !path.is_empty() {
                    return Some((path, sel.sel.name.clone()));
                }
            }
        }
    }

    let Expr::Ident(pkg_ident) = sel.x.as_ref() else {
        return None;
    };
    let pkg_path = imported_pkg_path(pass, pkg_ident)?;
    Some((pkg_path, sel.sel.name.clone()))
}

fn bind_all_pattern() -> &'static Regex {
    // Match upstream gosec: `^(0.0.0.0|:).*$` (dots are wildcards in Go regexp).
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(0.0.0.0|:).*$").expect("G102 pattern"))
}

fn g303_tmp_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(G303_TMP_PATTERN).expect("G303 tmp pattern"))
}

fn object_of(pass: &Pass<'_>, ident: &Ident) -> Option<ObjectId> {
    let info = pass.types_info()?;
    if let Some(obj) = info.defs.get(&ident.id).copied().flatten() {
        return Some(obj);
    }
    info.uses.get(&ident.id).copied()
}

/// G101 tuning resolved for one run (`linters.settings.gosec.config.G101`).
///
/// Holds the compiled name pattern so the regex is built once per package
/// rather than per candidate identifier.
struct G101Rt {
    name_pattern: Regex,
    ignore_entropy: bool,
    entropy_threshold: f64,
    per_char_threshold: f64,
    truncate: usize,
    min_entropy_length: usize,
}

impl G101Rt {
    fn new(opts: &crate::options::G101Options) -> Self {
        // An invalid user pattern falls back to the upstream default rather
        // than killing the run; upstream would panic in `regexp.MustCompile`,
        // but a linter that still reports the other 30 rules is more useful.
        let name_pattern = Regex::new(&opts.pattern)
            .unwrap_or_else(|_| Regex::new(G101_NAME_PATTERN).expect("G101 default name pattern"));
        Self {
            name_pattern,
            ignore_entropy: opts.ignore_entropy,
            entropy_threshold: opts.entropy_threshold,
            per_char_threshold: opts.per_char_threshold,
            truncate: opts.truncate,
            min_entropy_length: opts.min_entropy_length,
        }
    }

    fn cred_name_match(&self, name: &str) -> bool {
        self.name_pattern.is_match(name)
    }

    /// Upstream gates every G101 report on `ignoreEntropy || isHighEntropyString`.
    fn entropy_ok(&self, s: &str) -> bool {
        self.ignore_entropy || self.is_high_entropy_string(s)
    }

    fn is_high_entropy_string(&self, s: &str) -> bool {
        if s.len() < self.min_entropy_length {
            return false;
        }
        let truncated = truncate_bytes(s, self.truncate);
        if truncated.is_empty() {
            return false;
        }
        let total = crate::zxcvbn::entropy(truncated);
        let per_char = total / truncated.len() as f64;
        total >= self.entropy_threshold
            || (total >= self.entropy_threshold / 2.0 && per_char >= self.per_char_threshold)
    }

    fn is_secret_pattern(&self, s: &str) -> Option<&'static str> {
        if s.len() < self.min_entropy_length {
            return None;
        }
        for (i, re) in g101_secret_regexes().iter().enumerate() {
            if re.1.is_match(s) {
                return Some(G101_SECRET_PATTERNS[i].name);
            }
        }
        None
    }
}

fn g101_secret_regexes() -> &'static [(String, Regex)] {
    static RE: OnceLock<Vec<(String, Regex)>> = OnceLock::new();
    RE.get_or_init(|| {
        G101_SECRET_PATTERNS
            .iter()
            .map(|p| {
                (
                    p.name.to_string(),
                    Regex::new(p.re).unwrap_or_else(|e| panic!("G101 pattern {}: {e}", p.name)),
                )
            })
            .collect()
    })
}

/// `truncate(s, n)`: the first `n` **bytes**, as gosec takes them.
///
/// Go slices bytes and does not care whether the cut lands inside a character;
/// `&str` does, so a cut that would split one steps back to the boundary. Only
/// a credential whose first 16 bytes contain a multi-byte character can tell
/// the difference, and the entropy either side of it stays on the same side of
/// gosec's thresholds (measured over 20k corpus strings: 4 differ in value,
/// none in verdict).
fn truncate_bytes(s: &str, n: usize) -> &str {
    if s.len() <= n {
        return s;
    }
    let mut end = n;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn report_g101(
    pending: &mut Vec<(u32, u32, String)>,
    pos: u32,
    end: u32,
    pattern_name: Option<&str>,
) {
    let msg = match pattern_name {
        Some(p) => format!("G101: {G101_WHAT}: {p}"),
        None => format!("G101: {G101_WHAT}"),
    };
    pending.push((pos, end, msg));
}

fn check_cred_value(
    rt: &G101Rt,
    pending: &mut Vec<(u32, u32, String)>,
    pos: u32,
    end: u32,
    name_matched: bool,
    value: &str,
) -> bool {
    if name_matched {
        if rt.entropy_ok(value) {
            report_g101(pending, pos, end, None);
            return true;
        }
    } else if rt.entropy_ok(value) {
        if let Some(pattern) = rt.is_secret_pattern(value) {
            report_g101(pending, pos, end, Some(pattern));
            return true;
        }
    }
    false
}

/// `ast.Node.Pos()` for the three node kinds this file reports but that guff's
/// AST only exposes as structs (the `pos()` methods live on the `Expr` / `Stmt`
/// / `Spec` enums). gosec's `NewIssue(node, …)` takes `node.Pos()`, so getting
/// these wrong shows up as a column diff and nothing else.
fn assign_pos(assign: &AssignStmt) -> u32 {
    assign
        .lhs
        .first()
        .map(|e| e.pos())
        .unwrap_or(assign.tok_pos)
        .0 as u32
}

fn composite_lit_pos(lit: &CompositeLit) -> u32 {
    lit.ty.as_ref().map(|t| t.pos()).unwrap_or(lit.lbrace).0 as u32
}

fn import_spec_pos(imp: &ImportSpec) -> u32 {
    imp.name
        .as_ref()
        .map(|n| n.pos())
        .unwrap_or(imp.path.value_pos)
        .0 as u32
}

/// Upstream reports `assign` — `AssignStmt.Pos()`, i.e. the first LHS operand,
/// not the `=`/`:=` token.
fn check_g101_assign(rt: &G101Rt, assign: &AssignStmt, pending: &mut Vec<(u32, u32, String)>) {
    for lhs in &assign.lhs {
        let Expr::Ident(ident) = lhs else {
            continue;
        };
        let name_matched = rt.cred_name_match(&ident.name);
        if name_matched {
            for rhs in &assign.rhs {
                if let Some(val) = string_lit_from_expr(rhs) {
                    if check_cred_value(rt, pending, assign_pos(assign), node_end(NodeRef::AssignStmt(assign)).0 as u32, true, &val) {
                        return;
                    }
                }
            }
        }
        for rhs in &assign.rhs {
            if let Some(val) = string_lit_from_expr(rhs) {
                if check_cred_value(rt, pending, assign_pos(assign), node_end(NodeRef::AssignStmt(assign)).0 as u32, false, &val) {
                    return;
                }
            }
        }
    }
}

fn check_g101_value_spec(rt: &G101Rt, spec: &ValueSpec, pending: &mut Vec<(u32, u32, String)>) {
    let pos = spec.names.first().map(|n| n.pos().0 as u32).unwrap_or(0);
    let end = node_end(NodeRef::ValueSpec(spec)).0 as u32;
    for (index, ident) in spec.names.iter().enumerate() {
        if !rt.cred_name_match(&ident.name) || spec.values.is_empty() {
            continue;
        }
        let idx = if index < spec.values.len() {
            index
        } else {
            spec.values.len() - 1
        };
        if let Some(val) = string_lit_from_expr(&spec.values[idx]) {
            if check_cred_value(rt, pending, pos, end, true, &val) {
                return;
            }
        }
    }
    for value in &spec.values {
        if let Some(val) = string_lit_from_expr(value) {
            if check_cred_value(rt, pending, pos, end, false, &val) {
                return;
            }
        }
    }
}

fn check_g101_equality(rt: &G101Rt, bin: &BinaryExpr, pending: &mut Vec<(u32, u32, String)>) {
    if bin.op != Token::EQL && bin.op != Token::NEQ {
        return;
    }
    // `ctx.NewIssue(binaryExpr, …)`: a BinaryExpr's `Pos()` is `X.Pos()`, so
    // `password == "…"` is reported on the `password`, not on the `==`.
    let pos = bin.x.pos().0 as u32;
    let end = bin.y.end().0 as u32;

    let (ident, value_node) = match (bin.x.as_ref(), bin.y.as_ref()) {
        (Expr::Ident(id), other) => (Some(id), other),
        (other, Expr::Ident(id)) => (Some(id), other),
        _ => (None, bin.y.as_ref()),
    };
    if let Some(ident) = ident {
        if rt.cred_name_match(&ident.name) {
            if let Some(val) = string_lit_from_expr(value_node) {
                if check_cred_value(rt, pending, pos, end, true, &val) {
                    return;
                }
            }
        }
    }

    let lit = match (bin.x.as_ref(), bin.y.as_ref()) {
        (Expr::BasicLit(lit), _) if lit.kind == Some(Token::STRING) => Some(lit),
        (_, Expr::BasicLit(lit)) if lit.kind == Some(Token::STRING) => Some(lit),
        _ => None,
    };
    if let Some(lit) = lit {
        if let Some(val) = unquote_string_lit(&lit.value) {
            if check_cred_value(rt, pending, pos, end, false, &val) {
                return;
            }
        }
    }
}

fn check_g101_composite(rt: &G101Rt, lit: &CompositeLit, pending: &mut Vec<(u32, u32, String)>) {
    // `ctx.NewIssue(lit, …)`, and `CompositeLit.Pos()` is `Type.Pos()` when the
    // type is written — `&corev1api.SecretVolumeSource{…}` reports at the `S`,
    // not at the `{` four columns further on. `composite_lit_pos` is the same
    // helper G112 already uses.
    let pos = composite_lit_pos(lit);
    let end = lit.rbrace.0 as u32;
    for elt in &lit.elts {
        let Expr::KeyValueExpr(kv) = elt else {
            continue;
        };
        let mut matched_key = false;
        if let Expr::Ident(id) = kv.key.as_ref() {
            if rt.cred_name_match(&id.name) {
                matched_key = true;
            }
        }
        if let Some(key_str) = string_lit_from_expr(kv.key.as_ref()) {
            if rt.cred_name_match(&key_str) {
                matched_key = true;
            }
        }
        if matched_key {
            if let Some(val) = string_lit_from_expr(kv.value.as_ref()) {
                if check_cred_value(rt, pending, pos, end, true, &val) {
                    return;
                }
            }
        }
        if let Some(val) = string_lit_from_expr(kv.value.as_ref()) {
            if check_cred_value(rt, pending, pos, end, false, &val) {
                return;
            }
        }
    }
}

fn check_g101(
    pass: &Pass<'_>,
    enabled: &HashSet<&'static str>,
    opts: &GosecOptions,
    pending: &mut Vec<(u32, u32, String)>,
) {
    if !enabled.contains("G101") {
        return;
    }
    let rt = G101Rt::new(&opts.g101);
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::AssignStmt(a) => check_g101_assign(&rt, a, pending),
                NodeRef::ValueSpec(v) => check_g101_value_spec(&rt, v, pending),
                NodeRef::BinaryExpr(b) => check_g101_equality(&rt, b, pending),
                NodeRef::CompositeLit(c) => check_g101_composite(&rt, c, pending),
                _ => {}
            }
            true
        });
    }
}

fn string_lit_from_expr(expr: &Expr) -> Option<String> {
    match expr {
        Expr::BasicLit(lit) if lit.kind == Some(Token::STRING) => unquote_string_lit(&lit.value),
        _ => None,
    }
}

fn is_resolvable_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::BasicLit(lit)
            if matches!(
                lit.kind,
                Some(Token::STRING | Token::CHAR | Token::INT | Token::FLOAT | Token::IMAG)
            )
    )
}

/// Upstream `gosec.GetInt`: parse `ast.BasicLit` INT with base 0 (`0o755`, `0755`, `493`).
fn get_int(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::BasicLit(lit) if lit.kind == Some(Token::INT) => parse_go_int_lit(&lit.value),
        _ => None,
    }
}

fn parse_go_int_lit(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // Go allows underscores in numeric literals; strip them.
    let cleaned: String = s.chars().filter(|&c| c != '_').collect();
    if let Some(hex) = cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
    {
        return i64::from_str_radix(hex, 16).ok();
    }
    if let Some(bin) = cleaned
        .strip_prefix("0b")
        .or_else(|| cleaned.strip_prefix("0B"))
    {
        return i64::from_str_radix(bin, 2).ok();
    }
    if let Some(oct) = cleaned
        .strip_prefix("0o")
        .or_else(|| cleaned.strip_prefix("0O"))
    {
        return i64::from_str_radix(oct, 8).ok();
    }
    // Legacy octal: leading 0 with only octal digits (not a single "0").
    if cleaned.len() > 1
        && cleaned.starts_with('0')
        && cleaned.chars().all(|c| matches!(c, '0'..='7'))
    {
        return i64::from_str_radix(&cleaned, 8).ok();
    }
    cleaned.parse::<i64>().ok()
}

fn mode_is_subset(subset: i64, superset: i64) -> bool {
    (subset | superset) == superset
}

/// Upstream `isOsPerm`: `os.ModePerm` always fails the permission check.
fn is_os_mode_perm(expr: &Expr) -> bool {
    let Expr::SelectorExpr(sel) = expr else {
        return false;
    };
    let Expr::Ident(x) = sel.x.as_ref() else {
        return false;
    };
    x.name == "os" && sel.sel.name == "ModePerm"
}

fn format_octal_mode(mode: i64) -> String {
    // Match Go `%#o` for these masks: leading 0 + octal digits.
    format!("0{mode:o}")
}

fn type_name_of(pass: &Pass<'_>, expr: &Expr) -> Option<String> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let tav = info.types.get(&expr.id())?;
    Some(type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        tav.typ,
        None,
    ))
}

fn is_tls_config_type_name(name: &str) -> bool {
    let bare = name.strip_prefix('*').unwrap_or(name);
    bare == "crypto/tls.Config"
}

/// Upstream G402 `requiredType` is exactly `crypto/tls.Config` (no pointer).
/// Assignments through `*tls.Config` (the common `cfg := new(tls.Config)` case)
/// are intentionally not matched by gosec v2.26 / golangci 2.12.
fn is_tls_config_assign_type_name(name: &str) -> bool {
    name == "crypto/tls.Config"
}

fn is_http_server_type_name(name: &str) -> bool {
    let bare = name.strip_prefix('*').unwrap_or(name);
    bare == "net/http.Server"
}

fn resolve_bool_const(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::Ident(id) if id.name == "true" => Some(true),
        Expr::Ident(id) if id.name == "false" => Some(false),
        Expr::UnaryExpr(u) if u.op == Token::NOT => resolve_bool_const(&u.x).map(|v| !v),
        _ => None,
    }
}

/// `pkg.Func(...)` where `pkg` is an import name (PkgName), not a method recv.
fn is_pkg_sel_call(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return false;
    };
    let Expr::Ident(id) = sel.x.as_ref() else {
        return false;
    };
    imported_pkg_path(pass, id).is_some()
}

/// True when a `#nosec` / `//nosec` / `gosec:disable` suppresses `rule`.
///
/// Matches gosec CommentMap association: same-line trailing comments, and a
/// short preceding comment block (e.g. `// #nosec G124` one or two lines above).
///
/// Uses in-memory [`Package::source_bytes`] — never re-reads the filesystem
/// (cold-wall sensitive; production parses without `PARSE_COMMENTS`).
/// The line ranges a `#nosec` directive suppresses, per file.
///
/// Upstream builds an `ast.CommentMap` and asks it which node each comment
/// belongs to, then records **that node's line range** as ignored
/// (`analyzer.go`'s `ignores.add` / `ignores.get`). So a directive written
/// *inside* a composite literal covers the whole literal, including the
/// position the finding is reported at — which is the literal's own `Pos()`,
/// several lines above the comment.
///
/// velero's `pkg/install/daemonset.go` is exactly that:
///
/// ```go
/// Secret: &corev1api.SecretVolumeSource{        // ← G101 is reported here
///     DefaultMode: ptr.To(int32(0444)),
///     // #nosec G101 -- a Secret resource name, not a credential
///     SecretName: "cloud-credentials",
/// },
/// ```
///
/// Looking only *backwards* from the reported line, as the fallback below
/// does, never sees it.
#[derive(Default)]
struct NosecRanges {
    /// (file, first line, last line, directive args)
    ranges: Vec<(String, i64, i64, String)>,
}

impl NosecRanges {
    /// Reparse each file for comments — the shared load drops them — build the
    /// comment map against that tree, and record the line range of every node a
    /// `#nosec` attaches to. Line numbers are the same in both parses because
    /// the bytes are.
    fn build(pass: &Pass<'_>) -> Self {
        use guff::commentmap::{new_comment_map, node_end, node_pos};
        use guff::parser::{parse_file, PARSE_COMMENTS};
        use guff::position::FileSet;

        let mut out = NosecRanges::default();
        for (index, path) in pass.pkg().compiled_go_files.iter().enumerate() {
            let owned;
            let src: &[u8] = match pass.pkg().source_bytes(index) {
                Some(b) => b,
                None => match std::fs::read(path) {
                    Ok(b) => {
                        owned = b;
                        &owned
                    }
                    Err(_) => continue,
                },
            };
            // Cheap filter: almost no file carries a directive, and the
            // reparse below is the expensive part.
            if !src.windows(5).any(|w| w == b"nosec") {
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let rfset = FileSet::new();
            let Ok(rfile) = parse_file(&rfset, name, src, PARSE_COMMENTS) else {
                continue;
            };
            let cmap = new_comment_map(&rfset, NodeRef::File(&rfile), &rfile.comments);
            let mut ranges: Vec<(i64, i64, String)> = Vec::new();
            preorder(NodeRef::File(&rfile), |n| {
                let Some(groups) = cmap.get(n) else {
                    return true;
                };
                for group in groups {
                    let Some(first) = group.list.first() else {
                        continue;
                    };
                    let Some(args) = find_nosec_directive(group) else {
                        continue;
                    };
                    // `updateIgnoredRulesForNode`: the recorded range is the
                    // union of the node's and the comment group's, so a
                    // directive that trails the node it belongs to still
                    // covers its own line.
                    let mut start = node_pos(n);
                    let mut end = node_end(n);
                    if first.pos() < start {
                        start = first.pos();
                    }
                    let group_end = group.list.last().map(|c| c.end()).unwrap_or(end);
                    if group_end > end {
                        end = group_end;
                    }
                    ranges.push((
                        rfset.position(start).line,
                        rfset.position(end).line,
                        args,
                    ));
                }
                true
            });
            for (start, end, text) in ranges {
                out.ranges.push((name.to_string(), start, end, text));
            }
        }
        out
    }

    /// `ignores.get`: the recorded range and the issue's own range match when
    /// either contains the other. The second half is what makes velero work —
    /// the directive sits *inside* the composite literal the finding is
    /// reported on, so the issue's range is the wider of the two.
    fn suppresses(&self, file: &str, start: i64, end: i64, rule: &str) -> bool {
        self.ranges.iter().any(|(f, ig_start, ig_end, text)| {
            (f == file || same_basename(f, file))
                && ((*ig_start <= start && *ig_end >= end)
                    || (start <= *ig_start && end >= *ig_end))
                && directive_suppresses(text, rule)
        })
    }
}

fn same_basename(a: &str, b: &str) -> bool {
    std::path::Path::new(a).file_name() == std::path::Path::new(b).file_name()
}

/// `findNoSecTag`: the tag counts only at the very start of the group's text
/// or at the start of one of its lines. A prose mention such as
/// "a `#nosec` comment suppresses …" is not a directive — which is why this is
/// a substring search with a position test rather than `contains`.
fn find_nosec_tag<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(rest) = text.strip_prefix(tag) {
        return Some(rest);
    }
    let idx = text.find(tag).filter(|i| *i > 0)?;
    for (i, b) in text.as_bytes()[..idx].iter().enumerate().rev() {
        let _ = i;
        if *b == b'\n' {
            return Some(&text[idx + tag.len()..]);
        }
        if *b != b' ' && *b != b'\t' {
            break;
        }
    }
    None
}

/// `findNoSecDirective`: `#nosec` anywhere the tag rule allows, or a
/// `//gosec:disable` comment. The latter is checked over the raw comments
/// because `CommentGroup.Text()` drops directive-shaped lines.
fn find_nosec_directive(group: &guff::ast::CommentGroup) -> Option<String> {
    if let Some(args) = find_nosec_tag(&group.text(), NOSEC_TAG) {
        return Some(args.to_string());
    }
    for c in &group.list {
        if let Some(after) = c.text.strip_prefix(GOSEC_DISABLE_PREFIX) {
            if after.is_empty() || after.starts_with(' ') {
                return Some(after.trim().to_string());
            }
        }
    }
    None
}

/// The rule-id half of `astVisitor.ignore`: strip the `-- justification`,
/// then scan for `G` followed by exactly three digits. An empty directive —
/// including one that is empty only because everything after it was a
/// justification — suppresses *every* rule.
fn directive_suppresses(args: &str, rule: &str) -> bool {
    let mut args = args;
    if let Some(idx) = args.find("--") {
        args = &args[..idx];
    }
    let directive = args.trim();
    if directive.is_empty() || directive == "block" {
        return true;
    }
    let bytes = directive.as_bytes();
    let mut found_any = false;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'G' && i + 4 <= bytes.len() {
            let id = &directive[i..i + 4];
            if id.as_bytes()[1..4].iter().all(|b| b.is_ascii_digit()) {
                found_any = true;
                if id == rule {
                    return true;
                }
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    // `#nosec somethingElse` names no rule at all, and upstream falls back to
    // "all rules" (`if len(ignores) == 0`).
    !found_any
}

const NOSEC_TAG: &str = "#nosec";
const GOSEC_DISABLE_PREFIX: &str = "//gosec:disable";

fn check_g402_tls_field(
    field: &str,
    value: &Expr,
    report_pos: u32,
    pending: &mut Vec<(u32, u32, String)>,
) {
    if field != "InsecureSkipVerify" {
        return;
    }
    match resolve_bool_const(value) {
        Some(true) => pending.push((
            report_pos,
            report_pos,
            "G402: TLS InsecureSkipVerify set to true.".to_string(),
        )),
        None => pending.push((
            report_pos,
            report_pos,
            "G402: TLS InsecureSkipVerify may be set to true.".to_string(),
        )),
        Some(false) => {}
    }
}

fn check_g402_composite(
    pass: &Pass<'_>,
    lit: &CompositeLit,
    enabled: &HashSet<&'static str>,
    pending: &mut Vec<(u32, u32, String)>,
) {
    if !enabled.contains("G402") {
        return;
    }
    let Some(ty) = lit.ty.as_ref() else {
        return;
    };
    let Some(name) = type_name_of(pass, ty) else {
        // AST fallback: `tls.Config{…}` when types info is incomplete.
        let is_tls_config = match ty.as_ref() {
            Expr::SelectorExpr(sel) => {
                sel.sel.name == "Config"
                    && matches!(
                        sel.x.as_ref(),
                        Expr::Ident(id) if imported_pkg_path(pass, id).as_deref() == Some("crypto/tls")
                    )
            }
            _ => false,
        };
        if !is_tls_config {
            return;
        }
        for elt in &lit.elts {
            let Expr::KeyValueExpr(kv) = elt else {
                continue;
            };
            let Expr::Ident(key) = kv.key.as_ref() else {
                continue;
            };
            check_g402_tls_field(
                &key.name,
                kv.value.as_ref(),
                kv.value.pos().0 as u32,
                pending,
            );
        }
        return;
    };
    if !is_tls_config_type_name(&name) {
        return;
    }
    for elt in &lit.elts {
        let Expr::KeyValueExpr(kv) = elt else {
            continue;
        };
        let Expr::Ident(key) = kv.key.as_ref() else {
            continue;
        };
        check_g402_tls_field(
            &key.name,
            kv.value.as_ref(),
            kv.value.pos().0 as u32,
            pending,
        );
    }
}

fn check_g402_assign(
    pass: &Pass<'_>,
    assign: &AssignStmt,
    enabled: &HashSet<&'static str>,
    pending: &mut Vec<(u32, u32, String)>,
) {
    if !enabled.contains("G402") || assign.lhs.is_empty() || assign.rhs.is_empty() {
        return;
    }
    let Expr::SelectorExpr(sel) = &assign.lhs[0] else {
        return;
    };
    let Some(name) = type_name_of(pass, &sel.x) else {
        return;
    };
    if !is_tls_config_assign_type_name(&name) {
        return;
    }
    check_g402_tls_field(
        &sel.sel.name,
        &assign.rhs[0],
        assign.rhs[0].pos().0 as u32,
        pending,
    );
}

fn composite_has_timeout_field(lit: &CompositeLit) -> bool {
    for elt in &lit.elts {
        let Expr::KeyValueExpr(kv) = elt else {
            continue;
        };
        let Expr::Ident(key) = kv.key.as_ref() else {
            continue;
        };
        if key.name == "ReadHeaderTimeout" || key.name == "ReadTimeout" {
            return true;
        }
    }
    false
}

fn is_http_cookie_type_name(name: &str) -> bool {
    let bare = name.strip_prefix('*').unwrap_or(name);
    bare == "net/http.Cookie"
}

const G124_WHAT: &str =
    "http.Cookie missing or has insecure Secure, HttpOnly, or SameSite attribute";

/// Whether `expr` is `http.SameSiteLaxMode` or `http.SameSiteStrictMode`.
fn is_safe_samesite(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Expr::SelectorExpr(sel) = expr else {
        return false;
    };
    if sel.sel.name != "SameSiteLaxMode" && sel.sel.name != "SameSiteStrictMode" {
        return false;
    }
    matches!(
        sel.x.as_ref(),
        Expr::Ident(id) if imported_pkg_path(pass, id).as_deref() == Some("net/http")
    )
}

fn is_http_cookie_type_expr(pass: &Pass<'_>, ty: &Expr) -> bool {
    if let Some(name) = type_name_of(pass, ty) {
        return is_http_cookie_type_name(&name);
    }
    match ty {
        Expr::SelectorExpr(sel) => {
            sel.sel.name == "Cookie"
                && matches!(
                    sel.x.as_ref(),
                    Expr::Ident(id) if imported_pkg_path(pass, id).as_deref() == Some("net/http")
                )
        }
        Expr::StarExpr(star) => is_http_cookie_type_expr(pass, &star.x),
        _ => false,
    }
}

#[derive(Default)]
struct CookieSecurity {
    secure_ok: bool,
    http_only_ok: bool,
    same_site_ok: bool,
}

impl CookieSecurity {
    fn is_secure(&self) -> bool {
        self.secure_ok && self.http_only_ok && self.same_site_ok
    }

    fn record_field(&mut self, pass: &Pass<'_>, field: &str, value: &Expr) {
        match field {
            "Secure" => {
                self.secure_ok = resolve_bool_const(value) == Some(true);
            }
            "HttpOnly" => {
                self.http_only_ok = resolve_bool_const(value) == Some(true);
            }
            "SameSite" => {
                // SSA G124 only upgrades to safe (Lax/Strict); an unsafe value
                // does not clear a prior safe SameSite (gosec insecure_cookie).
                if is_safe_samesite(pass, value) {
                    self.same_site_ok = true;
                }
            }
            _ => {}
        }
    }
}

fn cookie_security_from_composite(pass: &Pass<'_>, lit: &CompositeLit) -> CookieSecurity {
    let mut sec = CookieSecurity::default();
    for elt in &lit.elts {
        let Expr::KeyValueExpr(kv) = elt else {
            continue;
        };
        let Expr::Ident(key) = kv.key.as_ref() else {
            continue;
        };
        sec.record_field(pass, &key.name, &kv.value);
    }
    sec
}

fn check_g124_composite(
    pass: &Pass<'_>,
    lit: &CompositeLit,
    enabled: &HashSet<&'static str>,
    pending: &mut Vec<(u32, u32, String)>,
) {
    if !enabled.contains("G124") {
        return;
    }
    let Some(ty) = lit.ty.as_deref() else {
        return;
    };
    if !is_http_cookie_type_expr(pass, ty) {
        return;
    }
    let mut sec = cookie_security_from_composite(pass, lit);
    if !sec.is_secure() {
        // Upstream SSA G124 folds later field stores on the same allocation
        // (non-path-sensitive). Match that so `Secure: false` followed by
        // `cookie.Secure = true` is not reported (caddy CookieHashSelection).
        // Scope the binding search to the enclosing function body only —
        // a whole-package preorder per cookie lit is far too expensive.
        if let Some(body) = enclosing_func_body(pass, lit.lbrace.0 as u32) {
            if let Some(name) = cookie_composite_binding_name(body, lit.lbrace.0 as u32) {
                collect_cookie_param_stores(pass, body, &name, &mut sec);
            }
        }
    }
    if sec.is_secure() {
        return;
    }
    pending.push((lit.lbrace.0 as u32, lit.rbrace.0 as u32, format!("G124: {G124_WHAT}")));
}

/// If `lit` is the RHS of `name := &http.Cookie{…}` / `name = &http.Cookie{…}`
/// inside `body`, return `name`.
fn cookie_composite_binding_name(body: &guff::ast::BlockStmt, lit_pos: u32) -> Option<String> {
    let mut found = None;
    preorder(NodeRef::BlockStmt(body), |n| {
        if found.is_some() {
            return false;
        }
        match n {
            NodeRef::AssignStmt(assign) => {
                for (lhs, rhs) in assign.lhs.iter().zip(assign.rhs.iter()) {
                    if cookie_lit_under(rhs, lit_pos) {
                        if let Expr::Ident(id) = lhs {
                            found = Some(id.name.clone());
                        }
                    }
                }
            }
            NodeRef::ValueSpec(spec) => {
                for (name, val) in spec.names.iter().zip(spec.values.iter()) {
                    if cookie_lit_under(val, lit_pos) {
                        found = Some(name.name.clone());
                    }
                }
            }
            _ => {}
        }
        true
    });
    found
}

fn cookie_lit_under(expr: &Expr, lit_pos: u32) -> bool {
    match expr {
        Expr::UnaryExpr(u) if u.op == Token::AND => cookie_lit_under(&u.x, lit_pos),
        Expr::ParenExpr(p) => cookie_lit_under(&p.x, lit_pos),
        Expr::CompositeLit(cl) => cl.lbrace.0 as u32 == lit_pos,
        _ => false,
    }
}

fn enclosing_func_body<'a>(
    pass: &'a Pass<'_>,
    pos: u32,
) -> Option<&'a guff::ast::BlockStmt> {
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(func) = decl else {
                continue;
            };
            let Some(body) = func.body.as_ref() else {
                continue;
            };
            let start = body.lbrace.0 as u32;
            let end = body.rbrace.0 as u32;
            if pos >= start && pos <= end {
                return Some(body);
            }
        }
    }
    None
}

/// Track `*http.Cookie` parameters: SSA G124 flags params whose Secure /
/// HttpOnly / SameSite are never set to safe values before use (gin
/// `SetCookieData`).
fn check_g124_cookie_params(
    pass: &Pass<'_>,
    enabled: &HashSet<&'static str>,
    pending: &mut Vec<(u32, u32, String)>,
) {
    if !enabled.contains("G124") {
        return;
    }
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(func) = decl else {
                continue;
            };
            let Some(body) = func.body.as_ref() else {
                continue;
            };
            let Some(params) = func.ty.params.as_ref() else {
                continue;
            };
            for field in &params.list {
                let Some(ty) = field.ty.as_ref() else {
                    continue;
                };
                // Prefer `*http.Cookie` (pointer receivers of the cookie value).
                let Expr::StarExpr(star) = ty else {
                    continue;
                };
                if !is_http_cookie_type_expr(pass, &star.x) {
                    continue;
                }
                for name in &field.names {
                    if name.name == "_" {
                        continue;
                    }
                    let mut sec = CookieSecurity::default();
                    collect_cookie_param_stores(pass, body, &name.name, &mut sec);
                    if !sec.is_secure() {
                        pending.push((name.name_pos.0 as u32, name.end().0 as u32, format!("G124: {G124_WHAT}")));
                    }
                }
            }
        }
    }
}

fn collect_cookie_param_stores(
    pass: &Pass<'_>,
    body: &guff::ast::BlockStmt,
    param: &str,
    sec: &mut CookieSecurity,
) {
    preorder(NodeRef::BlockStmt(body), |n| {
        if let NodeRef::AssignStmt(assign) = n {
            for (lhs, rhs) in assign.lhs.iter().zip(assign.rhs.iter()) {
                let Expr::SelectorExpr(sel) = lhs else {
                    continue;
                };
                let Expr::Ident(base) = sel.x.as_ref() else {
                    continue;
                };
                if base.name != param {
                    continue;
                }
                sec.record_field(pass, &sel.sel.name, rhs);
            }
        }
        true
    });
}

fn check_g112_composite(
    pass: &Pass<'_>,
    lit: &CompositeLit,
    enabled: &HashSet<&'static str>,
    pending: &mut Vec<(u32, u32, String)>,
) {
    if !enabled.contains("G112") {
        return;
    }
    let Some(ty) = lit.ty.as_ref() else {
        return;
    };
    let is_server = match type_name_of(pass, ty) {
        Some(name) => is_http_server_type_name(&name),
        None => match ty.as_ref() {
            Expr::SelectorExpr(sel) => {
                sel.sel.name == "Server"
                    && matches!(
                        sel.x.as_ref(),
                        Expr::Ident(id) if imported_pkg_path(pass, id).as_deref() == Some("net/http")
                    )
            }
            _ => false,
        },
    };
    if !is_server {
        return;
    }
    if !composite_has_timeout_field(lit) {
        // `NewIssue(node, …)` on the CompositeLit: its Pos() is the type, not
        // the `{`.
        pending.push((composite_lit_pos(lit), lit.rbrace.0 as u32, format!("G112: {G112_WHAT}")));
    }
}

/// Upstream `findTempDirArgs`: string `/tmp` path, `os.TempDir()`, or nested Join/concat.
fn find_temp_dir_args(pass: &Pass<'_>, suspect: &Expr) -> bool {
    if let Some(s) = string_lit_from_expr(suspect) {
        return g303_tmp_pattern().is_match(&s);
    }
    if let Expr::CallExpr(call) = suspect {
        if let Some((pkg, name)) = resolve_pkg_call(pass, call) {
            if pkg == "os" && name == "TempDir" {
                return true;
            }
            if (pkg == "path" || pkg == "path/filepath") && name == "Join" && !call.args.is_empty()
            {
                return find_temp_dir_args(pass, &call.args[0]);
            }
        }
    }
    if let Expr::BinaryExpr(be) = suspect {
        if be.op == Token::ADD {
            return find_temp_dir_args(pass, be.x.as_ref());
        }
    }
    false
}

fn g107_url_tainted(pass: &Pass<'_>, arg: &Expr) -> bool {
    // Upstream gosec `ResolveVar` only inspects *ast.Ident URL args; calls,
    // selectors, and binary exprs (e.g. `srv.URL+"/x"`, `fmt.Sprintf(...)`)
    // are not flagged.
    let Expr::Ident(ident) = arg else {
        return false;
    };
    // Literal URL via const folding elsewhere is handled by Const below.
    if string_lit_from_expr(arg).is_some() {
        return false;
    }
    let Some(obj) = object_of(pass, ident) else {
        return true;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return true;
    };
    match artifacts.objects.get(obj) {
        // const url = "…" is safe.
        ObjectData::Const(_) => false,
        // Vars (package-level, params, locals): treat as tainted.
        // DEFERRED: local string-lit init TryResolve (upstream marks those safe).
        ObjectData::Var(_) => true,
        _ => true,
    }
}

fn check_g109_assign(
    pass: &Pass<'_>,
    assign: &AssignStmt,
    atoi_vars: &mut HashSet<ObjectId>,
) {
    for expr in &assign.rhs {
        let Expr::CallExpr(call) = expr else {
            continue;
        };
        let Some((pkg, name)) = resolve_pkg_call(pass, call) else {
            continue;
        };
        if pkg != G109_ATOI.0 || name != G109_ATOI.1 {
            continue;
        }
        if let Some(Expr::Ident(id)) = assign.lhs.first() {
            if id.name != "_" {
                if let Some(obj) = object_of(pass, id) {
                    atoi_vars.insert(obj);
                }
            }
        }
    }
}

fn check_g109_call(
    pass: &Pass<'_>,
    call: &CallExpr,
    atoi_vars: &HashSet<ObjectId>,
    pending: &mut Vec<(u32, u32, String)>,
) {
    // int16(x) / int32(x) conversions.
    let Expr::Ident(fun) = call.fun.as_ref() else {
        return;
    };
    if fun.name != "int16" && fun.name != "int32" {
        return;
    }
    let Some(arg) = call.args.first() else {
        return;
    };
    let Expr::Ident(id) = arg else {
        return;
    };
    let Some(obj) = object_of(pass, id) else {
        return;
    };
    if atoi_vars.contains(&obj) {
        pending.push((call.pos().0 as u32, call.end().0 as u32, format!("G109: {G109_WHAT}")));
    }
}

fn check_g109(pass: &Pass<'_>, enabled: &HashSet<&'static str>, pending: &mut Vec<(u32, u32, String)>) {
    if !enabled.contains("G109") {
        return;
    }
    let mut atoi_vars: HashSet<ObjectId> = HashSet::new();
    // First pass: collect Atoi results; second: flag conversions.
    // Upstream walks in source order with stateful PassedValues; a two-pass
    // scan is equivalent for the common same-function pattern.
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::AssignStmt(a) = n {
                check_g109_assign(pass, a, &mut atoi_vars);
            }
            true
        });
    }
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::CallExpr(c) = n {
                check_g109_call(pass, c, &atoi_vars, pending);
            }
            true
        });
    }
}

/// The node the parser would hang off an identifier's `Obj.Decl`, for the two
/// kinds `TryResolve`'s switch knows how to walk.
enum DeclNode<'a> {
    Assign(&'a AssignStmt),
    Value(&'a ValueSpec),
}

/// One file's view of gosec's `ast.Ident.Obj` graph, which is what
/// `resolve.go` walks.
///
/// Upstream reads the **parser's** per-file object resolution, so an
/// identifier whose declaration lives in another file of the same package has
/// `Obj == nil`, and `resolveIdent` calls that *resolved* (returns true).
/// guff's type information is package-wide and would happily follow the link,
/// reporting where gosec is silent — so the lookup is deliberately file-local:
/// a definition this file does not contain is a miss, and a miss means
/// "no Obj".
struct FileDecls<'a> {
    /// Defining-ident position → its declaration, when `TryResolve` handles it.
    decls: HashMap<u32, DeclNode<'a>>,
    /// Every object defined in this file. A hit here without an entry in
    /// `decls` is a parameter, a range clause, a type switch … — an
    /// `Obj.Decl` the switch does not cover, so `TryResolve` returns false.
    defined_here: HashSet<u32>,
    /// `(lbrace, rbrace_end)` of every function body, in preorder. Upstream's
    /// `getEnclosingBodyStart` keeps the **last** enclosing match, which for a
    /// preorder walk is the innermost one.
    bodies: Vec<(u32, u32)>,
}

impl<'a> FileDecls<'a> {
    fn build(pass: &Pass<'_>, file: &'a guff::ast::File) -> Self {
        let mut out = FileDecls {
            decls: HashMap::new(),
            defined_here: HashSet::new(),
            bodies: Vec::new(),
        };
        let Some(info) = pass.types_info() else {
            return out;
        };
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::Ident(id) => {
                    if info.defs.contains_key(&id.id) {
                        out.defined_here.insert(id.pos().0 as u32);
                    }
                }
                NodeRef::AssignStmt(a) => {
                    if a.tok == Some(Token::DEFINE) {
                        for lhs in &a.lhs {
                            if let Expr::Ident(id) = lhs {
                                out.decls.insert(id.pos().0 as u32, DeclNode::Assign(a));
                            }
                        }
                    }
                }
                NodeRef::ValueSpec(vs) => {
                    for id in &vs.names {
                        out.decls.insert(id.pos().0 as u32, DeclNode::Value(vs));
                    }
                }
                NodeRef::FuncDecl(fd) => {
                    if let Some(body) = fd.body.as_ref() {
                        out.bodies
                            .push((body.lbrace.0 as u32, body.rbrace.0 as u32 + 1));
                    }
                }
                NodeRef::FuncLit(fl) => {
                    out.bodies
                        .push((fl.body.lbrace.0 as u32, fl.body.rbrace.0 as u32 + 1));
                }
                _ => {}
            }
            true
        });
        out
    }

    /// `getEnclosingBodyStart`: the `{` of the innermost function body around
    /// `pos`, or `None` when there is none.
    fn enclosing_body_start(&self, pos: u32) -> Option<u32> {
        self.bodies
            .iter()
            .filter(|(lo, hi)| *lo <= pos && pos < *hi)
            .next_back()
            .map(|(lo, _)| *lo)
    }
}

/// Port of gosec's `TryResolve` (`resolve.go`): can this expression be reduced
/// to a compile-time constant by walking declarations?
///
/// The node kinds absent from upstream's switch — `SelectorExpr`, `ParenExpr`,
/// `UnaryExpr`, … — fall through to `false`, so the list here is exhaustive on
/// purpose and `_ => false` is the upstream default, not a gap.
fn try_resolve(pass: &Pass<'_>, decls: &FileDecls<'_>, expr: &Expr, depth: u32) -> bool {
    if depth > 32 {
        return false;
    }
    match expr {
        Expr::BasicLit(_) => true,
        Expr::CompositeLit(lit) => {
            !lit.elts.is_empty()
                && lit
                    .elts
                    .iter()
                    .all(|e| try_resolve(pass, decls, e, depth + 1))
        }
        Expr::Ident(id) => resolve_ident(pass, decls, id, depth),
        Expr::CallExpr(_) => false, // upstream `resolveCallExpr` is a stub
        Expr::BinaryExpr(b) => {
            try_resolve(pass, decls, &b.x, depth + 1) && try_resolve(pass, decls, &b.y, depth + 1)
        }
        Expr::KeyValueExpr(kv) => {
            try_resolve(pass, decls, &kv.key, depth + 1)
                && try_resolve(pass, decls, &kv.value, depth + 1)
        }
        Expr::IndexExpr(ix) => try_resolve(pass, decls, &ix.x, depth + 1),
        Expr::SliceExpr(sl) => try_resolve(pass, decls, &sl.x, depth + 1),
        _ => false,
    }
}

/// `resolveIdent`: only `ast.Var` objects are followed; everything else
/// (constants, functions, types) counts as resolved.
fn resolve_ident(pass: &Pass<'_>, decls: &FileDecls<'_>, id: &Ident, depth: u32) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return true;
    };
    let Some(obj) = code::object_of(pass, id) else {
        return true; // no Obj
    };
    if !matches!(artifacts.objects.get(obj), ObjectData::Var(_)) {
        return true; // Obj.Kind != ast.Var
    }
    let decl_pos = obj.pos(&artifacts.objects);
    if !decls.defined_here.contains(&decl_pos) {
        return true; // declared in another file of the package: no Obj
    }
    match decls.decls.get(&decl_pos) {
        Some(DeclNode::Assign(a)) => {
            !a.rhs.is_empty() && a.rhs.iter().all(|e| try_resolve(pass, decls, e, depth + 1))
        }
        Some(DeclNode::Value(vs)) => {
            !vs.values.is_empty()
                && vs
                    .values
                    .iter()
                    .all(|e| try_resolve(pass, decls, e, depth + 1))
        }
        // A parameter, a receiver, a range clause …: `Obj.Decl` is a node
        // `TryResolve` does not handle.
        None => false,
    }
}

/// G204 — subprocess launched with a variable.
///
/// A pass of its own because it needs [`FileDecls`]. Everything else in this
/// file decides from the call alone.
fn check_g204(pass: &Pass<'_>, enabled: &HashSet<&'static str>, pending: &mut Vec<(u32, u32, String)>) {
    if !enabled.contains("G204") {
        return;
    }
    for file in pass.files() {
        let decls = FileDecls::build(pass, file);
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::CallExpr(call) = n {
                check_g204_call(pass, &decls, call, pending);
            }
            true
        });
    }
}

fn check_g204_call(
    pass: &Pass<'_>,
    decls: &FileDecls<'_>,
    call: &CallExpr,
    pending: &mut Vec<(u32, u32, String)>,
) {
    let Some((pkg, name)) = resolve_pkg_call(pass, call) else {
        return;
    };
    if !G204_CALLS.iter().any(|(p, n)| *p == pkg && *n == name) {
        return;
    }
    // `isContext` matches on the *written* selector, not the resolved import
    // path: only a call spelled `exec.CommandContext` drops its first argument.
    let is_context = matches!(
        call.fun.as_ref(),
        Expr::SelectorExpr(sel)
            if sel.sel.name == "CommandContext"
                && matches!(sel.x.as_ref(), Expr::Ident(x) if x.name == "exec")
    );
    let args: &[Expr] = if is_context && !call.args.is_empty() {
        &call.args[1..]
    } else {
        &call.args
    };

    let artifacts = pass.pkg().type_artifacts.as_ref();
    for (i, arg) in args.iter().enumerate() {
        if let Expr::Ident(id) = arg {
            let Some(artifacts) = artifacts else { continue };
            let Some(obj) = code::object_of(pass, id) else {
                continue;
            };
            let ObjectData::Var(var) = artifacts.objects.get(obj) else {
                continue; // only *types.Var arguments are considered
            };
            if i == 0 {
                if var.is_field() {
                    continue;
                }
                // Declared before the enclosing body's `{`: a parameter or a
                // receiver, which upstream exempts in the executable-name slot.
                let ident_pos = id.pos().0 as u32;
                if decls
                    .enclosing_body_start(ident_pos)
                    .is_some_and(|start| obj.pos(&artifacts.objects) < start)
                {
                    continue;
                }
            }
            if !try_resolve(pass, decls, arg, 0) {
                pending.push((
                    call.pos().0 as u32,
                    call.end().0 as u32,
                    "G204: Subprocess launched with variable".to_string(),
                ));
                return;
            }
        } else if !try_resolve(pass, decls, arg, 0) {
            pending.push((
                call.pos().0 as u32,
                call.end().0 as u32,
                "G204: Subprocess launched with a potential tainted input or cmd arguments"
                    .to_string(),
            ));
            return;
        }
    }
}

fn is_g110_reader_call(pass: &Pass<'_>, call: &CallExpr) -> bool {
    if resolve_pkg_call(pass, call).is_some_and(|(pkg, name)| {
        G110_READER_CALLS
            .iter()
            .any(|(p, n)| *p == pkg && *n == name)
    }) {
        return true;
    }

    // archive/zip.File.Open is a method call rather than a package function.
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return false;
    };
    if sel.sel.name != "Open" {
        return false;
    }
    type_name_of(pass, sel.x.as_ref()).is_some_and(|name| {
        let bare = name.strip_prefix('*').unwrap_or(&name);
        bare == "archive/zip.File"
    })
}

fn check_g110(pass: &Pass<'_>, enabled: &HashSet<&'static str>, pending: &mut Vec<(u32, u32, String)>) {
    if !enabled.contains("G110") {
        return;
    }

    let mut reader_vars: HashSet<ObjectId> = HashSet::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::AssignStmt(assign) => {
                    for (index, rhs) in assign.rhs.iter().enumerate() {
                        let Expr::CallExpr(call) = rhs else {
                            continue;
                        };
                        if !is_g110_reader_call(pass, call) {
                            continue;
                        }
                        let Some(Expr::Ident(id)) = assign.lhs.get(index) else {
                            continue;
                        };
                        if id.name != "_" {
                            if let Some(obj) = object_of(pass, id) {
                                reader_vars.insert(obj);
                            }
                        }
                    }
                }
                NodeRef::CallExpr(call) => {
                    let is_copy = resolve_pkg_call(pass, call).is_some_and(|(pkg, name)| {
                        G110_COPY_CALLS
                            .iter()
                            .any(|(p, n)| *p == pkg && *n == name)
                    });
                    if is_copy {
                        if let Some(Expr::Ident(src)) = call.args.get(1) {
                            if object_of(pass, src)
                                .is_some_and(|obj| reader_vars.contains(&obj))
                            {
                                pending.push((
                                    call.pos().0 as u32,
                                    call.end().0 as u32,
                                    format!("G110: {G110_WHAT}"),
                                ));
                            }
                        }
                    }
                }
                _ => {}
            }
            true
        });
    }
}

fn result_type_errors(pass: &Pass<'_>, typ: guff_types::TypeId) -> Vec<bool> {
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return vec![false],
    };
    match artifacts.types.get(typ) {
        TypeData::Tuple(t) => (0..t.len())
            .map(|i| {
                t.at(i)
                    .typ(&artifacts.objects)
                    .is_some_and(|rt| code::type_with_name(pass, rt, "error"))
            })
            .collect(),
        _ => vec![code::type_with_name(pass, typ, "error")],
    }
}

fn call_returns_error(pass: &Pass<'_>, call: &CallExpr) -> bool {
    errors_by_arg(pass, call).iter().any(|&e| e)
}

fn errors_by_arg(pass: &Pass<'_>, call: &CallExpr) -> Vec<bool> {
    let info = match pass.types_info() {
        Some(i) => i,
        None => return vec![false],
    };
    let Some(tav) = info.types.get(&call.id) else {
        return vec![false];
    };
    result_type_errors(pass, tav.typ)
}

/// Default G104 whitelist from securego/gosec `rules/errors.go` (non-audit).
const G104_FMT_FUNCS: &[&str] = &[
    "Print", "Printf", "Println", "Fprint", "Fprintf", "Fprintln",
];
const G104_BUFFER_METHODS: &[&str] = &["Write", "WriteByte", "WriteRune", "WriteString"];

fn expr_type_id(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn imported_named_type(pass: &Pass<'_>, import_path: &str, name: &str) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let pkg_id = artifacts.packages.find_by_path(import_path)?;
    let scope = artifacts.packages.get(pkg_id).scope();
    let obj = lookup(&artifacts.scopes, scope, name)?;
    obj.typ(&artifacts.objects)
}

fn recv_implements_named_iface(
    pass: &Pass<'_>,
    recv: &Expr,
    iface_path: &str,
    iface_name: &str,
) -> bool {
    let Some(typ) = expr_type_id(pass, recv) else {
        return false;
    };
    let Some(iface) = imported_named_type(pass, iface_path, iface_name) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let resolved = unalias_readonly(&artifacts.types, typ);
    let mut types = artifacts.types.clone();
    let v = if matches!(types.get(resolved), TypeData::Named(_))
        && !matches!(
            types.get(resolved.underlying(&types)),
            TypeData::Interface(_)
        ) {
        new_pointer(&mut types, resolved)
    } else {
        typ
    };
    implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        v,
        iface,
        false,
    )
    .is_ok()
}

fn g104_whitelisted(pass: &Pass<'_>, call: &CallExpr) -> bool {
    if let Some((pkg, name)) = resolve_pkg_call(pass, call) {
        if pkg == "fmt" && G104_FMT_FUNCS.contains(&name.as_str()) {
            return true;
        }
        if pkg == "os" && name == "Unsetenv" {
            return true;
        }
        if (pkg == "math/rand" || pkg == "math/rand/v2") && name == "Read" {
            return true;
        }
    }

    let Expr::SelectorExpr(sel) = &*call.fun else {
        return false;
    };
    let method = sel.sel.name.as_str();

    if let Some(ty_name) = type_name_of(pass, sel.x.as_ref()) {
        let bare = ty_name.strip_prefix('*').unwrap_or(&ty_name);
        match bare {
            "bytes.Buffer" | "strings.Builder"
                if G104_BUFFER_METHODS.contains(&method) =>
            {
                return true;
            }
            "io.PipeWriter" if method == "CloseWithError" => return true,
            "hash.Hash" if method == "Write" => return true,
            _ => {}
        }
    }

    // hash.Hash.Write — concrete digests (sha1, sha256, …) implement the iface.
    if method == "Write" && recv_implements_named_iface(pass, sel.x.as_ref(), "hash", "Hash") {
        return true;
    }

    false
}

fn check_g104_call(
    pass: &Pass<'_>,
    call: &CallExpr,
    enabled: &HashSet<&'static str>,
    pending: &mut Vec<(u32, u32, String)>,
) {
    if !enabled.contains("G104") || g104_whitelisted(pass, call) {
        return;
    }
    if call_returns_error(pass, call) {
        // `NewIssue(n, …)` where n is the ExprStmt, whose Pos() is the call's,
        // which is the callee's — not the `(`.
        pending.push((call.pos().0 as u32, call.end().0 as u32, "G104: Errors unhandled".to_string()));
    }
}

// G104 AssignStmt / ValueSpec (`_ = err`-style) are only reported by upstream
// gosec when global `audit` is enabled. golangci does not enable audit by
// default, so guff matches non-audit behaviour (ExprStmt only — upstream's
// registered node set excludes `go`/`defer`).
// Audit mode + config allowlist remain DEFERRED.

fn check_call(
    pass: &Pass<'_>,
    call: &CallExpr,
    enabled: &HashSet<&'static str>,
    pending: &mut Vec<(u32, u32, String)>,
) {
    // Every rule reached from here is a `MatchCallByPackage` /
    // `ContainsPkgCallExpr` rule upstream, so the receiver has to be the
    // package identifier itself — see [`resolve_pkg_qualified_call`].
    let Some((pkg, name)) = resolve_pkg_qualified_call(pass, call) else {
        return;
    };
    for rule in RULES {
        if !enabled.contains(rule.id) || rule.calls.is_empty() {
            continue;
        }
        if rule.calls.iter().any(|(p, n)| *p == pkg && *n == name) {
            // G114 is package-level `http.ListenAndServe` only — not
            // `(*http.Server).ListenAndServe` (gin Run/RunTLS).
            if rule.id == "G114" && !is_pkg_sel_call(pass, call) {
                continue;
            }
            pending.push((
                call.pos().0 as u32,
                call.end().0 as u32,
                format!("{}: {}", rule.id, rule.call_what),
            ));
        }
    }

    if enabled.contains("G102") && G102_CALLS.iter().any(|(p, n)| *p == pkg && *n == name) {
        // net.Listen(network, address) / tls.Listen(network, address, …)
        if call.args.len() >= 2 {
            if let Some(addr) = string_lit_from_expr(&call.args[1]) {
                if bind_all_pattern().is_match(&addr) {
                    pending.push((
                        call.pos().0 as u32,
                        call.end().0 as u32,
                        "G102: Binds to all network interfaces".to_string(),
                    ));
                }
            }
            // DEFERRED: Ident const resolution (GetIdentStringValues).
        }
    }

    // G204 needs the file's declarations, so it runs as its own pass over the
    // files (`check_g204`) rather than from here.

    if enabled.contains("G111") && G111_CALLS.iter().any(|(p, n)| *p == pkg && *n == name) {
        // Upstream matches `http.Dir("/")` / `http.Dir('/')` via regex on reconstructed call text.
        if call.args.len() == 1 {
            if let Some(arg) = string_lit_from_expr(&call.args[0]) {
                if arg == "/" {
                    pending.push((
                        call.pos().0 as u32,
                        call.end().0 as u32,
                        "G111: Potential directory traversal".to_string(),
                    ));
                }
            }
        }
    }

    if enabled.contains("G122") {
        check_g122_walk_call(pass, call, &pkg, &name, pending);
    }

    if enabled.contains("G301") && G301_CALLS.iter().any(|(p, n)| *p == pkg && *n == name) {
        if let Some(mode_arg) = call.args.last() {
            let bad = is_os_mode_perm(mode_arg)
                || get_int(mode_arg).is_some_and(|m| !mode_is_subset(m, G301_MODE));
            if bad {
                pending.push((
                    call.pos().0 as u32,
                    call.end().0 as u32,
                    format!(
                        "G301: Expect directory permissions to be {} or less",
                        format_octal_mode(G301_MODE)
                    ),
                ));
            }
        }
    }

    if enabled.contains("G302") && G302_CALLS.iter().any(|(p, n)| *p == pkg && *n == name) {
        if let Some(mode_arg) = call.args.last() {
            let bad = is_os_mode_perm(mode_arg)
                || get_int(mode_arg).is_some_and(|m| !mode_is_subset(m, G302_MODE));
            if bad {
                pending.push((
                    call.pos().0 as u32,
                    call.end().0 as u32,
                    format!(
                        "G302: Expect file permissions to be {} or less",
                        format_octal_mode(G302_MODE)
                    ),
                ));
            }
        }
    }

    if enabled.contains("G306") && G306_CALLS.iter().any(|(p, n)| *p == pkg && *n == name) {
        if let Some(mode_arg) = call.args.last() {
            let bad = is_os_mode_perm(mode_arg)
                || get_int(mode_arg).is_some_and(|m| !mode_is_subset(m, G306_MODE));
            if bad {
                pending.push((
                    call.pos().0 as u32,
                    call.end().0 as u32,
                    format!(
                        "G306: Expect WriteFile permissions to be {} or less",
                        format_octal_mode(G306_MODE)
                    ),
                ));
            }
        }
    }

    if enabled.contains("G403") && G403_CALLS.iter().any(|(p, n)| *p == pkg && *n == name) {
        // crypto/rsa.GenerateKey(random, bits)
        if call.args.len() >= 2 {
            if let Some(bits) = get_int(&call.args[1]) {
                if bits < G403_MIN_BITS {
                    pending.push((
                        call.pos().0 as u32,
                        call.end().0 as u32,
                        format!("G403: RSA keys should be at least {G403_MIN_BITS} bits"),
                    ));
                }
            }
        }
    }

    if enabled.contains("G303") && G303_CALLS.iter().any(|(p, n)| *p == pkg && *n == name) {
        if !call.args.is_empty() && find_temp_dir_args(pass, &call.args[0]) {
            pending.push((call.pos().0 as u32, call.end().0 as u32, format!("G303: {G303_WHAT}")));
        }
    }

    if enabled.contains("G107") && G107_CALLS.iter().any(|(p, n)| *p == pkg && *n == name) {
        // Only package-level `http.Get` / `http.Post` / … — not methods like
        // `Header.Get` (gin context.go FP).
        if is_pkg_sel_call(pass, call)
            && !call.args.is_empty()
            && g107_url_tainted(pass, &call.args[0])
        {
            pending.push((call.pos().0 as u32, call.end().0 as u32, format!("G107: {G107_WHAT}")));
        }
    }

    if enabled.contains("G203") && G203_CALLS.iter().any(|(p, n)| *p == pkg && *n == name) {
        let has_non_lit = call.args.iter().any(|a| !matches!(a, Expr::BasicLit(_)));
        if has_non_lit {
            pending.push((call.pos().0 as u32, call.end().0 as u32, format!("G203: {G203_WHAT}")));
        }
    }
}

fn check_imports(
    pass: &Pass<'_>,
    enabled: &HashSet<&'static str>,
    pending: &mut Vec<(u32, u32, String)>,
) {
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::GenDecl(gd) = decl else {
                continue;
            };
            for spec in &gd.specs {
                let Spec::ImportSpec(imp) = spec else {
                    continue;
                };
                let path = unquote_import(&imp.path.value);
                let is_blank = imp.name.as_ref().map(|n| n.name == "_").unwrap_or(false);
                for rule in RULES {
                    if !enabled.contains(rule.id) {
                        continue;
                    }
                    if rule.blank_import_only && !is_blank {
                        continue;
                    }
                    for (blocked, desc) in rule.imports {
                        if *blocked == path {
                            // Both the blocklist rules and G108 report the
                            // ImportSpec node, whose Pos() is the local name
                            // when there is one. `_ "net/http/pprof"` reports
                            // at the `_`, not at the path literal.
                            pending
                                .push((
                            import_spec_pos(imp),
                            node_end(NodeRef::ImportSpec(imp)).0 as u32,
                            format!("{}: {}", rule.id, desc),
                        ));
                        }
                    }
                }
            }
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "gosec requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<GosecOptions>("gosec")
        .cloned()
        .unwrap_or_default();
    let enabled = enabled_rules(&opts);

    let mut pending: Vec<(u32, u32, String)> = Vec::new();
    check_imports(pass, &enabled, &mut pending);
    check_g101(pass, &enabled, &opts, &mut pending);
    check_g109(pass, &enabled, &mut pending);
    check_g110(pass, &enabled, &mut pending);
    check_g124_cookie_params(pass, &enabled, &mut pending);
    check_g204(pass, &enabled, &mut pending);
    crate::gosec_ssa::check_ssa_analyzers(pass, &enabled, &mut pending);

    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::CallExpr(call) => check_call(pass, call, &enabled, &mut pending),
                NodeRef::CompositeLit(lit) => {
                    check_g402_composite(pass, lit, &enabled, &mut pending);
                    check_g112_composite(pass, lit, &enabled, &mut pending);
                    check_g124_composite(pass, lit, &enabled, &mut pending);
                }
                NodeRef::ExprStmt(stmt) => {
                    // Upstream gosec G104 only visits AssignStmt + ExprStmt
                    // (not `go`/`defer`); match that node set for parity.
                    if let Expr::CallExpr(call) = &stmt.x {
                        check_g104_call(pass, call, &enabled, &mut pending);
                        if enabled.contains("G202") {
                            check_g202_call(pass, file, call, &mut pending);
                        }
                    }
                }
                NodeRef::AssignStmt(stmt) => {
                    // G104 assign (`_ = f()`) is audit-mode only upstream; skip.
                    check_g402_assign(pass, stmt, &enabled, &mut pending);
                    // G202's node set is the same two statements, and it looks
                    // at the calls on an assignment's right-hand side.
                    if enabled.contains("G202") {
                        for rhs in &stmt.rhs {
                            if let Expr::CallExpr(call) = rhs {
                                check_g202_call(pass, file, call, &mut pending);
                            }
                        }
                    }
                }
                _ => {}
            }
            true
        });
    }

    let min_severity = threshold_score(&opts.severity);
    let min_confidence = threshold_score(&opts.confidence);
    let nosec_ranges = NosecRanges::build(pass);
    for (pos, end, msg) in pending {
        let rule = msg.split(':').next().unwrap_or("");
        let start_pos = pass.fset().position(guff::position::Pos(pos as i64));
        let end_line = pass
            .fset()
            .position(guff::position::Pos(end.max(pos) as i64))
            .line;
        if nosec_ranges.suppresses(&start_pos.filename, start_pos.line, end_line, rule) {
            continue;
        }
        let (severity, confidence) = issue_scores(rule, &msg);
        if min_severity.is_some_and(|min| severity < min)
            || min_confidence.is_some_and(|min| confidence < min)
        {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message: msg,
            severity: severity.as_str().to_string(),
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "gosec",
        doc: "Inspects source code for security problems",
        url: "https://github.com/securego/gosec",
        run: run as RunFn,
        // AST/types-info rules still produce useful findings when guff's
        // typechecker marks a package ill-typed (e.g. gin interface assign
        // FPs). Match golangci/gosec which runs against go/types-clean packages.
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn includes_filters_rules() {
        let opts = GosecOptions {
            includes: vec!["G501".into()],
            ..Default::default()
        };
        let e = enabled_rules(&opts);
        assert!(e.contains("G501"));
        assert!(!e.contains("G103"));
        assert!(!e.contains("G404"));
        assert!(!e.contains("G102"));
        assert!(!e.contains("G101"));
        assert!(!e.contains("G104"));
        assert!(!e.contains("G204"));
    }

    #[test]
    fn excludes_removes_rules() {
        let opts = GosecOptions {
            excludes: vec![
                "G501".into(),
                "G505".into(),
                "G102".into(),
                "G101".into(),
                "G104".into(),
            ],
            ..Default::default()
        };
        let e = enabled_rules(&opts);
        assert!(!e.contains("G501"));
        assert!(!e.contains("G505"));
        assert!(!e.contains("G102"));
        assert!(!e.contains("G101"));
        assert!(!e.contains("G104"));
        assert!(e.contains("G103"));
        assert!(e.contains("G204"));
    }

    #[test]
    fn split_fq_handles_dotted_paths() {
        let (pkg, name) = split_fq_name("golang.org/x/crypto/md4.New").unwrap();
        assert_eq!(pkg, "golang.org/x/crypto/md4");
        assert_eq!(name, "New");
    }

    #[test]
    fn bind_all_matches_upstream_addrs() {
        let re = bind_all_pattern();
        assert!(re.is_match("0.0.0.0:8080"));
        assert!(re.is_match(":8080"));
        assert!(re.is_match("0.0.0.0"));
        assert!(!re.is_match("127.0.0.1:8080"));
        assert!(!re.is_match("localhost:8080"));
    }

    #[test]
    fn g101_entropy_flags_hex_password_not_secret() {
        let rt = G101Rt::new(&crate::options::G101Options::default());
        assert!(rt.is_high_entropy_string("f62e5bcda4fae4f82370da0c6f20697b8f8447ef"));
        assert!(!rt.is_high_entropy_string("secret"));
        assert!(rt.cred_name_match("password"));
        assert!(rt.cred_name_match("apiKey"));
        assert!(!rt.cred_name_match("username"));
        assert_eq!(
            rt.is_secret_pattern("AKIAI44QH8DHBEXAMPLE"),
            Some("AWS API Key")
        );
        assert_eq!(
            rt.is_secret_pattern("ghp_iR54dhCYg9Tfmoywi9xLmmKZrrnAw438BYh3"),
            Some("GitHub personal access token")
        );
    }

    #[test]
    fn g101_config_overrides_name_pattern() {
        let opts = crate::options::G101Options {
            pattern: "(?i)example".into(),
            ..Default::default()
        };
        let rt = G101Rt::new(&opts);
        // `pattern` replaces the default list outright — upstream assigns, not appends.
        assert!(rt.cred_name_match("exampleToken"));
        assert!(!rt.cred_name_match("password"));
    }

    #[test]
    fn g101_ignore_entropy_skips_the_gate() {
        let low_entropy = "aaaaaaaaaaaaaaaa";
        let strict = G101Rt::new(&crate::options::G101Options::default());
        assert!(!strict.entropy_ok(low_entropy));
        let relaxed = G101Rt::new(&crate::options::G101Options {
            ignore_entropy: true,
            ..Default::default()
        });
        assert!(relaxed.entropy_ok(low_entropy));
    }

    #[test]
    fn g101_entropy_threshold_is_configurable() {
        let value = "f62e5bcda4fae4f82370da0c6f20697b8f8447ef";
        assert!(G101Rt::new(&crate::options::G101Options::default()).is_high_entropy_string(value));
        let strict = G101Rt::new(&crate::options::G101Options {
            entropy_threshold: 1_000.0,
            per_char_threshold: 100.0,
            ..Default::default()
        });
        assert!(!strict.is_high_entropy_string(value));
    }

    #[test]
    fn threshold_score_matches_golangci_convert_to_score() {
        assert_eq!(threshold_score(""), Some(Score::Low));
        assert_eq!(threshold_score("low"), Some(Score::Low));
        assert_eq!(threshold_score("MEDIUM"), Some(Score::Medium));
        assert_eq!(threshold_score("high"), Some(Score::High));
        // Upstream returns -1 for anything else, which never filters.
        assert_eq!(threshold_score("bogus"), None);
    }

    #[test]
    fn issue_scores_match_upstream_metadata() {
        // G101 is High severity but only Low confidence, so `confidence: medium`
        // drops every hardcoded-credential finding.
        assert_eq!(issue_scores("G101", "G101: x"), (Score::High, Score::Low));
        assert_eq!(issue_scores("G112", "G112: x"), (Score::Medium, Score::Low));
        assert_eq!(issue_scores("G104", "G104: x"), (Score::Low, Score::High));
        // Unknown ids stay at Low/Low so a new rule is never filtered by default.
        assert_eq!(issue_scores("G999", "G999: x"), (Score::Low, Score::Low));
    }

    #[test]
    fn g402_confidence_depends_on_the_message() {
        assert_eq!(
            issue_scores("G402", "G402: TLS InsecureSkipVerify set to true."),
            (Score::High, Score::High)
        );
        assert_eq!(
            issue_scores("G402", "G402: TLS InsecureSkipVerify may be set to true."),
            (Score::High, Score::Low)
        );
    }

    #[test]
    fn parse_go_int_lit_handles_bases() {
        assert_eq!(parse_go_int_lit("0o777"), Some(0o777));
        assert_eq!(parse_go_int_lit("0755"), Some(0o755));
        assert_eq!(parse_go_int_lit("493"), Some(493));
        assert_eq!(parse_go_int_lit("0x100"), Some(256));
        assert_eq!(parse_go_int_lit("0b1010"), Some(10));
        assert_eq!(parse_go_int_lit("1_024"), Some(1024));
    }

    #[test]
    fn mode_subset_matches_upstream() {
        assert!(mode_is_subset(0o750, 0o750));
        assert!(mode_is_subset(0o700, 0o750));
        assert!(!mode_is_subset(0o755, 0o750));
        assert!(!mode_is_subset(0o644, 0o600));
        assert!(mode_is_subset(0o600, 0o600));
    }

    #[test]
    fn g303_tmp_pattern_matches_upstream() {
        let re = g303_tmp_pattern();
        assert!(re.is_match("/tmp"));
        assert!(re.is_match("/tmp/demo"));
        assert!(re.is_match("/var/tmp/x"));
        assert!(re.is_match("/usr/tmp/x"));
        assert!(!re.is_match("/var/lib/demo"));
        assert!(!re.is_match("/home/tmp"));
    }
}
