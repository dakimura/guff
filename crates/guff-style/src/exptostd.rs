//! Port of [`github.com/ldez/exptostd`](https://github.com/ldez/exptostd).
//!
//! Detects `golang.org/x/exp/{maps,slices,constraints}` usages that can be
//! replaced by the Go standard library (`maps`, `slices`, `cmp`).
//!
//! `maps.Keys` / `maps.Values` rewrite to `slices.AppendSeq(make([]FIXME, 0,
//! len(m)), maps.Keys(m))`. The `FIXME` is upstream's own — it carries
//! `// TODO(ldez) improve the type detection.` beside it (exptostd.go:376) —
//! so the rewritten tree does not compile. That is upstream's output, and the
//! fix tier records such trees rather than refusing them
//! (`compat/fix/README.md`); reproducing it is what compatibility means here.
//!
//! Note the message and the fix differ for these two, which is unusual: the
//! message keeps upstream's documentation placeholders (`[]T`, `data`) while
//! the fix uses `FIXME` and the call's real argument.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{
    BasicLit, BinaryExpr, CallExpr, Expr, FuncDecl, SelectorExpr, TypeSpec, UnaryExpr,
};
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::arena::ObjectData;

const PKG_EXP_MAPS: &str = "golang.org/x/exp/maps";
const PKG_EXP_SLICES: &str = "golang.org/x/exp/slices";
const PKG_EXP_CONSTRAINTS: &str = "golang.org/x/exp/constraints";

const PKG_MAPS: &str = "maps";
const PKG_SLICES: &str = "slices";
const PKG_CMP: &str = "cmp";

const GO121: u32 = 121;
const GO123: u32 = 123;

#[derive(Clone, Copy)]
struct CallReplacement {
    min_go: u32,
    text: &'static str,
    /// `"clear"` → rewrite call fun to `clear`; others have message-only / deferred fixes.
    kind: CallFixKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CallFixKind {
    None,
    Clear,
    /// `slices.AppendSeq(make([]FIXME, 0, len($arg)), <original call>)`
    /// — `suggestedFixForKeysOrValues` (exptostd.go:364).
    AppendSeq,
}

#[derive(Clone, Copy)]
struct ConstraintReplacement {
    min_go: u32,
    text: &'static str,
}

fn maps_replacements() -> &'static HashMap<&'static str, CallReplacement> {
    static M: OnceLock<HashMap<&'static str, CallReplacement>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            (
                "Keys",
                CallReplacement {
                    min_go: GO123,
                    text: "slices.AppendSeq(make([]T, 0, len(data)), maps.Keys(data))",
                    kind: CallFixKind::AppendSeq,
                },
            ),
            (
                "Values",
                CallReplacement {
                    min_go: GO123,
                    text: "slices.AppendSeq(make([]T, 0, len(data)), maps.Values(data))",
                    kind: CallFixKind::AppendSeq,
                },
            ),
            (
                "Equal",
                CallReplacement {
                    min_go: GO121,
                    text: "maps.Equal()",
                    kind: CallFixKind::None,
                },
            ),
            (
                "EqualFunc",
                CallReplacement {
                    min_go: GO121,
                    text: "maps.EqualFunc()",
                    kind: CallFixKind::None,
                },
            ),
            (
                "Clone",
                CallReplacement {
                    min_go: GO121,
                    text: "maps.Clone()",
                    kind: CallFixKind::None,
                },
            ),
            (
                "Copy",
                CallReplacement {
                    min_go: GO121,
                    text: "maps.Copy()",
                    kind: CallFixKind::None,
                },
            ),
            (
                "DeleteFunc",
                CallReplacement {
                    min_go: GO121,
                    text: "maps.DeleteFunc()",
                    kind: CallFixKind::None,
                },
            ),
            (
                "Clear",
                CallReplacement {
                    min_go: GO121,
                    text: "clear()",
                    kind: CallFixKind::Clear,
                },
            ),
        ])
    })
}

