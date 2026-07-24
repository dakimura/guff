//! QF1008 — omit embedded fields from selector expressions.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1008`.
//!
//! Handles uninterrupted selector chains (`a.b.c`) and chains interrupted by
//! calls/indexes (`a.b.c().d.e`) by checking each continuous segment whose
//! root is a `SelectorExpr` whose parent is not another `SelectorExpr`.

use std::sync::OnceLock;

use guff::ast::{Expr, Ident, SelectorExpr};
use guff::walk::{preorder_stack, NodeRef};
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::alias::unalias_readonly;
use guff_types::arena::ObjectData;
use guff_types::lookup::{lookup_field_or_method, LookupResult};
use guff_types::TypeId;

fn flatten_selector(expr: &SelectorExpr) -> (&Expr, Vec<&Ident>) {
    let mut fields = vec![&expr.sel];
    let mut cur = &*expr.x;
    while let Expr::SelectorExpr(s) = cur {
        fields.push(&s.sel);
        cur = &*s.x;
    }
    fields.reverse();
    (cur, fields)
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

    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return,
    };
    let (base_expr, fields) = flatten_selector(expr);
    if fields.len() < 2 {
        return;
    }

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
            None,
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
            None,
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
            None,
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
    // Only process the root of each uninterrupted selector chain (parent is
    // not a SelectorExpr). That yields separate segments around calls/indexes,
    // matching upstream `extractSelectors`.
    for file in pass.files() {
        let mut stack = Vec::new();
        preorder_stack(NodeRef::File(file), &mut stack, |node, stack| {
            let NodeRef::SelectorExpr(sel) = node else {
                return true;
            };
            let parent_is_selector = stack
                .last()
                .is_some_and(|n| matches!(n, NodeRef::SelectorExpr(_)));
            // Upstream extractSelectors + PathEnclosingInterval never flags
            // method-call selectors like `d.Metric.GetCounter()` — skip when
            // this SelectorExpr is the Fun of a CallExpr. Field chains
            // (`x := a.b.c`) and post-call segments (`f().a.b`) still run.
            let is_call_fun = stack.last().is_some_and(|n| {
                matches!(
                    n,
                    NodeRef::CallExpr(c)
                        if matches!(
                            c.fun.as_ref(),
                            Expr::SelectorExpr(s) if s.id == sel.id
                        )
                )
            });
            if !parent_is_selector && !is_call_fun {
                check_selector(pass, sel, &mut pending);
            }
            true
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
