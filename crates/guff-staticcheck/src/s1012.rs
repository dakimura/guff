//! S1012 — replace time.Now().Sub with time.Since.
//!
//! Port of `honnef.co/go/tools/simple/s1012`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, SelectorExpr};
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to, type_func_name};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn is_now_sub(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = &*call.fun else {
        return false;
    };
    if sel.name != "Sub" {
        return false;
    }
    let Expr::CallExpr(now) = &**x else {
        return false;
    };
    if !is_call_to(pass, now, "time.Now") {
        return false;
    }
    let Some(obj) = pass.types_info().and_then(|info| info.uses.get(&sel.id).copied()) else {
        return false;
    };
    let Some(a) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    type_func_name(&a.types, &a.objects, &a.packages, obj) == "(time.Time).Sub"
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1012 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if is_now_sub(pass, call) {
            pending.push((
                match_pos(node),
                "should use time.Since instead of time.Now().Sub".into(),
            ));
        }
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1012_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1012",
        doc: "replace time.Now().Sub with time.Since",
        url: "https://staticcheck.dev/docs/checks/#S1012",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1012 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1012_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1012_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
