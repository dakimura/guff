//! Port of [`go.augendre.info/fatcontext`](https://github.com/Crocmagnon/fatcontext).

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, BlockStmt, CallExpr, Expr, ForStmt, FuncDecl, FuncLit, Ident, RangeStmt,
    SelectorExpr, Stmt,
};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::selection::SelectionKind;

const CATEGORY_IN_LOOP: &str = "nested context in loop";
const CATEGORY_IN_FUNC_LIT: &str = "nested context in function literal";

fn type_string(pass: &Pass<'_>, e: &Expr) -> Option<String> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = info.types.get(&e.id())?.typ;
    Some(guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    ))
}

fn is_context_ctx(pass: &Pass<'_>, e: &Expr) -> bool {
    type_string(pass, e).as_deref() == Some("context.Context")
}

fn unparen(e: &Expr) -> &Expr {
    let mut cur = e;
    while let Expr::ParenExpr(p) = cur {
        cur = &p.x;
    }
    cur
}

fn is_empty_context(e: &Expr) -> bool {
    let Expr::CallExpr(CallExpr { fun, .. }) = unparen(e) else {
        return false;
    };
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(fun) else {
        return false;
    };
    matches!(unparen(x), Expr::Ident(Ident { name, .. }) if name == "context")
        && matches!(sel.name.as_str(), "Background" | "TODO")
}

fn var_name(e: &Expr) -> Option<String> {
    match unparen(e) {
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::SelectorExpr(sel) => {
            let left = var_name(&sel.x)?;
            Some(format!("{left}.{}", sel.sel.name))
        }
        _ => None,
    }
}

fn is_pointer_sel(pass: &Pass<'_>, e: &Expr) -> bool {
    let Expr::SelectorExpr(sel) = unparen(e) else {
        return false;
    };
    let Some(info) = pass.types_info() else {
        return false;
    };
    info.selections
        .get(&sel.id)
        .is_some_and(|s| s.kind() == SelectionKind::FieldVal && s.indirect())
}

fn get_stmt_list(stmt: &Stmt) -> Option<&[Stmt]> {
    match stmt {
        Stmt::BlockStmt(b) => Some(&b.list),
        Stmt::IfStmt(i) => Some(&i.body.list),
        Stmt::SwitchStmt(s) => Some(&s.body.list),
        Stmt::CaseClause(c) => Some(&c.body),
        Stmt::SelectStmt(s) => Some(&s.body.list),
        Stmt::CommClause(c) => Some(&c.body),
        _ => None,
    }
}

fn find_nested_context<'a>(
    pass: &Pass<'_>,
    stmts: &'a [Stmt],
) -> Option<&'a AssignStmt> {
    let mut reset: HashSet<String> = HashSet::new();
    for stmt in stmts {
        if let Some(list) = get_stmt_list(stmt) {
            if let Some(found) = find_nested_context(pass, list) {
                return Some(found);
            }
        }
        let Stmt::AssignStmt(assign) = stmt else {
            continue;
        };
        if assign.lhs.is_empty() || assign.rhs.is_empty() {
            continue;
        }
        if !is_context_ctx(pass, &assign.lhs[0]) {
            continue;
        }
        if assign.tok == Some(Token::DEFINE) {
            continue;
        }
        let name = var_name(&assign.lhs[0]).unwrap_or_default();
        if is_empty_context(&assign.rhs[0]) {
            if !name.is_empty() {
                reset.insert(name);
            }
            continue;
        }
        if !name.is_empty() && reset.contains(&name) {
            continue;
        }
        // Pointer root / within-loop local copies: report pointers;
        // skip complex within-loop analysis (DEFERRED subtlety).
        if is_pointer_sel(pass, &assign.lhs[0]) {
            return Some(assign);
        }
        return Some(assign);
    }
    None
}

fn body_of(n: NodeRef<'_>) -> Option<&BlockStmt> {
    match n {
        NodeRef::ForStmt(ForStmt { body, .. }) => Some(body),
        NodeRef::RangeStmt(RangeStmt { body, .. }) => Some(body),
        NodeRef::FuncLit(FuncLit { body, .. }) => Some(body),
        NodeRef::FuncDecl(FuncDecl {
            body: Some(body), ..
        }) => Some(body),
        _ => None,
    }
}

fn category_for(n: NodeRef<'_>) -> &'static str {
    match n {
        NodeRef::ForStmt(_) | NodeRef::RangeStmt(_) => CATEGORY_IN_LOOP,
        NodeRef::FuncLit(_) | NodeRef::FuncDecl(_) => CATEGORY_IN_FUNC_LIT,
        _ => CATEGORY_IN_LOOP,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "fatcontext requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::preorder(NodeRef::File(file), |n| {
            let Some(body) = body_of(n) else {
                return true;
            };
            let Some(assign) = find_nested_context(pass, &body.list) else {
                return true;
            };
            let category = category_for(n);
            let start = assign.tok_pos.0 as u32;
            let end = assign
                .rhs
                .last()
                .map(|e| e.end().0 as u32)
                .unwrap_or(start);
            // Suggested fix: replace `=` with `:=` (tok span).
            let tok_end = start + 1; // "="
            pending.push(Diagnostic {
                pos: start,
                end,
                message: category.into(),
                suggested_fixes: vec![SuggestedFix {
                    message: "replace `=` with `:=`".into(),
                    text_edits: vec![TextEdit {
                        pos: start,
                        end: tok_end,
                        new_text: ":=".into(),
                    }],
                }],
                ..Diagnostic::default()
            });
            true
        });
    }
    for d in pending {
        pass.report(d);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "fatcontext",
        doc: "detects nested contexts in loops and function literals",
        url: "https://github.com/Crocmagnon/fatcontext",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
