//! Port of [`github.com/butuzov/ireturn`](https://github.com/butuzov/ireturn)
//! (golangci-lint wrapper in `pkg/golinters/ireturn`).
//!
//! "Accept Interfaces, Return Concrete Types" — report functions that return
//! interfaces. Default allow-list: `anon`, `error`, `empty`, `stdlib`.
//!
//! DEFERRED: collision error when both `allow` and `reject` are
//! set (prefer `reject`); per-func `//nolint:ireturn` (guff CLI nolint covers
//! this); generic type-param `OfType` detail string parity.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{Decl, Expr, FuncDecl, InterfaceType};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::{TypeData, TypeId};
use regex::Regex;

use crate::options::IreturnOptions;

const KW_EMPTY: &str = "empty";
const KW_ANON: &str = "anon";
const KW_ERROR: &str = "error";
const KW_STDLIB: &str = "stdlib";
const KW_GENERIC: &str = "generic";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum IFaceKind {
    Empty = 1 << 0,
    Anon = 1 << 1,
    Error = 1 << 2,
    Named = 1 << 3,
    NamedStd = 1 << 4,
    Generic = 1 << 5,
}

#[derive(Debug, Clone)]
struct IFace {
    name: String,
    kind: IFaceKind,
    of_type: String,
}

fn default_allow() -> Vec<String> {
    vec![
        KW_ANON.to_string(),
        KW_ERROR.to_string(),
        KW_EMPTY.to_string(),
        KW_STDLIB.to_string(),
    ]
}

/// Upstream's `std` table (`analyzer/std.go`, generated), transcribed rather
/// than guessed. A "no `.` in the first path element" heuristic calls every
/// package of a dotless module (`module myapp`) standard library, and `stdlib`
/// is on the default allow-list, so those interfaces were never reported.
const STD_PKGS: &[&str] = &[
    "archive/tar", "archive/zip", "bufio", "bytes", "cmd/cgo", "cmd/fix", "cmd/go", "cmd/gofmt",
    "cmd/yacc", "compress/bzip2", "compress/flate", "compress/gzip", "compress/lzw",
    "compress/zlib", "container/heap", "container/list", "container/ring", "crypto",
    "crypto/aes", "crypto/cipher", "crypto/des", "crypto/dsa", "crypto/ecdsa",
    "crypto/elliptic", "crypto/hmac", "crypto/md5", "crypto/rand", "crypto/rc4", "crypto/rsa",
    "crypto/sha1", "crypto/sha256", "crypto/sha512", "crypto/subtle", "crypto/tls",
    "crypto/x509", "crypto/x509/pkix", "database/sql", "database/sql/driver", "debug/dwarf",
    "debug/elf", "debug/gosym", "debug/macho", "debug/pe", "encoding", "encoding/ascii85",
    "encoding/asn1", "encoding/base32", "encoding/base64", "encoding/binary", "encoding/csv",
    "encoding/gob", "encoding/hex", "encoding/json", "encoding/pem", "encoding/xml", "errors",
    "expvar", "flag", "fmt", "go/ast", "go/build", "go/doc", "go/format", "go/parser",
    "go/printer", "go/scanner", "go/token", "hash", "hash/adler32", "hash/crc32", "hash/crc64",
    "hash/fnv", "html", "html/template", "image", "image/color", "image/color/palette",
    "image/draw", "image/gif", "image/jpeg", "image/png", "index/suffixarray", "io",
    "io/ioutil", "log", "log/syslog", "math", "math/big", "math/cmplx", "math/rand", "mime",
    "mime/multipart", "net", "net/http", "net/http/cgi", "net/http/cookiejar", "net/http/fcgi",
    "net/http/httptest", "net/http/httputil", "net/http/pprof", "net/mail", "net/rpc",
    "net/rpc/jsonrpc", "net/smtp", "net/textproto", "net/url", "os", "os/exec", "os/signal",
    "os/user", "path", "path/filepath", "reflect", "regexp", "regexp/syntax", "runtime",
    "runtime/cgo", "runtime/debug", "runtime/pprof", "runtime/race", "sort", "strconv",
    "strings", "sync", "sync/atomic", "syscall", "testing", "testing/iotest", "testing/quick",
    "text/scanner", "text/tabwriter", "text/template", "text/template/parse", "time", "unicode",
    "unicode/utf16", "unicode/utf8", "unsafe", "cmd/addr2line", "cmd/nm", "cmd/objdump",
    "cmd/pack", "debug/plan9obj", "cmd/pprof", "go/constant", "go/importer", "go/types",
    "mime/quotedprintable", "runtime/trace", "context", "net/http/httptrace", "plugin",
    "math/bits", "crypto/ed25519", "hash/maphash", "time/tzdata", "embed",
    "go/build/constraint", "io/fs", "runtime/metrics", "testing/fstest", "debug/buildinfo",
    "net/netip", "go/doc/comment", "crypto/ecdh", "runtime/coverage", "cmp", "log/slog", "maps",
    "slices", "testing/slogtest", "go/version", "math/rand/v2", "iter", "structs", "unique",
    "crypto/fips140", "crypto/hkdf", "crypto/mlkem", "crypto/pbkdf2", "crypto/sha3", "weak",
    "testing/synctest", "crypto/hpke", "crypto/mlkem/mlkemtest", "testing/cryptotest",
];

