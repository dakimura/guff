//! S1039 — unnecessary use of `fmt.Sprint`.
//!
//! Port of `honnef.co/go/tools/simple/s1039`.
//!
//! **Parentheses.** Upstream states this check as a `pattern` query, and
//! `pattern.match` strips `*ast.ParenExpr` at every recursion (before binding),
//! so `f((x))` matches wherever `f(x)` does. This port descends by hand, so
//! every descent has to `unparen` — `compat/fuzz.py`'s `paren` mutation found
//! nine S-checks going quiet on a parenthesized subexpression at once
//! (COMPAT-HARDENING §4, 2026-08-13).

use std::sync::OnceLock;

use guff::ast::{BasicLit, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{self, expr_to_string, unparen};
use guff_analysis::passes::inspect;
use guff_analysis::{
    match_pos, AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::render::render_node;

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1039 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String, Option<TextEdit>)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        let Some(cname) = guff_analysis::code::call_name(pass, &call.fun) else {
            return;
        };
        let short = match cname.as_str() {
            "fmt.Sprint" => "Sprint",
            "fmt.Sprintf" => "Sprintf",
            _ => return,
        };
        if call.args.len() != 1 {
            return;
        }
        let arg = unparen(&call.args[0]);
        let Expr::BasicLit(BasicLit { .. }) = arg else {
            return;
        };
        let Some(val) = expr_to_string(pass, arg) else {
            return;
        };
        // Match upstream: only Sprintf treats '%' as a possible format string.
        if short == "Sprintf" && val.contains('%') {
            return;
        }
        // `edit.ReplaceWithNode(fset, node, lit)`: the whole call goes and the
        // literal takes its place. `lit` is the pattern's binding, which is
        // made *after* parens are stripped — so `fmt.Sprint(("x"))` becomes
        // `"x"`, not `("x")`. `arg` is already unparenthesized for the same
        // reason.
        let edit = render_node(pass, arg).map(|text| TextEdit {
            pos: call.pos().0 as u32,
            end: call.end().0 as u32,
            new_text: text,
        });
        pending.push((
            match_pos(node),
            format!("unnecessary use of fmt.{short}"),
            edit,
        ));
    });
    for (pos, message, edit) in pending {
        let Some(edit) = edit else {
            pass.report_unless_generated(pos, message);
            continue;
        };
        if code::is_generated_at(pass, pos) {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Replace with string literal".into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn s1039_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1039",
        doc: "unnecessary use of fmt.Sprint",
        url: "https://staticcheck.dev/docs/checks/#S1039",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1039_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1039_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
