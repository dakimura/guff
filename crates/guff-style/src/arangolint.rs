//! Port of [`go.augendre.info/arangolint`](https://github.com/Crocmagnon/arangolint)
//! (golangci-lint linter `arangolint`, available since v2.2.0).
//!
//! Opinionated best practices for the ArangoDB Go driver v2
//! (`github.com/arangodb/go-driver/v2/arangodb`). Two checks:
//!
//! 1. **Missing `AllowImplicit`** — a `db.BeginTransaction(ctx, cols, opts)`
//!    call whose `opts` (3rd argument) does not explicitly set the
//!    `AllowImplicit` field. This forces the developer to consider collection
//!    locking / deadlock risks.
//! 2. **Query concatenation** — an AQL string passed to
//!    `Query` / `QueryBatch` / `ValidateQuery` / `ExplainQuery` that is built
//!    via string concatenation (`+` with a non-literal operand) or
//!    `fmt.Sprintf`, which risks AQL injection. Use bind variables instead.
//!
//! The analysis is **intra-procedural** and flow/block sensitive: prior
//! statements in the nearest block and its ancestor blocks (plus package-level
//! var declarations) are considered when evaluating a call site. It stays
//! conservative when the options / query value comes from an unknown factory or
//! helper call, to avoid false positives.
//!
//! Receiver types are matched by their fully-qualified type-string suffix
//! (`…/arangodb.Database` / `…/arangodb.Transaction`), mirroring upstream's
//! last-resort check. DEFERRED (see DEVELOPMENT.md R13): the `types.Implements`
//! / `AssignableTo` path for wrapper/embedding receiver types, indexed-element
//! (`arr[i]`) option tracking, and SuggestedFix emission.

use std::sync::OnceLock;

use guff::ast::{BlockStmt, CallExpr, Expr, Ident, Spec, Stmt};
use guff::token::Token;
use guff::walk::{preorder_stack, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::ObjectId;

const ALLOW_IMPLICIT_FIELD: &str = "AllowImplicit";
const MSG_MISSING_ALLOW_IMPLICIT: &str = "missing AllowImplicit option";
const MSG_QUERY_CONCATENATION: &str = "query string uses concatenation instead of bind variables";

const METHOD_BEGIN_TRANSACTION: &str = "BeginTransaction";
const EXPECTED_BEGIN_TXN_ARGS: usize = 3;

const DATABASE_TYPE_SUFFIX: &str = "github.com/arangodb/go-driver/v2/arangodb.Database";
const TRANSACTION_TYPE_SUFFIX: &str = "github.com/arangodb/go-driver/v2/arangodb.Transaction";

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

fn obj_of(pass: &Pass<'_>, id: &Ident) -> Option<ObjectId> {
    let info = pass.types_info()?;
    info.uses
        .get(&id.id)
        .copied()
        .or_else(|| info.defs.get(&id.id).copied().flatten())
}

fn unwrap_parens(e: &Expr) -> &Expr {
    let mut cur = e;
    while let Expr::ParenExpr(p) = cur {
        cur = &p.x;
    }
    cur
}

fn is_nil_ident(e: &Expr) -> bool {
    matches!(e, Expr::Ident(id) if id.name == "nil")
}

/// Peels parens, stars, selectors, index and slice expressions down to the
/// underlying base identifier. Port of `rootIdent`.
fn root_ident(expr: &Expr) -> Option<&Ident> {
    let mut cur = expr;
    loop {
        match cur {
            Expr::Ident(id) => return Some(id),
            Expr::ParenExpr(p) => cur = &p.x,
            Expr::StarExpr(s) => cur = &s.x,
            Expr::SelectorExpr(s) => cur = &s.x,
            Expr::IndexExpr(i) => cur = &i.x,
            Expr::SliceExpr(s) => cur = &s.x,
            _ => return None,
        }
    }
}

fn is_allow_implicit_selector(e: &Expr) -> bool {
    matches!(e, Expr::SelectorExpr(s) if s.sel.name == ALLOW_IMPLICIT_FIELD)
}

/// Whether `expr` is a composite literal (or `&CompositeLit`) that sets the
/// `AllowImplicit` field. Returns `Some(has)` when a composite literal shape was
/// recognized, `None` otherwise. Port of `compositeAllowsImplicit`.
fn composite_allows_implicit(expr: &Expr) -> Option<bool> {
    let mut e = unwrap_parens(expr);
    if let Expr::UnaryExpr(u) = e {
        e = unwrap_parens(&u.x);
    }
    let Expr::CompositeLit(cl) = e else {
        return None;
    };
    for elt in &cl.elts {
        if let Expr::KeyValueExpr(kv) = elt {
            if let Expr::Ident(id) = kv.key.as_ref() {
                if id.name == ALLOW_IMPLICIT_FIELD {
                    return Some(true);
                }
            }
        }
    }
    Some(false)
}

/// Enclosing blocks from nearest to outermost. Port of `ancestorBlocks`.
fn ancestor_blocks<'a>(stack: &[NodeRef<'a>]) -> Vec<&'a BlockStmt> {
    let mut blocks = Vec::new();
    for node in stack.iter().rev() {
        if let NodeRef::BlockStmt(blk) = node {
            blocks.push(*blk);
        }
    }
    blocks
}

