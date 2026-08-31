//! S1001 — replace for loop with call to copy.
//!
//! Port of `honnef.co/go/tools/simple/s1001`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, BinaryExpr, CallExpr, Expr, ForStmt, Ident, IncDecStmt, IndexExpr, RangeStmt, Stmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{self, expr_to_int, is_call_to, object_of};
use guff_analysis::passes::inspect;
use guff_analysis::{
    match_pos, AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::render::{render_expr, render_node};
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

/// Upstream's `elType`: the element type, whether the value is an array, and
/// whether it was reached through a pointer.
///
/// The pointer flag is what decides between `dst = src` and `*dst = *src` in
/// the assignment branch. It used to be dropped here.
fn elem_kind(pass: &Pass<'_>, typ: TypeId) -> Option<(TypeId, bool, bool)> {
    let types = &pass.pkg().type_artifacts.as_ref()?.types;
    let u = typ.underlying(types);
    match types.get(u) {
        TypeData::Slice(s) => Some((s.elem(), false, false)),
        TypeData::Array(a) => Some((a.elem(), true, false)),
        TypeData::Pointer(p) => {
            let (elem, is_array, _) = elem_kind(pass, p.elem())?;
            Some((elem, is_array, true))
        }
        _ => None,
    }
}

/// `T.Underlying().(*types.Pointer).Elem()` — what upstream compares once it
/// knows a side was reached through a pointer.
fn pointee(pass: &Pass<'_>, typ: TypeId) -> Option<TypeId> {
    let types = &pass.pkg().type_artifacts.as_ref()?.types;
    match types.get(typ.underlying(types)) {
        TypeData::Pointer(p) => Some(p.elem()),
        _ => None,
    }
}

fn same_key(pass: &Pass<'_>, a: &Ident, b: &Ident) -> bool {
    match (object_of(pass, a), object_of(pass, b)) {
        (Some(oa), Some(ob)) => oa == ob,
        _ => a.name == b.name,
    }
}

/// `(AssignStmt (IndexExpr dst key) "=" (IndexExpr src key))`, where `src` is
/// the *same binding* the loop header bound — `range src`, or `len(src)` in the
/// three-clause form.
///
/// A repeated binding in a `honnef.co/go/tools/pattern` query is not a wildcard:
/// `Binding.Match` recalls the first value and compares it with `matchAST`,
/// which walks the two trees field by field (positions, objects and comments
/// skipped). So `for i := range dst { dst[i] = src[i] }` does not match — the
/// header bound `src` to `dst`, and `src` is not `dst`. Without the comparison
/// guff reported thanos' `pkg/compact/planner_test.go` three times over, a
/// `metasByMinTime[i] = c.metas[i]` loop upstream is silent about.
fn is_index_copy(pass: &Pass<'_>, dst: &Expr, src_expr: &Expr, src: &Expr, key: &Ident) -> bool {
    let Expr::IndexExpr(IndexExpr { index: dst_key, .. }) = dst else {
        return false;
    };
    let Expr::IndexExpr(IndexExpr { x: src_x, index: src_key, .. }) = src_expr else {
        return false;
    };
    let Expr::Ident(dst_key_id) = &**dst_key else {
        return false;
    };
    let Expr::Ident(src_key_id) = &**src_key else {
        return false;
    };
    same_key(pass, dst_key_id, key)
        && same_key(pass, src_key_id, key)
        && same_source(src_x, src)
}

/// `matchAST` over the expressions a recalled binding can hold: structural
/// equality with positions left out, which is what rendering the two trees and
/// comparing the text gives. Identifiers compare by name, as upstream's does
/// (`matchAST` skips the `Obj` field).
fn same_source(a: &Expr, b: &Expr) -> bool {
    render_expr(a) == render_expr(b)
}

/// What the loop copies, and how upstream would rewrite it.
struct CopyLoop<'a> {
    message: String,
    /// The *container* being written to — `dst`, not `dst[i]`.
    dst: &'a Expr,
    src: &'a Expr,
    dst_arr: bool,
    src_arr: bool,
    dst_ptr: bool,
    src_ptr: bool,
    /// Both sides are arrays of the same type: a plain assignment, not `copy`.
    assign: bool,
}