fn slices_replacements() -> &'static HashMap<&'static str, CallReplacement> {
    static M: OnceLock<HashMap<&'static str, CallReplacement>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([
            ("Equal", CallReplacement { min_go: GO121, text: "slices.Equal()", kind: CallFixKind::None }),
            ("EqualFunc", CallReplacement { min_go: GO121, text: "slices.EqualFunc()", kind: CallFixKind::None }),
            ("Compare", CallReplacement { min_go: GO121, text: "slices.Compare()", kind: CallFixKind::None }),
            ("CompareFunc", CallReplacement { min_go: GO121, text: "slices.CompareFunc()", kind: CallFixKind::None }),
            ("Index", CallReplacement { min_go: GO121, text: "slices.Index()", kind: CallFixKind::None }),
            ("IndexFunc", CallReplacement { min_go: GO121, text: "slices.IndexFunc()", kind: CallFixKind::None }),
            ("Contains", CallReplacement { min_go: GO121, text: "slices.Contains()", kind: CallFixKind::None }),
            ("ContainsFunc", CallReplacement { min_go: GO121, text: "slices.ContainsFunc()", kind: CallFixKind::None }),
            ("Insert", CallReplacement { min_go: GO121, text: "slices.Insert()", kind: CallFixKind::None }),
            ("Delete", CallReplacement { min_go: GO121, text: "slices.Delete()", kind: CallFixKind::None }),
            ("DeleteFunc", CallReplacement { min_go: GO121, text: "slices.DeleteFunc()", kind: CallFixKind::None }),
            ("Replace", CallReplacement { min_go: GO121, text: "slices.Replace()", kind: CallFixKind::None }),
            ("Clone", CallReplacement { min_go: GO121, text: "slices.Clone()", kind: CallFixKind::None }),
            ("Compact", CallReplacement { min_go: GO121, text: "slices.Compact()", kind: CallFixKind::None }),
            ("CompactFunc", CallReplacement { min_go: GO121, text: "slices.CompactFunc()", kind: CallFixKind::None }),
            ("Grow", CallReplacement { min_go: GO121, text: "slices.Grow()", kind: CallFixKind::None }),
            ("Clip", CallReplacement { min_go: GO121, text: "slices.Clip()", kind: CallFixKind::None }),
            ("Reverse", CallReplacement { min_go: GO121, text: "slices.Reverse()", kind: CallFixKind::None }),
            ("Sort", CallReplacement { min_go: GO121, text: "slices.Sort()", kind: CallFixKind::None }),
            ("SortFunc", CallReplacement { min_go: GO121, text: "slices.SortFunc()", kind: CallFixKind::None }),
            ("SortStableFunc", CallReplacement { min_go: GO121, text: "slices.SortStableFunc()", kind: CallFixKind::None }),
            ("IsSorted", CallReplacement { min_go: GO121, text: "slices.IsSorted()", kind: CallFixKind::None }),
            ("IsSortedFunc", CallReplacement { min_go: GO121, text: "slices.IsSortedFunc()", kind: CallFixKind::None }),
            ("Min", CallReplacement { min_go: GO121, text: "slices.Min()", kind: CallFixKind::None }),
            ("MinFunc", CallReplacement { min_go: GO121, text: "slices.MinFunc()", kind: CallFixKind::None }),
            ("Max", CallReplacement { min_go: GO121, text: "slices.Max()", kind: CallFixKind::None }),
            ("MaxFunc", CallReplacement { min_go: GO121, text: "slices.MaxFunc()", kind: CallFixKind::None }),
            ("BinarySearch", CallReplacement { min_go: GO121, text: "slices.BinarySearch()", kind: CallFixKind::None }),
            ("BinarySearchFunc", CallReplacement { min_go: GO121, text: "slices.BinarySearchFunc()", kind: CallFixKind::None }),
        ])
    })
}

fn constraints_replacements() -> &'static HashMap<&'static str, ConstraintReplacement> {
    static M: OnceLock<HashMap<&'static str, ConstraintReplacement>> = OnceLock::new();
    M.get_or_init(|| {
        HashMap::from([(
            "Ordered",
            ConstraintReplacement {
                min_go: GO121,
                text: "cmp.Ordered",
            },
        )])
    })
}

struct Pending {
    pos: u32,
    end: u32,
    message: String,
    fixes: Vec<SuggestedFix>,
}

