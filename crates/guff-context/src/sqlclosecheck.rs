//! Port of [`github.com/ryanrolds/sqlclosecheck`](https://github.com/ryanrolds/sqlclosecheck)
//! (golangci-lint uses the `defer-only` analyzer).
//!
//! Checks that `sql.Rows` / `sql.Stmt` / `sqlx.NamedStmt` / pgx Rows are closed,
//! and that `Close` uses `defer`.
//!
//! Upstream uses `buildssa`. This port is an **AST / intra-procedural
//! approximation**: track named target assignments in a function body and
//! require a subsequent `.Close()` (including in deferred no-arg closures).
//! Non-deferred `Close` reports `"Close should use defer"`. Functions that
//! return a target type are skipped. Passing the value as a call argument
//! counts as handled (upstream `actionPassed` when last use).
//!
//! Built-in packages: `database/sql`, `github.com/jmoiron/sqlx`,
//! `github.com/jackc/pgx/v5`, `github.com/jackc/pgx/v5/pgxpool`.
//!
//! A target stored into a **struct field** is settled: upstream's
//! `*ssa.Store` arm answers `actionReturned` for an `*ssa.FieldAddr`
//! destination, with the comment "A Row/Stmt is stored in a struct, which may
//! be closed later by a different flow". A slice element, a map entry, a
//! pointer indirection and another local are all *not* `FieldAddr`, and stay
//! findings.
//!
//! DEFERRED: full SSA referrer / Phi / closure-capture /
//! MakeInterface parity.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{AssignStmt, BlockStmt, CallExpr, Expr, FieldList, FuncType, ValueSpec};
use guff::walk::{inspect, preorder, NodeRef};
use guff_analysis::passes::inspect as inspect_pass;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::arena::TypeData;
use guff_types::named::named_obj;
use guff_types::pointer::pointer_elem;
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::SelectionKind;
use guff_types::TypeId;

const SQL_PACKAGES: &[&str] = &[
    "database/sql",
    "github.com/jmoiron/sqlx",
    "github.com/jackc/pgx/v5",
    "github.com/jackc/pgx/v5/pgxpool",
];

const TARGET_TYPE_NAMES: &[&str] = &["Rows", "Stmt", "NamedStmt"];
const CLOSE_METHOD: &str = "Close";
const MSG_NOT_CLOSED: &str = "Rows/Stmt/NamedStmt was not closed";
const MSG_USE_DEFER: &str = "Close should use defer";

fn cut_vendor(path: &str) -> &str {
    if let Some(idx) = path.rfind("/vendor/") {
        &path[idx + "/vendor/".len()..]
    } else if let Some(rest) = path.strip_prefix("vendor/") {
        rest
    } else {
        path
    }
}

/// Whether the package being analysed lists a SQL package among its **own**
/// imports.
///
/// `getTargetTypes` asks the SSA program for each package by name:
///
/// ```go
/// pkg := pssa.Pkg.Prog.ImportedPackage(sqlPkg)
/// if pkg == nil {
///     // the SQL package being checked isn't imported
///     continue
/// }
/// ```
///
/// and `buildssa` only creates SSA packages for `pass.Pkg.Imports()` — the
/// **direct** imports. With no target types the analyzer returns before it
/// looks at a single instruction, so a package that reaches `*sql.Rows` only
/// through a dependency is skipped whole. vitess's `test/client/client.go`
/// opens its database through `vitessdriver` and never names `database/sql`;
/// upstream is silent there and guff reported both of its unclosed `rows`.
///
/// The same gate as `bodyclose`'s `imports_net_http`, which arrived for the
/// same reason.
fn imports_sql_package(pass: &Pass<'_>) -> bool {
    pass.files().iter().any(|file| {
        file.imports.iter().any(|spec| {
            let path = spec.path.value.trim_matches('"');
            SQL_PACKAGES.contains(&cut_vendor(path))
        })
    })
}

fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn is_target_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let named_typ = match artifacts.types.get(typ) {
        TypeData::Pointer(_) => {
            let elem = pointer_elem(&artifacts.types, typ);
            unalias_readonly(&artifacts.types, elem)
        }
        _ => typ,
    };
    let TypeData::Named(_) = artifacts.types.get(named_typ) else {
        return false;
    };
    let obj = named_obj(&artifacts.types, named_typ);
    let name = obj.name(&artifacts.objects);
    if !TARGET_TYPE_NAMES.contains(&name) {
        return false;
    }
    let Some(pkg_id) = obj.pkg(&artifacts.objects) else {
        return false;
    };
    let path = cut_vendor(artifacts.packages.get(pkg_id).path());
    SQL_PACKAGES.contains(&path)
}

