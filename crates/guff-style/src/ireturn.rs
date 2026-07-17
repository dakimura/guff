//! Port of [`github.com/butuzov/ireturn`](https://github.com/butuzov/ireturn)
//! (golangci-lint wrapper in `pkg/golinters/ireturn`).
//!
//! "Accept Interfaces, Return Concrete Types" — report functions that return
//! interfaces. Default allow-list: `anon`, `error`, `empty`, `stdlib`.
//!
//! DEFERRED: full upstream std package table (we use the Go "first path element
//! has no `.`" heuristic); collision error when both `allow` and `reject` are
//! set (prefer `reject`); per-func `//nolint:ireturn` (guff CLI nolint covers
//! this); generic type-param `OfType` detail string parity.

use std::sync::OnceLock;

use guff::ast::{Decl, Expr, FuncDecl, InterfaceType};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::predicates::is_type_param;
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

/// Go / `golang.org/x/mod` heuristic: std packages have no `.` in the first path element.
fn is_std_pkg(pkg: &str) -> bool {
    if pkg.is_empty() {
        return false;
    }
    let elem = pkg.split('/').next().unwrap_or(pkg);
    !elem.contains('.')
}

fn pkg_of_named(named: &str) -> Option<&str> {
    let idx = named.rfind('.')?;
    Some(&named[..idx])
}

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

fn classify_typed(pass: &Pass<'_>, expr: &Expr) -> Option<IFace> {
    let typ = type_of_expr(pass, expr)?;
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return None;
    };

    // Resolve aliases so `type E = error` still classifies as error.
    let resolved = match artifacts.types.get(typ) {
        TypeData::Alias(_) => typ.underlying(&artifacts.types),
        _ => typ,
    };
    let under = resolved.underlying(&artifacts.types);
    let name = type_string(pass, typ);

    // Type parameters → generic keyword.
    if is_type_param(&artifacts.types, typ) || is_type_param(&artifacts.types, resolved) {
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

    if !matches!(artifacts.types.get(under), TypeData::Interface(_)) {
        return None;
    }

    // `any` / empty interface
    if iface_is_empty(pass, resolved) && (name == "any" || name == "interface{}") {
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

    // Named interface (same-package types may print without a `pkg.` prefix when
    // the fixture package path is empty — still Named, not Generic).
    let is_named_type = matches!(
        artifacts.types.get(resolved),
        TypeData::Named(_) | TypeData::Alias(_)
    );
    if is_named_type {
        // Prefer package path from the type object for stdlib detection.
        let pkg_path = match artifacts.types.get(resolved) {
            TypeData::Named(n) => {
                let obj = n.obj();
                obj.pkg(&artifacts.objects)
                    .map(|p| artifacts.packages.get(p).path().to_string())
            }
            TypeData::Alias(a) => {
                let obj = a.obj();
                obj.pkg(&artifacts.objects)
                    .map(|p| artifacts.packages.get(p).path().to_string())
            }
            _ => None,
        };
        if pkg_path.as_deref().is_some_and(is_std_pkg) || is_std_named_interface(&name) {
            return Some(IFace {
                name,
                kind: IFaceKind::NamedStd,
                of_type: String::new(),
            });
        }
        return Some(IFace {
            name,
            kind: IFaceKind::Named,
            of_type: String::new(),
        });
    }

    // Unnamed interface from types info (shouldn't normally reach here).
    if iface_is_empty(pass, resolved) {
        Some(IFace {
            name: if name.is_empty() {
                "interface{}".to_string()
            } else {
                name
            },
            kind: IFaceKind::Empty,
            of_type: String::new(),
        })
    } else {
        Some(IFace {
            name: "anonymous interface".to_string(),
            kind: IFaceKind::Anon,
            of_type: String::new(),
        })
    }
}

fn collect_results(pass: &Pass<'_>, fd: &FuncDecl) -> Vec<IFace> {
    let Some(results) = fd.ty.results.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for field in &results.list {
        let Some(ty) = field.ty.as_ref() else {
            continue;
        };
        match ty {
            Expr::InterfaceType(it) => out.push(classify_ast_interface(it)),
            Expr::Ident(_) | Expr::SelectorExpr(_) => {
                if let Some(issue) = classify_typed(pass, ty) {
                    out.push(issue);
                }
            }
            _ => {
                // StarExpr / IndexExpr etc. — try types info.
                if let Some(issue) = classify_typed(pass, ty) {
                    out.push(issue);
                }
            }
        }
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
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            if fd.ty.results.is_none() {
                continue;
            }
            let func_name = fd.name.name.as_str();
            let pos = fd.name.pos().0 as u32;
            let mut seen = std::collections::HashSet::new();
            for issue in collect_results(pass, fd) {
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
