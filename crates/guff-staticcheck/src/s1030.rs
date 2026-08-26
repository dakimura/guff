//! S1030 — use `bytes.Buffer.String` or `bytes.Buffer.Bytes`.
//!
//! Port of `honnef.co/go/tools/simple/s1030`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, SelectorExpr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{self, type_func_name};
use guff_analysis::passes::inspect;
use guff_analysis::{
    match_pos, AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;
use guff_types::TypeId;

use crate::render::{render_expr, render_node};

fn expr_type(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    pass.types_info()?.types.get(&expr.id()).map(|tv| tv.typ)
}

/// The callee's `typeutil.FuncName`, e.g. `(*bytes.Buffer).Bytes`.
///
/// `code::is_call_to` cannot be used here: it goes through `func_name`, which
/// drops the receiver, so every method would read as `bytes.Bytes`.
fn method_name(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    let Expr::SelectorExpr(SelectorExpr { sel, .. }) = &*call.fun else {
        return None;
    };
    let obj = pass.types_info()?.uses.get(&sel.id).copied()?;
    let a = pass.pkg().type_artifacts.as_ref()?;
    Some(type_func_name(&a.types, &a.objects, &a.packages, obj))
}

/// Is `t` the predeclared `string` itself (not a named type whose underlying
/// type is `string`)? Upstream compares against `types.Universe`'s entry.
fn is_universe_string(pass: &Pass<'_>, t: TypeId) -> bool {
    let Some(a) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    matches!(a.types.get(t), TypeData::Basic(b) if b.kind() == BasicKind::String)
}

/// Is `t` exactly `[]byte`?
fn is_byte_slice(pass: &Pass<'_>, t: TypeId) -> bool {
    let Some(a) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let TypeData::Slice(s) = a.types.get(t) else {
        return false;
    };
    matches!(a.types.get(s.elem()), TypeData::Basic(b) if b.kind() == guff_types::basic::BYTE)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let pkg = pass.pkg().pkg_path.as_str();
    if pkg == "bytes" || pkg == "bytes_test" {
        return Ok(None);
    }

    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1030 requires inspect analyzer".to_string())?
        .clone();

    // Candidates as `(conversion node id, pos, message)`, before the
    // `m[string(buf.Bytes())]` exemption below.
    let mut pending: Vec<(u32, u32, String, Option<TextEdit>)> = Vec::new();
    // Node ids of the `string(...)` conversions among them, for that exemption.
    let mut needs_parent: Vec<u32> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        // `(CallExpr _ [(CallExpr sel@(SelectorExpr recv _) [])])`: a
        // one-argument conversion whose argument is a no-argument method call.
        if call.args.len() != 1 {
            return;
        }
        let Expr::CallExpr(inner) = &call.args[0] else {
            return;
        };
        if !inner.args.is_empty() {
            return;
        }
        let Expr::SelectorExpr(SelectorExpr { x: recv, .. }) = &*inner.fun else {
            return;
        };

        let Some(typ) = expr_type(pass, &call.fun) else {
            return;
        };
        // `Bytes` and `String` are declared on `*bytes.Buffer`; a call on an
        // addressable value takes the address implicitly, so the method's
        // receiver is the pointer either way.
        let (want, method) = if is_universe_string(pass, typ) {
            ("(*bytes.Buffer).Bytes", "String")
        } else if is_byte_slice(pass, typ) {
            ("(*bytes.Buffer).String", "Bytes")
        } else {
            return;
        };
        if method_name(pass, inner).as_deref() != Some(want) {
            return;
        }

        if method == "String" {
            needs_parent.push(call.id);
        }
        // `edit.ReplaceWithPattern`: the outer conversion goes and the
        // receiver's own `String()` / `Bytes()` call takes its place.
        let edit = render_node(pass, recv).map(|recv_text| TextEdit {
            pos: call.pos().0 as u32,
            end: call.end().0 as u32,
            new_text: format!("{recv_text}.{method}()"),
        });
        pending.push((
            call.id,
            match_pos(node),
            format!(
                "should use {}.{}() instead of {}",
                render_expr(recv),
                method,
                render_expr(&Expr::CallExpr(call.clone()))
            ),
            edit,
        ));
    });

    // Upstream reads the cursor's parent to skip `m[string(buf.Bytes())]`, a
    // shape the compiler optimizes so that it really is faster than
    // `m[buf.String()]`. guff's preorder has no parent, so the direct children
    // of every IndexExpr have to be collected in a second traversal — done
    // only when there is a candidate to exempt, which in real code is almost
    // never. (Upstream exempts *either* child, not just the index, because it
    // only looks at the parent's node type.)
    let mut exempt: std::collections::HashSet<u32> = std::collections::HashSet::new();
    if !needs_parent.is_empty() {
        inspect.preorder_typed(node_mask!(IndexExpr), pass.files(), |node| {
            if let NodeRef::IndexExpr(ix) = node {
                for id in [ix.x.id(), ix.index.id()] {
                    if needs_parent.contains(&id) {
                        exempt.insert(id);
                    }
                }
            }
        });
    }

    for (id, pos, message, edit) in pending {
        if exempt.contains(&id) {
            continue;
        }
        let Some(edit) = edit else {
            pass.report_unless_generated(pos, message);
            continue;
        };
        if code::is_generated_at(pass, pos) {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Simplify conversion".into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn s1030_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1030",
        doc: "use bytes.Buffer.String or bytes.Buffer.Bytes",
        url: "https://staticcheck.dev/docs/checks/#S1030",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1030_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1030_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
