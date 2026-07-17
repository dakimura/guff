//! Port of [`github.com/securego/gosec`](https://github.com/securego/gosec)
//! (golangci-lint wrapper in `pkg/golinters/gosec`).
//!
//! First batch (AST / types-info only):
//! - **G103** — `unsafe` calls (`Pointer` / `String` / `StringData` / `Slice` / `SliceData`)
//! - **G401** — weak hash (`crypto/md5` / `crypto/sha1` `New`/`Sum`)
//! - **G404** — weak RNG (`math/rand` / `math/rand/v2`)
//! - **G405** — weak encryption (`crypto/des` / `crypto/rc4`)
//! - **G406** — deprecated weak hash (`golang.org/x/crypto/{md4,ripemd160}`)
//! - **G501–G507** — blocklisted imports
//!
//! Message format matches golangci: `"Gxxx: <what>"`.
//!
//! DEFERRED: remaining rules (G101 credentials, G104, G106–G117, G201–G307, G402–G403,
//! G601, SSA analyzers), `severity`/`confidence` filters, `config` map, concurrency.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{CallExpr, Decl, Expr, Ident, Spec};
use guff::walk::{preorder, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::ObjectData;

use crate::options::GosecOptions;

struct RuleDef {
    id: &'static str,
    /// For call rules: `(pkg_path, func_name)`.
    calls: &'static [(&'static str, &'static str)],
    /// For import rules: `(import_path, description)`.
    imports: &'static [(&'static str, &'static str)],
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
        call_what: "Use of unsafe calls should be audited",
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
        call_what: "Use of weak cryptographic primitive",
    },
    RuleDef {
        id: "G406",
        calls: &[
            ("golang.org/x/crypto/md4", "New"),
            ("golang.org/x/crypto/ripemd160", "New"),
        ],
        imports: &[],
        call_what: "Use of deprecated weak cryptographic primitive",
    },
    RuleDef {
        id: "G501",
        calls: &[],
        imports: &[(
            "crypto/md5",
            "Blocklisted import crypto/md5: weak cryptographic primitive",
        )],
        call_what: "",
    },
    RuleDef {
        id: "G502",
        calls: &[],
        imports: &[(
            "crypto/des",
            "Blocklisted import crypto/des: weak cryptographic primitive",
        )],
        call_what: "",
    },
    RuleDef {
        id: "G503",
        calls: &[],
        imports: &[(
            "crypto/rc4",
            "Blocklisted import crypto/rc4: weak cryptographic primitive",
        )],
        call_what: "",
    },
    RuleDef {
        id: "G504",
        calls: &[],
        imports: &[(
            "net/http/cgi",
            "Blocklisted import net/http/cgi: Go versions < 1.6.3 are vulnerable to Httpoxy attack: (CVE-2016-5386)",
        )],
        call_what: "",
    },
    RuleDef {
        id: "G505",
        calls: &[],
        imports: &[(
            "crypto/sha1",
            "Blocklisted import crypto/sha1: weak cryptographic primitive",
        )],
        call_what: "",
    },
    RuleDef {
        id: "G506",
        calls: &[],
        imports: &[(
            "golang.org/x/crypto/md4",
            "Blocklisted import golang.org/x/crypto/md4: deprecated and weak cryptographic primitive",
        )],
        call_what: "",
    },
    RuleDef {
        id: "G507",
        calls: &[],
        imports: &[(
            "golang.org/x/crypto/ripemd160",
            "Blocklisted import golang.org/x/crypto/ripemd160: deprecated and weak cryptographic primitive",
        )],
        call_what: "",
    },
];

fn enabled_rules(opts: &GosecOptions) -> HashSet<&'static str> {
    let mut ids: HashSet<&'static str> = RULES.iter().map(|r| r.id).collect();
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
            pending.push((
                call.pos().0 as u32,
                format!("{}: {}", rule.id, rule.call_what),
            ));
        }
    }
}

fn check_imports(pass: &Pass<'_>, enabled: &HashSet<&'static str>, pending: &mut Vec<(u32, String)>) {
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
                for rule in RULES {
                    if !enabled.contains(rule.id) {
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

    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::CallExpr(call) = n {
                check_call(pass, call, &enabled, &mut pending);
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
        name: "gosec",
        doc: "Inspects source code for security problems",
        url: "https://github.com/securego/gosec",
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
    fn includes_filters_rules() {
        let opts = GosecOptions {
            includes: vec!["G501".into()],
            excludes: vec![],
        };
        let e = enabled_rules(&opts);
        assert!(e.contains("G501"));
        assert!(!e.contains("G103"));
        assert!(!e.contains("G404"));
    }

    #[test]
    fn excludes_removes_rules() {
        let opts = GosecOptions {
            includes: vec![],
            excludes: vec!["G501".into(), "G505".into()],
        };
        let e = enabled_rules(&opts);
        assert!(!e.contains("G501"));
        assert!(!e.contains("G505"));
        assert!(e.contains("G103"));
    }

    #[test]
    fn split_fq_handles_dotted_paths() {
        let (pkg, name) = split_fq_name("golang.org/x/crypto/md4.New").unwrap();
        assert_eq!(pkg, "golang.org/x/crypto/md4");
        assert_eq!(name, "New");
    }
}
