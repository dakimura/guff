//! Port of [`github.com/securego/gosec`](https://github.com/securego/gosec)
//! (golangci-lint wrapper in `pkg/golinters/gosec`).
//!
//! Implemented rules (AST / types-info only):
//! - **G101** — potential hardcoded credentials (name pattern + Shannon entropy approx /
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
//! - **G122** — `filepath.Walk`/`WalkDir` callback path into race-prone `os` sinks (AST approx of SSA)
//! - **G124** — `http.Cookie` missing Secure / HttpOnly / SameSite (AST approx of SSA rule)
//! - **G203** — `html/template` non-escaping helpers with non-literal args
//! - **G204** — subprocess launched with non-literal args (`os/exec` / `syscall` / `execabs`)
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
//!
//! Message format matches golangci: `"Gxxx: <what>"`.
//!
//! DEFERRED: remaining rules (G113, G115–G118, G201–G202, G304–G305, G307
//! config-gated, G402 MinVersion/CipherSuites, G601, SSA analyzers), G101 zxcvbn
//! entropy / full `gosec:disable` block directives / per-rule `config` map,
//! G104 audit mode + config allowlist extensions, G107 local string-lit
//! TryResolve, full G204 TryResolve / G102 Ident const resolution,
//! `severity`/`confidence` filters, concurrency.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, BinaryExpr, CallExpr, CompositeLit, Decl, Expr, Ident, Spec, ValueSpec,
};
use guff::token::Token;
use guff::walk::{preorder, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
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
        calls: &[
            ("math/rand", "New"),
            ("math/rand", "Read"),
            ("math/rand", "ExpFloat64"),
            ("math/rand", "Float32"),
            ("math/rand", "Float64"),
            ("math/rand", "Int"),
            ("math/rand", "Int31"),
            ("math/rand", "Int31n"),
            ("math/rand", "Int63"),
            ("math/rand", "Int63n"),
            ("math/rand", "Intn"),
            ("math/rand", "NormFloat64"),
            ("math/rand", "Perm"),
            ("math/rand", "Shuffle"),
            ("math/rand", "Uint32"),
            ("math/rand", "Uint64"),
            ("math/rand/v2", "New"),
            ("math/rand/v2", "ExpFloat64"),
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
            ("math/rand/v2", "Perm"),
            ("math/rand/v2", "Shuffle"),
            ("math/rand/v2", "Uint"),
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
    "G101", "G102", "G104", "G107", "G109", "G110", "G111", "G112", "G122", "G124", "G203", "G204",
    "G301", "G302", "G303", "G306", "G402", "G403",
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
const G101_ENTROPY_THRESHOLD: f64 = 80.0;
const G101_PER_CHAR_THRESHOLD: f64 = 3.0;
const G101_TRUNCATE: usize = 16;
const G101_MIN_ENTROPY_LENGTH: usize = 8;

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
    pending: &mut Vec<(u32, String)>,
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
    let path_name = path_param.to_string();
    preorder(NodeRef::FuncLit(fl), |n| {
        if let NodeRef::CallExpr(inner) = n {
            if let Some(pos) = g122_sink_pos(pass, inner, &path_name) {
                pending.push((pos, format!("G122: {G122_WHAT}")));
            }
        }
        true
    });
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
        if expr_mentions_ident(arg, path_param) {
            return Some(call.pos().0 as u32);
        }
    }
    None
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

fn g101_name_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(G101_NAME_PATTERN).expect("G101 name pattern"))
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

/// Shannon entropy × length over a truncated prefix.
/// Approximates gosec's zxcvbn thresholds (DEFERRED: true zxcvbn parity).
fn shannon_entropy_total(s: &str) -> (f64, f64) {
    let truncated = if s.len() > G101_TRUNCATE {
        // Truncate on a char boundary — secrets may contain multi-byte UTF-8.
        let mut end = G101_TRUNCATE;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    } else {
        s
    };
    if truncated.is_empty() {
        return (0.0, 0.0);
    }
    let mut freq = [0u32; 256];
    for b in truncated.bytes() {
        freq[b as usize] += 1;
    }
    let n = truncated.len() as f64;
    let mut h = 0.0_f64;
    for &c in &freq {
        if c > 0 {
            let p = f64::from(c) / n;
            h -= p * p.log2();
        }
    }
    (h * n, h)
}

fn is_high_entropy_string(s: &str) -> bool {
    if s.len() < G101_MIN_ENTROPY_LENGTH {
        return false;
    }
    let (total, per_char) = shannon_entropy_total(s);
    total >= G101_ENTROPY_THRESHOLD
        || (total >= G101_ENTROPY_THRESHOLD / 2.0 && per_char >= G101_PER_CHAR_THRESHOLD)
}

