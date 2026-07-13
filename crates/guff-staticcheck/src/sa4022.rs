//! SA4022 — comparing the address of a variable against nil
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4022`.

use std::sync::OnceLock;

use guff::ast::{BinaryExpr, Expr, UnaryExpr};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_pattern::{must_parse, Pattern};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, matches, AnalysisResult, Analyzer, RunError, RunFn, Pass};

static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| must_parse(r#"(BinaryExpr (UnaryExpr "&" _) (Or "==" "!=") (Or nil (Ident "nil")))"#))
}

fn is_addr_nil_compare(bin: &BinaryExpr) -> bool {
    if !matches!(bin.op, Token::EQL | Token::NEQ) {
        return false;
    }
    let check = |x: &Expr, y: &Expr| {
        matches!(x, Expr::UnaryExpr(UnaryExpr { op: Token::AND, .. })) && is_nil_expr(y)
    };
    check(&bin.x, &bin.y) || check(&bin.y, &bin.x)
}

fn is_nil_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(id) if id.name == "nil")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4022 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    matches(pass, &inspect, pat(), |node, _| {
        pending.push((match_pos(node), "the address of a variable cannot be nil".into()));
        true
    });
    inspect.preorder(pass.files(), |node| {
        let NodeRef::BinaryExpr(bin) = node else {
            return;
        };
        if !is_addr_nil_compare(bin) {
            return;
        }
        pending.push((
            bin.op_pos.0 as u32,
            "the address of a variable cannot be nil".into(),
        ));
    });
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4022_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4022",
        doc: "comparing the address of a variable against nil",
        url: "https://staticcheck.dev/docs/checks/#SA4022",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4022_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4022_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
