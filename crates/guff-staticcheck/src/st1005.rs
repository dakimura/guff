//! ST1005 — incorrectly formatted error string.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1005`.
//! AST-based (upstream uses buildir); string constant args to `errors.New` /
//! `fmt.Errorf` are inspected directly. Package type/func names are collected
//! from AST to allow capitalized proper nouns / local identifiers.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{Decl, Spec};
use guff::walk::NodeRef;
use guff_analysis::code::{expr_to_string, is_call_to_any};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn is_test_file(path: &str) -> bool {
    path.ends_with("_test.go")
}

fn collect_obj_names(pass: &Pass<'_>) -> HashSet<String> {
    let mut names = HashSet::new();
    for file in pass.files() {
        for decl in &file.decls {
            match decl {
                Decl::FuncDecl(fd) => {
                    names.insert(fd.name.name.clone());
                }
                Decl::GenDecl(gen) => {
                    for spec in &gen.specs {
                        if let Spec::TypeSpec(ts) = spec {
                            names.insert(ts.name.name.clone());
                        }
                    }
                }
                Decl::BadDecl(_) => {}
            }
        }
    }
    names
}

fn check_error_string(
    s: &str,
    obj_names: &HashSet<String>,
    pending: &mut Vec<(u32, String)>,
    pos: u32,
) {
    if s.is_empty() {
        return;
    }
    match s.as_bytes()[s.len() - 1] {
        b'.' | b':' | b'!' | b'\n' => {
            pending.push((
                pos,
                "error strings should not end with punctuation or newlines".into(),
            ));
        }
        _ => {}
    }

    let Some((word, _)) = s.split_once(' ') else {
        return;
    };
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return;
    };
    if !first.is_uppercase() {
        return;
    }
    for c in chars {
        if c.is_uppercase() || c.is_ascii_digit() {
            return;
        }
    }
    if word.contains('(') {
        return;
    }
    let trimmed: String = word
        .chars()
        .rev()
        .skip_while(|c| c.is_ascii_punctuation())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if obj_names.contains(&trimmed) {
        return;
    }
    pending.push((pos, "error strings should not be capitalized".into()));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ST1005 requires inspect analyzer".to_string())?
        .clone();

    let obj_names = collect_obj_names(pass);
    let mut pending = Vec::new();

    let go_files = &pass.pkg().go_files;
    for (fi, file) in pass.files().iter().enumerate() {
        if go_files
            .get(fi)
            .is_some_and(|p| is_test_file(p.to_string_lossy().as_ref()))
        {
            continue;
        }
        inspect.preorder(std::slice::from_ref(file), |node| {
            let NodeRef::CallExpr(call) = node else {
                return;
            };
            if !is_call_to_any(pass, call, &["errors.New", "fmt.Errorf"]) {
                return;
            }
            if call.args.is_empty() {
                return;
            }
            let Some(s) = expr_to_string(pass, &call.args[0]) else {
                return;
            };
            check_error_string(&s, &obj_names, &mut pending, call.pos().0 as u32);
        });
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

fn st1005_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1005",
        doc: "incorrectly formatted error string",
        url: "https://staticcheck.dev/docs/checks/#ST1005",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1005_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1005_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
