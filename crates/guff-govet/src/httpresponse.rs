//! `httpresponse` — check for mistakes using HTTP responses.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, Stmt};
use guff::walk::{preorder_stack, stmt_ref, NodeRef};
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::TypeData;
use guff_types::named::named_obj;
use guff_types::TypeId;

use crate::govet_util::{
    expr_type, imports_package, is_type_named, root_ident, tuple_len_of, tuple_type_at,
};

/// `types.Identical(t, types.Universe.Lookup("error").Type())`.
///
/// The universe `error` is the one named type with no package, so the package
/// test is what separates it from a locally declared `type error …` that
/// shadows it — which is legal Go and prints the same in a type string.
fn is_universe_error(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = guff_types::alias::unalias_readonly(&artifacts.types, typ);
    let TypeData::Named(_) = artifacts.types.get(typ) else {
        return false;
    };
    let obj = named_obj(&artifacts.types, typ);
    obj.name(&artifacts.objects) == "error" && obj.pkg(&artifacts.objects).is_none()
}

/// `types.Unalias(t).(*types.Pointer)` whose element is `pkg_path.name`.
///
/// Deliberately not `underlying()`: upstream reaches the pointer through
/// `typesinternal.ReceiverNamed`, which unaliases and then type-asserts, so a
/// *named* pointer type (`type respPtr *http.Response`) is not a pointer for
/// this purpose.
fn is_pointer_to_named(pass: &Pass<'_>, typ: TypeId, pkg_path: &str, name: &str) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = guff_types::alias::unalias_readonly(&artifacts.types, typ);
    let TypeData::Pointer(p) = artifacts.types.get(typ) else {
        return false;
    };
    is_type_named(pass, p.elem(), pkg_path, name)
}

/// Port of upstream's `isHTTPFuncOrMethodOnClient`.
///
/// The signature test is the whole check: `net/http` also exports
/// `MaxBytesReader` and `NewRequest`, and treating "the callee lives in
/// net/http" as sufficient reported every `r.Body = http.MaxBytesReader(…)`
/// followed by `defer r.Body.Close()` — the standard request-body idiom.
fn is_http_func_or_method_on_client(pass: &Pass<'_>, call: &CallExpr) -> bool {
    // `x.f()`, never a bare `f()`: upstream asserts the callee is a selector
    // before it looks at anything else, so a local function returning
    // `(*http.Response, error)` is out of scope.
    let Expr::SelectorExpr(fun) = &*call.fun else {
        return false;
    };
    let Some(sig) = expr_type(pass, &call.fun) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    // A conversion `http.HandlerFunc(f)` records a named type here, not a
    // signature, and upstream's type assertion rejects it.
    if !matches!(artifacts.types.get(sig), TypeData::Signature(_)) {
        return false;
    }

    let results = guff_types::signature::signature_results(&artifacts.types, sig);
    if tuple_len_of(pass, results) != 2 {
        return false;
    }
    let Some(r0) = tuple_type_at(pass, results, 0) else {
        return false;
    };
    if !is_pointer_to_named(pass, r0, "net/http", "Response") {
        return false;
    }
    let Some(r1) = tuple_type_at(pass, results, 1) else {
        return false;
    };
    if !is_universe_error(pass, r1) {
        return false;
    }

    // The receiver: either the `http` package itself, or an http.Client.
    match expr_type(pass, &fun.x) {
        // No recorded type means `fun.X` is not a value — a package name. The
        // test upstream makes there is on the *identifier*, so a `net/http`
        // imported under another name does not qualify and some other package
        // imported as `http` does.
        None => matches!(fun.x.as_ref(), Expr::Ident(id) if id.name == "http"),
        Some(recv) => {
            is_type_named(pass, recv, "net/http", "Client")
                || is_pointer_to_named(pass, recv, "net/http", "Client")
        }
    }
}

fn same_ident(pass: &Pass<'_>, a: &guff::ast::Ident, b: &guff::ast::Ident) -> bool {
    let Some(info) = pass.types_info() else {
        return a.name == b.name;
    };
    let oa = info
        .defs
        .get(&a.id)
        .and_then(|o| *o)
        .or_else(|| info.uses.get(&a.id).copied());
    let ob = info
        .defs
        .get(&b.id)
        .and_then(|o| *o)
        .or_else(|| info.uses.get(&b.id).copied());
    oa == ob
}

/// Port of upstream's `restOfBlock`: the suffix of the innermost enclosing
/// block's statement list starting with the statement that contains `call`,
/// plus the number of calls crossed on the way up.
///
/// `stack` is the enclosing chain excluding `call` itself, so the walk starts
/// one past its end. Reaching a `BlockStmt` whose list holds no ancestor of the
/// call — which is what a `case` or `select` clause body looks like from here,
/// since those are bare `[]Stmt` — ends the search with nothing, and upstream
/// therefore never reports inside one.
fn rest_of_block<'a>(
    stack: &[NodeRef<'a>],
    call: NodeRef<'a>,
) -> Option<(&'a [Stmt], usize)> {
    let n = stack.len();
    let at = |i: usize| -> NodeRef<'a> {
        if i == n {
            call
        } else {
            stack[i]
        }
    };
    let mut ncalls = 0usize;
    for i in (0..=n).rev() {
        match at(i) {
            NodeRef::BlockStmt(b) => {
                let want = at(i + 1).erased_ptr();
                for (j, v) in b.list.iter().enumerate() {
                    if stmt_ref(v).erased_ptr() == want {
                        return Some((&b.list[j..], ncalls));
                    }
                }
                return None;
            }
            NodeRef::CallExpr(_) => ncalls += 1,
            _ => {}
        }
    }
    None
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if !imports_package(pass, "net/http") {
        return Ok(None);
    }
    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        let mut stack: Vec<NodeRef<'_>> = Vec::new();
        preorder_stack(NodeRef::File(file), &mut stack, |node, stack| {
            let NodeRef::CallExpr(call) = node else {
                return true;
            };
            if !is_http_func_or_method_on_client(pass, call) {
                return true;
            }
            let Some((stmts, ncalls)) = rest_of_block(stack, node) else {
                return true;
            };
            // The call is the last statement of its block, or it is wrapped by
            // another call (`resp, err := checkError(http.Get(url))`).
            if stmts.len() < 2 || ncalls > 1 {
                return true;
            }
            let Stmt::AssignStmt(asg) = &stmts[0] else {
                return true;
            };
            let Some(resp) = asg.lhs.first().and_then(root_ident) else {
                return true;
            };
            let Stmt::DeferStmt(def) = &stmts[1] else {
                return true;
            };
            let Some(root) = root_ident(&def.call.fun) else {
                return true;
            };
            if same_ident(pass, resp, root) {
                pending.push((
                    root.pos().0 as u32,
                    format!("using {} before checking for errors", resp.name),
                ));
            }
            true
        });
    }
    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "httpresponse",
        doc: "check for mistakes using HTTP responses",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/httpresponse",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    })
}
