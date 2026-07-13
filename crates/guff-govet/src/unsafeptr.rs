//! `unsafeptr` — check for invalid conversions of uintptr to unsafe.Pointer.

use std::sync::OnceLock;

use guff::ast::{BinaryExpr, CallExpr, Expr, SelectorExpr, StarExpr, UnaryExpr};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::basic::BasicKind;

use crate::expreq::unparen;
use crate::govet_util::{
    expr_type, has_basic_kind, is_type_named, is_uintptr_type, is_unsafe_pointer_type,
};

fn is_reflect_header(pass: &Pass<'_>, typ: guff_types::TypeId) -> bool {
    is_type_named(pass, typ, "reflect", "SliceHeader")
        || is_type_named(pass, typ, "reflect", "StringHeader")
}

fn is_safe_uintptr(pass: &Pass<'_>, expr: &Expr) -> bool {
    match unparen(expr) {
        Expr::SelectorExpr(SelectorExpr { x, sel, .. }) if sel.name == "Data" => {
            let Some(typ) = expr_type(pass, &x) else {
                return false;
            };
            let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                return false;
            };
            let u = typ.underlying(&artifacts.types);
            if let guff_types::arena::TypeData::Pointer(p) = artifacts.types.get(u) {
                return is_reflect_header(pass, p.elem());
            }
            false
        }
        Expr::CallExpr(call) if call.args.is_empty() => {
            let Expr::SelectorExpr(sel) = unparen(&call.fun) else {
                return is_safe_arith(pass, expr);
            };
            if matches!(sel.sel.name.as_str(), "Pointer" | "UnsafeAddr") {
                return expr_type(pass, &sel.x)
                    .is_some_and(|t| is_type_named(pass, t, "reflect", "Value"));
            }
            is_safe_arith(pass, expr)
        }
        _ => is_safe_arith(pass, expr),
    }
}

fn is_safe_arith(pass: &Pass<'_>, expr: &Expr) -> bool {
    match unparen(expr) {
        Expr::CallExpr(call) if call.args.len() == 1 => {
            is_uintptr_type(pass, &call.fun) && is_unsafe_pointer_type(pass, &call.args[0])
        }
        Expr::BinaryExpr(BinaryExpr { op, x, y, .. }) => {
            matches!(op, Token::ADD | Token::SUB | Token::AndNot) && is_safe_arith(pass, x)
        }
        _ => false,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "unsafeptr requires inspect analyzer".to_string())?
        .clone();
    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |n| {
        match n {
            NodeRef::CallExpr(call) if call.args.len() == 1 => {
                if is_unsafe_pointer_type(pass, &call.fun)
                    && has_basic_kind(pass, &call.args[0], BasicKind::Uintptr)
                    && !is_safe_uintptr(pass, &call.args[0])
                {
                    pending.push((call.pos().0 as u32, "possible misuse of unsafe.Pointer".into()));
                }
            }
            NodeRef::StarExpr(star) => {
                if let Some(typ) = expr_type(pass, &Expr::StarExpr(star.clone())) {
                    if is_reflect_header(pass, typ) {
                        let name = if is_type_named(pass, typ, "reflect", "SliceHeader") {
                            "reflect.SliceHeader"
                        } else {
                            "reflect.StringHeader"
                        };
                        pending.push((
                            star.x.pos().0 as u32,
                            format!("possible misuse of {name}"),
                        ));
                    }
                }
            }
            NodeRef::UnaryExpr(UnaryExpr { op: Token::AND, x, .. }) => {
                if let Some(typ) = expr_type(pass, x) {
                    if is_reflect_header(pass, typ) {
                        pending.push((
                            x.pos().0 as u32,
                            "possible misuse of reflect header type".into(),
                        ));
                    }
                }
            }
            _ => {}
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
        name: "unsafeptr",
        doc: "check for invalid conversions of uintptr to unsafe.Pointer",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/unsafeptr",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
