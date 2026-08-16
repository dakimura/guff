//! `tests` — check Go test, benchmark, fuzz, and example naming conventions.

use std::sync::OnceLock;

use guff::ast::{BasicLit, CallExpr, Expr, FuncDecl, FuncLit, FuncType, StarExpr};
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;
use guff_types::tuple::tuple_len;

use crate::govet_util::{is_method_named, is_testing_type, tuple_type_at};

fn is_test_file(pass: &Pass<'_>, _f: &guff::ast::File) -> bool {
    pass.pkg()
        .go_files
        .iter()
        .any(|p| p.to_string_lossy().ends_with("_test.go"))
}

fn is_test_param(ty: &Expr, want: &str) -> bool {
    let Expr::StarExpr(StarExpr { x, .. }) = ty else {
        return false;
    };
    match x.as_ref() {
        Expr::Ident(id) => id.name == want,
        Expr::SelectorExpr(sel) => sel.sel.name == want,
        _ => false,
    }
}

fn is_test_suffix(name: &str) -> bool {
    name.is_empty() || !name.chars().next().is_some_and(|c| c.is_lowercase())
}

fn check_test(pass: &Pass<'_>, fn_: &FuncDecl, prefix: &str) -> Option<String> {
    let ft = &fn_.ty;
    if ft.results.as_ref().is_some_and(|r| !r.list.is_empty()) {
        return None;
    }
    let params = ft.params.as_ref()?;
    if params.list.len() != 1 || params.list[0].names.len() > 1 {
        return None;
    }
    let ty = params.list[0].ty.as_ref()?;
    if !is_test_param(ty, &prefix[..1]) {
        return None;
    }
    let suffix = fn_.name.name.strip_prefix(prefix).unwrap_or(&fn_.name.name);
    if !is_test_suffix(suffix) {
        return Some(format!(
            "{} has malformed name: first letter after '{prefix}' must not be lowercase",
            fn_.name.name
        ));
    }
    None
}

fn check_example(pass: &Pass<'_>, fn_: &FuncDecl) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let name = &fn_.name.name;
    if fn_.ty.params.as_ref().is_some_and(|p| !p.list.is_empty()) {
        out.push((fn_.name.pos().0 as u32, format!("{name} should be niladic")));
    }
    if fn_.ty.results.as_ref().is_some_and(|r| !r.list.is_empty()) {
        out.push((fn_.name.pos().0 as u32, format!("{name} should return nothing")));
    }
    out
}

fn is_fuzz_target_dot(pass: &Pass<'_>, call: &CallExpr, method: &str) -> bool {
    if method.is_empty() {
        return is_method_named(pass, call, "testing", "F", "");
    }
    is_method_named(pass, call, "testing", "F", method)
}

fn accepted_fuzz_type(pass: &Pass<'_>, typ: guff_types::TypeId) -> bool {
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return false,
    };
    let u = typ.underlying(&artifacts.types);
    match artifacts.types.get(u) {
        TypeData::Basic(b) => matches!(
            b.kind(),
            BasicKind::String
                | BasicKind::Bool
                | BasicKind::Float32
                | BasicKind::Float64
                | BasicKind::Int
                | BasicKind::Int8
                | BasicKind::Int16
                | BasicKind::Int32
                | BasicKind::Int64
                | BasicKind::Uint
                | BasicKind::Uint8
                | BasicKind::Uint16
                | BasicKind::Uint32
                | BasicKind::Uint64
        ),
        TypeData::Slice(s) => {
            let elem = s.elem().underlying(&artifacts.types);
            matches!(artifacts.types.get(elem), TypeData::Basic(b) if b.kind() == BasicKind::Uint8)
        }
        _ => false,
    }
}