fn expr_is_target(pass: &Pass<'_>, expr: &Expr) -> bool {
    type_of(pass, expr).is_some_and(|t| is_target_type(pass, t))
}

/// Whether a call's result — a lone value or any element of its tuple — is one
/// of the target types.
fn call_result_is_target(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    if matches!(artifacts.types.get(typ), TypeData::Tuple(_)) {
        return (0..tuple_len(&artifacts.types, Some(typ))).any(|i| {
            tuple_at(&artifacts.types, typ, i)
                .typ(&artifacts.objects)
                .is_some_and(|t| is_target_type(pass, t))
        });
    }
    is_target_type(pass, typ)
}

fn rhs_result_is_target(pass: &Pass<'_>, assign: &AssignStmt, lhs_index: usize) -> bool {
    if assign.rhs.len() == 1 {
        let Some(typ) = type_of(pass, &assign.rhs[0]) else {
            return false;
        };
        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
            return false;
        };
        let typ = unalias_readonly(&artifacts.types, typ);
        if matches!(artifacts.types.get(typ), TypeData::Tuple(_)) {
            if lhs_index >= tuple_len(&artifacts.types, Some(typ)) {
                return false;
            }
            let elem = tuple_at(&artifacts.types, typ, lhs_index);
            let Some(elem_typ) = elem.typ(&artifacts.objects) else {
                return false;
            };
            return is_target_type(pass, elem_typ);
        }
        return lhs_index == 0 && is_target_type(pass, typ);
    }
    assign
        .rhs
        .get(lhs_index)
        .is_some_and(|e| expr_is_target(pass, e))
}

/// The right-hand side feeding `lhs_index`: the single multi-value call, or
/// the expression in the same position.
fn rhs_for_index(assign: &AssignStmt, lhs_index: usize) -> Option<&Expr> {
    if assign.rhs.len() == 1 {
        assign.rhs.first()
    } else {
        assign.rhs.get(lhs_index)
    }
}

/// A call whose result upstream would start tracking — not a conversion and
/// not `make`/`new`, which `getTargetTypesValues` never sees as an `ssa.Call`
/// producing a target.
fn is_tracking_call(expr: &Expr) -> bool {
    let Expr::CallExpr(call) = expr else {
        return false;
    };
    !matches!(call.fun.as_ref(), Expr::Ident(id) if id.name == "make" || id.name == "new")
}

/// `x.f` where `f` is a struct field — upstream's `*ssa.FieldAddr` store
/// destination. A package-qualified name (`pkg.Var`) is an `*ssa.Global` and
/// has no `Selections` entry, so it is not one.
fn is_struct_field(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Expr::SelectorExpr(sel) = expr else {
        return false;
    };
    let Some(info) = pass.types_info() else {
        return false;
    };
    info.selections
        .get(&sel.id)
        .is_some_and(|s| s.kind() == SelectionKind::FieldVal)
}

/// A **struct** composite literal hands whatever it is given to whoever owns
/// the struct, exactly as `x.f = rows` does:
///
/// ```go
/// case *ssa.Store:
///     // A Row/Stmt is stored in a struct, which may be closed later
///     // by a different flow.
///     if _, ok := instr.Addr.(*ssa.FieldAddr); ok {
///         return actionReturned
///     }
/// ```
///
/// `&T{rows: rows}`, `T{rows: rows}` and the positional `T{rows}` all build the
/// field through a `FieldAddr`. A slice or map literal does not, and stays a
/// finding — measured on all five.
fn composite_lit_is_struct(pass: &Pass<'_>, lit_id: u32) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(typ) = info.types.get(&lit_id).map(|tv| tv.typ) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let under = typ.underlying(&artifacts.types);
    matches!(artifacts.types.get(under), TypeData::Struct(_))
}

fn ident_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn type_expr_looks_like_target(expr: &Expr) -> bool {
    match expr {
        Expr::StarExpr(s) => type_expr_looks_like_target(&s.x),
        Expr::ParenExpr(p) => type_expr_looks_like_target(&p.x),
        Expr::SelectorExpr(sel) => TARGET_TYPE_NAMES.contains(&sel.sel.name.as_str()),
        Expr::Ident(id) => TARGET_TYPE_NAMES.contains(&id.name.as_str()),
        _ => false,
    }
}

