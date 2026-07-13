//! SA1005 — invalid first argument to `exec.Command`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1005`.

use std::sync::OnceLock;

use guff::walk::NodeRef;
use guff_analysis::code::{expr_to_string, is_call_to};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn looks_like_shell_command(val: &str) -> bool {
    val.contains(' ') && !val.contains('\\') && !val.contains('/')
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA1005 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if !is_call_to(pass, call, "os/exec.Command") {
            return;
        }
        let Some(arg1) = call.args.first() else {
            return;
        };
        let Some(val) = expr_to_string(pass, arg1) else {
            return;
        };
        if !looks_like_shell_command(&val) {
            return;
        }
        pending.push((
            match_pos(node),
            "first argument to exec.Command looks like a shell command, but a program name or path are expected"
                .into(),
        ));
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn sa1005_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1005",
        doc: "invalid first argument to exec.Command",
        url: "https://staticcheck.dev/docs/checks/#SA1005",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// SA1005 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1005_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1005_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn looks_like_shell_command_heuristic() {
        assert!(!looks_like_shell_command("ls"));
        assert!(looks_like_shell_command("ls arg1"));
        assert!(!looks_like_shell_command(r"C:\Program Files\foo"));
        assert!(!looks_like_shell_command("/bin/ls arg"));
    }
}
