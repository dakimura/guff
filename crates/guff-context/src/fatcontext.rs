//! Port of [`go.augendre.info/fatcontext`](https://github.com/Crocmagnon/fatcontext).

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, BlockStmt, CallExpr, Expr, ForStmt, FuncLit, Ident, RangeStmt, SelectorExpr, Stmt,
};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::arena::TypeData;
use guff_types::selection::SelectionKind;

const CATEGORY_IN_LOOP: &str = "nested context in loop";
const CATEGORY_IN_FUNC_LIT: &str = "nested context in function literal";
const CATEGORY_IN_STRUCT_POINTER: &str = "potential nested context in struct pointer";
const CATEGORY_UNSUPPORTED: &str = "unsupported nested context type";

/// Pass-time options from `linters.settings.fatcontext`.
///
/// Upstream's three flags gate the three reportable categories, and only one of
/// them is off by default — which is why the struct-pointer category needs a
/// configuration to be reachable at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FatcontextOptions {
    pub check_struct_pointers: bool,
    pub check_loops: bool,
    pub check_function_literals: bool,
}

impl Default for FatcontextOptions {
    fn default() -> Self {
        Self {
            check_struct_pointers: false,
            check_loops: true,
            check_function_literals: true,
        }
    }
}

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

/// `isEmptyContext`: `context.Background()` / `context.TODO()`, or
/// `t.Context()` where `t` is a parameter declared `*testing.T`, `*testing.B`
/// or `testing.TB` — upstream resolves `ident.Obj.Decl` to that `*ast.Field`;
/// `testing_params` holds the name positions of such parameters.
fn is_empty_context(pass: &Pass<'_>, e: &Expr, testing_params: &HashSet<u32>) -> bool {
    let Expr::CallExpr(CallExpr { fun, .. }) = e else {
        return false;
    };
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = fun.as_ref() else {
        return false;
    };
    let Expr::Ident(ident) = x.as_ref() else {
        return false;
    };
    if ident.name == "context" && matches!(sel.name.as_str(), "Background" | "TODO") {
        return true;
    }
    if sel.name != "Context" {
        return false;
    }
    let (Some(info), Some(a)) = (pass.types_info(), pass.pkg().type_artifacts.as_ref()) else {
        return false;
    };
    info.uses
        .get(&ident.id)
        .is_some_and(|&obj| testing_params.contains(&obj.pos(&a.objects)))
}