fn field_list_has_target(fields: Option<&FieldList>) -> bool {
    let Some(fl) = fields else {
        return false;
    };
    for field in &fl.list {
        let Some(ty) = &field.ty else {
            continue;
        };
        if type_expr_looks_like_target(ty) {
            return true;
        }
    }
    false
}

fn func_returns_target(ty: &FuncType) -> bool {
    field_list_has_target(ty.results.as_ref())
}

struct SqlUsage {
    pos: u32,
    /// Further assignments to the same name that no assignment *after* them
    /// can reach — the arms of an `if`/`switch`. Upstream sees one φ and one
    /// close settling every edge into it; here they are extra positions the
    /// same close settles, and the same absence reports.
    also: Vec<u32>,
    /// The statement list the assignment sits in, so a *sequential* reassign
    /// (same list) can still orphan what came before it.
    block: u32,
    closed: bool,
    deferred: bool,
    passed: bool,
    close_pos: Option<u32>,
}

impl SqlUsage {
    fn report(self, pending: &mut Vec<(u32, String)>) {
        if self.passed {
            return;
        }
        if self.closed {
            if !self.deferred {
                let pos = self.close_pos.unwrap_or(self.pos);
                pending.push((pos, MSG_USE_DEFER.to_string()));
            }
            return;
        }
        pending.push((self.pos, MSG_NOT_CLOSED.to_string()));
        for pos in self.also {
            pending.push((pos, MSG_NOT_CLOSED.to_string()));
        }
    }
}

    // Upstream reports an **SSA instruction's** position, not an AST node's:
    // `pass.Reportf(instr.Pos(), …)`. go/ssa sets a call's position to the
    // *left parenthesis* — `c.pos = e.Lparen` in `builder.go`, returned by
    // `(*Call).Pos()` — so `db.Query("…")` is reported at the `(`, eight
    // columns right of where the expression starts. Reconstructing the
    // instruction from the AST means reconstructing that convention too.
fn assign_report_pos(assign: &AssignStmt, lhs_index: usize) -> u32 {
    if assign.rhs.len() == 1 {
        if let Expr::CallExpr(call) = &assign.rhs[0] {
            return call.lparen.0 as u32;
        }
        return assign.rhs[0].pos().0 as u32;
    }
    if let Some(rhs) = assign.rhs.get(lhs_index) {
        if let Expr::CallExpr(call) = rhs {
            return call.lparen.0 as u32;
        }
        return rhs.pos().0 as u32;
    }
    assign
        .lhs
        .get(lhs_index)
        .map(|e| e.pos().0 as u32)
        .unwrap_or(assign.tok_pos.0 as u32)
}

/// `x.Close()` → Some("x")
fn close_var(call: &CallExpr) -> Option<&str> {
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return None;
    };
    if sel.sel.name != CLOSE_METHOD {
        return None;
    }
    ident_name(&sel.x)
}

fn mark_close(call: &CallExpr, deferred: bool, usages: &mut HashMap<String, SqlUsage>) {
    if let Some(var) = close_var(call) {
        if let Some(u) = usages.get_mut(var) {
            u.closed = true;
            if deferred {
                u.deferred = true;
            }
            // `pass.Reportf(instr.Pos(), "Close should use defer")` — the same
            // go/ssa convention as `assign_report_pos`: a call's position is
            // its `(`, not where the expression starts.
            u.close_pos = Some(call.lparen.0 as u32);
        }
    }
}

fn mark_passed_args(call: &CallExpr, usages: &mut HashMap<String, SqlUsage>) {
    // Skip Close itself — handled by mark_close.
    if close_var(call).is_some() {
        return;
    }
    for arg in &call.args {
        if let Some(name) = ident_name(arg) {
            if let Some(u) = usages.get_mut(name) {
                u.passed = true;
            }
        }
    }
}

fn handle_defer_close(call: &CallExpr, usages: &mut HashMap<String, SqlUsage>) {
    match call.fun.as_ref() {
        Expr::SelectorExpr(_) => mark_close(call, true, usages),
        Expr::FuncLit(fun) => {
            if fun.ty.params.as_ref().is_some_and(|p| !p.list.is_empty()) {
                return;
            }
            inspect(NodeRef::BlockStmt(&fun.body), |n| {
                let Some(n) = n else {
                    return true;
                };
                if matches!(n, NodeRef::FuncLit(_)) {
                    return false;
                }
                if let NodeRef::CallExpr(c) = n {
                    mark_close(c, true, usages);
                }
                true
            });
        }
        _ => {}
    }
}

