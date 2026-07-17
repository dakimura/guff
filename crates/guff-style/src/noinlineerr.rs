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
//! DEFERRED: SuggestedFix (upstream's `--fix` is known to break compilation, see
//! golangci/golangci-lint#5905). guff reports only.

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::walk::{expr_ref, preorder, NodeRef};
use guff_analysis::code::{object_of, type_with_name};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
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

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "noinlineerr requires inspect analyzer".to_string())?;

    let mut pending: Vec<u32> = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            let NodeRef::IfStmt(if_stmt) = n else {
                return true;
            };
            let Some(init) = if_stmt.init.as_ref() else {
                return true;
            };
            let guff::ast::Stmt::AssignStmt(assign) = init.as_ref() else {
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
                if ident_is_error(pass, ident) {
                    pending.push(ident.pos().0 as u32);
                }
            }
            true
        });
    }

    for pos in pending {
        pass.reportf(pos, MESSAGE);
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
