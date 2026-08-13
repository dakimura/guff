//! `testinggoroutine` — `t.Fatal` and friends called from a non-test goroutine.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/testinggoroutine` (checked
//! against v0.47.0; the analyzer is byte-identical from v0.47.0 to x/tools HEAD).
//!
//! `testing.T.Fatal`, `Fatalf`, `FailNow`, `Skip`, `Skipf` and `SkipNow` all end
//! in `runtime.Goexit`, which only stops the goroutine that calls it. From a
//! `go` statement that means the test neither fails nor stops — it keeps running
//! with one goroutine silently gone.
//!
//! The shape of the analysis is "regions": each `go fun()` and each
//! `t.Run(name, fun)` names a stretch of code that runs concurrently with (or
//! separately from) the test function, and the check looks for forbidden calls
//! inside it. A region nested in another belongs to the inner one only — which
//! is why `t.Run` regions are collected even though upstream's `-subtest`
//! reporting is off by default: without them, a `t.Fatal` inside a subtest
//! literal inside a `go` statement would be attributed to the goroutine and
//! reported, and upstream says nothing.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, FuncDecl, FuncLit, Ident};
use guff::scope::ObjDecl;
use guff::walk::NodeRef;
use guff_analysis::code::{self, unparen};
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::ObjectId;

/// `(*testing.common)` methods that end in `runtime.Goexit`.
const FORBIDDEN: &[&str] = &["FailNow", "Fatal", "Fatalf", "Skip", "Skipf", "SkipNow"];

/// A region's span, used as its identity. Upstream keys a `map[ast.Node]` on
/// pointer identity; two distinct region nodes cannot share a start *and* an
/// end offset, so the pair stands in for it.
type Span = (i64, i64);

/// What the region was started from, which decides where a report is placed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AsyncKind {
    /// `go fun()` — reported, since the goroutine is not the test's.
    Go,
    /// `t.Run(name, fun)` — only reported under upstream's experimental
    /// `-subtest` flag, which golangci-lint leaves off. Collected anyway: the
    /// region still claims the calls inside it.
    Run,
}

struct AsyncCall {
    kind: AsyncKind,
    /// Position of the `go` statement / `t.Run` call — where a report goes
    /// unless the started function is a literal.
    async_pos: u32,
    /// `fun` in `go fun()` / `t.Run(name, fun)`: a literal reports at the
    /// offending call instead, and a plain identifier adds "(f calls …)".
    fun_is_lit: bool,
    fun_ident: Option<String>,
    /// Report only when the receiver is declared outside this span
    /// (`withinScope`). `None` — every `go` statement — always reports.
    scope: Option<Span>,
}

/// The span of a node that can be a region. Every other node kind answers
/// `None` — nothing else is ever compared against the region set, and giving
/// each kind its own arm would only invite one of them to be wrong.
fn node_span(node: NodeRef<'_>) -> Option<Span> {
    Some(match node {
        NodeRef::FuncLit(lit) => func_lit_span(lit),
        NodeRef::FuncDecl(fd) => (
            fd.ty.pos().0,
            fd.body
                .as_ref()
                .map(|b| b.end())
                .unwrap_or_else(|| fd.ty.end())
                .0,
        ),
        NodeRef::GoStmt(go) => (go.go_.0, go.call.end().0),
        NodeRef::CallExpr(call) => (call.pos().0, call.end().0),
        _ => return None,
    })
}

fn func_lit_span(lit: &FuncLit) -> Span {
    (lit.ty.pos().0, lit.body.end().0)
}

/// `hasBenchmarkOrTestParams` — purely syntactic, like upstream: a parameter
/// spelled `*testing.T` or `*testing.B`.
fn has_benchmark_or_test_params(fd: &FuncDecl) -> bool {
    let Some(params) = fd.ty.params.as_ref() else {
        return false;
    };
    params.list.iter().any(|p| {
        let Some(Expr::StarExpr(star)) = &p.ty else {
            return false;
        };
        let Expr::SelectorExpr(sel) = star.x.as_ref() else {
            return false;
        };
        let Expr::Ident(pkg) = sel.x.as_ref() else {
            return false;
        };
        pkg.name == "testing" && (sel.sel.name == "T" || sel.sel.name == "B")
    })
}

/// `funcLitInScope` — the function literal an identifier was (first) assigned.
///
/// Upstream reads `id.Obj`, the parser's own scope resolution, and so does this:
/// guff's parser fills the same field.
fn func_lit_in_scope(id: &Ident) -> Option<FuncLit> {
    let obj = id.obj.lock().ok()?.clone()?;
    let rhs: Option<Expr> = match &obj.decl {
        ObjDecl::AssignStmt(a) => a.lhs.iter().enumerate().find_map(|(i, x)| {
            let Expr::Ident(name) = x else { return None };
            (name.name == id.name && i < a.rhs.len()).then(|| a.rhs[i].clone())
        }),
        ObjDecl::ValueSpec(v) => v.names.iter().enumerate().find_map(|(i, n)| {
            (n.name == id.name && i < v.values.len()).then(|| v.values[i].clone())
        }),
        _ => None,
    };
    match rhs {
        Some(Expr::FuncLit(lit)) => Some(lit),
        _ => None,
    }
}

