//! S1038 — unnecessarily complex way of printing formatted string.
//!
//! Port of `honnef.co/go/tools/simple/s1038`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{call_name, is_call_to, is_call_to_any};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn fmt_print_message(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    let name = call_name(pass, &call.fun)?;
    let short = name.strip_prefix("fmt.").unwrap_or(&name);
    let Expr::CallExpr(inner) = call.args.first()? else {
        return None;
    };
    if !is_call_to(pass, inner, "fmt.Sprintf") {
        return None;
    }
    match short {
        "Print" | "Fprint" | "Sprint" => {
            Some(format!("should use fmt.{short}f instead of fmt.{short}(fmt.Sprintf(...))"))
        }
        "Println" | "Fprintln" | "Sprintln" => {
            if inner.args.first().is_some_and(|e| matches!(e, Expr::BasicLit(_))) {
                let base = &short[..short.len() - 2];
                Some(format!(
                    "should use fmt.{base}f instead of fmt.{short}(fmt.Sprintf(...))"
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn log_message(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    if !is_call_to_any(
        pass,
        call,
        &[
            "log.Fatal",
            "log.Fatalln",
            "log.Panic",
            "log.Panicln",
            "log.Print",
            "log.Println",
        ],
    ) {
        return None;
    }
    let name = call_name(pass, &call.fun)?;
    let Expr::CallExpr(inner) = call.args.first()? else {
        return None;
    };
    if !is_call_to(pass, inner, "fmt.Sprintf") {
        return None;
    }
    let alt = match name.as_str() {
        "log.Fatal" | "log.Fatalln" => "log.Fatalf",
        "log.Panic" | "log.Panicln" => "log.Panicf",
        "log.Print" | "log.Println" => "log.Printf",
        _ => return None,
    };
    Some(format!(
        "should use {alt}(...) instead of {name}(fmt.Sprintf(...))"
    ))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1038 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if let Some(msg) = fmt_print_message(pass, call).or_else(|| log_message(pass, call)) {
            pending.push((match_pos(node), msg));
        }
    });

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1038_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1038",
        doc: "unnecessarily complex way of printing formatted string",
        url: "https://staticcheck.dev/docs/checks/#S1038",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1038_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1038_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
