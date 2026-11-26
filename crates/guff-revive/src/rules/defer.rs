//! `defer` — warn on common defer gotchas.
//!
//! Six independent sub-checks, and the rule's **argument list picks which of
//! them run**: `arguments: [["immediate-recover", "recover", "return"]]` turns
//! the other three off. With no arguments all six are on
//! (`DeferRule.allowFromArgs`). tailscale configures exactly those three, and
//! running the full set anyway put 55 findings on that target — 42 "prefer not
//! to defer inside loops" and 13 "prefer not to defer chains of function
//! calls" — none of which golangci-lint reports.

use std::collections::HashSet;

use guff::ast::{CallExpr, Expr};
use guff::scope::ObjKind;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::config::{normalize_rule_option, rule_arguments};
use crate::failure::Failure;
use crate::settings::RuleArgument;
use crate::util::is_ident;

/// The sub-cases, spelled as upstream normalizes them (lowercase, `-` removed).
const SUBCASES: [&str; 6] = [
    "loop",
    "callchain",
    "methodcall",
    "return",
    "recover",
    "immediaterecover",
];

/// Which sub-cases this configuration allows.
///
/// `allowFromArgs`: no arguments means every sub-case; otherwise the first
/// argument is a list of names, and only those are on. A name that matches
/// nothing simply turns nothing on — upstream does not validate the spelling
/// beyond requiring a string.
pub fn allow_from_args(pass: &Pass<'_>) -> HashSet<String> {
    allow_from_arg_list(&rule_arguments(pass, "defer"))
}

/// [`allow_from_args`] over an argument list, so the parse-time gate that
/// decides whether `ast::Ident.obj` is needed can ask the same question
/// without a `Pass` (see `guff_lint::revive_needs_ast_object_resolution`).
pub fn allow_from_arg_list(args: &[RuleArgument]) -> HashSet<String> {
    let Some(first) = args.first() else {
        return SUBCASES.iter().map(|s| s.to_string()).collect();
    };
    let RuleArgument::List(items) = first else {
        // Upstream errors out here ("Expecting []string"); guff has no channel
        // for a configuration error from a rule, and treating it as "no
        // arguments" would silently enable everything. Enable nothing instead,
        // so a malformed list cannot masquerade as the default.
        return HashSet::new();
    };
    items
        .iter()
        .filter_map(|a| match a {
            RuleArgument::String(s) => Some(normalize_rule_option(s)),
            _ => None,
        })
        .collect()
}

#[derive(Clone, Copy)]
struct State {
    in_defer: bool,
    in_loop: bool,
    /// 0 = not in a func literal, 1 = in the top-level one, >1 = nested.
    in_func_lit: u8,
}

struct Ctx {
    allow: HashSet<String>,
    failures: Vec<Failure>,
}

impl Ctx {
    fn report(&mut self, msg: &str, pos: u32, subcase: &str) {
        if !self.allow.contains(subcase) {
            return;
        }
        self.failures.push(Failure {
            rule: "defer",
            pos,
            message: msg.into(),
            ..Failure::default()
        });
    }
}

/// The shared-walk entry point: the rule walks each file itself, because its
/// visitor carries state (in a defer / in a loop / how deep in a func literal)
/// that a single flat pre-order walk cannot express.
pub struct Checker {
    ctx: Ctx,
}

