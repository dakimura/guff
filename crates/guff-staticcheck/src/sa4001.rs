//! SA4001 — &*x gets simplified to x, it does not copy x
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4001`.

use std::sync::OnceLock;

use guff_pattern::{must_parse, Pattern};
use guff_analysis::passes::inspect;
use guff_analysis::{entry_mask, match_pattern, match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};


static PAT1: OnceLock<Pattern> = OnceLock::new();
static PAT2: OnceLock<Pattern> = OnceLock::new();

fn pat1() -> &'static Pattern { PAT1.get_or_init(|| must_parse(r#"(UnaryExpr "&" (StarExpr obj))"#)) }
fn pat2() -> &'static Pattern { PAT2.get_or_init(|| must_parse(r#"(StarExpr (UnaryExpr "&" _))"#)) }

fn is_cgo_ident(name: &str) -> bool {
    name.starts_with("_Cfunc_") || name.starts_with("_Cvar_")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4001 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(
        entry_mask(pat1()).union(entry_mask(pat2())),
        pass.files(),
        |node| {
        if let Some(m) = match_pattern(pass, pat1(), node) {
            let skip = m.state.get("obj").and_then(|v| v.as_ident()).is_some_and(|id| is_cgo_ident(&id.name));
            if !skip {
                pending.push((match_pos(node), "&*x will be simplified to x. It will not copy x.".into()));
            }
        } else if match_pattern(pass, pat2(), node).is_some() {
            pending.push((match_pos(node), "*&x will be simplified to x. It will not copy x.".into()));
        }
    });
    for (pos, msg) in pending { pass.reportf(pos, msg); }
    Ok(None)
}


fn sa4001_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4001",
        doc: "&*x gets simplified to x, it does not copy x",
        url: "https://staticcheck.dev/docs/checks/#SA4001",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4001_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4001_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