/// The identifier `e` denotes, if any (`typesinternal.UsedIdent`, restricted to
/// the plain-identifier case this analyzer looks up in the parser's scope).
fn used_plain_ident(e: &Expr) -> Option<&Ident> {
    match unparen(e) {
        Expr::Ident(id) => Some(id),
        _ => None,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    // Upstream: `typesinternal.Imports(pass.Pkg, "testing")`.
    if !pass.pkg().imports.contains_key("testing") {
        return Ok(None);
    }

    // --- collect regions ---------------------------------------------------
    //
    // One `ast.Inspect` per file, pruning function declarations that take no
    // `*testing.T` / `*testing.B`: a `go` statement in ordinary code is not
    // this analyzer's business.
    let mut asyncs: HashMap<Span, Vec<AsyncCall>> = HashMap::new();
    let mut regions: Vec<Span> = Vec::new();
    let mut add = |region: Span, call: AsyncCall, regions: &mut Vec<Span>| {
        if !asyncs.contains_key(&region) {
            regions.push(region);
        }
        asyncs.entry(region).or_default().push(call);
    };

    for file in pass.files() {
        guff::walk::preorder_prune(NodeRef::File(file), |n| {
            match n {
                NodeRef::FuncDecl(fd) => return has_benchmark_or_test_params(fd),
                NodeRef::GoStmt(go) => {
                    let fun = unparen(go.call.fun.as_ref());
                    let async_pos = go.go_.0 as u32;
                    let fun_ident = used_plain_ident(fun).map(|i| i.name.clone());
                    let fun_is_lit = matches!(fun, Expr::FuncLit(_));

                    // `go f()` where f is a variable holding a literal: the
                    // literal is the region.
                    let mut region = None;
                    if let Some(id) = used_plain_ident(fun) {
                        if let Some(lit) = func_lit_in_scope(id) {
                            region = Some(func_lit_span(&lit));
                        }
                    }
                    // `go f()` where f is a function of this package: its
                    // declaration is the region.
                    if region.is_none() {
                        if let Some(decl) = local_decl_span(pass, &go.call) {
                            region = Some(decl);
                        }
                    }
                    // Otherwise the `go` statement itself — which covers
                    // `go t.Fatal()` and `go func(){ t.Fatal() }()`.
                    let region = region.unwrap_or((go.go_.0, go.call.end().0));
                    add(
                        region,
                        AsyncCall {
                            kind: AsyncKind::Go,
                            async_pos,
                            fun_is_lit,
                            fun_ident,
                            scope: None,
                        },
                        &mut regions,
                    );
                }
                NodeRef::CallExpr(call) => {
                    if let Some((region, ac)) = t_run_async_call(pass, call) {
                        add(region, ac, &mut regions);
                    }
                }
                _ => {}
            }
            true
        });
    }

    if regions.is_empty() {
        return Ok(None);
    }
    let region_set: std::collections::HashSet<Span> = regions.iter().copied().collect();

    // --- check each region -------------------------------------------------
    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        guff::walk::preorder_prune(NodeRef::File(file), |n| {
            let Some(s) = node_span(n) else {
                return true;
            };
            let Some(calls) = asyncs.get(&s) else {
                return true;
            };
            // Walk this region, stopping at any nested region: those calls
            // belong to the inner one.
            guff::walk::preorder_prune(n, |m| {
                if let Some(ms) = node_span(m) {
                    if ms != s && region_set.contains(&ms) {
                        return false;
                    }
                }
                let NodeRef::CallExpr(call) = m else {
                    return true;
                };
                let Some((recv, method)) = forbidden_method(pass, call) else {
                    return true;
                };
                for e in calls {
                    if within_scope(pass, e.scope, recv) {
                        continue;
                    }
                    if e.kind != AsyncKind::Go {
                        // `-subtest` is off in golangci-lint; the region still
                        // did its job by claiming this call.
                        continue;
                    }
                    // `where`: the `go` statement, unless the goroutine runs a
                    // literal — then the offending call itself.
                    let where_pos = if e.fun_is_lit {
                        call.pos().0 as u32
                    } else {
                        e.async_pos
                    };
                    let context = match &e.fun_ident {
                        Some(name) => format!(" ({name} calls {method})"),
                        None => String::new(),
                    };
                    pending.push((
                        where_pos,
                        format!("call to {method} from a non-test goroutine{context}"),
                    ));
                }
                true
            });
            true
        });
    }

    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

/// The span of the declaration of `call`'s static callee, when that function is
/// declared in this package (`localFunctionDecls` + `toDecl`).
fn local_decl_span(pass: &Pass<'_>, call: &CallExpr) -> Option<Span> {
    let obj = code::call_target_object(pass, &call.fun)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if !matches!(
        artifacts.objects.get(obj),
        guff_types::arena::ObjectData::Func(_)
    ) {
        return None;
    }
    let info = pass.types_info()?;
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::FuncDecl(fd) = decl else {
                continue;
            };
            if info.defs.get(&fd.name.id).copied().flatten() == Some(obj) {
                return Some(node_span(NodeRef::FuncDecl(fd)).expect("FuncDecl has a span"));
            }
        }
    }
    None
}

