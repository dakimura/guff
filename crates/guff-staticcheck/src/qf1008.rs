//! QF1008 — omit embedded fields from selector expressions.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1008`.
//!
//! Handles uninterrupted selector chains (`a.b.c`) and chains interrupted by
//! calls/indexes (`a.b.c().d.e`) by checking every continuous segment of the
//! chain.
//!
//! Upstream calls `astutil.PathEnclosingInterval(file, expr.Pos(), expr.Pos())`
//! and bails when the *outermost* `SelectorExpr` on that path is not the visited
//! expression. The path spans the whole enclosing chain, so a selector nested
//! anywhere inside another selector — including across statement and function
//! boundaries, as in `T{f: func() { x.Emb.F = nil }}.Run()` — is skipped. We get
//! the same result by pruning the subtree of every chain root we visit.

use std::sync::OnceLock;

use guff::ast::{Expr, Ident, SelectorExpr};
use guff::walk::{inspect as walk_inspect, NodeRef};
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::alias::unalias_readonly;
use guff_types::arena::ObjectData;
use guff_types::lookup::{lookup_field_or_method, LookupResult};
use guff_types::TypeId;

/// One continuous run of selectors in a chain: the expression it starts from
/// and the field identifiers applied to it, left to right.
struct Segment<'a> {
    x: &'a Expr,
    fields: Vec<&'a Ident>,
}

/// The leftmost sub-expression starting at the same offset as `e` — i.e. the
/// single child `PathEnclosingInterval` descends into for a zero-width interval
/// at `e.Pos()`. `ParenExpr`, `StarExpr` and friends open with their own token,
/// so the spine ends there.
fn spine_child(e: &Expr) -> Option<&Expr> {
    let child: &Expr = match e {
        Expr::SelectorExpr(s) => &s.x,
        Expr::CallExpr(c) => &c.fun,
        Expr::IndexExpr(i) => &i.x,
        Expr::IndexListExpr(i) => &i.x,
        Expr::SliceExpr(s) => &s.x,
        Expr::TypeAssertExpr(t) => &t.x,
        Expr::BinaryExpr(b) => &b.x,
        _ => return None,
    };
    (child.pos() == e.pos()).then_some(child)
}

/// Upstream `extractSelectors`: split the chain rooted at `expr` into the
/// continuous selector runs separated by calls, indexes and the like, so
/// `a.b.c().d.e` yields `[a.b.c, d.e]`.
fn extract_selectors(expr: &SelectorExpr) -> Vec<Segment<'_>> {
    // Walk the leftmost spine outermost → innermost.
    let mut spine: Vec<Option<&SelectorExpr>> = vec![Some(expr)];
    let mut cur: Option<&Expr> = Some(&expr.x);
    while let Some(e) = cur {
        match e {
            Expr::SelectorExpr(s) => {
                spine.push(Some(s));
                cur = Some(&s.x);
            }
            other => {
                spine.push(None);
                cur = spine_child(other);
            }
        }
    }

    // Group innermost → outermost, the order upstream walks the enclosing path.
    let mut out: Vec<Segment<'_>> = Vec::new();
    let mut in_chain = false;
    for el in spine.iter().rev() {
        match el {
            Some(sel) => {
                if !in_chain {
                    in_chain = true;
                    out.push(Segment {
                        x: &sel.x,
                        fields: Vec::new(),
                    });
                }
                out.last_mut()
                    .expect("in_chain implies a segment")
                    .fields
                    .push(&sel.sel);
            }
            None => in_chain = false,
        }
    }
    out
}