fn is_std_pkg(pkg: &str) -> bool {
    STD_PKGS.contains(&pkg)
}

fn pkg_of_named(named: &str) -> Option<&str> {
    let idx = named.rfind('.')?;
    Some(&named[..idx])
}

/// Upstream `isStdPkgInterface`: the text before the last `.` is in the table.
fn is_std_named_interface(named: &str) -> bool {
    pkg_of_named(named).is_some_and(is_std_pkg)
}

fn type_of_expr(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn type_string(pass: &Pass<'_>, typ: TypeId) -> String {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return String::new();
    };
    guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    )
}

fn iface_is_empty(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let under = typ.underlying(&artifacts.types);
    match artifacts.types.get(under) {
        TypeData::Interface(i) => i.num_explicit_methods() == 0 && i.num_embeddeds() == 0,
        _ => false,
    }
}

fn classify_ast_interface(it: &InterfaceType) -> IFace {
    if it.methods.list.is_empty() {
        IFace {
            name: "interface{}".to_string(),
            kind: IFaceKind::Empty,
            of_type: String::new(),
        }
    } else {
        IFace {
            name: "anonymous interface".to_string(),
            kind: IFaceKind::Anon,
            of_type: String::new(),
        }
    }
}

/// The interface behind a result type, if it has one.
fn interface_under(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let typ = type_of_expr(pass, expr)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let under = typ.underlying(&artifacts.types);
    matches!(artifacts.types.get(under), TypeData::Interface(_)).then_some(typ)
}

/// Upstream's `*ast.Ident` arm, which classifies by the type's *string*:
/// `TypeOf(ident).String()`.
///
/// An alias is its own name there — `type handler = other.Handler` prints as
/// `pkg.handler` — so it is a named interface even when it aliases `error` or
/// a stdlib interface. guff used to classify the aliased type's *underlying*
/// interface instead, which is unnamed, and so called it `anon`: on the
/// default allow-list, and never reported.
fn classify_ident(pass: &Pass<'_>, expr: &Expr, dot_imported_std: &HashSet<String>) -> Option<IFace> {
    let typ = interface_under(pass, expr)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let under = typ.underlying(&artifacts.types);
    let name = type_string(pass, typ);
    // Upstream asks whether the string has a package qualifier. A package's
    // own named type always prints with one — except when the package path is
    // empty, which only a test harness produces — so ask the type as well.
    let has_pkg = match artifacts.types.get(typ) {
        TypeData::Named(n) => n.obj().pkg(&artifacts.objects).is_some(),
        TypeData::Alias(a) => a.obj().pkg(&artifacts.objects).is_some(),
        _ => false,
    };
    let is_named = name.contains('.') || has_pkg;

    if iface_is_empty(pass, typ) && name == "any" {
        return Some(IFace {
            name,
            kind: IFaceKind::Empty,
            of_type: String::new(),
        });
    }
    if name == "error" {
        return Some(IFace {
            name,
            kind: IFaceKind::Error,
            of_type: String::new(),
        });
    }
    // No package qualifier: a type parameter.
    if !is_named {
        let of_type = type_string(pass, under)
            .trim_start_matches("interface{")
            .trim_end_matches('}')
            .trim()
            .to_string();
        return Some(IFace {
            name,
            kind: IFaceKind::Generic,
            of_type,
        });
    }
    // A stdlib interface reached by a bare name only through a dot import.
    let kind = if pkg_of_named(&name)
        .is_some_and(|pkg| is_std_pkg(pkg) && dot_imported_std.contains(pkg))
    {
        IFaceKind::NamedStd
    } else {
        IFaceKind::Named
    };
    Some(IFace {
        name,
        kind,
        of_type: String::new(),
    })
}

