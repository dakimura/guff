//! ST1012 — poorly chosen name for error variable.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1012`.

use std::sync::OnceLock;

use guff::ast::{Decl, Expr, Spec};
use guff::token::Token;
use guff_analysis::code::is_call_to_any;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut pending: Vec<(u32, String)> = Vec::new();
    let pkg_path = pass.pkg().pkg_path.as_str();

    for file in pass.files() {
        for decl in &file.decls {
            let Decl::GenDecl(gen) = decl else {
                continue;
            };
            if gen.tok != Some(Token::VAR) {
                continue;
            }
            for spec in &gen.specs {
                let Spec::ValueSpec(spec) = spec else {
                    continue;
                };
                if spec.names.len() != spec.values.len() {
                    continue;
                }
                for (i, name) in spec.names.iter().enumerate() {
                    let Expr::CallExpr(call) = &spec.values[i] else {
                        continue;
                    };
                    if !is_call_to_any(pass, call, &["errors.New", "fmt.Errorf"]) {
                        continue;
                    }
                    if pkg_path == "net/http" && name.name.starts_with("http2err") {
                        continue;
                    }
                    let prefix = if is_exported(&name.name) {
                        "Err"
                    } else {
                        "err"
                    };
                    if !name.name.starts_with(prefix) {
                        pending.push((
                            name.pos().0 as u32,
                            format!(
                                "error var {} should have name of the form {}Foo",
                                name.name, prefix
                            ),
                        ));
                    }
                }
            }
        }
    }

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn st1012_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1012",
        doc: "poorly chosen name for error variable",
        url: "https://staticcheck.dev/docs/checks/#ST1012",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1012_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1012_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