/// The name positions of every parameter, result or receiver declared with
/// type `*testing.T`, `*testing.B`, `*testing.TB` or the same without the star
/// — by the spelling, as upstream reads `decl.Type`.
fn testing_params(file: &guff::ast::File) -> HashSet<u32> {
    let mut out = HashSet::new();
    let mut add = |fl: Option<&guff::ast::FieldList>| {
        for field in fl.into_iter().flat_map(|l| l.list.iter()) {
            let Some(mut ty) = field.ty.as_ref() else {
                continue;
            };
            if let Expr::StarExpr(star) = ty {
                ty = &star.x;
            }
            let Expr::SelectorExpr(sel) = ty else {
                continue;
            };
            let testing = matches!(sel.x.as_ref(), Expr::Ident(i) if i.name == "testing");
            if testing && matches!(sel.sel.name.as_str(), "T" | "B" | "TB") {
                out.extend(field.names.iter().map(|n| n.name_pos.0 as u32));
            }
        }
    };
    walk::inspect(NodeRef::File(file), |n| {
        match n {
            Some(NodeRef::FuncDecl(fd)) => add(fd.recv.as_ref()),
            Some(NodeRef::FuncType(ft)) => {
                add(ft.params.as_ref());
                add(ft.results.as_ref());
            }
            _ => {}
        }
        true
    });
    out
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

/// Upstream `getRootIdent`: peel Index/Selector; pointer-indirected selectors
/// are not a safe local copy.
fn get_root_ident<'a>(pass: &Pass<'_>, mut node: &'a Expr) -> Option<&'a Ident> {
    loop {
        match unparen(node) {
            Expr::Ident(id) => return Some(id),
            Expr::IndexExpr(i) => node = &i.x,
            Expr::SelectorExpr(sel) => {
                if is_pointer_sel(pass, node) {
                    return None;
                }
                node = &sel.x;
            }
            _ => return None,
        }
    }
}

fn enclosing_span(n: NodeRef<'_>) -> Option<(u32, u32)> {
    match n {
        NodeRef::ForStmt(ForStmt { for_, body, .. }) => {
            Some((for_.0 as u32, body.end().0 as u32))
        }
        NodeRef::RangeStmt(RangeStmt { for_, body, .. }) => {
            Some((for_.0 as u32, body.end().0 as u32))
        }
        NodeRef::FuncLit(FuncLit { ty, body, .. }) => {
            Some((ty.pos().0 as u32, body.end().0 as u32))
        }
        // `FuncDecl` was added to [`body_of`] for the struct-pointer category
        // and not here, so `is_defined_within` answered `None` — false — for
        // every assignment in a plain function body. A `context.Context`
        // *parameter*'s scope is the function body, which is inside the
        // declaration, so upstream lets `ctx = ctx2(ctx)` at the top level of
        // `func f(ctx context.Context)` alone; guff reported all of them
        // (prometheus, 14 findings). Go's `(*ast.FuncDecl).Pos()` is
        // `d.Type.Pos()` and its `End()` is the body's `}`.
        NodeRef::FuncDecl(fd) => {
            let end = fd
                .body
                .as_ref()
                .map(|b| b.end())
                .unwrap_or_else(|| fd.ty.end());
            Some((fd.ty.pos().0 as u32, end.0 as u32))
        }
        _ => None,
    }
}

/// Upstream `isWithinLoop`: lhs object's parent scope lies inside `node`.
/// Locals like `var tracingCtx context.Context` assigned with `=` are OK.
fn is_defined_within(pass: &Pass<'_>, exp: &Expr, node: NodeRef<'_>) -> bool {
    let Some((npos, nend)) = enclosing_span(node) else {
        return false;
    };
    let Some(lhs) = get_root_ident(pass, exp) else {
        return false;
    };
    let Some(obj) = code::object_of(pass, lhs) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(scope) = obj.parent(&artifacts.objects) else {
        return false;
    };
    let s = artifacts.scopes.get(scope);
    s.pos() >= npos && s.end() <= nend
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
    enclosing: NodeRef<'_>,
    testing_params: &HashSet<u32>,
) -> Option<&'a AssignStmt> {
    let mut reset: HashSet<String> = HashSet::new();
    for stmt in stmts {
        if let Some(list) = get_stmt_list(stmt) {
            if let Some(found) = find_nested_context(pass, list, enclosing, testing_params) {
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
        if is_empty_context(pass, &assign.rhs[0], testing_params) {
            if !name.is_empty() {
                reset.insert(name);
            }
            continue;
        }
        if !name.is_empty() && reset.contains(&name) {
            continue;
        }
        // A pointer-indirected field used to be skipped here, standing in for
        // the `check-struct-pointers` flag guff had no wiring for. It is a
        // *category* upstream, not an exclusion: `getCategory` names it and
        // `shouldIgnoreReport` drops it when the flag is off. Deciding it here
        // instead also changed which assignment gets found — upstream takes the
        // first nested context in the body and then classifies that one, so
        // skipping it would report a later one upstream never looks at.
        // Locals defined inside this For/FuncLit/FuncDecl may be reassigned.
        if is_defined_within(pass, &assign.lhs[0], enclosing) {
            continue;
        }
        return Some(assign);
    }
    None
}

/// The bodies upstream's node filter covers.
///
/// FuncDecl was left out here on the grounds that it reports every
/// `mw.ctx, mw.cancel = context.WithCancel(…)` written in a plain method. It
/// does — as `potential nested context in struct pointer`, which is off by
/// default. Dropping the node dropped the whole category with it.
fn body_of(n: NodeRef<'_>) -> Option<&BlockStmt> {
    match n {
        NodeRef::ForStmt(ForStmt { body, .. }) => Some(body),
        NodeRef::RangeStmt(RangeStmt { body, .. }) => Some(body),
        NodeRef::FuncLit(FuncLit { body, .. }) => Some(body),
        // Upstream's node filter is ForStmt / RangeStmt / FuncLit / **FuncDecl**.
        // Without the last one an assignment in a plain function body is never
        // looked at, which is exactly where the struct-pointer category lives:
        // `getCategory` only reaches the pointer test when the enclosing node is
        // not a loop, and a FuncLit is the only non-loop node that was reaching
        // it here.
        NodeRef::FuncDecl(fd) => fd.body.as_ref(),
        _ => None,
    }
}

/// Port of upstream `getCategory`.
///
/// The order matters and is not obvious: the enclosing node decides first, so
/// an assignment to a struct pointer field *inside a loop* is
/// `nested context in loop`, not the struct-pointer category. Only when the
/// enclosing node is not a loop does the pointer test get a turn.
fn category_for(pass: &Pass<'_>, n: NodeRef<'_>, assign: &AssignStmt) -> &'static str {
    if matches!(n, NodeRef::ForStmt(_) | NodeRef::RangeStmt(_)) {
        return CATEGORY_IN_LOOP;
    }
    if assign.lhs.first().is_some_and(|e| is_pointer(pass, e)) {
        return CATEGORY_IN_STRUCT_POINTER;
    }
    match n {
        NodeRef::FuncLit(_) | NodeRef::FuncDecl(_) => CATEGORY_IN_FUNC_LIT,
        _ => CATEGORY_UNSUPPORTED,
    }
}

/// Upstream `isPointer`: a selector whose selection required indirection.
fn is_pointer(pass: &Pass<'_>, e: &Expr) -> bool {
    if !matches!(e, Expr::SelectorExpr(_)) {
        return false;
    }
    let Some(info) = pass.types_info() else {
        return false;
    };
    // `Info.selections` is keyed on the SelectorExpr's own node id.
    info.selections.get(&e.id()).is_some_and(|s| s.indirect())
}

/// Upstream `shouldIgnoreReport`: each category has its own flag.
fn category_enabled(category: &str, opts: &FatcontextOptions) -> bool {
    match category {
        CATEGORY_IN_LOOP => opts.check_loops,
        CATEGORY_IN_FUNC_LIT => opts.check_function_literals,
        CATEGORY_IN_STRUCT_POINTER => opts.check_struct_pointers,
        _ => true,
    }
}

/// `isCleanupCall`: `x.Cleanup(…)` where `x` is a `testing.T`, `B` or `TB`
/// (or a pointer to one) — the named type itself, no unalias.
fn is_cleanup_call(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return false;
    };
    if sel.sel.name != "Cleanup" {
        return false;
    }
    let Some(a) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(mut t) = pass.types_info().and_then(|i| i.types.get(&sel.x.id())).map(|tv| tv.typ) else {
        return false;
    };
    if let TypeData::Pointer(p) = a.types.get(t) {
        t = p.elem();
    }
    if !matches!(a.types.get(t), TypeData::Named(_)) {
        return false;
    }
    let obj = guff_types::named::named_obj(&a.types, t);
    let in_testing = obj
        .pkg(&a.objects)
        .is_some_and(|p| a.packages.get(p).path() == "testing");
    in_testing && matches!(obj.name(&a.objects), "T" | "B" | "TB")
}