/// Upstream's `*ast.SelectorExpr` arm.
fn classify_selector(pass: &Pass<'_>, expr: &Expr) -> Option<IFace> {
    let typ = interface_under(pass, expr)?;
    let name = type_string(pass, typ);
    let kind = if is_std_named_interface(&name) {
        IFaceKind::NamedStd
    } else {
        IFaceKind::Named
    };
    Some(IFace {
        name,
        kind,
        of_type: String::new(),
    })
}

/// Upstream `filterInterfaces`: only three spellings of a result type are
/// looked at. `*I`, `G[int]`, `[]I`, a parenthesized type … are not results to
/// ireturn at all.
fn collect_results(pass: &Pass<'_>, fd: &FuncDecl, dot_imported_std: &HashSet<String>) -> Vec<IFace> {
    let Some(results) = fd.ty.results.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for field in &results.list {
        let Some(ty) = field.ty.as_ref() else {
            continue;
        };
        let issue = match ty {
            Expr::InterfaceType(it) => Some(classify_ast_interface(it)),
            Expr::Ident(_) => classify_ident(pass, ty, dot_imported_std),
            Expr::SelectorExpr(_) => classify_selector(pass, ty),
            _ => None,
        };
        out.extend(issue);
    }
    out
}

fn format_message(func_name: &str, issue: &IFace) -> String {
    if issue.kind == IFaceKind::Generic {
        if issue.of_type.is_empty() {
            format!("{func_name} returns generic interface ({})", issue.name)
        } else {
            format!(
                "{func_name} returns generic interface ({}) of type param {}",
                issue.name, issue.of_type
            )
        }
    } else {
        format!("{} returns interface ({})", func_name, issue.name)
    }
}

struct Validator {
    allow_mode: bool,
    quick: u8,
    patterns: Vec<Regex>,
}

impl Validator {
    fn from_opts(opts: &IreturnOptions) -> Self {
        let (allow_mode, list) = if !opts.reject.is_empty() {
            (false, opts.reject.clone())
        } else if !opts.allow.is_empty() {
            (true, opts.allow.clone())
        } else {
            (true, default_allow())
        };

        let mut quick = 0u8;
        let mut patterns = Vec::new();
        for s in &list {
            match s.as_str() {
                KW_EMPTY => quick |= IFaceKind::Empty as u8,
                KW_ANON => quick |= IFaceKind::Anon as u8,
                KW_ERROR => quick |= IFaceKind::Error as u8,
                KW_STDLIB => quick |= IFaceKind::NamedStd as u8,
                KW_GENERIC => quick |= IFaceKind::Generic as u8,
                _ => {}
            }
            if let Ok(re) = Regex::new(s) {
                patterns.push(re);
            }
        }
        Self {
            allow_mode,
            quick,
            patterns,
        }
    }

    fn has(&self, issue: &IFace) -> bool {
        if self.quick & (issue.kind as u8) != 0 {
            return true;
        }
        // Keywords only match named interfaces via regex.
        if issue.kind != IFaceKind::Named && issue.kind != IFaceKind::NamedStd {
            return false;
        }
        self.patterns.iter().any(|re| re.is_match(&issue.name))
    }