struct ExpPkgState {
    should_keep_import: bool,
    diagnostics: Vec<Pending>,
}

impl ExpPkgState {
    fn new() -> Self {
        Self {
            should_keep_import: false,
            diagnostics: Vec::new(),
        }
    }
}

fn go_version_num(pass: &Pass<'_>) -> u32 {
    let raw = code::module_go_version(pass);
    let raw = raw.strip_prefix("go").unwrap_or(&raw);
    let end = raw
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(raw.len());
    let raw = &raw[..end];
    let parts: Vec<&str> = raw.split('.').collect();
    let major: u32 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(1);
    let minor: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    major * 100 + minor
}

fn trim_import_path(lit: &BasicLit) -> String {
    let v = lit.value.as_str();
    if v.len() >= 2 {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

fn pkg_path_of_ident(pass: &Pass<'_>, ident: &guff::ast::Ident) -> Option<String> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let obj_id = info
        .defs
        .get(&ident.id)
        .and_then(|o| *o)
        .or_else(|| info.uses.get(&ident.id).copied())?;
    let ObjectData::PkgName(pn) = artifacts.objects.get(obj_id) else {
        return None;
    };
    Some(artifacts.packages.get(pn.imported()).path().to_string())
}

fn is_exp_pkg(pass: &Pass<'_>, ident: &guff::ast::Ident, import_path: &str) -> bool {
    pkg_path_of_ident(pass, ident).as_deref() == Some(import_path)
}

/// Structural text for the pieces the `AppendSeq` fix reassembles.
///
/// Upstream prints its rebuilt call with `printer.Fprint(buf,
/// token.NewFileSet(), s)` — a *fresh* FileSet — so the output is structural
/// rather than a slice of the original source. Only the shapes that reach this
/// fix are handled; anything else yields `None`, and the finding is reported
/// without a fix rather than with a guess.
///
/// Local on purpose: `expr_text` exists elsewhere in this crate with different
/// jobs, and folding them together swaps a faithful port for an approximation
/// of a different one (続き 62).
fn expr_text(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::BasicLit(lit) => Some(lit.value.clone()),
        Expr::SelectorExpr(sel) => Some(format!("{}.{}", expr_text(&sel.x)?, sel.sel.name)),
        _ => None,
    }
}

/// The call's arguments, comma-joined — what upstream feeds to `len(...)`.
fn call_args_text(call: &CallExpr) -> Option<String> {
    let parts: Option<Vec<String>> = call.args.iter().map(expr_text).collect();
    Some(parts?.join(", "))
}

/// The whole call, as `slices.AppendSeq`'s second argument.
fn call_text(call: &CallExpr) -> Option<String> {
    Some(format!("{}({})", expr_text(&call.fun)?, call_args_text(call)?))
}

fn call_diagnostic(
    call: &CallExpr,
    import_path: &str,
    sel_name: &str,
    rp: &CallReplacement,
) -> Pending {
    let mut fixes = Vec::new();
    if rp.kind == CallFixKind::AppendSeq {
        // Upstream rebuilds the call and prints it with a fresh FileSet
        // (exptostd.go:364), so the shape is structural: `len()` takes the
        // original call's arguments, and the whole call becomes the second
        // argument of `slices.AppendSeq`.
        if let (Some(args), Some(whole)) = (call_args_text(call), call_text(call)) {
            fixes.push(SuggestedFix {
                message: String::new(),
                text_edits: vec![TextEdit {
                    pos: call.pos().0 as u32,
                    end: call.end().0 as u32,
                    new_text: format!(
                        "slices.AppendSeq(make([]FIXME, 0, len({args})), {whole})"
                    ),
                }],
            });
        }
    }
    if rp.kind == CallFixKind::Clear {
        fixes.push(SuggestedFix {
            message: "Replace with clear(...)".into(),
            text_edits: vec![TextEdit {
                pos: call.fun.pos().0 as u32,
                end: call.fun.end().0 as u32,
                new_text: "clear".into(),
            }],
        });
    }
    Pending {
        pos: call.pos().0 as u32,
        end: call.end().0 as u32,
        message: format!("{import_path}.{sel_name}() can be replaced by {}", rp.text),
        fixes,
    }
}

fn detect_call_usage(
    pass: &Pass<'_>,
    replacements: &HashMap<&'static str, CallReplacement>,
    sel: &SelectorExpr,
    call: &CallExpr,
    import_path: &str,
    go_version: u32,
    state: &mut ExpPkgState,
    report_immediately: bool,
    pending_out: &mut Vec<Pending>,
) {
    let Expr::Ident(ident) = &*sel.x else {
        return;
    };
    if !is_exp_pkg(pass, ident, import_path) {
        return;
    }

    let Some(rp) = replacements.get(sel.sel.name.as_str()) else {
        state.should_keep_import = true;
        return;
    };
    if rp.min_go > go_version {
        state.should_keep_import = true;
        return;
    }

    let diag = call_diagnostic(call, import_path, &sel.sel.name, rp);
    if report_immediately {
        pending_out.push(diag);
    } else {
        state.diagnostics.push(diag);
    }
}

fn constraint_diagnostic(sel: &SelectorExpr, rp: &ConstraintReplacement) -> Pending {
    let pos = sel.x.pos().0 as u32;
    let end = sel.sel.end().0 as u32;
    Pending {
        pos,
        end,
        message: format!(
            "{PKG_EXP_CONSTRAINTS}.{} can be replaced by {}",
            sel.sel.name, rp.text
        ),
        fixes: vec![SuggestedFix {
            message: format!("Replace with {}", rp.text),
            text_edits: vec![TextEdit {
                pos,
                end,
                new_text: rp.text.into(),
            }],
        }],
    }
}

fn detect_constraints_usage(
    pass: &Pass<'_>,
    expr: &Expr,
    state: &mut ExpPkgState,
    go_version: u32,
    pending_out: &mut Vec<Pending>,
) {
    match expr {
        Expr::SelectorExpr(sel) => {
            let Expr::Ident(ident) = &*sel.x else {
                return;
            };
            if !is_exp_pkg(pass, ident, PKG_EXP_CONSTRAINTS) {
                return;
            }
            let Some(rp) = constraints_replacements().get(sel.sel.name.as_str()) else {
                state.should_keep_import = true;
                return;
            };
            if rp.min_go > go_version {
                state.should_keep_import = true;
                return;
            }
            pending_out.push(constraint_diagnostic(sel, rp));
        }
        Expr::BinaryExpr(BinaryExpr { x, y, .. }) => {
            detect_constraints_usage(pass, x, state, go_version, pending_out);
            detect_constraints_usage(pass, y, state, go_version, pending_out);
        }
        Expr::UnaryExpr(UnaryExpr { x, .. }) => {
            detect_constraints_usage(pass, x, state, go_version, pending_out);
        }
        _ => {}
    }
}

fn suggest_replace_import(
    imports: &HashMap<String, (u32, u32, String)>,
    should_keep: bool,
    import_path: &str,
    std_package: &str,
    pending_out: &mut Vec<Pending>,
) {
    if should_keep {
        return;
    }
    let Some(&(pos, end, ref quoted)) = imports.get(import_path) else {
        return;
    };
    let quote = quoted.chars().next().unwrap_or('"');
    let new_text = format!("{quote}{std_package}{quote}");
    pending_out.push(Pending {
        pos,
        end,
        message: format!("Import statement '{import_path}' may be replaced by '{std_package}'"),
        fixes: vec![SuggestedFix {
            message: format!("Replace import with {std_package}"),
            text_edits: vec![TextEdit {
                pos,
                end,
                new_text,
            }],
        }],
    });
}

fn check_type_params(
    pass: &Pass<'_>,
    type_params: Option<&guff::ast::FieldList>,
    constraints_state: &mut ExpPkgState,
    go_version: u32,
    pending_out: &mut Vec<Pending>,
) {
    let Some(fields) = type_params else {
        return;
    };
    for field in &fields.list {
        if let Some(ty) = &field.ty {
            detect_constraints_usage(pass, ty, constraints_state, go_version, pending_out);
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "exptostd requires inspect analyzer".to_string())?;

    let go_version = go_version_num(pass);
    let mut imports: HashMap<String, (u32, u32, String)> = HashMap::new();
    let mut maps_state = ExpPkgState::new();
    let mut slices_state = ExpPkgState::new();
    let mut constraints_state = ExpPkgState::new();
    let mut pending = Vec::new();

    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::ImportSpec(sp) => {
                    // Upstream only tracks non-aliased imports for rewrite.
                    if sp.name.is_none() {
                        let path = trim_import_path(&sp.path);
                        imports.insert(
                            path,
                            (
                                sp.path.pos().0 as u32,
                                sp.path.end().0 as u32,
                                sp.path.value.clone(),
                            ),
                        );
                    }
                }
                NodeRef::CallExpr(call) => {
                    let Expr::SelectorExpr(sel) = &*call.fun else {
                        return true;
                    };
                    let Expr::Ident(ident) = &*sel.x else {
                        return true;
                    };
                    match ident.name.as_str() {
                        "maps" => detect_call_usage(
                            pass,
                            maps_replacements(),
                            sel,
                            call,
                            PKG_EXP_MAPS,
                            go_version,
                            &mut maps_state,
                            true,
                            &mut pending,
                        ),
                        "slices" => detect_call_usage(
                            pass,
                            slices_replacements(),
                            sel,
                            call,
                            PKG_EXP_SLICES,
                            go_version,
                            &mut slices_state,
                            false,
                            &mut pending,
                        ),
                        _ => {}
                    }
                }
                NodeRef::FuncDecl(FuncDecl { ty, .. }) => {
                    check_type_params(
                        pass,
                        ty.type_params.as_ref(),
                        &mut constraints_state,
                        go_version,
                        &mut pending,
                    );
                }
                NodeRef::TypeSpec(TypeSpec {
                    type_params, ty, ..
                }) => {
                    check_type_params(
                        pass,
                        type_params.as_ref(),
                        &mut constraints_state,
                        go_version,
                        &mut pending,
                    );
                    if let Expr::InterfaceType(iface) = ty {
                        for method in &iface.methods.list {
                            if let Some(mty) = &method.ty {
                                match mty {
                                    Expr::BinaryExpr(BinaryExpr { x, y, .. }) => {
                                        detect_constraints_usage(
                                            pass,
                                            x,
                                            &mut constraints_state,
                                            go_version,
                                            &mut pending,
                                        );
                                        detect_constraints_usage(
                                            pass,
                                            y,
                                            &mut constraints_state,
                                            go_version,
                                            &mut pending,
                                        );
                                    }
                                    Expr::SelectorExpr(_) => {
                                        detect_constraints_usage(
                                            pass,
                                            mty,
                                            &mut constraints_state,
                                            go_version,
                                            &mut pending,
                                        );
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
            true
        });
    }

    suggest_replace_import(
        &imports,
        maps_state.should_keep_import,
        PKG_EXP_MAPS,
        PKG_MAPS,
        &mut pending,
    );

    // Upstream: when every slices usage is replaceable, only rewrite the import
    // (APIs are 1:1). Otherwise emit per-call diagnostics and keep the import.
    if slices_state.should_keep_import {
        pending.append(&mut slices_state.diagnostics);
    } else {
        suggest_replace_import(
            &imports,
            slices_state.should_keep_import,
            PKG_EXP_SLICES,
            PKG_SLICES,
            &mut pending,
        );
    }

    suggest_replace_import(
        &imports,
        constraints_state.should_keep_import,
        PKG_EXP_CONSTRAINTS,
        PKG_CMP,
        &mut pending,
    );

    for p in pending {
        pass.report(Diagnostic {
            pos: p.pos,
            end: p.end,
            category: String::new(),
            message: p.message,
            severity: String::new(),
            url: String::new(),
            suggested_fixes: p.fixes,
            related: Vec::new(),
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "exptostd",
        doc: "Detects functions from golang.org/x/exp/ that can be replaced by std functions.",
        url: "https://github.com/ldez/exptostd",
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
    fn go_version_encoding() {
        assert_eq!(GO121, 121);
        assert_eq!(GO123, 123);
    }

    #[test]
    fn maps_table_has_clear() {
        assert_eq!(
            maps_replacements().get("Clear").map(|r| r.kind),
            Some(CallFixKind::Clear)
        );
    }
}
