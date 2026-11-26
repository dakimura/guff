//! `unconditional-recursion` — warn on recursive calls not guarded by control flow.

use guff::ast::{
    BlockStmt, CallExpr, Expr, FuncDecl, GoStmt, ReturnStmt, Stmt,
};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_ident, is_pkg_dot_name, unparen};

#[derive(Clone)]
struct FuncDesc {
    receiver: Option<String>,
    name: String,
}

#[derive(Clone)]
struct FuncStatus {
    desc: FuncDesc,
    seen_conditional_exit: bool,
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::FuncDecl(f) = decl else {
                continue;
            };
            check_func_decl(f, &mut failures);
        }
    }
    failures
}

fn check_func_decl(f: &FuncDecl, failures: &mut Vec<Failure>) {
    let Some(body) = &f.body else {
        return;
    };
    // Upstream distinguishes **three** cases, not two:
    //
    //     case n.Recv == nil:                    rec = nil
    //     case … len(n.Recv.List[0].Names) < 1:  rec = &ast.Ident{Name: "_"}
    //     default:                               rec = n.Recv.List[0].Names[0]
    //
    // and `funcDesc.equal` treats nil and non-nil as different. guff collapsed
    // the middle case into `None`, so a method with an **unnamed receiver**
    // looked like a free function and a bare call to the package function of
    // the same name became "unconditional recursion". telegraf's
    // `func (*configurationOriginal) normalizeInputDatatype(…)` ends with
    // `return normalizeInputDatatype(dataType)` — the free function, not
    // itself — three times over.
    let receiver = f.recv.as_ref().map(|recv| {
        recv.list
            .first()
            .and_then(|field| field.names.first().map(|id| id.name.clone()))
            .unwrap_or_else(|| "_".to_string())
    });
    let status = FuncStatus {
        desc: FuncDesc {
            receiver,
            name: f.name.name.clone(),
        },
        seen_conditional_exit: false,
    };
    walk_body(body, status, false, failures);
}

fn walk_body(
    block: &BlockStmt,
    mut status: FuncStatus,
    in_go: bool,
    failures: &mut Vec<Failure>,
) {
    for stmt in &block.list {
        walk_stmt(stmt, &mut status, in_go, failures);
    }
}

fn walk_stmt(stmt: &Stmt, status: &mut FuncStatus, in_go: bool, failures: &mut Vec<Failure>) {
    match stmt {
        Stmt::IfStmt(i) => {
            update_conditional_exit(status, &i.body);
            if let Some(else_) = &i.else_ {
                if has_control_exit(else_) {
                    status.seen_conditional_exit = true;
                }
            }
        }
        Stmt::SelectStmt(s) => update_conditional_exit(status, &s.body),
        Stmt::RangeStmt(r) => update_conditional_exit(status, &r.body),
        Stmt::TypeSwitchStmt(s) => update_conditional_exit(status, &s.body),
        Stmt::SwitchStmt(s) => update_conditional_exit(status, &s.body),
        Stmt::ForStmt(f) if f.cond.is_none() => {
            walk_body(&f.body, status.clone(), in_go, failures);
        }
        Stmt::GoStmt(g) => walk_call(&g.call, status, true, failures),
        Stmt::ExprStmt(e) => {
            if let Expr::CallExpr(call) = unparen(&e.x) {
                walk_call(call, status, in_go, failures);
            }
        }
        Stmt::AssignStmt(a) => {
            for rhs in &a.rhs {
                if let Expr::CallExpr(call) = unparen(rhs) {
                    walk_call(call, status, in_go, failures);
                }
            }
        }
        Stmt::ReturnStmt(r) => {
            for e in &r.results {
                if let Expr::CallExpr(call) = unparen(e) {
                    walk_call(call, status, in_go, failures);
                }
            }
        }
        Stmt::BlockStmt(b) => walk_body(b, status.clone(), in_go, failures),
        _ => {}
    }
}

fn walk_call(call: &CallExpr, status: &mut FuncStatus, in_go: bool, failures: &mut Vec<Failure>) {
    for arg in &call.args {
        if let Expr::CallExpr(inner) = unparen(arg) {
            walk_call(inner, status, in_go, failures);
        }
    }
    if in_go {
        return;
    }
    let (receiver, name) = match unparen(&call.fun) {
        Expr::Ident(id) => (None, id.name.clone()),
        Expr::SelectorExpr(sel) => {
            let recv = match unparen(&sel.x) {
                Expr::Ident(id) => Some(id.name.clone()),
                _ => return,
            };
            (recv, sel.sel.name.clone())
        }
        Expr::FuncLit(_) => return,
        _ => return,
    };
    if !status.seen_conditional_exit
        && status.desc.name == name
        && status.desc.receiver == receiver
    {
        failures.push(Failure {
            rule: "unconditional-recursion",
            pos: call.fun.pos().0 as u32,
            message: "unconditional recursive call".into(),
            ..Failure::default()
        });
    }
}

fn update_conditional_exit(status: &mut FuncStatus, block: &BlockStmt) {
    if !status.seen_conditional_exit && has_control_exit_in_block(block) {
        status.seen_conditional_exit = true;
    }
}

fn has_control_exit(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::ReturnStmt(_) => true,
        Stmt::ExprStmt(e) => is_exit_call(&e.x),
        Stmt::BlockStmt(b) => has_control_exit_in_block(b),
        Stmt::IfStmt(i) => {
            has_control_exit_in_block(&i.body)
                || i.else_.as_ref().is_some_and(|s| has_control_exit(s))
        }
        Stmt::ForStmt(f) => has_control_exit_in_block(&f.body),
        Stmt::RangeStmt(r) => has_control_exit_in_block(&r.body),
        Stmt::SwitchStmt(s) => has_control_exit_in_block(&s.body),
        Stmt::TypeSwitchStmt(s) => has_control_exit_in_block(&s.body),
        Stmt::SelectStmt(s) => has_control_exit_in_block(&s.body),
        Stmt::LabeledStmt(l) => has_control_exit(&l.stmt),
        Stmt::BranchStmt(_) => true,
        _ => false,
    }
}

fn has_control_exit_in_block(block: &BlockStmt) -> bool {
    block.list.iter().any(has_control_exit)
}

fn is_exit_call(expr: &Expr) -> bool {
    let Expr::CallExpr(call) = unparen(expr) else {
        return false;
    };
    if is_ident(&call.fun, "panic") {
        return true;
    }
    let Expr::SelectorExpr(sel) = unparen(&call.fun) else {
        return false;
    };
    let Expr::Ident(pkg) = unparen(&sel.x) else {
        return false;
    };
    match (pkg.name.as_str(), sel.sel.name.as_str()) {
        ("os", "Exit") => true,
        ("log", "Fatal" | "Fatalf" | "Fatalln" | "Panic" | "Panicf" | "Panicln") => true,
        _ => is_pkg_dot_name(&call.fun, "log", "Fatal"),
    }
}
