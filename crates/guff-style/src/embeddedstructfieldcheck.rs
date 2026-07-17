//! Port of [`github.com/manuelarte/embeddedstructfieldcheck`](https://github.com/manuelarte/embeddedstructfieldcheck)
//! (golangci-lint wrapper in `pkg/golinters/embeddedstructfieldcheck`).
//!
//! Checks that embedded struct fields:
//! 1. Appear before regular (named) fields.
//! 2. Are separated from regular fields by a blank line (`empty-line`, default true).
//! 3. Optionally are not `sync.Mutex` / `sync.RWMutex` (`forbid-mutex`, default false).
//!
//! DEFERRED: SuggestedFix for missing blank line; field-doc-aware empty-line
//! (load uses `Mode::NONE`, so `Field.doc` is unset — comment lines between
//! embedded and regular fields may be under-counted until `PARSE_COMMENTS`).

use std::sync::OnceLock;

use guff::ast::{Expr, Field, StructType};
use guff::position::FileSet;
use guff::walk::{preorder, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::EmbeddedstructfieldcheckOptions;

fn is_embedded(field: &Field) -> bool {
    field.names.is_empty()
}

fn line_of(fset: &FileSet, pos: guff::position::Pos) -> i64 {
    fset.position(pos).line
}

fn sync_mutex_name(ty: &Expr) -> Option<&'static str> {
    let sel = match ty {
        Expr::SelectorExpr(se) => se,
        Expr::StarExpr(star) => match star.x.as_ref() {
            Expr::SelectorExpr(se) => se,
            _ => return None,
        },
        _ => return None,
    };
    let Expr::Ident(pkg) = sel.x.as_ref() else {
        return None;
    };
    if pkg.name != "sync" {
        return None;
    }
    match sel.sel.name.as_str() {
        "Mutex" => Some("sync.Mutex"),
        "RWMutex" => Some("sync.RWMutex"),
        _ => None,
    }
}

fn analyze_struct(
    fset: &FileSet,
    st: &StructType,
    opts: &EmbeddedstructfieldcheckOptions,
    pending: &mut Vec<(u32, String)>,
) {
    let mut first_embedded: Option<&Field> = None;
    let mut last_embedded: Option<&Field> = None;
    let mut first_regular: Option<&Field> = None;

    for field in &st.fields.list {
        if is_embedded(field) {
            if opts.forbid_mutex {
                if let Some(ty) = field.ty.as_ref() {
                    if let Some(name) = sync_mutex_name(ty) {
                        let pos = match ty {
                            Expr::StarExpr(star) => match star.x.as_ref() {
                                Expr::SelectorExpr(se) => se.x.pos(),
                                _ => field.pos(),
                            },
                            Expr::SelectorExpr(se) => se.x.pos(),
                            _ => field.pos(),
                        };
                        pending.push((pos.0 as u32, format!("{name} should not be embedded")));
                    }
                }
            }

            if first_embedded.is_none() {
                first_embedded = Some(field);
            }
            if last_embedded
                .map(|f| f.pos().0 < field.pos().0)
                .unwrap_or(true)
            {
                last_embedded = Some(field);
            }

            if let Some(reg) = first_regular {
                if reg.pos().0 < field.pos().0 {
                    pending.push((
                        field.pos().0 as u32,
                        "embedded fields should be listed before regular fields".into(),
                    ));
                    // Upstream returns early: skip empty-line for this struct.
                    return;
                }
            }
        } else if first_regular.is_none() {
            first_regular = Some(field);
        }
    }

    let _ = first_embedded;

    if !opts.empty_line {
        return;
    }
    let (Some(last_emb), Some(first_reg)) = (last_embedded, first_regular) else {
        return;
    };

    let line = line_of(fset, last_emb.end());
    // DEFERRED: when Field.doc is populated, use doc.pos() like upstream.
    let next_line = if let Some(doc) = first_reg.doc.as_ref() {
        line_of(fset, doc.pos())
    } else {
        line_of(fset, first_reg.pos())
    };

    if next_line != line + 2 {
        pending.push((
            last_emb.pos().0 as u32,
            "there must be an empty line separating embedded fields from regular fields".into(),
        ));
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "embeddedstructfieldcheck requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<EmbeddedstructfieldcheckOptions>("embeddedstructfieldcheck")
        .cloned()
        .unwrap_or_default();

    let fset = pass.fset().clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::StructType(st) = n {
                analyze_struct(&fset, st, &opts, &mut pending);
            }
            true
        });
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "embeddedstructfieldcheck",
        doc: "Embedded types should be at the top of the field list of a struct, \
              and there must be an empty line separating embedded fields from regular fields.",
        url: "https://github.com/manuelarte/embeddedstructfieldcheck",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff::ast::{Field, Ident, SelectorExpr, StarExpr};
    use guff::position::Pos;

    #[test]
    fn detects_embedded_by_empty_names() {
        assert!(is_embedded(&Field::default()));
        let named = Field {
            names: vec![Ident {
                name: "x".into(),
                name_pos: Pos(10),
                obj: Default::default(),
                id: 0,
            }],
            ..Default::default()
        };
        assert!(!is_embedded(&named));
    }

    #[test]
    fn sync_mutex_detection() {
        let se = Expr::SelectorExpr(SelectorExpr {
            x: Box::new(Expr::Ident(Ident {
                name: "sync".into(),
                name_pos: Pos::default(),
                obj: Default::default(),
                id: 0,
            })),
            sel: Ident {
                name: "Mutex".into(),
                name_pos: Pos::default(),
                obj: Default::default(),
                id: 0,
            },
            id: 0,
        });
        assert_eq!(sync_mutex_name(&se), Some("sync.Mutex"));

        let star = Expr::StarExpr(StarExpr {
            star: Pos::default(),
            x: Box::new(Expr::SelectorExpr(SelectorExpr {
                x: Box::new(Expr::Ident(Ident {
                    name: "sync".into(),
                    name_pos: Pos::default(),
                    obj: Default::default(),
                    id: 0,
                })),
                sel: Ident {
                    name: "RWMutex".into(),
                    name_pos: Pos::default(),
                    obj: Default::default(),
                    id: 0,
                },
                id: 0,
            })),
            id: 0,
        });
        assert_eq!(sync_mutex_name(&star), Some("sync.RWMutex"));
    }
}