impl Checker {
    pub fn new(pass: &Pass<'_>) -> Self {
        Self {
            ctx: Ctx {
                allow: allow_from_args(pass),
                failures: Vec::new(),
            },
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::File(_) = n else {
            return;
        };
        walk(
            n,
            State {
                in_defer: false,
                in_loop: false,
                in_func_lit: 0,
            },
            &mut self.ctx,
        );
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.ctx.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new(pass);
    for file in pass.files() {
        c.visit(NodeRef::File(file));
    }
    c.into_failures()
}

/// `lintDeferRule.Visit`, as a walk that carries the visitor's state instead of
/// rebuilding it: the node kinds below either descend with changed state or
/// stop, and everything else descends unchanged.
fn walk(node: NodeRef<'_>, st: State, ctx: &mut Ctx) {
    match node {
        NodeRef::ForStmt(f) => {
            walk_subtree(NodeRef::BlockStmt(&f.body), st.in_defer, true, st.in_func_lit, ctx);
            return;
        }
        NodeRef::RangeStmt(r) => {
            walk_subtree(NodeRef::BlockStmt(&r.body), st.in_defer, true, st.in_func_lit, ctx);
            return;
        }
        NodeRef::FuncLit(l) => {
            walk_subtree(
                NodeRef::BlockStmt(&l.body),
                st.in_defer,
                false,
                st.in_func_lit.saturating_add(1),
                ctx,
            );
            return;
        }
        NodeRef::ReturnStmt(r) => {
            if !r.results.is_empty() && st.in_defer && st.in_func_lit == 1 {
                ctx.report(
                    "return in a defer function has no effect",
                    r.return_.0 as u32,
                    "return",
                );
            }
        }
        NodeRef::CallExpr(c) => {
            let is_recover = is_ident(&c.fun, "recover");
            if is_recover && !st.in_defer {
                // `func fn() { recover() }` — including the assignment form
                // `_ = recover()`, which a walk that only looks at expression
                // statements never reaches.
                ctx.report(
                    "recover must be called inside a deferred function",
                    c.fun.pos().0 as u32,
                    "recover",
                );
            } else if is_recover && st.in_defer && st.in_func_lit == 0 {
                // `defer helper(recover())`
                ctx.report(
                    "recover must be called inside a deferred function, this is executing recover immediately",
                    c.fun.pos().0 as u32,
                    "immediaterecover",
                );
            }
            // Upstream returns nil here: the arguments of a call are not
            // analysed, so `f(recover())` outside a defer is not a finding.
            return;
        }
        NodeRef::DeferStmt(d) => {
            visit_defer(d, st, ctx);
            return;
        }
        _ => {}
    }
    walk::for_each_child(node, |c| walk(c, st, ctx));
}

fn walk_subtree(node: NodeRef<'_>, in_defer: bool, in_loop: bool, in_func_lit: u8, ctx: &mut Ctx) {
    walk(
        node,
        State {
            in_defer,
            in_loop,
            in_func_lit,
        },
        ctx,
    );
}

fn visit_defer(d: &guff::ast::DeferStmt, st: State, ctx: &mut Ctx) {
    let call: &CallExpr = &d.call;
    if is_ident(&call.fun, "recover") {
        // `defer recover()`
        ctx.report(
            "recover must be called inside a deferred function, this is executing recover immediately",
            d.defer_.0 as u32,
            "immediaterecover",
        );
    }
    walk_subtree(walk::expr_ref(&call.fun), true, false, 0, ctx);
    for a in &call.args {
        // Too hard to analyse deferred calls with func literal arguments.
        if matches!(a, Expr::FuncLit(_)) {
            continue;
        }
        walk_subtree(walk::expr_ref(a), true, false, 0, ctx);
    }

    if st.in_loop {
        ctx.report("prefer not to defer inside loops", d.defer_.0 as u32, "loop");
    }

    // These two report on the *callee*, not on the `defer` keyword: upstream
    // passes `fn` as the failure node while the loop case passes the statement.
    match &*call.fun {
        Expr::CallExpr(inner) => ctx.report(
            "prefer not to defer chains of function calls",
            inner.pos().0 as u32,
            "callchain",
        ),
        Expr::SelectorExpr(sel) => {
            if let Expr::Ident(id) = &*sel.x {
                let is_method_call = id
                    .obj
                    .lock()
                    .ok()
                    .and_then(|o| o.as_ref().map(|o| o.kind == ObjKind::Typ))
                    .unwrap_or(false);
                if is_method_call {
                    ctx.report(
                        "be careful when deferring calls to methods without pointer receiver",
                        sel.x.pos().0 as u32,
                        "methodcall",
                    );
                }
            }
        }
        _ => {}
    }
}