/// The statement list each assignment sits directly in, keyed by the
/// assignment's node id and valued by the block's `{` position.
///
/// `0` means "not a direct statement of any block" (an `if` initialiser, say),
/// which keeps the old sequential reading for shapes this cannot place.
fn assign_block_ids(body: &BlockStmt) -> HashMap<u32, u32> {
    let mut out = HashMap::new();
    preorder(NodeRef::BlockStmt(body), |n| {
        if let NodeRef::BlockStmt(b) = n {
            for stmt in &b.list {
                if let guff::ast::Stmt::AssignStmt(a) = stmt {
                    out.insert(a.tok_pos.0 as u32, b.lbrace.0 as u32);
                }
            }
        }
        true
    });
    out
}

/// Marks every tracked target the literal closes, at any depth.
///
/// The close counts as deferred: upstream's `defer-only` analyzer asks whether
/// the `Close` is reached through a `*ssa.Defer`, and inside the literal it is.
fn mark_closed_in_closure(lit: &guff::ast::FuncLit, usages: &mut HashMap<String, SqlUsage>) {
    if usages.is_empty() {
        return;
    }
    let mut closed: Vec<String> = Vec::new();
    preorder(NodeRef::FuncLit(lit), |n| {
        if let NodeRef::CallExpr(c) = n {
            if let Some(name) = close_var(c) {
                closed.push(name.to_string());
            }
        }
        true
    });
    for name in closed {
        if let Some(u) = usages.get_mut(&name) {
            u.closed = true;
            u.deferred = true;
        }
    }
}

/// Whether the function's **own** body defers anything (a literal nested inside
/// it does not count).
///
/// With a `defer` in the function, go/ssa stops handing the call's tuple
/// straight to `return` and materialises the results, so the value's referrer
/// is a store rather than an `*ssa.Return` and `getAction` never reaches
///
/// ```go
/// case *ssa.Return:
///     … return actionReturned
/// ```
///
/// Measured as the rule, not inferred: `return db.Query(…)` is silent, and the
/// same function with a `defer fmt.Println("x")` anywhere in it — including
/// inside an `if` — is a finding, while a `defer` that only appears in a nested
/// literal leaves it silent again. vitess's `VTGateProxy.ShowTablets` hands the
/// rows to its caller and carries `defer span.Finish()`, which is why it needs
/// the `//nolint:sqlclosecheck` that guff called unused.
fn body_has_own_defer(body: &BlockStmt) -> bool {
    let mut found = false;
    preorder(NodeRef::BlockStmt(body), |n| {
        if found {
            return false;
        }
        match n {
            NodeRef::FuncLit(_) => return false,
            NodeRef::DeferStmt(_) => {
                found = true;
                return false;
            }
            _ => {}
        }
        true
    });
    found
}