/// `isRunOnce` (v0.10): the starts of the function literals that run at most
/// once per execution of the statement registering them — `defer func() {…}()`
/// and `t.Cleanup(func() {…})` — and so cannot grow the context they capture.
/// Inside a loop (anywhere above, even outside the function) they run once
/// per iteration and stay reported.
fn run_once_func_lits(pass: &Pass<'_>, file: &guff::ast::File) -> HashSet<u32> {
    let mut out = HashSet::new();
    let mut stack: Vec<NodeRef<'_>> = Vec::new();
    walk::inspect(NodeRef::File(file), |n| {
        let Some(n) = n else {
            stack.pop();
            return true;
        };
        stack.push(n);
        let NodeRef::FuncLit(lit) = n else {
            return true;
        };
        let len = stack.len();
        let Some(NodeRef::CallExpr(call)) = len.checked_sub(2).map(|i| stack[i]) else {
            return true;
        };
        let lit_pos = lit.ty.pos().0;
        let run_once = if matches!(call.fun.as_ref(), Expr::FuncLit(f) if f.ty.pos().0 == lit_pos) {
            matches!(len.checked_sub(3).map(|i| stack[i]), Some(NodeRef::DeferStmt(_)))
        } else {
            is_cleanup_call(pass, call)
        };
        let in_loop = stack
            .iter()
            .any(|a| matches!(a, NodeRef::ForStmt(_) | NodeRef::RangeStmt(_)));
        if run_once && !in_loop {
            out.insert(lit_pos as u32);
        }
        true
    });
    out
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "fatcontext requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<FatcontextOptions>("fatcontext")
        .copied()
        .unwrap_or_default();

    let mut pending = Vec::new();
    for file in pass.files() {
        let run_once = run_once_func_lits(pass, file);
        let testing = testing_params(file);
        walk::preorder(NodeRef::File(file), |n| {
            if let NodeRef::FuncLit(lit) = n {
                if run_once.contains(&(lit.ty.pos().0 as u32)) {
                    return true;
                }
            }
            let Some(body) = body_of(n) else {
                return true;
            };
            let Some(assign) = find_nested_context(pass, &body.list, n, &testing) else {
                return true;
            };
            let category = category_for(pass, n, assign);
            if !category_enabled(category, &opts) {
                return true;
            }
            // `Pos: assignStmt.Pos()` — the statement's first LHS operand, not
            // the `=` between the sides.
            let report_pos = assign
                .lhs
                .first()
                .map(|e| e.pos().0 as u32)
                .unwrap_or(assign.tok_pos.0 as u32);
            let start = assign.tok_pos.0 as u32;
            let end = assign
                .rhs
                .last()
                .map(|e| e.end().0 as u32)
                .unwrap_or(start);
            // Suggested fix: replace `=` with `:=` (tok span).
            //
            // KNOWN DIFFERENCE: upstream's edit spans the *whole* statement
            // (`assignStmt.Pos()`..`End()`) and substitutes a re-rendered copy
            // with `Tok: token.DEFINE`. The visible result is the same text for
            // every shape we have, and no tier compares replacements — §1 of
            // docs/COMPAT-HARDENING.md lists SuggestedFix as uncompared — so
            // this is recorded rather than ported blind.
            let tok_end = start + 1; // "="
            // `getSuggestedFixes` returns nil for these two: rewriting `=` to
            // `:=` would not be correct for a field, and there is nothing to
            // suggest for a shape upstream does not recognise.
            let fixes = if matches!(category, CATEGORY_IN_STRUCT_POINTER | CATEGORY_UNSUPPORTED) {
                Vec::new()
            } else {
                vec![SuggestedFix {
                    message: "replace `=` with `:=`".into(),
                    text_edits: vec![TextEdit {
                        pos: start,
                        end: tok_end,
                        new_text: ":=".into(),
                    }],
                }]
            };
            pending.push(Diagnostic {
                pos: report_pos,
                end,
                message: category.into(),
                suggested_fixes: fixes,
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