fn check_copy_loop<'a>(
    pass: &Pass<'_>,
    key: &Ident,
    value: Option<&Ident>,
    src: &'a Expr,
    body: &'a [Stmt],
) -> Option<CopyLoop<'a>> {
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
    // Upstream's pattern binds `dst` inside `(IndexExpr dst key)`, so the
    // container is what gets checked and rewritten — never `dst[i]`, which
    // contains the loop variable and can never be invariant. Passing the whole
    // index expression made `isInvariant` answer a question nobody asked, and
    // the three-clause `for` form went unreported because of it.
    let Expr::IndexExpr(IndexExpr { x: dst_x, index: dst_key, .. }) = dst else {
        return None;
    };
    let matched = if let Some(val) = value {
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
        is_index_copy(pass, dst, src_expr, src, key)
            && is_invariant(pass, key_obj, val_obj, dst_x)
            && is_invariant(pass, key_obj, val_obj, src)
    };
    if !matched {
        return None;
    }

    let tsrc = expr_type(pass, src)?;
    let tdst = expr_type(pass, dst_x)?;
    let (src_elem, src_arr, src_ptr) = elem_kind(pass, tsrc)?;
    let (dst_elem, dst_arr, dst_ptr) = elem_kind(pass, tdst)?;
    if render_type(pass, src_elem) != render_type(pass, dst_elem) {
        return None;
    }
    // Upstream compares the *pointees* when a side came through a pointer, so
    // `*[4]int` and `*[4]int` are two identical `[4]int`.
    let tsrc_cmp = if src_ptr { pointee(pass, tsrc)? } else { tsrc };
    let tdst_cmp = if dst_ptr { pointee(pass, tdst)? } else { tdst };

    let assign = src_arr && dst_arr && render_type(pass, tsrc_cmp) == render_type(pass, tdst_cmp);
    let message = if assign {
        "should copy arrays using assignment instead of using a loop".to_string()
    } else {
        // `to` and `from` are literal words in upstream's message, but each
        // grows a `[:]` when that side is an array — the same `[:]` the fix
        // writes. Hardcoding `copy(to, from)` said the wrong thing for every
        // array shape.
        let to = if dst_arr { "to[:]" } else { "to" };
        let from = if src_arr { "from[:]" } else { "from" };
        format!("should use copy({to}, {from}) instead of a loop")
    };
    Some(CopyLoop {
        message,
        dst: dst_x,
        src,
        dst_arr,
        src_arr,
        dst_ptr,
        src_ptr,
        assign,
    })
}

fn check_range<'a>(pass: &Pass<'_>, rs: &'a RangeStmt) -> Option<CopyLoop<'a>> {
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

fn check_for<'a>(pass: &Pass<'_>, fs: &'a ForStmt) -> Option<CopyLoop<'a>> {
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

    let mut pending: Vec<(u32, String, Option<TextEdit>)> = Vec::new();
    inspect.preorder_typed(node_mask!(ForStmt, RangeStmt), pass.files(), |node| {
        let (found, span) = match node {
            NodeRef::RangeStmt(rs) => (
                check_range(pass, rs),
                (rs.for_.0 as u32, rs.body.end().0 as u32),
            ),
            NodeRef::ForStmt(fs) => (
                check_for(pass, fs),
                (fs.for_.0 as u32, fs.body.end().0 as u32),
            ),
            _ => (None, (0, 0)),
        };
        let Some(found) = found else {
            return;
        };
        let edit = replacement(pass, &found).map(|new_text| TextEdit {
            pos: span.0,
            end: span.1,
            new_text,
        });
        pending.push((match_pos(node), found.message, edit));
    });
    for (pos, message, edit) in pending {
        let Some(edit) = edit else {
            pass.report_unless_generated(pos, message);
            continue;
        };
        if code::is_generated_at(pass, pos) {
            continue;
        }
        let fix_message = if edit.new_text.starts_with("copy(") {
            "Replace loop with call to copy()"
        } else {
            "Replace loop with assignment"
        };
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: fix_message.into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

/// The statement upstream puts in the loop's place.
///
/// Two shapes. Both sides arrays of one type gives an assignment, with a `*` on
/// whichever side was reached through a pointer. Anything else gives
/// `copy(dst, src)`, where an array side is sliced instead of starred —
/// `p[:]` is legal on a `*[N]T`, so the pointer needs no separate treatment
/// there.
fn replacement(pass: &Pass<'_>, c: &CopyLoop<'_>) -> Option<String> {
    let dst = render_node(pass, c.dst)?;
    let src = render_node(pass, c.src)?;
    if c.assign {
        let star = |ptr: bool, text: &str| {
            if ptr {
                format!("*{text}")
            } else {
                text.to_string()
            }
        };
        return Some(format!(
            "{} = {}",
            star(c.dst_ptr, &dst),
            star(c.src_ptr, &src)
        ));
    }
    let slice = |arr: bool, text: &str| {
        if arr {
            format!("{text}[:]")
        } else {
            text.to_string()
        }
    };
    Some(format!(
        "copy({}, {})",
        slice(c.dst_arr, &dst),
        slice(c.src_arr, &src)
    ))
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