fn check_body(pass: &Pass<'_>, body: &BlockStmt, pending: &mut Vec<(u32, String)>) {
    let has_defer = body_has_own_defer(body);
    let mut usages: HashMap<String, SqlUsage> = HashMap::new();
    let assign_blocks = assign_block_ids(body);

    inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        if let NodeRef::FuncLit(lit) = n {
            // A target closed inside a func literal is closed, wherever that
            // literal goes. Only `defer func(){ … }()` at the site was
            // recognised, so syncthing's `PrefixKV` — which hands back
            // `func(yield …) { defer rows.Close(); … }` — read as a leak, and
            // twice over, once for each branch of the `if` that assigns `rows`.
            //
            // A capture on its own settles nothing here: a literal that only
            // ranges over the rows is still a finding, which is the difference
            // from bodyclose's `MakeClosure` branch.
            mark_closed_in_closure(lit, &mut usages);
            return false;
        }

        match n {
            NodeRef::AssignStmt(assign) => {
                let block = assign_blocks.get(&(assign.tok_pos.0 as u32)).copied().unwrap_or(0);
                handle_assign(pass, assign, block, &mut usages, pending);
            }
            NodeRef::ValueSpec(spec) => {
                handle_value_spec(pass, spec, &mut usages, pending);
            }
            NodeRef::CallExpr(call) => {
                mark_close(call, false, &mut usages);
                mark_passed_args(call, &mut usages);
            }
            NodeRef::CompositeLit(lit) => {
                if composite_lit_is_struct(pass, lit.id) {
                    for elt in &lit.elts {
                        let value = match elt {
                            Expr::KeyValueExpr(kv) => kv.value.as_ref(),
                            other => other,
                        };
                        if let Some(name) = ident_name(value) {
                            if let Some(u) = usages.get_mut(name) {
                                u.passed = true;
                            }
                        }
                    }
                }
            }
            NodeRef::DeferStmt(d) => {
                handle_defer_close(&d.call, &mut usages);
            }
            NodeRef::ReturnStmt(ret) => {
                if has_defer {
                    // Nothing is transferred: with the results in memory the
                    // value's referrer is a store, and a call written straight
                    // into the `return` is a finding of its own — nothing
                    // names it, so it is reported here rather than tracked.
                    for result in &ret.results {
                        if let Expr::CallExpr(call) = result {
                            if is_tracking_call(result) && call_result_is_target(pass, result) {
                                pending.push((call.lparen.0 as u32, MSG_NOT_CLOSED.to_string()));
                            }
                        }
                    }
                    return true;
                }
                // Returning a tracked value clears it (ownership transferred).
                for result in &ret.results {
                    if let Some(name) = ident_name(result) {
                        usages.remove(name);
                    }
                }
            }
            _ => {}
        }
        true
    });

    for (_, u) in usages {
        u.report(pending);
    }
}

fn handle_assign(
    pass: &Pass<'_>,
    assign: &AssignStmt,
    block: u32,
    usages: &mut HashMap<String, SqlUsage>,
    pending: &mut Vec<(u32, String)>,
) {
    for (i, lhs) in assign.lhs.iter().enumerate() {
        let Some(name) = ident_name(lhs) else {
            // Not a plain name. Storing into a **struct field** hands the
            // value to whoever owns the struct:
            //
            //     case *ssa.Store:
            //         // A Row/Stmt is stored in a struct, which may be closed
            //         // later by a different flow.
            //         if _, ok := instr.Addr.(*ssa.FieldAddr); ok {
            //             return actionReturned
            //         }
            //
            // telegraf's `plugins/inputs/sql/sql.go:327` keeps its prepared
            // statements in `s.Queries[i].statement` and closes them from
            // `Stop`. A slice element, a map entry and a pointer
            // indirection are not `FieldAddr` and are still findings.
            if is_struct_field(pass, lhs) {
                if let Some(name) = rhs_for_index(assign, i).and_then(ident_name) {
                    if let Some(u) = usages.get_mut(name) {
                        u.passed = true;
                    }
                }
            }
            continue;
        };
        if name == "_" {
            // `_, err := db.Query(…)`. The extract still exists — it is the
            // call's referrer that `getTargetTypesValues` picks up — and
            // nothing refers to *it*, so `checkClosed` walks an empty list and
            // reports. Nothing can close it later either, there being no name,
            // so the finding is emitted here rather than tracked.
            //
            // A bare `db.Query(…)` statement is the opposite case and must stay
            // silent: no destination means no extract, so the call has no
            // referrer of a target type and upstream never starts.
            if rhs_for_index(assign, i).is_some_and(is_tracking_call)
                && rhs_result_is_target(pass, assign, i)
            {
                pending.push((assign_report_pos(assign, i), MSG_NOT_CLOSED.to_string()));
            }
            continue;
        }

        // `getTargetTypesValues` starts from an `*ssa.Call` and nothing else:
        //
        //     instr := b.Instrs[i]
        //     call, ok := instr.(*ssa.Call)
        //     if !ok { return targetValues }
        //
        // so `y = stmt` never becomes a value of its own — the rows it names
        // are the *call's*, already tracked. guff asked only what the type
        // was, and reported the plain copy a second time.
        let from_call = rhs_for_index(assign, i).is_some_and(is_tracking_call);
        let is_tgt =
            from_call && (expr_is_target(pass, lhs) || rhs_result_is_target(pass, assign, i));
        let pos = assign_report_pos(assign, i);

        // Two assignments in the *same* statement list run one after the other,
        // so the first really does lose its rows — upstream reports it even
        // when a close follows the second. Two in different lists are the arms
        // of a branch: upstream sees one φ, and one close settles every edge
        // into it. Orphaning the first there reported syncthing's `PrefixKV`
        // twice, once per arm of the `if` that assigns `rows`.
        if let Some(prev) = usages.get_mut(name) {
            if is_tgt && prev.block != block && !prev.closed && !prev.passed {
                prev.also.push(pos);
                continue;
            }
            if let Some(prev) = usages.remove(name) {
                prev.report(pending);
            }
        }

        if is_tgt {
            usages.insert(
                name.to_string(),
                SqlUsage {
                    pos,
                    also: Vec::new(),
                    block,
                    closed: false,
                    deferred: false,
                    passed: false,
                    close_pos: None,
                },
            );
        }
    }
}

