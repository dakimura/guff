//! SA4031 — checking never-nil value against nil.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4031`.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::Expr;
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::{buildir, inspect};
use guff_analysis::{
    match_pattern, AnalysisResult, Analyzer, Diagnostic, RelatedInformation, RunError, RunFn, Pass,
};
use guff_pattern::{must_parse, Pattern};
use guff_ssa::function::Function;
use guff_ssa::instr::InstrData;
use guff_ssa::value::Value;

static PAT4022: OnceLock<Pattern> = OnceLock::new();

/// The shape SA4022 already reports (`&x == nil`), which SA4031 skips so the
/// two do not both fire. (Go: `sa4022.CheckAddressIsNilQ`.)
fn pat4022() -> &'static Pattern {
    PAT4022.get_or_init(|| {
        must_parse(r#"(BinaryExpr (UnaryExpr "&" _) (Or "==" "!=") (Or nil (Ident "nil")))"#)
    })
}

/// One value that makes the comparison's operand never-nil, for the related
/// information upstream attaches ("this is the value of x").
struct Tracked {
    pos: u32,
}

/// Walks back from `v` asking whether every way it could have been produced
/// yields a non-nil value, collecting the producers worth naming.
///
/// (Go: the `neverNil` closure. `track` distinguishes a value the report should
/// point at from one reached only while proving the first.)
///
/// guff's IR has no `Sigma` — go/ssa is not SSI — so that arm has nothing to
/// recurse through here; every other arm is ported.
fn never_nil(
    func: &Function,
    v: Value,
    track: bool,
    seen: &mut HashSet<Value>,
    values: &mut Vec<Tracked>,
) -> bool {
    if !seen.insert(v) {
        return true;
    }
    match v {
        Value::Function(_) => {
            if track {
                // A package-level function has no instruction position; upstream
                // sorts by Pos and a function's is its declaration. Nothing in
                // the report needs it (the function arm takes the message that
                // carries no related info), so record it as unpositioned.
                values.push(Tracked { pos: 0 });
            }
            true
        }
        Value::Instr(iid) => match func.instrs.get(iid) {
            InstrData::MakeClosure(_)
            | InstrData::MakeChan(_)
            | InstrData::MakeMap(_)
            | InstrData::MakeSlice(_)
            | InstrData::Alloc(_) => {
                if track {
                    values.push(Tracked { pos: func.pos(iid).0 as u32 });
                }
                true
            }
            InstrData::Slice(s) => {
                if track {
                    values.push(Tracked { pos: func.pos(iid).0 as u32 });
                }
                never_nil(func, s.x, false, seen, values)
            }
            InstrData::FieldAddr(fa) => {
                if track {
                    values.push(Tracked { pos: func.pos(iid).0 as u32 });
                }
                never_nil(func, fa.x, false, seen, values)
            }
            InstrData::Phi(phi) => {
                for e in &phi.edges {
                    let Some(e) = e else { return false };
                    if !never_nil(func, *e, true, seen, values) {
                        return false;
                    }
                }
                true
            }
            _ => false,
        },
        _ => false,
    }
}

/// Whether the value is a function (closure or declared), which takes a message
/// of its own upstream.
fn is_function_value(func: &Function, v: Value) -> bool {
    match v {
        Value::Function(_) => true,
        Value::Instr(iid) => matches!(func.instrs.get(iid), InstrData::MakeClosure(_)),
        _ => false,
    }
}

fn is_nil_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(id) if id.name == "nil")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let ir = pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .ok_or_else(|| "SA4031 requires buildir analyzer".to_string())?;
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4031 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<Diagnostic> = Vec::new();
    // Upstream walks `*ast.IfStmt` only. A bare comparison — `_ = make(chan int)
    // == nil` — is not a finding, which is what an earlier AST arm here got
    // wrong in both directions: it reported that and nothing else.
    inspect.preorder_typed(node_mask!(IfStmt), pass.files(), |node| {
        let NodeRef::IfStmt(if_) = node else {
            return;
        };
        let Expr::BinaryExpr(cond) = &if_.cond else {
            return;
        };
        if !matches!(cond.op, Token::EQL | Token::NEQ) {
            return;
        }
        // The pattern is positional: `(BinaryExpr lhs op (Builtin "nil"))`, so
        // `nil` has to be the *right* operand.
        if !is_nil_expr(cond.y.as_ref()) {
            return;
        }
        if match_pattern(pass, pat4022(), NodeRef::BinaryExpr(cond)).is_some() {
            return; // SA4022 reports this one
        }
        let lhs = cond.x.as_ref();
        let Some(ev) = ir.expr_values().get(lhs) else {
            return;
        };
        if ev.is_addr {
            return;
        }
        let func = ir.prog.functions.get(ev.func);

        let mut seen = HashSet::new();
        let mut values = Vec::new();
        if !never_nil(func, ev.value, true, &mut seen, &mut values) {
            return;
        }

        let qualifier = if cond.op == Token::EQL { "never" } else { "always" };
        let fallback = format!("this nil check is {qualifier} true");
        values.sort_by_key(|t| t.pos);

        // Only a comparison of a *variable* carries related information naming
        // it; everything else gets the bare message.
        let ident_name = match lhs {
            Expr::Ident(id) => {
                let is_var = guff_analysis::code::object_of(pass, id).is_some_and(|o| {
                    pass.pkg().type_artifacts.as_ref().is_some_and(|a| {
                        matches!(a.objects.get(o), guff_types::arena::ObjectData::Var(_))
                    })
                });
                is_var.then(|| id.name.clone())
            }
            _ => None,
        };

        let is_func = is_function_value(func, ev.value);
        let (message, related) = match ident_name {
            Some(name) => {
                let related: Vec<RelatedInformation> = values
                    .iter()
                    .filter(|t| t.pos != 0)
                    .map(|t| RelatedInformation {
                        pos: t.pos,
                        end: 0,
                        message: if values.len() == 1 {
                            format!("this is the value of {name}")
                        } else {
                            format!("this is one of the value of {name}")
                        },
                    })
                    .collect();
                let msg = if is_func {
                    "the checked variable contains a function and is never nil; did you mean to call it?"
                        .to_string()
                } else {
                    fallback
                };
                (msg, related)
            }
            None => {
                let msg = if matches!(ev.value, Value::Function(_)) {
                    "functions are never nil; did you mean to call it?".to_string()
                } else {
                    fallback
                };
                (msg, Vec::new())
            }
        };

        pending.push(Diagnostic {
            pos: cond.x.pos().0 as u32,
            message,
            related,
            ..Diagnostic::default()
        });
    });

    for d in pending {
        pass.report(d);
    }
    Ok(None)
}

fn sa4031_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4031",
        doc: "checking never-nil value against nil",
        url: "https://staticcheck.dev/docs/checks/#SA4031",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer(), inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4031_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4031_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
