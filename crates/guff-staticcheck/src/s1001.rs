//! S1001 — replace for loop with call to copy.
//!
//! Port of `honnef.co/go/tools/simple/s1001`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, BinaryExpr, CallExpr, Expr, ForStmt, Ident, IncDecStmt, IndexExpr, RangeStmt, Stmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{expr_to_int, is_call_to, object_of};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::{TypeData, TypeId};

fn expr_type(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    pass.types_info()?.types.get(&expr.id()).map(|tv| tv.typ)
}

fn render_type(pass: &Pass<'_>, typ: TypeId) -> Option<String> {
    let a = pass.pkg().type_artifacts.as_ref()?;
    Some(guff_types::typestring::type_string(
        &a.types,
        &a.objects,
        &a.packages,
        typ,
        None,
    ))
}

fn is_invariant(pass: &Pass<'_>, key: Option<guff_types::ObjectId>, val: Option<guff_types::ObjectId>, expr: &Expr) -> bool {
    fn walk(pass: &Pass<'_>, key: Option<guff_types::ObjectId>, val: Option<guff_types::ObjectId>, expr: &Expr) -> bool {
        // Side-effecting expressions (calls) must not be treated as loop-invariant
        // for S1001 — `range h.GetPositiveCount()` cannot become `copy(...)`.
        if matches!(expr, Expr::CallExpr(_)) {
            return false;
        }
        if let Expr::Ident(ident) = expr {
            if let Some(obj) = object_of(pass, ident) {
                if Some(obj) == key || Some(obj) == val {
                    return false;
                }
            }
        }
        match expr {
            Expr::UnaryExpr(e) => walk(pass, key, val, &e.x),
            Expr::BinaryExpr(e) => walk(pass, key, val, &e.x) && walk(pass, key, val, &e.y),
            Expr::IndexExpr(e) => walk(pass, key, val, &e.x) && walk(pass, key, val, &e.index),
            Expr::SliceExpr(e) => {
                walk(pass, key, val, &e.x)
                    && e.low.as_ref().is_none_or(|l| walk(pass, key, val, l))
                    && e.high.as_ref().is_none_or(|h| walk(pass, key, val, h))
            }
            Expr::SelectorExpr(e) => walk(pass, key, val, &e.x),
            Expr::ParenExpr(e) => walk(pass, key, val, &e.x),
            Expr::StarExpr(e) => walk(pass, key, val, &e.x),
            _ => true,
        }
    }
    walk(pass, key, val, expr)
}

fn elem_kind(pass: &Pass<'_>, typ: TypeId) -> Option<(TypeId, bool)> {
    let types = &pass.pkg().type_artifacts.as_ref()?.types;
    let u = typ.underlying(types);
    match types.get(u) {
        TypeData::Slice(s) => Some((s.elem(), false)),
        TypeData::Array(a) => Some((a.elem(), true)),
        TypeData::Pointer(p) => elem_kind(pass, p.elem()),
        _ => None,
    }
}

fn same_key(pass: &Pass<'_>, a: &Ident, b: &Ident) -> bool {
    match (object_of(pass, a), object_of(pass, b)) {
        (Some(oa), Some(ob)) => oa == ob,
        _ => a.name == b.name,
    }
}

fn is_index_copy(pass: &Pass<'_>, dst: &Expr, src_expr: &Expr, key: &Ident) -> bool {
    let Expr::IndexExpr(IndexExpr { index: dst_key, .. }) = dst else {
        return false;
    };
    let Expr::IndexExpr(IndexExpr { index: src_key, .. }) = src_expr else {
        return false;
    };
    let Expr::Ident(dst_key_id) = &**dst_key else {
        return false;
    };
    let Expr::Ident(src_key_id) = &**src_key else {
        return false;
    };
    same_key(pass, dst_key_id, key) && same_key(pass, src_key_id, key)
}