fn expr_type(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn check_selector(pass: &Pass<'_>, expr: &SelectorExpr, pending: &mut Vec<(u32, u32, String)>) {
    // Skip 1-level selectors (cannot omit anything).
    if !matches!(&*expr.x, Expr::SelectorExpr(_)) {
        return;
    }
    for segment in extract_selectors(expr) {
        check_segment(pass, segment.x, &segment.fields, pending);
    }
}

fn check_segment(
    pass: &Pass<'_>,
    base_expr: &Expr,
    fields: &[&Ident],
    pending: &mut Vec<(u32, u32, String)>,
) {
    if fields.len() < 2 {
        return;
    }
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return,
    };

    let Some(mut base) = expr_type(pass, base_expr) else {
        return;
    };
    base = unalias_readonly(&artifacts.types, base);

    let mut types = artifacts.types.clone();
    let mut i = 0;
    while i + 1 < fields.len() {
        let hop1 = fields[i];
        let hop2 = fields[i + 1];

        // Invalid / qualified-ident base.
        if !guff_types::predicates::is_valid(&types, base) {
            break;
        }

        let left = lookup_field_or_method(
            &mut types,
            &artifacts.objects,
            &artifacts.packages,
            base,
            true,
            Some(artifacts.type_pkg),
            &hop1.name,
        );
        let LookupResult::Found {
            obj: left_obj,
            index: left_leg,
            ..
        } = left
        else {
            break;
        };

        // Only skip embedded fields.
        let embedded = matches!(
            artifacts.objects.get(left_obj),
            ObjectData::Var(v) if v.embedded()
        );
        if !embedded {
            // Advance base through this hop and continue.
            let Some(next_ty) = left_obj.typ(&artifacts.objects) else {
                break;
            };
            base = unalias_readonly(&types, next_ty);
            i += 1;
            continue;
        }

        let direct = lookup_field_or_method(
            &mut types,
            &artifacts.objects,
            &artifacts.packages,
            base,
            true,
            Some(artifacts.type_pkg),
            &hop2.name,
        );
        let LookupResult::Found {
            obj: direct_obj,
            index: direct_path,
            ..
        } = direct
        else {
            let Some(next_ty) = left_obj.typ(&artifacts.objects) else {
                break;
            };
            base = unalias_readonly(&types, next_ty);
            i += 1;
            continue;
        };

        let Some(hop2_obj) = object_of(pass, hop2) else {
            let Some(next_ty) = left_obj.typ(&artifacts.objects) else {
                break;
            };
            base = unalias_readonly(&types, next_ty);
            i += 1;
            continue;
        };
        if direct_obj != hop2_obj {
            let Some(next_ty) = left_obj.typ(&artifacts.objects) else {
                break;
            };
            base = unalias_readonly(&types, next_ty);
            i += 1;
            continue;
        }

        let left_ty = match left_obj.typ(&artifacts.objects) {
            Some(t) => unalias_readonly(&types, t),
            None => break,
        };
        let right = lookup_field_or_method(
            &mut types,
            &artifacts.objects,
            &artifacts.packages,
            left_ty,
            true,
            Some(artifacts.type_pkg),
            &hop2.name,
        );
        let LookupResult::Found {
            index: right_leg, ..
        } = right
        else {
            base = left_ty;
            i += 1;
            continue;
        };

        if direct_path.len() != left_leg.len() + right_leg.len() {
            base = left_ty;
            i += 1;
            continue;
        }
        let mut path_ok = true;
        for (j, &step) in direct_path.iter().enumerate() {
            if j < left_leg.len() {
                if left_leg[j] != step {
                    path_ok = false;
                    break;
                }
            } else if right_leg[j - left_leg.len()] != step {
                path_ok = false;
                break;
            }
        }
        if path_ok {
            pending.push((
                hop1.pos().0 as u32,
                hop2.pos().0 as u32,
                hop1.name.clone(),
            ));
        }

        base = left_ty;
        i += 1;
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1008 requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    // Every node below a `SelectorExpr` has that selector as an ancestor, so
    // upstream's "outermost selector on the enclosing path must be me" test
    // fails for all of them. Pruning the subtree of each chain root we visit is
    // the same filter, and it also gives each root the whole chain to split.
    for file in pass.files() {
        walk_inspect(NodeRef::File(file), |node| {
            let Some(NodeRef::SelectorExpr(sel)) = node else {
                return true;
            };
            check_selector(pass, sel, &mut pending);
            false
        });
    }

    for (pos, end, name) in pending {
        pass.report(Diagnostic {
            pos,
            end,
            message: format!("could remove embedded field {name:?} from selector"),
            suggested_fixes: vec![SuggestedFix {
                message: format!("Remove embedded field {name:?} from selector"),
                text_edits: vec![TextEdit {
                    pos,
                    end,
                    new_text: String::new(),
                }],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn qf1008_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "QF1008",
        doc: "omit embedded fields from selector expression",
        url: "https://staticcheck.dev/docs/checks/#QF1008",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(qf1008_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn qf1008_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
