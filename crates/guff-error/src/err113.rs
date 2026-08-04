//! Port of [`github.com/Djarvur/go-err113`](https://github.com/Djarvur/go-err113).

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{BinaryExpr, CallExpr, Decl, Expr, FuncDecl, GenDecl, SelectorExpr, Spec};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_constant::string_val;
use guff_types::arena::ObjectData;

use crate::util::{expr_string, is_pure_error, type_of, unparen};

fn is_nil_expr(pass: &Pass<'_>, e: &Expr) -> bool {
    code::is_nil(pass, e)
}

fn is_eof(pass: &Pass<'_>, e: &Expr) -> bool {
    let Expr::SelectorExpr(sel) = unparen(e) else {
        return false;
    };
    if sel.sel.name != "EOF" {
        return false;
    }
    imported_path(pass, &sel.x).as_deref() == Some("io")
}

fn imported_path(pass: &Pass<'_>, e: &Expr) -> Option<String> {
    let Expr::Ident(id) = unparen(e) else {
        return None;
    };
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let obj = info.uses.get(&id.id).copied()?;
    let ObjectData::PkgName(pn) = artifacts.objects.get(obj) else {
        return None;
    };
    Some(artifacts.packages.get(pn.imported()).path().to_string())
}

fn are_both_errors(pass: &Pass<'_>, x: &Expr, y: &Expr) -> bool {
    if is_nil_expr(pass, x) || is_nil_expr(pass, y) {
        return false;
    }
    if is_eof(pass, x) || is_eof(pass, y) {
        return false;
    }
    is_pure_error(pass, x) || is_pure_error(pass, y)
}

fn enclosing_is_method(stack: &[NodeRef<'_>], pass: &Pass<'_>) -> bool {
    for n in stack.iter().rev() {
        let NodeRef::FuncDecl(FuncDecl { name, recv, .. }) = n else {
            continue;
        };
        if name.name != "Is" {
            return false;
        }
        let Some(recv) = recv else {
            return false;
        };
        let Some(field) = recv.list.first() else {
            return false;
        };
        let Some(ty) = field.ty.as_ref() else {
            return false;
        };
        let Some(typ) = type_of(pass, ty) else {
            return false;
        };
        return crate::util::implements_error(pass, typ);
    }
    false
}

fn check_comparison(
    pass: &Pass<'_>,
    be: &BinaryExpr,
    stack: &[NodeRef<'_>],
    pending: &mut Vec<Diagnostic>,
) {
    if be.op != Token::EQL && be.op != Token::NEQ {
        return;
    }
    if !are_both_errors(pass, &be.x, &be.y) {
        return;
    }
    if enclosing_is_method(stack, pass) {
        return;
    }
    let old = format!("{} {} {}", expr_string(&be.x), be.op, expr_string(&be.y));
    let negate = if be.op == Token::NEQ { "!" } else { "" };
    let new = format!(
        "{}errors.Is({}, {})",
        negate,
        expr_string(&be.x),
        expr_string(&be.y)
    );
    let start = be.x.pos().0 as u32;
    let end = be.y.end().0 as u32;
    pending.push(Diagnostic {
        pos: start,
        end,
        message: format!(
            "do not compare errors directly \"{old}\", use \"{new}\" instead"
        ),
        suggested_fixes: vec![SuggestedFix {
            message: format!("should replace \"{old}\" with \"{new}\""),
            text_edits: vec![TextEdit {
                pos: start,
                end,
                new_text: new,
            }],
        }],
        ..Diagnostic::default()
    });
}

fn const_string_arg(pass: &Pass<'_>, e: &Expr) -> Option<String> {
    let info = pass.types_info()?;
    let tav = info.types.get(&e.id())?;
    Some(string_val(tav.val.as_ref()?))
}

fn is_dynamic_error_call(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(&call.fun) else {
        return false;
    };
    let Some(path) = imported_path(pass, x) else {
        return false;
    };
    match (path.as_str(), sel.name.as_str()) {
        ("errors", "New") => true,
        ("fmt", "Errorf") => {
            // Allowed when wrapping with %w.
            let Some(fmt_arg) = call.args.first() else {
                return true;
            };
            !const_string_arg(pass, fmt_arg)
                .is_some_and(|s| s.contains("%w"))
        }
        _ => false,
    }
}

fn package_level_calls(file: &guff::ast::File) -> HashSet<u32> {
    let mut out = HashSet::new();
    for decl in &file.decls {
        let Decl::GenDecl(GenDecl {
            tok: Some(Token::VAR),
            specs,
            ..
        }) = decl
        else {
            continue;
        };
        for spec in specs {
            let Spec::ValueSpec(vs) = spec else { continue };
            for v in &vs.values {
                if let Expr::CallExpr(ce) = unparen(v) {
                    out.insert(ce.id);
                }
            }
        }
    }
    out
}

fn check_definition(
    pass: &Pass<'_>,
    call: &CallExpr,
    tld: &HashSet<u32>,
    pending: &mut Vec<(u32, String)>,
) {
    if tld.contains(&call.id) {
        return;
    }
    if !is_dynamic_error_call(pass, call) {
        return;
    }
    pending.push((
        call.lparen.0 as u32,
        format!(
            "do not define dynamic errors, use wrapped static errors instead: \"{}\"",
            expr_string(&Expr::CallExpr(call.clone())).replace('"', "\\\"")
        ),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "err113 requires inspect analyzer".to_string())?;

    let mut pending_diags = Vec::new();
    let mut pending_msgs = Vec::new();

    for file in pass.files() {
        let tld = package_level_calls(file);
        let mut stack = Vec::new();
        walk::preorder_stack(NodeRef::File(file), &mut stack, |n, stack| {
            match n {
                NodeRef::BinaryExpr(be) => {
                    check_comparison(pass, be, stack, &mut pending_diags);
                }
                NodeRef::CallExpr(call) => {
                    check_definition(pass, call, &tld, &mut pending_msgs);
                }
                _ => {}
            }
            true
        });
    }

    for d in pending_diags {
        pass.report(d);
    }
    for (pos, message) in pending_msgs {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "err113",
        doc: "checks the error handling rules according to the Go 1.13 error conventions",
        url: "https://github.com/Djarvur/go-err113",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