/// Visit statements that appear before `until` in the given blocks (nearest
/// first), stopping when `visit` returns true. Port of `scanPriorStatements`.
fn scan_prior<F>(blocks: &[&BlockStmt], until: i64, mut visit: F) -> bool
where
    F: FnMut(&Stmt) -> bool,
{
    for blk in blocks {
        for stmt in &blk.list {
            if stmt.pos().0 >= until {
                break;
            }
            if visit(stmt) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// AllowImplicit tracking
// ---------------------------------------------------------------------------

/// `X.AllowImplicit = …` where the root identifier of `X` denotes `obj`.
fn sets_allow_implicit_for_obj_in_assign(stmt: &Stmt, obj: ObjectId, pass: &Pass<'_>) -> bool {
    let Stmt::AssignStmt(assign) = stmt else {
        return false;
    };
    for lhs in &assign.lhs {
        if !is_allow_implicit_selector(lhs) {
            continue;
        }
        let Expr::SelectorExpr(sel) = lhs else {
            continue;
        };
        if let Some(r) = root_ident(&sel.x) {
            if obj_of(pass, r) == Some(obj) {
                return true;
            }
        }
    }
    false
}

/// `obj := &Foo{AllowImplicit: …}` / `obj = …composite…`.
fn init_has_allow_implicit_for_obj(
    assign: &guff::ast::AssignStmt,
    obj: ObjectId,
    pass: &Pass<'_>,
) -> bool {
    for (i, lhs) in assign.lhs.iter().enumerate() {
        let Expr::Ident(id) = lhs else {
            continue;
        };
        if obj_of(pass, id) != Some(obj) {
            continue;
        }
        let rhs = if assign.rhs.len() == assign.lhs.len() {
            assign.rhs.get(i)
        } else if assign.rhs.len() == 1 {
            assign.rhs.first()
        } else {
            None
        };
        if let Some(rhs) = rhs {
            if let Some(has) = composite_allows_implicit(rhs) {
                return has;
            }
        }
    }
    false
}

/// `var obj = &Foo{AllowImplicit: …}` inside a function body.
fn decl_init_has_allow_implicit_for_obj(stmt: &Stmt, obj: ObjectId, pass: &Pass<'_>) -> bool {
    let Stmt::DeclStmt(decl) = stmt else {
        return false;
    };
    let guff::ast::Decl::GenDecl(gen) = &decl.decl else {
        return false;
    };
    if gen.tok != Some(Token::VAR) {
        return false;
    }
    for spec in &gen.specs {
        if let Spec::ValueSpec(vs) = spec {
            if value_spec_has_allow_implicit_for_obj(vs, obj, pass) {
                return true;
            }
        }
    }
    false
}

fn value_spec_has_allow_implicit_for_obj(
    vs: &guff::ast::ValueSpec,
    obj: ObjectId,
    pass: &Pass<'_>,
) -> bool {
    let mut target = None;
    for (i, name) in vs.names.iter().enumerate() {
        if obj_of(pass, name) == Some(obj) {
            target = Some(i);
            break;
        }
    }
    let Some(target) = target else {
        return false;
    };
    let rhs = if target < vs.values.len() {
        vs.values.get(target)
    } else if vs.values.len() == 1 {
        vs.values.first()
    } else {
        None
    };
    rhs.and_then(composite_allows_implicit).unwrap_or(false)
}

/// Whether `stmt` sets `AllowImplicit` for `obj`, recursing into control flow.
/// Port of `stmtSetsAllowImplicitForObj`.
fn stmt_sets_allow_implicit_for_obj(stmt: &Stmt, obj: ObjectId, pass: &Pass<'_>) -> bool {
    if sets_allow_implicit_for_obj_in_assign(stmt, obj, pass) {
        return true;
    }
    if let Stmt::AssignStmt(assign) = stmt {
        if init_has_allow_implicit_for_obj(assign, obj, pass) {
            return true;
        }
    }
    if decl_init_has_allow_implicit_for_obj(stmt, obj, pass) {
        return true;
    }
    match stmt {
        Stmt::IfStmt(s) => {
            for st in &s.body.list {
                if stmt_sets_allow_implicit_for_obj(st, obj, pass) {
                    return true;
                }
            }
            match s.else_.as_deref() {
                Some(Stmt::BlockStmt(blk)) => {
                    for st in &blk.list {
                        if stmt_sets_allow_implicit_for_obj(st, obj, pass) {
                            return true;
                        }
                    }
                }
                Some(els @ Stmt::IfStmt(_)) => {
                    if stmt_sets_allow_implicit_for_obj(els, obj, pass) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        Stmt::ForStmt(s) => {
            if let Some(init) = s.init.as_deref() {
                if stmt_sets_allow_implicit_for_obj(init, obj, pass) {
                    return true;
                }
            }
            for st in &s.body.list {
                if stmt_sets_allow_implicit_for_obj(st, obj, pass) {
                    return true;
                }
            }
        }
        Stmt::RangeStmt(s) => {
            for st in &s.body.list {
                if stmt_sets_allow_implicit_for_obj(st, obj, pass) {
                    return true;
                }
            }
        }
        Stmt::SwitchStmt(s) => {
            if let Some(init) = s.init.as_deref() {
                if stmt_sets_allow_implicit_for_obj(init, obj, pass) {
                    return true;
                }
            }
            for cc in &s.body.list {
                if let Stmt::CaseClause(clause) = cc {
                    for st in &clause.body {
                        if stmt_sets_allow_implicit_for_obj(st, obj, pass) {
                            return true;
                        }
                    }
                }
            }
        }
        _ => {}
    }
    false
}

/// Package-level `var obj = &Foo{AllowImplicit: …}`.
fn has_allow_implicit_for_package_var(pass: &Pass<'_>, obj: ObjectId) -> bool {
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::GenDecl(gen) = decl else {
                continue;
            };
            if gen.tok != Some(Token::VAR) {
                continue;
            }
            for spec in &gen.specs {
                if let Spec::ValueSpec(vs) = spec {
                    if value_spec_has_allow_implicit_for_obj(vs, obj, pass) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn has_allow_implicit_for_ident(
    id: &Ident,
    pass: &Pass<'_>,
    blocks: &[&BlockStmt],
    call_pos: i64,
) -> bool {
    let Some(obj) = obj_of(pass, id) else {
        return false;
    };
    if scan_prior(blocks, call_pos, |stmt| {
        stmt_sets_allow_implicit_for_obj(stmt, obj, pass)
    }) {
        return true;
    }
    has_allow_implicit_for_package_var(pass, obj)
}

fn has_allow_implicit_for_selector(
    sel: &guff::ast::SelectorExpr,
    pass: &Pass<'_>,
    blocks: &[&BlockStmt],
    call_pos: i64,
) -> bool {
    let Some(root) = root_ident(&sel.x) else {
        return false;
    };
    let Some(obj) = obj_of(pass, root) else {
        return false;
    };
    scan_prior(blocks, call_pos, |stmt| {
        sets_allow_implicit_for_obj_in_assign(stmt, obj, pass)
    })
}

/// `(*arangodb.BeginTransactionOptions)(nil)`.
fn is_type_conversion_to_ptr_nil(call: &CallExpr, pass: &Pass<'_>) -> bool {
    if call.args.len() != 1 || !is_nil_ident(&call.args[0]) {
        return false;
    }
    // Syntactic check: the callee is `(*T)`.
    let mut fun = call.fun.as_ref();
    while let Expr::ParenExpr(p) = fun {
        fun = &p.x;
    }
    if matches!(fun, Expr::StarExpr(_)) {
        return true;
    }
    // Type-based fallback: callee has a pointer type.
    type_string(pass, &call.fun).is_some_and(|s| s.starts_with('*'))
}

fn should_report_missing_allow_implicit(
    arg: &Expr,
    pass: &Pass<'_>,
    blocks: &[&BlockStmt],
    call_pos: i64,
) -> bool {
    match arg {
        Expr::Ident(id) => {
            if id.name == "nil" {
                return true;
            }
            !has_allow_implicit_for_ident(id, pass, blocks, call_pos)
        }
        Expr::UnaryExpr(u) if u.op == Token::AND => {
            if let Some(has) = composite_allows_implicit(arg) {
                return !has;
            }
            if let Expr::Ident(id) = u.x.as_ref() {
                return !has_allow_implicit_for_ident(id, pass, blocks, call_pos);
            }
            // Unknown &shape: stay conservative.
            false
        }
        Expr::CompositeLit(_) => composite_allows_implicit(arg).map(|has| !has).unwrap_or(false),
        Expr::SelectorExpr(sel) => !has_allow_implicit_for_selector(sel, pass, blocks, call_pos),
        Expr::CallExpr(c) => is_type_conversion_to_ptr_nil(c, pass),
        // Unknown expression shapes: stay conservative and do not report.
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Query concatenation tracking
// ---------------------------------------------------------------------------

fn is_string_literal(expr: &Expr) -> bool {
    matches!(unwrap_parens(expr), Expr::BasicLit(l) if l.kind == Some(Token::STRING))
}

fn is_all_string_literals(expr: &Expr) -> bool {
    let expr = unwrap_parens(expr);
    if is_string_literal(expr) {
        return true;
    }
    if let Expr::BinaryExpr(b) = expr {
        if b.op == Token::ADD {
            return is_all_string_literals(&b.x) && is_all_string_literals(&b.y);
        }
    }
    false
}

/// `"…" + expr` where at least one operand is not a string literal.
fn is_concatenated_string(expr: &Expr) -> bool {
    let expr = unwrap_parens(expr);
    let Expr::BinaryExpr(b) = expr else {
        return false;
    };
    if b.op != Token::ADD {
        return false;
    }
    !(is_all_string_literals(&b.x) && is_all_string_literals(&b.y))
}

fn is_fmt_sprintf_call(expr: &Expr, pass: &Pass<'_>) -> bool {
    let expr = unwrap_parens(expr);
    let Expr::CallExpr(c) = expr else {
        return false;
    };
    code::call_name(pass, &c.fun).as_deref() == Some("fmt.Sprintf")
}

/// `q := "…" + var` / `q = fmt.Sprintf(…)` / `q += var` before the call site.
fn stmt_assigns_concatenation(stmt: &Stmt, obj: ObjectId, pass: &Pass<'_>) -> bool {
    match stmt {
        Stmt::AssignStmt(assign) => {
            for (i, lhs) in assign.lhs.iter().enumerate() {
                let Expr::Ident(id) = lhs else {
                    continue;
                };
                if obj_of(pass, id) != Some(obj) {
                    continue;
                }
                let rhs = if assign.rhs.len() == assign.lhs.len() {
                    assign.rhs.get(i)
                } else if assign.rhs.len() == 1 {
                    assign.rhs.first()
                } else {
                    None
                };
                let Some(rhs) = rhs else {
                    continue;
                };
                if is_concatenated_string(rhs) || is_fmt_sprintf_call(rhs, pass) {
                    return true;
                }
                if assign.tok == Some(Token::AddAssign) && !is_string_literal(rhs) {
                    return true;
                }
            }
            false
        }
        Stmt::DeclStmt(decl) => {
            let guff::ast::Decl::GenDecl(gen) = &decl.decl else {
                return false;
            };
            if gen.tok != Some(Token::VAR) {
                return false;
            }
            for spec in &gen.specs {
                if let Spec::ValueSpec(vs) = spec {
                    if var_decl_has_concatenation(vs, obj, pass) {
                        return true;
                    }
                }
            }
            false
        }
        Stmt::IfStmt(s) => {
            for st in &s.body.list {
                if stmt_assigns_concatenation(st, obj, pass) {
                    return true;
                }
            }
            match s.else_.as_deref() {
                Some(Stmt::BlockStmt(blk)) => blk
                    .list
                    .iter()
                    .any(|st| stmt_assigns_concatenation(st, obj, pass)),
                Some(els @ Stmt::IfStmt(_)) => stmt_assigns_concatenation(els, obj, pass),
                _ => false,
            }
        }
        Stmt::ForStmt(s) => {
            if let Some(init) = s.init.as_deref() {
                if stmt_assigns_concatenation(init, obj, pass) {
                    return true;
                }
            }
            s.body
                .list
                .iter()
                .any(|st| stmt_assigns_concatenation(st, obj, pass))
        }
        Stmt::RangeStmt(s) => s
            .body
            .list
            .iter()
            .any(|st| stmt_assigns_concatenation(st, obj, pass)),
        Stmt::SwitchStmt(s) => {
            if let Some(init) = s.init.as_deref() {
                if stmt_assigns_concatenation(init, obj, pass) {
                    return true;
                }
            }
            s.body.list.iter().any(|cc| {
                matches!(cc, Stmt::CaseClause(clause)
                    if clause.body.iter().any(|st| stmt_assigns_concatenation(st, obj, pass)))
            })
        }
        _ => false,
    }
}

fn var_decl_has_concatenation(
    vs: &guff::ast::ValueSpec,
    obj: ObjectId,
    pass: &Pass<'_>,
) -> bool {
    let mut target = None;
    for (i, name) in vs.names.iter().enumerate() {
        if obj_of(pass, name) == Some(obj) {
            target = Some(i);
            break;
        }
    }
    let Some(target) = target else {
        return false;
    };
    let rhs = if target < vs.values.len() {
        vs.values.get(target)
    } else if vs.values.len() == 1 {
        vs.values.first()
    } else {
        None
    };
    rhs.is_some_and(|rhs| is_concatenated_string(rhs) || is_fmt_sprintf_call(rhs, pass))
}

fn package_var_has_concatenation(pass: &Pass<'_>, obj: ObjectId) -> bool {
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::GenDecl(gen) = decl else {
                continue;
            };
            if gen.tok != Some(Token::VAR) {
                continue;
            }
            for spec in &gen.specs {
                if let Spec::ValueSpec(vs) = spec {
                    if var_decl_has_concatenation(vs, obj, pass) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn was_built_with_concatenation(
    id: &Ident,
    pass: &Pass<'_>,
    blocks: &[&BlockStmt],
    call_pos: i64,
) -> bool {
    let Some(obj) = obj_of(pass, id) else {
        return false;
    };
    if scan_prior(blocks, call_pos, |stmt| {
        stmt_assigns_concatenation(stmt, obj, pass)
    }) {
        return true;
    }
    package_var_has_concatenation(pass, obj)
}

fn should_report_query_concatenation(
    arg: &Expr,
    pass: &Pass<'_>,
    blocks: &[&BlockStmt],
    call_pos: i64,
) -> bool {
    if is_concatenated_string(arg) || is_fmt_sprintf_call(arg, pass) {
        return true;
    }
    if let Expr::Ident(id) = arg {
        return was_built_with_concatenation(id, pass, blocks, call_pos);
    }
    false
}

// ---------------------------------------------------------------------------
// Call-site dispatch
// ---------------------------------------------------------------------------

fn receiver_is(pass: &Pass<'_>, recv: &Expr, suffix: &str) -> bool {
    type_string(pass, recv).is_some_and(|s| s.ends_with(suffix))
}

/// Index of the query-string argument for a query method, or `None`.
fn query_arg_index(method: &str) -> Option<usize> {
    matches!(
        method,
        "Query" | "QueryBatch" | "ValidateQuery" | "ExplainQuery"
    )
    .then_some(1)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "arangolint requires inspect analyzer".to_string())?;

    let pass_ref: &Pass<'_> = pass;
    let mut pending: Vec<(u32, &'static str)> = Vec::new();

    for file in pass_ref.files() {
        let mut stack: Vec<NodeRef<'_>> = Vec::new();
        preorder_stack(NodeRef::File(file), &mut stack, |node, stack| {
            let NodeRef::CallExpr(call) = node else {
                return true;
            };
            let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
                return true;
            };
            let call_pos = call.pos().0;

            // Check 1: BeginTransaction missing AllowImplicit.
            if sel.sel.name == METHOD_BEGIN_TRANSACTION
                && call.args.len() == EXPECTED_BEGIN_TXN_ARGS
                && receiver_is(pass_ref, &sel.x, DATABASE_TYPE_SUFFIX)
            {
                let arg = unwrap_parens(&call.args[2]);
                let blocks = ancestor_blocks(stack);
                if should_report_missing_allow_implicit(arg, pass_ref, &blocks, call_pos) {
                    pending.push((call.args[2].pos().0 as u32, MSG_MISSING_ALLOW_IMPLICIT));
                }
            }

            // Check 2: query concatenation.
            if let Some(idx) = query_arg_index(&sel.sel.name) {
                if call.args.len() > idx
                    && (receiver_is(pass_ref, &sel.x, DATABASE_TYPE_SUFFIX)
                        || receiver_is(pass_ref, &sel.x, TRANSACTION_TYPE_SUFFIX))
                {
                    let arg = unwrap_parens(&call.args[idx]);
                    let blocks = ancestor_blocks(stack);
                    if should_report_query_concatenation(arg, pass_ref, &blocks, call_pos) {
                        pending.push((arg.pos().0 as u32, MSG_QUERY_CONCATENATION));
                    }
                }
            }

            true
        });
    }

    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "arangolint",
        doc: "opinionated best practices for arangodb client",
        url: "https://github.com/Crocmagnon/arangolint",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_arg_index_only_query_methods() {
        assert_eq!(query_arg_index("Query"), Some(1));
        assert_eq!(query_arg_index("QueryBatch"), Some(1));
        assert_eq!(query_arg_index("ValidateQuery"), Some(1));
        assert_eq!(query_arg_index("ExplainQuery"), Some(1));
        assert_eq!(query_arg_index("Begin"), None);
    }
}
