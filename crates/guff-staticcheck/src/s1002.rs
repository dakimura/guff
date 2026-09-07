//! S1002 — omit comparison with boolean constant.
//!
//! Port of `honnef.co/go/tools/simple/s1002`.

use std::sync::OnceLock;

use guff::ast::{BinaryExpr, Expr};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{self, bool_const, is_bool_const};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_analysis::Pass;
use guff_types::predicates::is_boolean;

fn expr_is_bool(pass: &Pass<'_>, expr: &Expr) -> bool {
    let info = match pass.types_info() {
        Some(i) => i,
        None => return false,
    };
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return false,
    };
    let tav = match info.types.get(&expr.id()) {
        Some(t) => t,
        None => return false,
    };
    is_boolean(
        &artifacts.types,
        tav.typ.underlying(&artifacts.types),
    )
}

/// `op + report.Render(pass, other)`, then upstream's parity trim.
///
/// Upstream renders the operand with go/printer, so *whatever* the expression
/// is, the message quotes its source. guff had a five-arm printer here —
/// `Ident`, `ParenExpr`, `UnaryExpr(!)`, `SelectorExpr`, `CallExpr` — and
/// everything else came out as the literal string `<expr>`. That is not a
/// cosmetic loss: the message is a *suggestion*, and `can be simplified to
/// !<expr>` tells the reader nothing. On hashicorp/packer,
/// `*corePP.KeepInputArtifact == false` was reported by both tools on the same
/// line and counted as a divergence on both sides, because guff wrote
/// `!<expr>` where upstream wrote `!*corePP.KeepInputArtifact`.
///
/// `crate::render::render_node` is the go/printer path (`format::node`), which
/// is what `report.Render` is. The shared `render::render_expr` approximation
/// is the fallback for the case the printer itself fails, and it already knows
/// the shapes this file did not (`StarExpr`, `IndexExpr`, `TypeAssertExpr`, …).
fn simplified_condition(pass: &Pass<'_>, op: Token, const_val: bool, other: &Expr) -> String {
    let rendered = crate::render::render_node(pass, other)
        .unwrap_or_else(|| crate::render::render_expr(other));
    collapse_negations(op, const_val, &rendered)
}

/// `op + rendered`, with upstream's parity trim on the leading `!`s.
///
/// `strings.TrimLeft(r, "!")` then re-prefixes one `!` when an odd number came
/// off, so `!!x == false` reads `!x` rather than `!!!x`. Split out from
/// [`simplified_condition`] because the rendering half needs a `Pass` and this
/// half is pure.
fn collapse_negations(op: Token, const_val: bool, rendered: &str) -> String {
    let negate = matches!(
        (op, const_val),
        (Token::EQL, false) | (Token::NEQ, true)
    );
    let with_op = format!("{}{}", if negate { "!" } else { "" }, rendered);
    let orig_len = with_op.len();
    let trimmed = with_op.trim_start_matches('!');
    let leading_bangs = orig_len - trimmed.len();
    if leading_bangs % 2 == 1 {
        format!("!{trimmed}")
    } else {
        trimmed.to_string()
    }
}

fn check_binary(pass: &Pass<'_>, expr: &BinaryExpr) -> Option<(u32, String, TextEdit)> {
    if expr.op != Token::EQL && expr.op != Token::NEQ {
        return None;
    }
    // Match honnef `code.IsBoolConst`: only untyped/predeclared bool idents
    // (`true`/`false`/aliases of those). Named bool types used as enums are
    // intentionally skipped (false-negative bias).
    let x_const = is_bool_const(pass, &expr.x);
    let y_const = is_bool_const(pass, &expr.y);
    if !x_const && !y_const {
        return None;
    }
    let (other, val) = if x_const {
        (&expr.y, bool_const(pass, &expr.x))
    } else {
        (&expr.x, bool_const(pass, &expr.y))
    };
    if !expr_is_bool(pass, other) {
        return None;
    }
    let simplified = simplified_condition(pass, expr.op, val, other);
    // `edit.ReplaceWithString(expr, r)` where `r` is the same string the
    // message quotes — the whole comparison goes and the simplified condition
    // takes its place.
    let edit = TextEdit {
        pos: expr.x.pos().0 as u32,
        end: expr.y.end().0 as u32,
        new_text: simplified.clone(),
    };
    Some((
        expr.x.pos().0 as u32,
        format!(
            "should omit comparison to bool constant, can be simplified to {simplified}"
        ),
        edit,
    ))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1002 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String, TextEdit)> = Vec::new();
    // Upstream `simple/s1002` skips `_test.go` (`code.IsInTest`).
    let compiled = &pass.pkg().compiled_go_files;
    for (fi, file) in pass.files().iter().enumerate() {
        if compiled
            .get(fi)
            .is_some_and(|p| p.to_string_lossy().ends_with("_test.go"))
        {
            continue;
        }
        inspect.preorder_typed(node_mask!(BinaryExpr), std::slice::from_ref(file), |n| {
            let NodeRef::BinaryExpr(expr) = n else {
                return;
            };
            if let Some((pos, msg, edit)) = check_binary(pass, expr) {
                pending.push((pos, msg, edit));
            }
        });
    }
    for (pos, message, edit) in pending {
        // `report.FilterGenerated()`: same gate as `reportf`, spelled out here
        // because the diagnostic carries a fix.
        if code::is_generated_at(pass, pos) {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Simplify bool comparison".into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn s1002_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1002",
        doc: "omit comparison with boolean constant",
        url: "https://staticcheck.dev/docs/checks/#S1002",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1002 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1002_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1002_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    /// The parity trim, on the four (op, constant) combinations and on an
    /// operand that already carries `!`s. The rendering half is asserted
    /// per-shape against golangci-lint in
    /// `tests/checks_test.rs::s1002_message_renders_every_operand_shape`.
    #[test]
    fn collapse_negations_examples() {
        assert_eq!(collapse_negations(Token::EQL, true, "x"), "x");
        assert_eq!(collapse_negations(Token::EQL, false, "x"), "!x");
        assert_eq!(collapse_negations(Token::NEQ, true, "x"), "!x");
        assert_eq!(collapse_negations(Token::NEQ, false, "x"), "x");
        // `!x == false` is `!!x` before the trim: two bangs, so none survive.
        assert_eq!(collapse_negations(Token::EQL, false, "!x"), "x");
        // `!!x == false` is `!!!x`: three, so one survives.
        assert_eq!(collapse_negations(Token::EQL, false, "!!x"), "!x");
        // A `!` that is not leading is not the operator's, and stays put.
        assert_eq!(collapse_negations(Token::EQL, false, "(!x)"), "!(!x)");
    }
}