fn is_secret_pattern(s: &str) -> Option<&'static str> {
    if s.len() < G101_MIN_ENTROPY_LENGTH {
        return None;
    }
    for (i, re) in g101_secret_regexes().iter().enumerate() {
        if re.1.is_match(s) {
            return Some(G101_SECRET_PATTERNS[i].name);
        }
    }
    None
}

fn cred_name_match(name: &str) -> bool {
    g101_name_pattern().is_match(name)
}

fn report_g101(pending: &mut Vec<(u32, String)>, pos: u32, pattern_name: Option<&str>) {
    let msg = match pattern_name {
        Some(p) => format!("G101: {G101_WHAT}: {p}"),
        None => format!("G101: {G101_WHAT}"),
    };
    pending.push((pos, msg));
}

fn check_cred_value(
    pending: &mut Vec<(u32, String)>,
    pos: u32,
    name_matched: bool,
    value: &str,
) -> bool {
    if name_matched {
        if is_high_entropy_string(value) {
            report_g101(pending, pos, None);
            return true;
        }
    } else if is_high_entropy_string(value) {
        if let Some(pattern) = is_secret_pattern(value) {
            report_g101(pending, pos, Some(pattern));
            return true;
        }
    }
    false
}

fn check_g101_assign(assign: &AssignStmt, pending: &mut Vec<(u32, String)>) {
    for lhs in &assign.lhs {
        let Expr::Ident(ident) = lhs else {
            continue;
        };
        let name_matched = cred_name_match(&ident.name);
        if name_matched {
            for rhs in &assign.rhs {
                if let Some(val) = string_lit_from_expr(rhs) {
                    if check_cred_value(pending, assign.tok_pos.0 as u32, true, &val) {
                        return;
                    }
                }
            }
        }
        for rhs in &assign.rhs {
            if let Some(val) = string_lit_from_expr(rhs) {
                if check_cred_value(pending, assign.tok_pos.0 as u32, false, &val) {
                    return;
                }
            }
        }
    }
}

fn check_g101_value_spec(spec: &ValueSpec, pending: &mut Vec<(u32, String)>) {
    let pos = spec.names.first().map(|n| n.pos().0 as u32).unwrap_or(0);
    for (index, ident) in spec.names.iter().enumerate() {
        if !cred_name_match(&ident.name) || spec.values.is_empty() {
            continue;
        }
        let idx = if index < spec.values.len() {
            index
        } else {
            spec.values.len() - 1
        };
        if let Some(val) = string_lit_from_expr(&spec.values[idx]) {
            if check_cred_value(pending, pos, true, &val) {
                return;
            }
        }
    }
    for value in &spec.values {
        if let Some(val) = string_lit_from_expr(value) {
            if check_cred_value(pending, pos, false, &val) {
                return;
            }
        }
    }
}

