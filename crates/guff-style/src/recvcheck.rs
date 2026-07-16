//! Port of [`github.com/raeperd/recvcheck`](https://github.com/raeperd/recvcheck)
//! (golangci-lint wrapper in `pkg/golinters/recvcheck`).
//!
//! Checks that methods of a named type do not mix pointer and value receivers.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{Decl, Expr, FieldList, Spec};
use guff::token::Token;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::RecvcheckOptions;

/// Built-in method excludes (Unmarshal*/GobDecode) — upstream default when
/// `disable-builtin` is false. See https://github.com/raeperd/recvcheck/issues/17
const BUILTIN_EXCLUSIONS: &[&str] = &[
    "*.UnmarshalText",
    "*.UnmarshalJSON",
    "*.UnmarshalYAML",
    "*.UnmarshalXML",
    "*.UnmarshalBinary",
    "*.GobDecode",
];

#[derive(Default)]
struct StructReceivers {
    star_used: bool,
    type_used: bool,
    /// Type-name position for reporting (from TYPE decl when available).
    pos: Option<u32>,
}

fn build_excluded(opts: &RecvcheckOptions) -> HashSet<String> {
    let mut excluded = HashSet::new();
    if !opts.disable_builtin {
        for e in BUILTIN_EXCLUSIONS {
            excluded.insert((*e).to_string());
        }
    }
    for e in &opts.exclusions {
        excluded.insert(e.clone());
    }
    excluded
}

fn is_excluded(excluded: &HashSet<String>, recv_name: &str, method_name: &str) -> bool {
    if method_name.is_empty() {
        return true;
    }
    excluded.contains(&format!("{recv_name}.{method_name}"))
        || excluded.contains(&format!("*.{method_name}"))
}

fn recv_type_ident(ty: &Expr) -> Option<(&guff::ast::Ident, bool)> {
    match ty {
        Expr::StarExpr(star) => {
            if let Expr::Ident(id) = star.x.as_ref() {
                Some((id, true))
            } else {
                None
            }
        }
        Expr::Ident(id) => Some((id, false)),
        _ => None,
    }
}

fn collect_type_positions(pass: &Pass<'_>) -> HashMap<String, u32> {
    let mut out = HashMap::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::GenDecl(gen) = decl else {
                continue;
            };
            if gen.tok != Some(Token::TYPE) {
                continue;
            }
            for spec in &gen.specs {
                let Spec::TypeSpec(ts) = spec else {
                    continue;
                };
                out.insert(ts.name.name.clone(), ts.name.pos().0 as u32);
            }
        }
    }
    out
}

fn check_method(
    recv: &FieldList,
    method_name: &str,
    excluded: &HashSet<String>,
    type_pos: &HashMap<String, u32>,
    structs: &mut HashMap<String, StructReceivers>,
) {
    if recv.list.len() != 1 {
        return;
    }
    let Some(ty) = recv.list[0].ty.as_ref() else {
        return;
    };
    let Some((ident, is_star)) = recv_type_ident(ty) else {
        return;
    };
    if is_excluded(excluded, &ident.name, method_name) {
        return;
    }

    let entry = structs.entry(ident.name.clone()).or_default();
    if entry.pos.is_none() {
        entry.pos = type_pos
            .get(&ident.name)
            .copied()
            .or(Some(ident.pos().0 as u32));
    }
    if is_star {
        entry.star_used = true;
    } else {
        entry.type_used = true;
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "recvcheck requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<RecvcheckOptions>("recvcheck")
        .cloned()
        .unwrap_or_default();
    let excluded = build_excluded(&opts);
    let type_pos = collect_type_positions(pass);

    let mut structs: HashMap<String, StructReceivers> = HashMap::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            let Some(recv) = fd.recv.as_ref() else {
                continue;
            };
            check_method(recv, &fd.name.name, &excluded, &type_pos, &mut structs);
        }
    }

    let mut pending: Vec<(u32, String)> = structs
        .into_iter()
        .filter(|(_, st)| st.star_used && st.type_used)
        .filter_map(|(name, st)| {
            let pos = st.pos?;
            Some((
                pos,
                format!(
                    "the methods of \"{name}\" use pointer receiver and non-pointer receiver."
                ),
            ))
        })
        .collect();
    pending.sort_by_key(|(pos, _)| *pos);

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "recvcheck",
        doc: "checks for receiver type consistency",
        url: "https://github.com/raeperd/recvcheck",
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
    fn builtin_exclusions_on_by_default() {
        let excluded = build_excluded(&RecvcheckOptions::default());
        assert!(is_excluded(&excluded, "JSON", "UnmarshalJSON"));
        assert!(is_excluded(&excluded, "Anything", "GobDecode"));
        assert!(!is_excluded(&excluded, "JSON", "MarshalJSON"));
    }

    #[test]
    fn disable_builtin_clears_defaults() {
        let excluded = build_excluded(&RecvcheckOptions {
            disable_builtin: true,
            exclusions: Vec::new(),
        });
        assert!(!is_excluded(&excluded, "JSON", "UnmarshalJSON"));
    }

    #[test]
    fn custom_exclusions_struct_and_wildcard() {
        let excluded = build_excluded(&RecvcheckOptions {
            disable_builtin: true,
            exclusions: vec!["SQL.Value".into(), "*.Scan".into()],
        });
        assert!(is_excluded(&excluded, "SQL", "Value"));
        assert!(!is_excluded(&excluded, "Other", "Value"));
        assert!(is_excluded(&excluded, "SQL", "Scan"));
        assert!(is_excluded(&excluded, "Other", "Scan"));
    }
}
