//! SA9004 — only the first constant has an explicit type.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa9004`.

use std::sync::OnceLock;

use guff::ast::{BasicLit, Expr, GenDecl, ValueSpec};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::code;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass, Diagnostic, SuggestedFix, TextEdit};

use crate::render::render_node;
use guff_types::api_predicates::api_convertible_to;

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA9004 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, Vec<TextEdit>)> = Vec::new();
    inspect.preorder_typed(node_mask!(GenDecl), pass.files(), |n| {
        let NodeRef::GenDecl(decl) = n else {
            return;
        };
        check_const_decl(pass, decl, &mut pending);
    });
    for (pos, edits) in pending {
        if edits.is_empty() {
            pass.report_unless_generated(
                pos,
                "only the first constant in this group has an explicit type",
            );
            continue;
        }
        if code::is_generated_at(pass, pos) {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message: "only the first constant in this group has an explicit type".to_string(),
            suggested_fixes: vec![SuggestedFix {
                message: "Add type to all constants in group".into(),
                text_edits: edits,
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn check_const_decl(pass: &Pass<'_>, decl: &GenDecl, pending: &mut Vec<(u32, Vec<TextEdit>)>) {
    if decl.tok != Some(Token::CONST) || !decl.lparen.is_valid() {
        return;
    }
    let groups = group_specs(pass, decl);
    for group in groups {
        if group.len() < 2 {
            continue;
        }
        let first = group[0];
        if first.ty.is_none() {
            continue;
        }
        let info = match pass.types_info() {
            Some(i) => i,
            None => continue,
        };
        let first_type = match first.values.first() {
            Some(v) => info.types.get(&v.id()).map(|t| t.typ),
            None => None,
        };
        let Some(first_type) = first_type else {
            continue;
        };
        let mut ok_group = true;
        for spec in group.iter().skip(1) {
            if spec.ty.is_some()
                || spec.names.len() != 1
                || spec.values.len() != 1
                || !is_simple_literal(&spec.values[0])
            {
                ok_group = false;
                break;
            }
            let typ = info.types.get(&spec.values[0].id()).map(|t| t.typ);
            let Some(typ) = typ else {
                ok_group = false;
                break;
            };
            let artifacts = match pass.pkg().type_artifacts.as_ref() {
                Some(a) => a,
                None => {
                    ok_group = false;
                    break;
                }
            };
            if !api_convertible_to(
                &mut artifacts.types.clone(),
                &artifacts.objects,
                &artifacts.packages,
                typ,
                first_type,
            ) {
                ok_group = false;
                break;
            }
        }
        if ok_group {
            // One edit per spec after the first: upstream copies the spec,
            // gives it the group's type and prints it. The loop above has
            // already established each has exactly one name and one value, so
            // the printed form is `name type = value` and nothing else.
            //
            // The span stops where the spec does, which is before any trailing
            // line comment — that is why upstream clears `Comment` on its copy
            // rather than reprinting it.
            let ty = first.ty.as_ref().and_then(|t| render_node(pass, t));
            let edits: Vec<TextEdit> = match ty {
                Some(ty) => group
                    .iter()
                    .skip(1)
                    .filter_map(|spec| {
                        let name = spec.names.first()?;
                        let value = spec.values.first()?;
                        Some(TextEdit {
                            pos: name.name_pos.0 as u32,
                            end: value.end().0 as u32,
                            new_text: format!(
                                "{} {ty} = {}",
                                name.name,
                                render_node(pass, value)?
                            ),
                        })
                    })
                    .collect(),
                None => Vec::new(),
            };
            // Upstream hands `report.Report` the group's first `*ast.ValueSpec`,
            // whose `Pos()` is its first *name* — not the type that follows it.
            // `const ( A E = 1; B = 2 )` reports at `A`, column 2.
            pending.push((
                first
                    .names
                    .first()
                    .map(|n| n.name_pos.0 as u32)
                    .unwrap_or(decl.lparen.0 as u32),
                edits,
            ));
        }
    }
}

fn is_simple_literal(expr: &Expr) -> bool {
    match expr {
        Expr::BasicLit(BasicLit { .. }) => true,
        Expr::UnaryExpr(u) => matches!(&*u.x, Expr::BasicLit(_)),
        _ => false,
    }
}

/// Port of `astutil.GroupSpecs`: two specs share a group only when they sit on
/// consecutive lines (`prev.End().Line + 1 == next.Pos().Line`), which is
/// `PositionFor(…, false)` — raw lines, not `//line` adjusted.
///
/// A doc comment between two constants splits the group just as a blank line
/// does, because `ValueSpec.Pos()` is the first name and starts *below* the
/// comment. guff used to split on "this spec has no values" instead, which is a
/// different rule entirely: it put `tsdb/head.go:239-242` — two constants, each
/// carrying its own doc comment — into one group and reported it, where
/// upstream sees two groups of one and never reaches the check. Dropping the
/// values test loses nothing, since the group body below already refuses any
/// spec that does not have exactly one value (the `iota` shape).
fn group_specs<'a>(pass: &Pass<'_>, decl: &'a GenDecl) -> Vec<Vec<&'a ValueSpec>> {
    let fset = pass.fset();
    let mut groups: Vec<Vec<&'a ValueSpec>> = Vec::new();
    let mut prev_end_line: Option<i64> = None;
    for spec in &decl.specs {
        let guff::ast::Spec::ValueSpec(vs) = spec else {
            continue;
        };
        let starts_new_group = match prev_end_line {
            Some(prev) => prev + 1 != fset.line_for(spec.pos(), false),
            None => true,
        };
        if starts_new_group {
            groups.push(Vec::new());
        }
        groups
            .last_mut()
            .expect("a group was just pushed when empty")
            .push(vs);
        prev_end_line = Some(fset.line_for(spec.end(), false));
    }
    groups
}

fn sa9004_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA9004",
        doc: "only the first constant has an explicit type",
        url: "https://staticcheck.dev/docs/checks/#SA9004",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa9004_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa9004_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
