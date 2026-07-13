//! `cgocall` — check for invalid cgo pointer passing.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, UnaryExpr};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::expreq::unparen;
use crate::govet_util::{cgo_base_type, imports_package, is_c_call, type_ok_for_cgo_call};

fn uses_cgo(pass: &Pass<'_>) -> bool {
    imports_package(pass, "runtime/cgo") || has_import_c(pass)
}

fn has_import_c(pass: &Pass<'_>) -> bool {
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::GenDecl(gd) = decl else {
                continue;
            };
            if gd.tok != Some(guff::token::Token::IMPORT) {
                continue;
            }
            for spec in &gd.specs {
                let guff::ast::Spec::ImportSpec(is) = spec else {
                    continue;
                };
                if is.path.value.trim_matches('"') == "C" {
                    return true;
                }
            }
        }
    }
    false
}

fn check_call(pass: &Pass<'_>, call: &CallExpr) -> Option<u32> {
    let name = is_c_call(&call.fun)?;
    if name == "CBytes" {
        return None;
    }
    for arg in &call.args {
        let base = cgo_base_type(pass, arg)?;
        if !type_ok_for_cgo_call(pass, base) {
            return Some(arg.pos().0 as u32);
        }
        if let Expr::UnaryExpr(UnaryExpr { op: Token::AND, x, .. }) = unparen(arg) {
            let base = cgo_base_type(pass, x)?;
            if !type_ok_for_cgo_call(pass, base) {
                return Some(arg.pos().0 as u32);
            }
        }
    }
    None
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if !uses_cgo(pass) {
        return Ok(None);
    }
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "cgocall requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if let Some(pos) = check_call(pass, call) {
            pending.push((
                pos,
                "possibly passing Go type with embedded pointer to C".into(),
            ));
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
        name: "cgocall",
        doc: "check for invalid cgo pointer passing",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/cgocall",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
