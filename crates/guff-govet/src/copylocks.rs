//! `copylocks` — check for locks erroneously passed by value.

use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, CallExpr, CompositeLit, Expr, FieldList, FuncType, GenDecl, Ident, RangeStmt,
    ReturnStmt, Spec, ValueSpec,
};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

use crate::expreq::unparen;
use crate::lockpath::{lock_path_display, LockChecker};

fn lock_path_expr(
    pass: &Pass<'_>,
    checker: &mut LockChecker,
    e: &Expr,
) -> Option<crate::lockpath::LockPath> {
    let e = unparen(e);
    if matches!(e, Expr::CompositeLit(_) | Expr::CallExpr(_)) {
        return None;
    }
    if let Expr::StarExpr(star) = e {
        if matches!(unparen(&star.x), Expr::CallExpr(_)) {
            return None;
        }
    }
    let info = pass.types_info()?;
    let tv = info.types.get(&e.id())?;
    if tv.mode != guff_types::operand::OperandMode::Value {
        return None;
    }
    checker.lock_path_rhs(pass, tv.typ)
}

fn collect_assign(
    pass: &Pass<'_>,
    checker: &mut LockChecker,
    assign: &AssignStmt,
) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for (i, rhs) in assign.rhs.iter().enumerate() {
        let Some(path) = lock_path_expr(pass, checker, rhs) else {
            continue;
        };
        let lhs = assign
            .lhs
            .get(i)
            .map(|e| expr_format(e))
            .unwrap_or_else(|| "_".into());
        out.push((
            rhs.pos().0 as u32,
            format!("assignment copies lock value to {lhs}: {}", lock_path_display(&path)),
        ));
    }
    out
}

fn collect_gendecl(
    pass: &Pass<'_>,
    checker: &mut LockChecker,
    gd: &GenDecl,
) -> Vec<(u32, String)> {
    if gd.tok != Some(Token::VAR) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for spec in &gd.specs {
        let Spec::ValueSpec(ValueSpec { names, values, .. }) = spec else {
            continue;
        };
        for (i, rhs) in values.iter().enumerate() {
            let Some(path) = lock_path_expr(pass, checker, rhs) else {
                continue;
            };
            let name = names
                .get(i)
                .map(|id| id.name.clone())
                .unwrap_or_else(|| "_".into());
            out.push((
                rhs.pos().0 as u32,
                format!(
                    "variable declaration copies lock value to {name}: {}",
                    lock_path_display(&path)
                ),
            ));
        }
    }
    out
}

fn collect_composite_lit(
    pass: &Pass<'_>,
    checker: &mut LockChecker,
    cl: &CompositeLit,
) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for elt in &cl.elts {
        let value = match elt {
            Expr::KeyValueExpr(kv) => &kv.value,
            other => other,
        };
        let Some(path) = lock_path_expr(pass, checker, value) else {
            continue;
        };
        out.push((
            value.pos().0 as u32,
            format!(
                "literal copies lock value from {}: {}",
                expr_format(value),
                lock_path_display(&path)
            ),
        ));
    }
    out
}

fn collect_return(
    pass: &Pass<'_>,
    checker: &mut LockChecker,
    ret: &ReturnStmt,
) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for r in &ret.results {
        let Some(path) = lock_path_expr(pass, checker, r) else {
            continue;
        };
        out.push((
            r.pos().0 as u32,
            format!("return copies lock value: {}", lock_path_display(&path)),
        ));
    }
    out
}

fn collect_call(
    pass: &Pass<'_>,
    checker: &mut LockChecker,
    call: &CallExpr,
) -> Vec<(u32, String)> {
    if is_sizeof_family(pass, &call.fun) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for arg in &call.args {
        let Some(path) = lock_path_expr(pass, checker, arg) else {
            continue;
        };
        out.push((
            arg.pos().0 as u32,
            format!(
                "call of {} copies lock value: {}",
                expr_format(&call.fun),
                lock_path_display(&path)
            ),
        ));
    }
    out
}

fn is_sizeof_family(pass: &Pass<'_>, fun: &Expr) -> bool {
    let Some(name) = guff_analysis::code::call_name(pass, fun) else {
        return false;
    };
    matches!(
        name.as_str(),
        "len" | "cap" | "unsafe.Sizeof" | "unsafe.Offsetof" | "unsafe.Alignof"
    )
}