/// `tRunAsyncCall` — the region started by `t.Run(name, fun)`.
fn t_run_async_call(pass: &Pass<'_>, call: &CallExpr) -> Option<(Span, AsyncCall)> {
    if call.args.len() != 2 {
        return None;
    }
    // `isMethodNamed(run, "testing", "Run")` ignores the receiver type name, so
    // `T.Run`, `B.Run` and `F.Run` all qualify.
    let obj = code::call_target_object(pass, &call.fun)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if !matches!(
        artifacts.objects.get(obj),
        guff_types::arena::ObjectData::Func(_)
    ) {
        return None;
    }
    if obj.name(&artifacts.objects) != "Run" {
        return None;
    }
    if code::object_pkg_path(pass, obj).as_deref() != Some("testing") {
        return None;
    }
    let sig = obj.typ(&artifacts.objects)?;
    guff_types::signature::signature_recv(&artifacts.types, sig)?;

    let fun = unparen(&call.args[1]);
    let async_pos = call.pos().0 as u32;

    if let Expr::FuncLit(lit) = fun {
        let s = func_lit_span(lit);
        return Some((
            s,
            AsyncCall {
                kind: AsyncKind::Run,
                async_pos,
                fun_is_lit: true,
                fun_ident: None,
                scope: Some(s),
            },
        ));
    }
    if let Some(id) = used_plain_ident(fun) {
        if let Some(lit) = func_lit_in_scope(id) {
            let s = func_lit_span(&lit);
            return Some((
                s,
                AsyncCall {
                    kind: AsyncKind::Run,
                    async_pos,
                    fun_is_lit: false,
                    fun_ident: Some(id.name.clone()),
                    scope: Some(s),
                },
            ));
        }
    }
    let s = (call.pos().0, call.end().0);
    Some((
        s,
        AsyncCall {
            kind: AsyncKind::Run,
            async_pos,
            fun_is_lit: false,
            fun_ident: used_plain_ident(fun).map(|i| i.name.clone()),
            scope: Some((fun.pos().0, fun.end().0)),
        },
    ))
}

/// `forbiddenMethod` + `formatMethod`: decompose `x.m()` into the receiver
/// variable and the rendered method name (`(*testing.T).Fatal`).
fn forbidden_method(pass: &Pass<'_>, call: &CallExpr) -> Option<(ObjectId, String)> {
    let Expr::SelectorExpr(sel_expr) = unparen(&call.fun) else {
        return None;
    };
    let info = pass.types_info()?;
    // A *selection*, not a qualified identifier: `pkg.Fatal()` has no receiver.
    let sel = info.selections.get(&sel_expr.id)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;

    let Expr::Ident(x_id) = unparen(&sel_expr.x) else {
        return None;
    };
    let x = *info.uses.get(&x_id.id)?;
    if !matches!(
        artifacts.objects.get(x),
        guff_types::arena::ObjectData::Var(_)
    ) {
        return None;
    }

    let fn_obj = sel.obj();
    if !matches!(
        artifacts.objects.get(fn_obj),
        guff_types::arena::ObjectData::Func(_)
    ) {
        return None;
    }
    let name = fn_obj.name(&artifacts.objects);
    if !FORBIDDEN.contains(&name) {
        return None;
    }
    if code::object_pkg_path(pass, fn_obj).as_deref() != Some("testing") {
        return None;
    }
    let sig = fn_obj.typ(&artifacts.objects)?;
    guff_types::signature::signature_recv(&artifacts.types, sig)?;

    // `formatMethod` renders the *selection's* receiver, not the method's own:
    // through an embedded `*testing.T` the report names the outer type.
    let recv = sel.recv();
    let (ptr, base) = match artifacts.types.get(guff_types::alias::unalias_readonly(
        &artifacts.types,
        recv,
    )) {
        guff_types::arena::TypeData::Pointer(p) => ("*", p.elem()),
        _ => ("", recv),
    };
    let base_str = guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        base,
        None,
    );
    Some((x, format!("({ptr}{base_str}).{name}")))
}

/// `withinScope`: is the receiver variable declared inside the region?
fn within_scope(pass: &Pass<'_>, scope: Option<Span>, x: ObjectId) -> bool {
    let Some((start, end)) = scope else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let pos = x.pos(&artifacts.objects) as i64;
    pos != 0 && start <= pos && pos <= end
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "testinggoroutine",
        doc: "report calls to (*testing.T).Fatal from goroutines started by a test",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/testinggoroutine",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    })
}