fn check_fuzz_call(pass: &Pass<'_>, fn_: &FuncDecl) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    walk_calls(fn_, &mut |call| {
        if !is_fuzz_target_dot(pass, call, "Fuzz") {
            return;
        }
        if call.args.len() != 1 {
            return;
        }
        let arg = &call.args[0];
        let Expr::FuncLit(lit) = arg else {
            out.push((arg.pos().0 as u32, "argument to Fuzz must be a function".into()));
            return;
        };
        let ty = &lit.ty;
        if ty.results.as_ref().is_some_and(|r| !r.list.is_empty()) {
            out.push((arg.pos().0 as u32, "fuzz target must not return any value".into()));
        }
        let ast_params = match ty.params.as_ref() {
            Some(p) => p,
            None => {
                out.push((arg.pos().0 as u32, "fuzz target must have 1 or more argument".into()));
                return;
            }
        };
        if ast_params.list.is_empty() {
            out.push((arg.pos().0 as u32, "fuzz target must have 1 or more argument".into()));
            return;
        }
        let Some(param_tuple) = type_of_func(pass, ty) else {
            return;
        };
        let artifacts = match pass.pkg().type_artifacts.as_ref() {
            Some(a) => a,
            None => return,
        };
        if tuple_len(&artifacts.types, Some(param_tuple)) == 0 {
            return;
        }
        let t0 = tuple_type_at(pass, Some(param_tuple), 0);
        if !t0.is_some_and(|t| is_testing_type(pass, t, "T")) {
            out.push((
                ast_params.list[0]
                    .ty
                    .as_ref()
                    .map(|t| t.pos().0)
                    .unwrap_or(arg.pos().0) as u32,
                "the first parameter of a fuzz target must be *testing.T".into(),
            ));
        }
        for i in 1..tuple_len(&artifacts.types, Some(param_tuple)) {
            let t = tuple_type_at(pass, Some(param_tuple), i);
            if !t.is_some_and(|t| accepted_fuzz_type(pass, t)) {
                out.push((
                    ast_params
                        .list
                        .get(i)
                        .and_then(|f| f.ty.as_ref())
                        .map(|t| t.pos().0)
                        .unwrap_or(arg.pos().0) as u32,
                    "fuzzing arguments can only have limited types".into(),
                ));
            }
        }
    });
    out
}

fn type_of_func(pass: &Pass<'_>, ty: &FuncType) -> Option<guff_types::TypeId> {
    let info = pass.types_info()?;
    info.types.get(&ty.id).map(|tv| tv.typ)
}

fn walk_calls(fn_: &FuncDecl, f: &mut dyn FnMut(&CallExpr)) {
    let Some(body) = &fn_.body else {
        return;
    };
    walk_stmts(&body.list, f);
}

fn walk_stmts(stmts: &[guff::ast::Stmt], f: &mut dyn FnMut(&CallExpr)) {
    for stmt in stmts {
        if let guff::ast::Stmt::ExprStmt(s) = stmt {
            if let Expr::CallExpr(c) = &s.x {
                f(c);
            }
        }
        if let guff::ast::Stmt::BlockStmt(b) = stmt {
            walk_stmts(&b.list, f);
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut pending = Vec::new();
    for file in pass.files() {
        if !is_test_file(pass, file) {
            continue;
        }
        for decl in &file.decls {
            let guff::ast::Decl::FuncDecl(fn_) = decl else {
                continue;
            };
            if fn_.recv.is_some() {
                continue;
            }
            let name = &fn_.name.name;
            if name.starts_with("Example") {
                pending.extend(check_example(pass, fn_));
            } else if name.starts_with("Test") {
                if let Some(msg) = check_test(pass, fn_, "Test") {
                    pending.push((fn_.name.pos().0 as u32, msg));
                }
            } else if name.starts_with("Benchmark") {
                if let Some(msg) = check_test(pass, fn_, "Benchmark") {
                    pending.push((fn_.name.pos().0 as u32, msg));
                }
            } else if name.starts_with("Fuzz") {
                if let Some(msg) = check_test(pass, fn_, "Fuzz") {
                    pending.push((fn_.name.pos().0 as u32, msg));
                }
                pending.extend(check_fuzz_call(pass, fn_));
            }
        }
    }
    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "tests",
        doc: "check naming conventions for tests, benchmarks, fuzz targets, and examples",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/tests",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![],
        fact_types: vec![],
    })
}