    fn is_valid(&self, issue: &IFace) -> bool {
        if self.allow_mode {
            self.has(issue)
        } else {
            !self.has(issue)
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ireturn requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<IreturnOptions>("ireturn")
        .cloned()
        .unwrap_or_default();
    let validator = Validator::from_opts(&opts);

    let mut pending: Vec<(u32, String)> = Vec::new();
    // Upstream collects dot imports across the whole pass, not per file.
    let dot_imported_std: HashSet<String> = pass
        .files()
        .iter()
        .flat_map(|f| f.imports.iter())
        .filter(|imp| imp.name.as_ref().is_some_and(|n| n.name == "."))
        .map(|imp| imp.path.value.trim_matches('"').to_string())
        .collect();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            if fd.ty.results.is_none() {
                continue;
            }
            let func_name = fd.name.name.as_str();
            // `IFace.Enrich` records `f.Pos()` — the `func` keyword, not the
            // name. `FuncDecl.Pos()` is `d.Type.Pos()` in go/ast.
            let pos = fd.ty.pos().0 as u32;
            let mut seen = std::collections::HashSet::new();
            for issue in collect_results(pass, fd, &dot_imported_std) {
                if validator.is_valid(&issue) {
                    continue;
                }
                let msg = format_message(func_name, &issue);
                let key = format!("{pos}-{msg}");
                if !seen.insert(key) {
                    continue;
                }
                pending.push((pos, msg));
            }
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "ireturn",
        doc: "Accept Interfaces, Return Concrete Types",
        url: "https://github.com/butuzov/ireturn",
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
    fn default_allows_empty_error_anon_stdlib() {
        let v = Validator::from_opts(&IreturnOptions::default());
        assert!(v.is_valid(&IFace {
            name: "interface{}".into(),
            kind: IFaceKind::Empty,
            of_type: String::new(),
        }));
        assert!(v.is_valid(&IFace {
            name: "error".into(),
            kind: IFaceKind::Error,
            of_type: String::new(),
        }));
        assert!(v.is_valid(&IFace {
            name: "anonymous interface".into(),
            kind: IFaceKind::Anon,
            of_type: String::new(),
        }));
        assert!(v.is_valid(&IFace {
            name: "io.Writer".into(),
            kind: IFaceKind::NamedStd,
            of_type: String::new(),
        }));
        assert!(!v.is_valid(&IFace {
            name: "example.Fooer".into(),
            kind: IFaceKind::Named,
            of_type: String::new(),
        }));
    }

    #[test]
    fn reject_empty_flags_interface() {
        let v = Validator::from_opts(&IreturnOptions {
            allow: vec![],
            reject: vec![KW_EMPTY.to_string()],
        });
        assert!(!v.is_valid(&IFace {
            name: "interface{}".into(),
            kind: IFaceKind::Empty,
            of_type: String::new(),
        }));
        assert!(v.is_valid(&IFace {
            name: "example.Fooer".into(),
            kind: IFaceKind::Named,
            of_type: String::new(),
        }));
    }

    #[test]
    fn allow_regex_matches_named() {
        let v = Validator::from_opts(&IreturnOptions {
            allow: vec![r"\.Doer$".to_string()],
            reject: vec![],
        });
        assert!(v.is_valid(&IFace {
            name: "internal/sample.Doer".into(),
            kind: IFaceKind::Named,
            of_type: String::new(),
        }));
        assert!(!v.is_valid(&IFace {
            name: "example.Fooer".into(),
            kind: IFaceKind::Named,
            of_type: String::new(),
        }));
    }

    #[test]
    fn std_pkg_heuristic() {
        assert!(is_std_pkg("io"));
        assert!(is_std_pkg("net/http"));
        assert!(is_std_pkg("go/types"));
        assert!(is_std_pkg("context"));
        assert!(!is_std_pkg("github.com/foo/bar"));
        assert!(!is_std_pkg("golang.org/x/sync"));
        assert!(!is_std_pkg("example.com/pkg"));
    }
}
