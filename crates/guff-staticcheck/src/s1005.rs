//! S1005 — drop unnecessary use of the blank identifier.
//!
//! Port of `honnef.co/go/tools/simple/s1005`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, Ident, RangeStmt};
use guff::walk::NodeRef;
use guff_pattern::{must_parse, Pattern};
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{match_pattern, match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

static PAT_BLANK_RECV1: OnceLock<Pattern> = OnceLock::new();
static PAT_BLANK_RECV2: OnceLock<Pattern> = OnceLock::new();

fn pat_blank_recv1() -> &'static Pattern {
    PAT_BLANK_RECV1.get_or_init(|| {
        must_parse(r#"(AssignStmt [_ (Ident "_")] _ (UnaryExpr "<-" _))"#)
    })
}

fn pat_blank_recv2() -> &'static Pattern {
    PAT_BLANK_RECV2
        .get_or_init(|| must_parse(r#"(AssignStmt (Ident "_") _ recv@(UnaryExpr "<-" _))"#))
}

fn is_blank(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Expr::Ident(ident) = expr else {
        return false;
    };
    ident.name == "_" && object_of(pass, ident).is_none()
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1005 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();

    inspect.preorder(pass.files(), |node| {
        if match_pattern(pass, pat_blank_recv1(), node).is_some() {
            pending.push((
                match_pos(node),
                "unnecessary assignment to the blank identifier".into(),
            ));
        }
    });

    inspect.preorder(pass.files(), |node| {
        let NodeRef::AssignStmt(assign) = node else {
            return;
        };
        if let Some(_m) = match_pattern(pass, pat_blank_recv2(), node) {
            pending.push((
                match_pos(node),
                "unnecessary assignment to the blank identifier".into(),
            ));
        }
    });

    inspect.preorder(pass.files(), |node| {
        let NodeRef::RangeStmt(rs) = node else {
            return;
        };
        check_range_blank(pass, rs, &mut pending);
    });

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn check_range_blank(pass: &Pass<'_>, rs: &RangeStmt, pending: &mut Vec<(u32, String)>) {
    if rs.value.is_none() && rs.key.as_ref().is_some_and(|k| is_blank(pass, k)) {
        pending.push((
            rs.key.as_ref().map(|k| k.pos().0 as u32).unwrap_or(0),
            "unnecessary assignment to the blank identifier".into(),
        ));
    }
    if rs
        .key
        .as_ref()
        .is_some_and(|k| is_blank(pass, k))
        && rs.value.as_ref().is_some_and(|v| is_blank(pass, v))
    {
        pending.push((
            rs.key.as_ref().map(|k| k.pos().0 as u32).unwrap_or(0),
            "unnecessary assignment to the blank identifier".into(),
        ));
    }
    if rs
        .key
        .as_ref()
        .is_some_and(|k| !is_blank(pass, k))
        && rs.value.as_ref().is_some_and(|v| is_blank(pass, v))
    {
        pending.push((
            rs.value.as_ref().map(|v| v.pos().0 as u32).unwrap_or(0),
            "unnecessary assignment to the blank identifier".into(),
        ));
    }
}

fn s1005_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1005",
        doc: "drop unnecessary use of the blank identifier",
        url: "https://staticcheck.dev/docs/checks/#S1005",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1005 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1005_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1005_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