fn check_g101_equality(bin: &BinaryExpr, pending: &mut Vec<(u32, String)>) {
    if bin.op != Token::EQL && bin.op != Token::NEQ {
        return;
    }
    let pos = bin.op_pos.0 as u32;

    let (ident, value_node) = match (bin.x.as_ref(), bin.y.as_ref()) {
        (Expr::Ident(id), other) => (Some(id), other),
        (other, Expr::Ident(id)) => (Some(id), other),
        _ => (None, bin.y.as_ref()),
    };
    if let Some(ident) = ident {
        if cred_name_match(&ident.name) {
            if let Some(val) = string_lit_from_expr(value_node) {
                if check_cred_value(pending, pos, true, &val) {
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
            if check_cred_value(pending, pos, false, &val) {
                return;
            }
        }
    }
}

fn check_g101_composite(lit: &CompositeLit, pending: &mut Vec<(u32, String)>) {
    let pos = lit.lbrace.0 as u32;
    for elt in &lit.elts {
        let Expr::KeyValueExpr(kv) = elt else {
            continue;
        };
        let mut matched_key = false;
        if let Expr::Ident(id) = kv.key.as_ref() {
            if cred_name_match(&id.name) {
                matched_key = true;
            }
        }
        if let Some(key_str) = string_lit_from_expr(kv.key.as_ref()) {
            if cred_name_match(&key_str) {
                matched_key = true;
            }
        }
        if matched_key {
            if let Some(val) = string_lit_from_expr(kv.value.as_ref()) {
                if check_cred_value(pending, pos, true, &val) {
                    return;
                }
            }
        }
        if let Some(val) = string_lit_from_expr(kv.value.as_ref()) {
            if check_cred_value(pending, pos, false, &val) {
                return;
            }
        }
    }
}

fn check_g101(pass: &Pass<'_>, enabled: &HashSet<&'static str>, pending: &mut Vec<(u32, String)>) {
    if !enabled.contains("G101") {
        return;
    }
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::AssignStmt(a) => check_g101_assign(a, pending),
                NodeRef::ValueSpec(v) => check_g101_value_spec(v, pending),
                NodeRef::BinaryExpr(b) => check_g101_equality(b, pending),
                NodeRef::CompositeLit(c) => check_g101_composite(c, pending),
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
fn nosec_suppresses(pass: &Pass<'_>, pos: u32, rule: &str) -> bool {
    let position = pass.fset().position(guff::position::Pos(pos as i64));
    let line = position.line;
    if line == 0 {
        return false;
    }

    // AST comments first (when PARSE_COMMENTS was used).
    const PREV_LINES: i64 = 3;
    for file in pass.files() {
        for cg in &file.comments {
            for c in &cg.list {
                let cline = pass.fset().position(c.slash).line;
                if cline > line || line - cline > PREV_LINES {
                    continue;
                }
                if comment_suppresses_nosec(&c.text, rule) {
                    return true;
                }
            }
        }
    }

    // Fall back to retained source bytes (same buffers typecheck already read).
    if position.filename.is_empty() {
        return false;
    }
    let Some(src) = package_source_for_file(pass, &position.filename) else {
        return false;
    };
    let text = String::from_utf8_lossy(src);
    let lines: Vec<&str> = text.lines().collect();
    if line < 1 || (line as usize) > lines.len() {
        return false;
    }
    let idx = (line as usize) - 1;
    if comment_suppresses_nosec(lines[idx], rule) {
        return true;
    }
    let mut i = idx;
    let mut walked = 0i64;
    while i > 0 && walked < PREV_LINES {
        i -= 1;
        walked += 1;
        let t = lines[i].trim();
        if t.is_empty() {
            continue;
        }
        if !(t.starts_with("//") || t.starts_with("/*") || t.starts_with('*')) {
            break;
        }
        if comment_suppresses_nosec(lines[i], rule) {
            return true;
        }
    }
    false
}

/// Locate retained source for `filename` (FileSet path from typecheck).
fn package_source_for_file<'a>(pass: &'a Pass<'_>, filename: &str) -> Option<&'a [u8]> {
    let pkg = pass.pkg();
    for (i, path) in pkg.compiled_go_files.iter().enumerate() {
        if path.to_str() == Some(filename) {
            return pkg.source_bytes(i);
        }
    }
    // Basename fallback for synthetic / relative FileSet names in tests.
    let want = std::path::Path::new(filename).file_name()?;
    for (i, path) in pkg.compiled_go_files.iter().enumerate() {
        if path.file_name() == Some(want) {
            return pkg.source_bytes(i);
        }
    }
    None
}

fn comment_suppresses_nosec(text: &str, rule: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let has_nosec =
        lower.contains("#nosec") || lower.contains("//nosec") || lower.contains("gosec:disable");
    if !has_nosec {
        return false;
    }
    // Bare `#nosec` / `#nosec G112` / `#nosec G112,G114`
    !text.contains('G') || text.contains(rule)
}

fn check_g402_tls_field(
    field: &str,
    value: &Expr,
    report_pos: u32,
    pending: &mut Vec<(u32, String)>,
) {
    if field != "InsecureSkipVerify" {
        return;
    }
    match resolve_bool_const(value) {
        Some(true) => pending.push((
            report_pos,
            "G402: TLS InsecureSkipVerify set to true.".to_string(),
        )),
        None => pending.push((
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
    pending: &mut Vec<(u32, String)>,
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
    pending: &mut Vec<(u32, String)>,
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
    pending: &mut Vec<(u32, String)>,
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
    pending.push((lit.lbrace.0 as u32, format!("G124: {G124_WHAT}")));
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
    pending: &mut Vec<(u32, String)>,
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
                        pending.push((name.name_pos.0 as u32, format!("G124: {G124_WHAT}")));
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
    pending: &mut Vec<(u32, String)>,
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
        pending.push((lit.lbrace.0 as u32, format!("G112: {G112_WHAT}")));
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
    pending: &mut Vec<(u32, String)>,
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
        pending.push((call.pos().0 as u32, format!("G109: {G109_WHAT}")));
    }
}

fn check_g109(pass: &Pass<'_>, enabled: &HashSet<&'static str>, pending: &mut Vec<(u32, String)>) {
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

fn check_g110(pass: &Pass<'_>, enabled: &HashSet<&'static str>, pending: &mut Vec<(u32, String)>) {
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
    pending: &mut Vec<(u32, String)>,
) {
    if !enabled.contains("G104") || g104_whitelisted(pass, call) {
        return;
    }
    if call_returns_error(pass, call) {
        pending.push((call.lparen.0 as u32, "G104: Errors unhandled.".to_string()));
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
    pending: &mut Vec<(u32, String)>,
) {
    let Some((pkg, name)) = resolve_pkg_call(pass, call) else {
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
                        "G102: Binds to all network interfaces".to_string(),
                    ));
                }
            }
            // DEFERRED: Ident const resolution (GetIdentStringValues).
        }
    }

    if enabled.contains("G204") && G204_CALLS.iter().any(|(p, n)| *p == pkg && *n == name) {
        let skip_first = name == "CommandContext";
        let args: &[Expr] = if skip_first && !call.args.is_empty() {
            &call.args[1..]
        } else {
            &call.args
        };
        let mut flagged = false;
        let mut msg = "G204: Subprocess launched with variable";
        for arg in args {
            if !is_resolvable_literal(arg) {
                flagged = true;
                if !matches!(arg, Expr::Ident(_)) {
                    msg =
                        "G204: Subprocess launched with a potential tainted input or cmd arguments";
                }
                break;
            }
        }
        if flagged {
            pending.push((call.pos().0 as u32, msg.to_string()));
        }
        // DEFERRED: full TryResolve / param/field skip parity with upstream.
    }

    if enabled.contains("G111") && G111_CALLS.iter().any(|(p, n)| *p == pkg && *n == name) {
        // Upstream matches `http.Dir("/")` / `http.Dir('/')` via regex on reconstructed call text.
        if call.args.len() == 1 {
            if let Some(arg) = string_lit_from_expr(&call.args[0]) {
                if arg == "/" {
                    pending.push((
                        call.pos().0 as u32,
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
                        format!("G403: RSA keys should be at least {G403_MIN_BITS} bits"),
                    ));
                }
            }
        }
    }

    if enabled.contains("G303") && G303_CALLS.iter().any(|(p, n)| *p == pkg && *n == name) {
        if !call.args.is_empty() && find_temp_dir_args(pass, &call.args[0]) {
            pending.push((call.pos().0 as u32, format!("G303: {G303_WHAT}")));
        }
    }

    if enabled.contains("G107") && G107_CALLS.iter().any(|(p, n)| *p == pkg && *n == name) {
        // Only package-level `http.Get` / `http.Post` / … — not methods like
        // `Header.Get` (gin context.go FP).
        if is_pkg_sel_call(pass, call)
            && !call.args.is_empty()
            && g107_url_tainted(pass, &call.args[0])
        {
            pending.push((call.pos().0 as u32, format!("G107: {G107_WHAT}")));
        }
    }

    if enabled.contains("G203") && G203_CALLS.iter().any(|(p, n)| *p == pkg && *n == name) {
        let has_non_lit = call.args.iter().any(|a| !matches!(a, Expr::BasicLit(_)));
        if has_non_lit {
            pending.push((call.pos().0 as u32, format!("G203: {G203_WHAT}")));
        }
    }
}

fn check_imports(
    pass: &Pass<'_>,
    enabled: &HashSet<&'static str>,
    pending: &mut Vec<(u32, String)>,
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
                            pending.push((
                                imp.path.value_pos.0 as u32,
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

    let mut pending: Vec<(u32, String)> = Vec::new();
    check_imports(pass, &enabled, &mut pending);
    check_g101(pass, &enabled, &mut pending);
    check_g109(pass, &enabled, &mut pending);
    check_g110(pass, &enabled, &mut pending);
    check_g124_cookie_params(pass, &enabled, &mut pending);

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
                    }
                }
                NodeRef::AssignStmt(stmt) => {
                    // G104 assign (`_ = f()`) is audit-mode only upstream; skip.
                    check_g402_assign(pass, stmt, &enabled, &mut pending);
                }
                _ => {}
            }
            true
        });
    }

    for (pos, msg) in pending {
        let rule = msg.split(':').next().unwrap_or("");
        if nosec_suppresses(pass, pos, rule) {
            continue;
        }
        pass.reportf(pos, &msg);
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
            excludes: vec![],
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
            includes: vec![],
            excludes: vec![
                "G501".into(),
                "G505".into(),
                "G102".into(),
                "G101".into(),
                "G104".into(),
            ],
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
        assert!(is_high_entropy_string(
            "f62e5bcda4fae4f82370da0c6f20697b8f8447ef"
        ));
        assert!(!is_high_entropy_string("secret"));
        assert!(cred_name_match("password"));
        assert!(cred_name_match("apiKey"));
        assert!(!cred_name_match("username"));
        assert_eq!(
            is_secret_pattern("AKIAI44QH8DHBEXAMPLE"),
            Some("AWS API Key")
        );
        assert_eq!(
            is_secret_pattern("ghp_iR54dhCYg9Tfmoywi9xLmmKZrrnAw438BYh3"),
            Some("GitHub personal access token")
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
