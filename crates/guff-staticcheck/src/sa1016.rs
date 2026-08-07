//! SA1016 — trapping a signal that cannot be trapped.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1016`.

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to, is_call_to_any, selector_name};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

const SIGNAL_CALLS: &[&str] = &["os/signal.Ignore", "os/signal.Notify", "os/signal.Reset"];

fn unwrap_signal_arg<'a>(pass: &Pass<'_>, expr: &'a Expr) -> &'a Expr {
    if let Expr::CallExpr(call) = expr {
        if is_call_to(pass, call, "os.Signal") {
            if let Some(arg) = call.args.first() {
                return arg;
            }
        }
    }
    expr
}

fn signal_selector_name(pass: &Pass<'_>, expr: &Expr) -> Option<String> {
    let Expr::SelectorExpr(sel) = expr else {
        return None;
    };
    selector_name(pass, sel)
}

fn is_sigterm(pass: &Pass<'_>, expr: &Expr) -> bool {
    signal_selector_name(pass, expr).as_deref() == Some("syscall.SIGTERM")
}

fn is_sigkill(pass: &Pass<'_>, expr: &Expr) -> bool {
    matches!(
        signal_selector_name(pass, expr).as_deref(),
        Some("os.Kill") | Some("syscall.SIGKILL")
    )
}

fn is_sigstop(pass: &Pass<'_>, expr: &Expr) -> bool {
    signal_selector_name(pass, expr).as_deref() == Some("syscall.SIGSTOP")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA1016 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if !is_call_to_any(pass, call, SIGNAL_CALLS) {
            return;
        }

        let has_sigterm = call
            .args
            .iter()
            .any(|arg| is_sigterm(pass, unwrap_signal_arg(pass, arg)));

        for arg in &call.args {
            let arg = unwrap_signal_arg(pass, arg);
            if is_sigkill(pass, arg) {
                let rendered = render_signal(pass, arg);
                let hint = if has_sigterm {
                    String::new()
                } else {
                    " (did you mean syscall.SIGTERM?)".to_string()
                };
                pending.push((
                    expr_pos(arg),
                    format!("{rendered} cannot be trapped{hint}"),
                ));
            } else if is_sigstop(pass, arg) {
                let rendered = render_signal(pass, arg);
                pending.push((
                    expr_pos(arg),
                    format!("{rendered} cannot be trapped"),
                ));
            }
        }
    });

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn render_signal(pass: &Pass<'_>, expr: &Expr) -> String {
    signal_selector_name(pass, expr).unwrap_or_else(|| "signal".into())
}

fn expr_pos(expr: &Expr) -> u32 {
    expr.pos().0 as u32
}

fn sa1016_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1016",
        doc: "trapping a signal that cannot be trapped",
        url: "https://staticcheck.dev/docs/checks/#SA1016",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// SA1016 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1016_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1016_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