fn handle_value_spec(
    pass: &Pass<'_>,
    spec: &ValueSpec,
    usages: &mut HashMap<String, SqlUsage>,
    pending: &mut Vec<(u32, String)>,
) {
    if spec.values.is_empty() {
        return;
    }
    for (i, name_id) in spec.names.iter().enumerate() {
        let name = name_id.name.as_str();
        if name == "_" {
            continue;
        }

        let is_tgt = if spec.values.len() == 1 {
            let Some(typ) = type_of(pass, &spec.values[0]) else {
                continue;
            };
            let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                continue;
            };
            let typ = unalias_readonly(&artifacts.types, typ);
            if matches!(artifacts.types.get(typ), TypeData::Tuple(_)) {
                if i >= tuple_len(&artifacts.types, Some(typ)) {
                    continue;
                }
                let elem = tuple_at(&artifacts.types, typ, i);
                let Some(elem_typ) = elem.typ(&artifacts.objects) else {
                    continue;
                };
                is_target_type(pass, elem_typ)
            } else {
                i == 0 && is_target_type(pass, typ)
            }
        } else {
            spec.values
                .get(i)
                .is_some_and(|e| expr_is_target(pass, e))
        };

        if let Some(prev) = usages.remove(name) {
            prev.report(pending);
        }

        if is_tgt {
            let pos = if spec.values.len() == 1 {
                if let Expr::CallExpr(call) = &spec.values[0] {
                    call.pos().0 as u32
                } else {
                    spec.values[0].pos().0 as u32
                }
            } else {
                spec.values
                    .get(i)
                    .map(|e| e.pos().0 as u32)
                    .unwrap_or(name_id.pos().0 as u32)
            };
            usages.insert(
                name.to_string(),
                SqlUsage {
                    pos,
                    also: Vec::new(),
                    block: 0,
                    closed: false,
                    deferred: false,
                    passed: false,
                    close_pos: None,
                },
            );
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect_pass::InspectResult>(inspect_pass::analyzer())
        .ok_or_else(|| "sqlclosecheck requires inspect analyzer".to_string())?;

    if !imports_sql_package(pass) {
        return Ok(None);
    }

    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        // Rooted at each `FuncDecl`, not at the file: `buildssa` builds
        // `SrcFuncs` from those alone, so a literal in a package-level `var`
        // initializer — every Ginkgo suite — is invisible upstream.
        // See [`code::src_func_decls`].
        for top in guff_analysis::code::src_func_decls(file) {
            preorder(NodeRef::FuncDecl(top), |n| {
            match n {
                NodeRef::FuncDecl(fd) => {
                    if let Some(body) = &fd.body {
                        // The "it returns rows, someone else closes them" skip
                        // only holds while `getAction` can reach its
                        // `*ssa.Return` arm; a `defer` in the body puts the
                        // results in memory and it cannot.
                        if func_returns_target(&fd.ty) && !body_has_own_defer(body) {
                            return true;
                        }
                        check_body(pass, body, &mut pending);
                    }
                }
                NodeRef::FuncLit(fl) => {
                    if func_returns_target(&fl.ty) && !body_has_own_defer(&fl.body) {
                        return true;
                    }
                    check_body(pass, &fl.body, &mut pending);
                }
                _ => {}
            }
            true
            });
        }
    }

    for (pos, msg) in pending {
        pass.reportf(pos, &msg);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "sqlclosecheck",
        doc: "Checks that sql.Rows, sql.Stmt, sqlx.NamedStmt, pgx.Query are closed.",
        url: "https://github.com/ryanrolds/sqlclosecheck",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect_pass::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyzer_metadata() {
        let a = analyzer();
        assert_eq!(a.name, "sqlclosecheck");
        assert!(!a.doc.is_empty());
    }
}
