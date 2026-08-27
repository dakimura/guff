//! Port of [`github.com/manuelarte/embeddedstructfieldcheck`](https://github.com/manuelarte/embeddedstructfieldcheck)
//! (golangci-lint wrapper in `pkg/golinters/embeddedstructfieldcheck`).
//!
//! Checks that embedded struct fields:
//! 1. Appear before regular (named) fields.
//! 2. Are separated from regular fields by a blank line (`empty-line`, default true).
//! 3. Optionally are not `sync.Mutex` / `sync.RWMutex` (`forbid-mutex`, default false).
//!
//! Re-parses with `PARSE_COMMENTS` because load uses `Mode::NONE` and upstream's
//! empty-line check uses `Field.Doc.Pos()` when a doc comment precedes the first
//! regular field (k8s CRDs, etc.).
//!
//! The missing-blank-line finding carries a fix; the misplaced-field and
//! forbidden-embed ones do not, which is upstream's split too — only
//! `NewMissingSpaceDiag` builds a `SuggestedFix` (`internal/diag.go:16`).

use std::fs;
use std::sync::{Arc, OnceLock};

use guff::ast::{Expr, Field, File, StructType};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::{FileSet, Pos};
use guff::walk::{preorder, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::options::EmbeddedstructfieldcheckOptions;

fn is_embedded(field: &Field) -> bool {
    field.names.is_empty()
}

fn line_of(fset: &FileSet, pos: Pos) -> i64 {
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
    pending: &mut Vec<(Pos, String, Option<Pos>)>,
) {
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
                        pending.push((pos, format!("{name} should not be embedded"), None));
                    }
                }
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
                        field.pos(),
                        "embedded fields should be listed before regular fields".into(),
                        None,
                    ));
                    // Upstream returns early: skip empty-line for this struct.
                    return;
                }
            }
        } else if first_regular.is_none() {
            first_regular = Some(field);
        }
    }

    if !opts.empty_line {
        return;
    }
    let (Some(last_emb), Some(first_reg)) = (last_embedded, first_regular) else {
        return;
    };

    // Upstream: nextLine from Field.Doc when present, else field Pos.
    let line = line_of(fset, last_emb.end());
    let next_line = if let Some(doc) = first_reg.doc.as_ref() {
        line_of(fset, doc.pos())
    } else {
        line_of(fset, first_reg.pos())
    };

    if next_line != line + 2 {
        // The insertion point is the first regular field — or its doc comment
        // when it has one. `next_line` above is already computed from exactly
        // that choice; this reuses it rather than deciding twice.
        let insert = first_reg
            .doc
            .as_ref()
            .map(|doc| doc.pos())
            .unwrap_or_else(|| first_reg.pos());
        pending.push((
            last_emb.pos(),
            "there must be an empty line separating embedded fields from regular fields".into(),
            Some(insert),
        ));
    }
}

fn reparse(path: &std::path::Path) -> Option<(Arc<FileSet>, File)> {
    let src = fs::read(path).ok()?;
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, &src, PARSE_COMMENTS).ok()?;
    Some((fset, file))
}

/// Map a reparsed file position onto the pass FileSet via line/column.
fn map_pos(pass: &Pass<'_>, pass_file: &File, re_fset: &FileSet, re_pos: Pos) -> u32 {
    let Some(ft) = pass.fset().file(pass_file.pos()) else {
        return re_pos.0 as u32;
    };
    let re_p = re_fset.position(re_pos);
    if re_p.line < 1 || re_p.line as usize > ft.line_count() {
        return pass_file.pos().0 as u32;
    }
    ft.line_start(re_p.line as usize).0 as u32 + (re_p.column as u32).saturating_sub(1)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "embeddedstructfieldcheck requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<EmbeddedstructfieldcheckOptions>("embeddedstructfieldcheck")
        .cloned()
        .unwrap_or_default();

    let mut pending: Vec<(u32, String, Option<u32>)> = Vec::new();
    let paths = pass.pkg().compiled_go_files.clone();
    let n = pass.files().len();

    for i in 0..n {
        let Some(path) = paths.get(i) else {
            continue;
        };
        let Some((re_fset, parsed)) = reparse(path) else {
            continue;
        };
        let pass_file = &pass.files()[i];
        let mut local: Vec<(Pos, String, Option<Pos>)> = Vec::new();
        preorder(NodeRef::File(&parsed), |n| {
            if let NodeRef::StructType(st) = n {
                analyze_struct(&re_fset, st, &opts, &mut local);
            }
            true
        });
        for (re_pos, message, re_insert) in local {
            // The insertion point is mapped through the same re-parse bridge as
            // the report position: both are positions in the comment-aware
            // FileSet, and an unmapped one would land anywhere.
            let insert = re_insert.map(|p| map_pos(pass, pass_file, &re_fset, p));
            pending.push((map_pos(pass, pass_file, &re_fset, re_pos), message, insert));
        }
    }

    for (pos, message, insert) in pending {
        let Some(at) = insert else {
            pass.reportf(pos, message);
            continue;
        };
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "adding extra line separating embedded fields from regular fields".into(),
                // `NewText: []byte("\n\n")` with no `End` — an empty range, so
                // this inserts rather than replaces. Landing after the field's
                // indentation leaves a stray tab that the post-fix gofmt cleans,
                // exactly as wsl's insert-at-statement-position does.
                text_edits: vec![TextEdit {
                    pos: at,
                    end: at,
                    new_text: "\n\n".into(),
                }],
            }],
            ..Diagnostic::default()
        });
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
