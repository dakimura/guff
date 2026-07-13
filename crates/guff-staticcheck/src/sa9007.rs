//! SA9007 — deleting a directory that shouldn't be deleted.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa9007` (AST-based).

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, SelectorExpr};
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to, is_call_to_any};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_remove_all(pass: &Pass<'_>, call: &CallExpr) -> Option<(u32, String)> {
    if !is_call_to(pass, call, "os.RemoveAll") || call.args.is_empty() {
        return None;
    }
    match &call.args[0] {
        Expr::CallExpr(inner) if is_call_to(pass, inner, "os.TempDir") => Some((
            call.lparen.0 as u32,
            "this call to os.RemoveAll deletes the user's entire temporary directory, not a subdirectory therein".into(),
        )),
        Expr::CallExpr(inner) if is_call_to_any(pass, inner, &["os.UserCacheDir", "os.UserConfigDir", "os.UserHomeDir"]) => {
            let kind = match guff_analysis::code::call_name(pass, &inner.fun).as_deref() {
                Some("os.UserCacheDir") => "cache",
                Some("os.UserConfigDir") => "config",
                Some("os.UserHomeDir") => "home",
                _ => return None,
            };
            Some((
                call.lparen.0 as u32,
                format!("this call to os.RemoveAll deletes the user's entire {kind} directory, not a subdirectory therein"),
            ))
        }
        _ => None,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA9007 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if let Some((pos, msg)) = check_remove_all(pass, call) {
            pending.push((pos, msg));
        }
    });
    for (pos, msg) in pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn sa9007_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA9007",
        doc: "deleting a directory that shouldn't be deleted",
        url: "https://staticcheck.dev/docs/checks/#SA9007",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa9007_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa9007_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