fn check_copy_loop(
    pass: &Pass<'_>,
    key: &Ident,
    value: Option<&Ident>,
    src: &Expr,
    body: &[Stmt],
) -> Option<&'static str> {
    if body.len() != 1 {
        return None;
    }
    let Stmt::AssignStmt(AssignStmt { lhs, rhs, tok, .. }) = &body[0] else {
        return None;
    };
    if tok != &Some(Token::ASSIGN) || lhs.len() != 1 || rhs.len() != 1 {
        return None;
    }
    let key_obj = object_of(pass, key);
    let val_obj = value.and_then(|v| object_of(pass, v));

    let dst = &lhs[0];
    let src_expr = &rhs[0];
    let matched = if let Some(val) = value {
        let Expr::IndexExpr(IndexExpr { x: dst_x, index: dst_key, .. }) = dst else {
            return None;
        };
        let Expr::Ident(dst_key_id) = &**dst_key else {
            return None;
        };
        if !same_key(pass, dst_key_id, key) {
            return None;
        }
        let Expr::Ident(v) = src_expr else {
            return None;
        };
        if !same_key(pass, v, val) {
            return None;
        }
        is_invariant(pass, key_obj, val_obj, dst_x) && is_invariant(pass, key_obj, val_obj, src)
    } else {
        is_index_copy(pass, dst, src_expr, key)
            && is_invariant(pass, key_obj, val_obj, dst)
            && is_invariant(pass, key_obj, val_obj, src)
    };
    if !matched {
        return None;
    }

    let tsrc = expr_type(pass, src)?;
    let Expr::IndexExpr(IndexExpr { x: dst_x, .. }) = dst else {
        return None;
    };
    let tdst = expr_type(pass, dst_x)?;
    let (src_elem, src_arr) = elem_kind(pass, tsrc)?;
    let (dst_elem, dst_arr) = elem_kind(pass, tdst)?;
    if render_type(pass, src_elem) != render_type(pass, dst_elem) {
        return None;
    }

    if src_arr && dst_arr && render_type(pass, tsrc) == render_type(pass, tdst) {
        Some("should copy arrays using assignment instead of using a loop")
    } else {
        Some("should use copy(to, from) instead of a loop")
    }
}

fn check_range(pass: &Pass<'_>, rs: &RangeStmt) -> Option<&'static str> {
    if !matches!(rs.tok, Some(Token::DEFINE)) {
        return None;
    }
    let key = rs.key.as_ref().and_then(|e| match e {
        Expr::Ident(id) => Some(id),
        _ => None,
    })?;
    let value = rs.value.as_ref().and_then(|e| match e {
        Expr::Ident(id) => Some(id),
        _ => None,
    });
    check_copy_loop(pass, key, value, &rs.x, &rs.body.list)
}

fn check_for(pass: &Pass<'_>, fs: &ForStmt) -> Option<&'static str> {
    let init = fs.init.as_deref()?;
    let Stmt::AssignStmt(init) = init else {
        return None;
    };
    let key = match init.lhs.first() {
        Some(Expr::Ident(id)) => id,
        _ => return None,
    };
    if !matches!(init.tok, Some(Token::DEFINE)) || !expr_to_int(pass, init.rhs.first()?).is_some_and(|n| n == 0) {
        return None;
    }
    let cond = fs.cond.as_ref()?;
    let Expr::BinaryExpr(BinaryExpr { x, op, y, .. }) = cond else {
        return None;
    };
    if *op != Token::LSS {
        return None;
    }
    let Expr::Ident(cond_key) = &**x else {
        return None;
    };
    if cond_key.name != key.name {
        return None;
    }
    let Expr::CallExpr(call) = &**y else {
        return None;
    };
    if !is_call_to(pass, call, "len") || call.args.len() != 1 {
        return None;
    }
    let src = &call.args[0];
    let post = fs.post.as_deref()?;
    let Stmt::IncDecStmt(IncDecStmt { x, tok, .. }) = post else {
        return None;
    };
    if *tok != Token::INC {
        return None;
    }
    let Expr::Ident(post_key) = &*x else {
        return None;
    };
    if post_key.name != key.name {
        return None;
    }
    check_copy_loop(pass, key, None, src, &fs.body.list)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1001 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(ForStmt, RangeStmt), pass.files(), |node| {
        let msg = match node {
            NodeRef::RangeStmt(rs) => check_range(pass, rs),
            NodeRef::ForStmt(fs) => check_for(pass, fs),
            _ => None,
        };
        if let Some(msg) = msg {
            pending.push((match_pos(node), msg.into()));
        }
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1001_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1001",
        doc: "replace for loop with call to copy",
        url: "https://staticcheck.dev/docs/checks/#S1001",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1001_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1001_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
