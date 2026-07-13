//! `printf` — check Printf-like format strings.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::walk::NodeRef;
use guff_analysis::code::{call_name, expr_to_string};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

const KNOWN_PRINTF: &[&str] = &[
    "fmt.Errorf",
    "fmt.Fprintf",
    "fmt.Printf",
    "fmt.Sprintf",
    "log.Printf",
    "log.Fatalf",
    "log.Panicf",
];

fn printf_kind(pass: &Pass<'_>, fun: &Expr) -> Option<()> {
    let name = call_name(pass, fun)?;
    if KNOWN_PRINTF.iter().any(|k| *k == name) {
        return Some(());
    }
    let short = name.rsplit('.').next()?;
    if matches!(short, "Printf" | "Sprintf" | "Fprintf" | "Errorf" | "Fatalf" | "Panicf") {
        return Some(());
    }
    None
}

fn format_string_from_arg(pass: &Pass<'_>, arg: &Expr) -> Option<String> {
    expr_to_string(pass, arg)
}

fn check_format(format: &str, is_errorf: bool) -> Option<String> {
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            continue;
        }
        let mut verb = match chars.next() {
            None => return Some("format string ends with %".into()),
            Some('%') => continue,
            Some(v) => v,
        };
        if verb == '[' {
            while let Some(ch) = chars.next() {
                if ch == ']' {
                    break;
                }
            }
            verb = chars.next()?;
            if verb == 'w' && !is_errorf {
                return Some("%w verb only allowed in Errorf".into());
            }
            if !is_valid_verb(verb) {
                return Some(format!("unknown verb %{verb}"));
            }
            continue;
        }
        while matches!(verb, '#' | '+' | '-' | ' ' | '0') {
            verb = chars.next()?;
        }
        if verb == '*' {
            verb = chars.next()?;
        }
        while verb.is_ascii_digit() {
            verb = chars.next()?;
        }
        if verb == '.' {
            if chars.peek() == Some(&'*') {
                chars.next();
            } else {
                while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                    chars.next();
                }
            }
            verb = chars.next()?;
        }
        if verb == 'w' && !is_errorf {
            return Some("%w verb only allowed in Errorf".into());
        }
        if !is_valid_verb(verb) {
            return Some(format!("unknown verb %{verb}"));
        }
    }
    None
}

fn is_valid_verb(v: char) -> bool {
    matches!(
        v,
        'b' | 'c' | 'd' | 'e' | 'E' | 'f' | 'F' | 'g' | 'G' | 'o' | 'p' | 'q' | 's' | 't' | 'T'
            | 'U' | 'v' | 'x' | 'X' | 'w'
    )
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "printf requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if printf_kind(pass, &call.fun).is_none() {
            return;
        }
        let Some(format_arg) = call.args.first() else {
            return;
        };
        let Some(format) = format_string_from_arg(pass, format_arg) else {
            return;
        };
        let is_errorf = call_name(pass, &call.fun).is_some_and(|n| n.ends_with("Errorf"));
        if let Some(msg) = check_format(&format, is_errorf) {
            let name = call_name(pass, &call.fun).unwrap_or_else(|| "Printf".into());
            pending.push((call.lparen.0 as u32, format!("{name} {msg}")));
        }
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "printf",
        doc: "check Printf format strings",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/printf",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_verb() {
        assert_eq!(check_format("%z", false), Some("unknown verb %z".into()));
    }

    #[test]
    fn allows_percent_w_in_errorf() {
        assert!(check_format("%w", true).is_none());
        assert!(check_format("%w", false).is_some());
    }
}
