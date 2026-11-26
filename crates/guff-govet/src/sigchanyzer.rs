//! `sigchanyzer` — check misuse of unbuffered signal channels with signal.Notify.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, Expr, Ident, ValueSpec};
use guff::node_mask;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::expreq::unparen;
use crate::govet_util::{format_expr, imports_package, is_builtin_named};

fn is_signal_notify(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Expr::SelectorExpr(sel) = unparen(&call.fun) else {
        return false;
    };
    if sel.sel.name != "Notify" {
        return false;
    }
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(obj) = info.uses.get(&sel.sel.id).copied() else {
        return false;
    };
    code_pkg_path(&artifacts.objects, &artifacts.packages, obj).as_deref() == Some("os/signal")
}

fn code_pkg_path(
    objects: &guff_types::arena::ObjectArena,
    packages: &guff_types::arena::PackageArena,
    obj: guff_types::ObjectId,
) -> Option<String> {
    let pkg = obj.pkg(objects)?;
    Some(packages.get(pkg).path().to_string())
}

/// Upstream `findDecl`: the right-hand side that declares `id`.
///
/// Upstream reaches it through `ast.Object` identity (`arg.Obj.Decl`); guff's
/// equivalent is the type checker's object id, so the search matches on
/// `Info.Defs` instead. The previous version compared the *use* ident's node id
/// against the *declaration* ident's node id — two different nodes, so it never
/// matched — and its function-body branch sat inside a `let ... else continue`
/// for `GenDecl`, making it unreachable. Between them, the `Ident` arm of
/// sigchanyzer never fired at all.
fn find_decl_rhs<'a>(pass: &'a Pass<'_>, id: &Ident) -> Option<&'a Expr> {
    let info = pass.types_info()?;
    let target = info
        .uses
        .get(&id.id)
        .copied()
        .or_else(|| info.defs.get(&id.id).copied().flatten())?;

    let defines = |name: &Ident| info.defs.get(&name.id).copied().flatten() == Some(target);

    let mut found: Option<&Expr> = None;
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if found.is_some() {
                return false;
            }
            match n {
                Some(NodeRef::AssignStmt(AssignStmt { lhs, rhs, .. })) => {
                    if lhs.len() == rhs.len() {
                        for (l, r) in lhs.iter().zip(rhs) {
                            if let Expr::Ident(li) = unparen(l) {
                                if defines(li) {
                                    found = Some(r);
                                    return false;
                                }
                            }
                        }
                    }
                }
                Some(NodeRef::ValueSpec(ValueSpec { names, values, .. })) => {
                    if names.len() == values.len() {
                        for (name, v) in names.iter().zip(values) {
                            if defines(name) {
                                found = Some(v);
                                return false;
                            }
                        }
                    }
                }
                _ => {}
            }
            true
        });
        if found.is_some() {
            break;
        }
    }
    found
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if !imports_package(pass, "os/signal") {
        return Ok(None);
    }
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "sigchanyzer requires inspect analyzer".to_string())?
        .clone();
    // (report pos, report end, message, decl pos, decl end, replacement)
    let mut pending: Vec<(u32, u32, String, u32, u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if !is_signal_notify(pass, call) {
            return;
        }
        let chan_decl = match call.args.first() {
            Some(Expr::Ident(id)) => find_decl_rhs(pass, id).and_then(|e| match unparen(e) {
                Expr::CallExpr(c) => Some(c),
                _ => None,
            }),
            Some(Expr::CallExpr(c)) => {
                // Only `signal.Notify(make(chan os.Signal), os.Interrupt)` is
                // exempt: upstream deliberately does not report a channel
                // created inline by `make`, and conservatively treats every
                // other call as not safe (golang/go#45043). The condition was
                // inverted here, so the one exempt form was the only one
                // reported.
                if is_builtin_named(pass, &c.fun, "make") {
                    return;
                }
                Some(c)
            }
            _ => None,
        };
        let Some(chan_decl) = chan_decl else {
            return;
        };
        if chan_decl.args.len() != 1 {
            return;
        }
        // Upstream copies the `make(chan T)` call, appends a `1`, and prints
        // the copy — so the replacement is the *rendered* two-argument call,
        // not the original text with something spliced in. Rendering the two
        // parts here reaches the same bytes without cloning the AST, which
        // upstream only does to avoid mutating it (golang/go#46129).
        let buffered = format!(
            "{}({}, 1)",
            format_expr(pass, &chan_decl.fun),
            format_expr(pass, &chan_decl.args[0])
        );
        pending.push((
            call.pos().0 as u32,
            call.end().0 as u32,
            "misuse of unbuffered os.Signal channel as argument to signal.Notify".into(),
            chan_decl.pos().0 as u32,
            chan_decl.end().0 as u32,
            buffered,
        ));
    });
    for (pos, end, message, decl_pos, decl_end, buffered) in pending {
        pass.report(Diagnostic {
            pos,
            end,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Change to buffer channel".into(),
                text_edits: vec![TextEdit {
                    pos: decl_pos,
                    end: decl_end,
                    new_text: buffered,
                }],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "sigchanyzer",
        doc: "check for misuse of unbuffered os.Signal channels with signal.Notify",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/sigchanyzer",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
