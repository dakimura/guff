//! S1018 — use copy for sliding elements in a slice.
//!
//! Port of `honnef.co/go/tools/simple/s1018`.

use std::sync::OnceLock;

use guff_pattern::{must_parse, Pattern};
use guff_analysis::callcheck::is_slice_type;
use guff_analysis::passes::inspect;
use guff_analysis::{match_pattern, match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| {
        must_parse(
            r#"(ForStmt (AssignStmt initvar@(Ident _) _ (IntegerLiteral "0")) (BinaryExpr initvar "<" limit@(Ident _)) (IncDecStmt initvar "++") [(AssignStmt (IndexExpr slice@(Ident _) initvar) "=" (IndexExpr slice (BinaryExpr offset@(Ident _) "+" initvar)))])"#,
        )
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1018 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let Some(m) = match_pattern(pass, pat(), node) else {
            return;
        };
        let Some(slice) = m.state.get("slice").and_then(|v| v.as_ident()) else {
            return;
        };
        let Some(typ) = pass
            .types_info()
            .and_then(|info| info.types.get(&slice.id).map(|tv| tv.typ))
        else {
            return;
        };
        let types = &pass.pkg().type_artifacts.as_ref().expect("types").types;
        if !is_slice_type(types, typ) {
            return;
        }
        pending.push((
            match_pos(node),
            "should use copy() instead of loop for sliding slice elements".into(),
        ));
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1018_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1018",
        doc: "use copy for sliding elements in a slice",
        url: "https://staticcheck.dev/docs/checks/#S1018",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1018_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1018_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
