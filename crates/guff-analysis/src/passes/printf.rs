//! Simplified `printf` analyzer for end-to-end smoke tests.
//!
//! Port of a tiny subset of `golang.org/x/tools/go/analysis/passes/printf`:
//! flags `fmt.Printf`-style calls (any resolved `Printf` function) and checks
//! that string-literal format strings use known verbs. Wrapper forwarding and
//! `fmt.Errorf`'s `%w` are intentionally omitted.

use std::sync::OnceLock;

use guff::ast::{BasicLit, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_constant::string_val_lossy;
use guff_types::arena::ObjectData;

use crate::analyzer::{AnalysisResult, Analyzer, RunError, RunFn};
use crate::pass::Pass;
use crate::passes::inspect;

/// Verbs accepted in a `Printf`-style format string (simplified).
const VALID_VERBS: &[char] = &[
    'b', 'c', 'd', 'e', 'E', 'f', 'F', 'g', 'G', 'o', 'p', 'q', 's', 't', 'T', 'U', 'v', 'x', 'X',
];

fn is_printf_call(pass: &Pass<'_>, fun: &Expr) -> bool {
    let info = match pass.types_info() {
        Some(info) => info,
        None => return false,
    };
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return false,
    };

    let obj_id = match fun {
        Expr::Ident(id) => info.uses.get(&id.id).copied(),
        Expr::SelectorExpr(sel) => info.uses.get(&sel.sel.id).copied(),
        _ => None,
    };
    let Some(obj_id) = obj_id else {
        return false;
    };
    match artifacts.objects.get(obj_id) {
        ObjectData::Func(f) => f.name() == "Printf",
        _ => false,
    }
}

fn format_string_from_arg(pass: &Pass<'_>, arg: &Expr) -> Option<String> {
    let info = pass.types_info()?;
    if let Expr::BasicLit(BasicLit { value, .. }) = arg {
        if value.starts_with('"') || value.starts_with('`') {
            return Some(unquote_go_string(value));
        }
    }
    let tav = info.types.get(&arg.id())?;
    // Lossy is faithful here: upstream's parser ranges over the format string,
    // and `for range` yields U+FFFD for each ill-formed byte.
    tav.val.as_ref().map(string_val_lossy)
}

fn unquote_go_string(lit: &str) -> String {
    if let Some(inner) = lit.strip_prefix('`').and_then(|s| s.strip_suffix('`')) {
        return inner.to_string();
    }
    let mut out = String::new();
    let mut chars = lit.chars();
    if chars.next() != Some('"') {
        return lit.to_string();
    }
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(match next {
                    'n' => '\n',
                    't' => '\t',
                    '"' => '"',
                    '\\' => '\\',
                    other => other,
                });
            }
        } else if c == '"' {
            break;
        } else {
            out.push(c);
        }
    }
    out
}

fn check_format(format: &str) -> Option<String> {
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            continue;
        }
        match chars.next() {
            None => return Some("format string ends with %".into()),
            Some('%') => continue,
            Some('[') => {
                while let Some(ch) = chars.next() {
                    if ch == ']' {
                        break;
                    }
                }
                let verb = chars.next()?;
                if !VALID_VERBS.contains(&verb) {
                    return Some(format!("unknown verb %{}", verb));
                }
            }
            Some(verb) if !VALID_VERBS.contains(&verb) => {
                return Some(format!("unknown verb %{}", verb));
            }
            Some(_) => {}
        }
    }
    None
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "printf requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    {
        let files = pass.files();
        inspect.preorder_typed(node_mask!(CallExpr), files, |n| {
            let NodeRef::CallExpr(call) = n else {
                return;
            };
            if !is_printf_call(pass, &call.fun) {
                return;
            }
            let Some(format_arg) = call.args.first() else {
                return;
            };
            let Some(format) = format_string_from_arg(pass, format_arg) else {
                return;
            };
            if let Some(msg) = check_format(&format) {
                pending.push((call.lparen.0 as u32, msg));
            }
        });
    }
    for (pos, message) in pending {
        pass.reportf(pos, message);
    }

    Ok(None)
}

fn printf_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "printf",
        doc: "check Printf format strings (simplified smoke port)",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/printf",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// Simplified printf analyzer for smoke tests.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(printf_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate;

    #[test]
    fn printf_validates() {
        assert!(validate::validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn check_format_flags_unknown_verb() {
        assert_eq!(
            check_format("%z"),
            Some("unknown verb %z".to_string())
        );
        assert!(check_format("%s").is_none());
    }
}
