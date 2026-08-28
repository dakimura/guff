//! Port of [`github.com/AlwxSin/noinlineerr`](https://github.com/AlwxSin/noinlineerr)
//! (golangci-lint wrapper in `pkg/golinters/noinlineerr`).
//!
//! Disallows inline error handling using `if err := ...; err != nil {`.
//! Prefer the more explicit two-statement form:
//!
//! ```go
//! err := doSomething()
//! if err != nil {
//!     return err
//! }
//! ```
//!
//! Upstream logic: for every `if` statement whose init clause is an assignment,
//! each left-hand identifier is reported when
//! (1) its type is assignable to the predeclared `error` interface,
//! (2) it is not the blank identifier `_`, and
//! (3) the identifier name appears in the `if` condition.
//!
//! `--fix` ports upstream's two edits — insert the hoisted assignment before the
//! `if`, delete the init clause and its `;` — together with upstream's own two
//! withholding conditions (`len(Lhs) != 1`, and a name already bound in the
//! scope the assignment would land in).
//!
//! guff withholds on one further shape upstream does not guard: `else if`.
//! There `ifStmt.Pos()` sits between `else` and `if`, so upstream's insertion
//! yields `} else err := do()` — `else must be followed by if or statement
//! block`. golangci-lint 2.12.2 logs a gofmt failure, writes the unparseable
//! file anyway, and still reports `0 issues`. golangci/golangci-lint#5905 was
//! closed 2025-06-30 as working-as-intended ("suggested fixes are not
//! guaranteed to produce code that compiles"), so this will not change upstream
//! and guff records it as a deliberate subset — see
//! `compat/fix/divergent/noinlineerr.diff`.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{Expr, Stmt};
use guff::walk::{expr_ref, preorder, NodeRef};
use guff_analysis::code::{object_of, stmt_text, type_with_name};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::api_predicates::api_implements;
use guff_types::arena::ObjectData;
use guff_types::TypeId;

const MESSAGE: &str =
    "avoid inline error handling using `if err := ...; err != nil`; use plain assignment `err := ...`";

fn universe_error(pass: &Pass<'_>) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    for oid in artifacts.objects.ids() {
        let ObjectData::TypeName(tn) = artifacts.objects.get(oid) else {
            continue;
        };
        if tn.name() != "error" {
            continue;
        }
        // The predeclared `error` lives in the universe scope (no package).
        if oid.pkg(&artifacts.objects).is_some() {
            continue;
        }
        return tn.typ();
    }
    None
}

/// Reports whether `typ` is assignable to the predeclared `error` interface.
///
/// `error` is an interface, so `types.AssignableTo(typ, error)` reduces to
/// "`typ` implements `error`".
fn is_error(pass: &Pass<'_>, typ: TypeId) -> bool {
    if type_with_name(pass, typ, "error") {
        return true;
    }
    let Some(err) = universe_error(pass) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        err,
    )
}

/// Reports whether the identifier `ident`'s declared type is assignable to
/// the predeclared `error` interface.
fn ident_is_error(pass: &Pass<'_>, ident: &guff::ast::Ident) -> bool {
    let Some(obj) = object_of(pass, ident) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(typ) = obj.typ(&artifacts.objects) else {
        return false;
    };
    is_error(pass, typ)
}

/// Reports whether the identifier `name` appears anywhere in `cond`.
fn error_used_in_condition(cond: &Expr, name: &str) -> bool {
    let mut used = false;
    preorder(expr_ref(cond), |n| {
        if let NodeRef::Ident(id) = n {
            if id.name == name {
                used = true;
                return false;
            }
        }
        true
    });
    used
}

/// Node ids of `if` statements that are the `else` branch of another `if`.
///
/// Collected up front because the fix's insertion point depends on what sits
/// immediately before the `if` keyword, which the node itself does not record.
fn else_if_ids(file: &guff::ast::File) -> HashSet<u32> {
    let mut ids = HashSet::new();
    preorder(NodeRef::File(file), |n| {
        if let NodeRef::IfStmt(outer) = n {
            if let Some(Stmt::IfStmt(inner)) = outer.else_.as_deref() {
                ids.insert(inner.id);
            }
        }
        true
    });
    ids
}

/// Upstream's `shadowVarsExists`: is `name` already bound in the **immediate
/// parent** of the `if`'s own scope?
///
/// That parent is exactly where the hoisted `err :=` lands, and `:=` only
/// conflicts within a single scope, so — like `types.Scope.Lookup` — this does
/// not walk ancestors. `lookup_local`, not `lookup_chain`.
fn shadow_vars_exists(pass: &Pass<'_>, if_id: u32, name: &str) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(&scope) = artifacts.info.scopes.get(&if_id) else {
        return false;
    };
    let Some(parent) = artifacts.scopes.get(scope).parent() else {
        return false;
    };
    artifacts.scopes.get(parent).lookup_local(name).is_some()
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "noinlineerr requires inspect analyzer".to_string())?;

    let mut pending: Vec<(u32, Vec<TextEdit>)> = Vec::new();
    for file in pass.files() {
        let else_ifs = else_if_ids(file);
        preorder(NodeRef::File(file), |n| {
            let NodeRef::IfStmt(if_stmt) = n else {
                return true;
            };
            let Some(init) = if_stmt.init.as_ref() else {
                return true;
            };
            let Stmt::AssignStmt(assign) = init.as_ref() else {
                return true;
            };
            for lhs in &assign.lhs {
                let Expr::Ident(ident) = lhs else {
                    continue;
                };
                if ident.name == "_" {
                    continue;
                }
                if !error_used_in_condition(&if_stmt.cond, &ident.name) {
                    continue;
                }
                if !ident_is_error(pass, ident) {
                    continue;
                }
                // Upstream reports without a fix and *returns* here, so a second
                // error-typed name on the left is never reached.
                if assign.lhs.len() != 1 || shadow_vars_exists(pass, if_stmt.id, &ident.name) {
                    pending.push((ident.pos().0 as u32, Vec::new()));
                    break;
                }
                // guff-only: see the module comment. Upstream emits its fix here
                // and writes a file that does not parse.
                if else_ifs.contains(&if_stmt.id) {
                    pending.push((ident.pos().0 as u32, Vec::new()));
                    continue;
                }
                let Some(assign_text) = stmt_text(pass, init) else {
                    pending.push((ident.pos().0 as u32, Vec::new()));
                    continue;
                };
                pending.push((
                    ident.pos().0 as u32,
                    vec![
                        TextEdit {
                            pos: if_stmt.if_.0 as u32,
                            end: if_stmt.if_.0 as u32,
                            new_text: format!("{assign_text}\n"),
                        },
                        TextEdit {
                            pos: init.pos().0 as u32,
                            // +1 for the `;` that separates init from cond.
                            end: (init.end().0 + 1) as u32,
                            new_text: String::new(),
                        },
                    ],
                ));
            }
            true
        });
    }

    for (pos, edits) in pending {
        if edits.is_empty() {
            pass.reportf(pos, MESSAGE);
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message: MESSAGE.to_string(),
            suggested_fixes: vec![SuggestedFix {
                message: "move err assignment outside if".to_string(),
                text_edits: edits,
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "noinlineerr",
        doc: "Disallows inline error handling (`if err := ...; err != nil {`).",
        url: "https://github.com/AlwxSin/noinlineerr",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
