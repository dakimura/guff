//! SA9002 — non-octal `os.FileMode` that looks like octal.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa9002`.

use std::sync::OnceLock;

use guff::ast::{BasicLit, CallExpr, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::is_of_type_with_name;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_lit(pass: &Pass<'_>, lit: &BasicLit) -> Option<(u32, String)> {
    if !is_of_type_with_name(pass, &Expr::BasicLit(lit.clone()), "os.FileMode")
        && !is_of_type_with_name(pass, &Expr::BasicLit(lit.clone()), "io/fs.FileMode")
    {
        return None;
    }
    let v = &lit.value;
    if v.len() != 3 {
        return None;
    }
    let bytes = v.as_bytes();
    if bytes[0] == b'0' {
        return None;
    }
    if !bytes.iter().all(|&b| (b'0'..=b'7').contains(&b)) {
        return None;
    }
    let n: i64 = v.parse().ok()?;
    Some((
        lit.value_pos.0 as u32,
        format!("file mode '{v}' evaluates to 0{n:o}; did you mean '0{v}'?"),
    ))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA9002 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        for arg in &call.args {
            let Expr::BasicLit(lit) = arg else {
                continue;
            };
            if let Some((pos, msg)) = check_lit(pass, lit) {
                pending.push((pos, msg));
            }
        }
    });
    for (pos, msg) in pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn sa9002_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA9002",
        doc: "using a non-octal os.FileMode that looks like it was meant to be in octal",
        url: "https://staticcheck.dev/docs/checks/#SA9002",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa9002_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa9002_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