fn collect_func_type(
    pass: &Pass<'_>,
    checker: &mut LockChecker,
    name: &str,
    recv: Option<&FieldList>,
    typ: &FuncType,
) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    if let Some(recv) = recv {
        if let Some(field) = recv.list.first() {
            if let Some(ty_expr) = &field.ty {
                out.extend(collect_field(pass, checker, name, "passes lock by value", ty_expr));
            }
        }
    }
    if let Some(params) = &typ.params {
        for field in &params.list {
            if let Some(ty_expr) = &field.ty {
                out.extend(collect_field(pass, checker, name, "passes lock by value", ty_expr));
            }
        }
    }
    out
}

fn collect_field(
    pass: &Pass<'_>,
    checker: &mut LockChecker,
    name: &str,
    msg: &str,
    expr: &Expr,
) -> Vec<(u32, String)> {
    let info = match pass.types_info() {
        Some(i) => i,
        None => return Vec::new(),
    };
    let Some(tv) = info.types.get(&expr.id()) else {
        return Vec::new();
    };
    let Some(path) = checker.lock_path_rhs(pass, tv.typ) else {
        return Vec::new();
    };
    vec![(
        expr.pos().0 as u32,
        format!("{name} {msg}: {}", lock_path_display(&path)),
    )]
}

fn collect_range(
    pass: &Pass<'_>,
    checker: &mut LockChecker,
    r: &RangeStmt,
) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    if let Some(key) = &r.key {
        out.extend(collect_range_var(pass, checker, r.tok, key));
    }
    if let Some(v) = &r.value {
        out.extend(collect_range_var(pass, checker, r.tok, v));
    }
    out
}

fn collect_range_var(
    pass: &Pass<'_>,
    checker: &mut LockChecker,
    tok: Option<Token>,
    e: &Expr,
) -> Vec<(u32, String)> {
    let Expr::Ident(id) = e else {
        return Vec::new();
    };
    if id.name == "_" {
        return Vec::new();
    }
    let Some(info) = pass.types_info() else {
        return Vec::new();
    };
    let typ = if tok == Some(Token::DEFINE) {
        let Some(obj) = info.defs.get(&id.id).and_then(|o| *o) else {
            return Vec::new();
        };
        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
            return Vec::new();
        };
        let Some(t) = obj.typ(&artifacts.objects) else {
            return Vec::new();
        };
        t
    } else {
        let Some(tv) = info.types.get(&id.id) else {
            return Vec::new();
        };
        tv.typ
    };
    let Some(path) = checker.lock_path_rhs(pass, typ) else {
        return Vec::new();
    };
    vec![(
        e.pos().0 as u32,
        format!("range var {} copies lock: {}", id.name, lock_path_display(&path)),
    )]
}

fn expr_format(e: &Expr) -> String {
    match e {
        Expr::Ident(Ident { name, .. }) => name.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", expr_format(&sel.x), sel.sel.name),
        Expr::StarExpr(s) => format!("*{}", expr_format(&s.x)),
        _ => "_".into(),
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "copylocks requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    let mut checker = LockChecker::new();
    inspect.preorder(pass.files(), |n| {
        match n {
            NodeRef::AssignStmt(s) => pending.extend(collect_assign(pass, &mut checker, s)),
            NodeRef::GenDecl(g) => pending.extend(collect_gendecl(pass, &mut checker, g)),
            NodeRef::CompositeLit(c) => {
                pending.extend(collect_composite_lit(pass, &mut checker, c))
            }
            NodeRef::ReturnStmt(r) => pending.extend(collect_return(pass, &mut checker, r)),
            NodeRef::CallExpr(c) => pending.extend(collect_call(pass, &mut checker, c)),
            NodeRef::FuncDecl(f) => pending.extend(collect_func_type(
                pass,
                &mut checker,
                &f.name.name,
                f.recv.as_ref(),
                &f.ty,
            )),
            NodeRef::FuncLit(fl) => {
                pending.extend(collect_func_type(pass, &mut checker, "func", None, &fl.ty))
            }
            NodeRef::RangeStmt(r) => pending.extend(collect_range(pass, &mut checker, r)),
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
        name: "copylocks",
        doc: "check for locks erroneously passed by value",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/copylock",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
