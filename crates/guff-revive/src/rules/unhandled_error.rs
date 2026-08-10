//! `unhandled-error` — warn on unhandled errors returned by function calls.

use guff::ast::{CallExpr, Expr};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;
use guff_analysis::code::{self, type_with_name};
use guff_types::arena::TypeData;
use guff_types::TypeId;

use crate::failure::Failure;
use crate::util::type_of;

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
}

impl<'a> Checker<'a> {
    pub fn try_new(pass: &'a Pass<'a>) -> Option<Self> {
        if pass.types_info().is_none() {
            return None;
        }
        Some(Self {
            pass,
            failures: Vec::new(),
        })
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::ExprStmt(stmt) = n else {
            return;
        };
        let Expr::CallExpr(call) = &stmt.x else {
            return;
        };
        if !callee_is_local(self.pass, call) {
            return;
        }
        if returns_error(self.pass, call) {
            let name = func_name(self.pass, call);
            self.failures.push(Failure {
                rule: "unhandled-error",
                pos: call.fun.pos().0 as u32,
                message: format!("Unhandled error in call to function {name}"),
                ..Failure::default()
            });
        }
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let Some(mut c) = Checker::try_new(pass) else {
        return Vec::new();
    };
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}

/// Whether the callee is declared in the package under analysis.
///
/// Upstream reaches this rule's decision through `w.pkg.TypeOf(fCall)`, and
/// revive type-checks its own packages with `types.Config{Importer:
/// importer.Default()}` — the gc export-data importer, which finds no export
/// data for anything the compiler has not installed. Every import therefore
/// resolves to an invalid package, the *result type* of a call into one is
/// invalid rather than `error` or a tuple, and the rule stays silent. Only
/// calls to functions declared in the package being linted have a result type
/// revive can see. Measured against golangci-lint 2.12.2: `fmt.Print(…)` and
/// `errors.New(…)` as statements are not reported; a local `func() error`
/// called as a statement is.
///
/// guff has real type information for the whole program, so it has to put the
/// boundary back by hand or it reports a superset of what the user sees from
/// golangci-lint.
fn callee_is_local(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Some(obj) = code::call_target_object(pass, &call.fun) else {
        // Calling a func value (a variable, a field, a literal) rather than a
        // declared function. Its signature came from wherever the value's type
        // was declared; upstream sees it only when that is this package, which
        // is also when guff's own resolution above returns None. Keep quiet.
        return false;
    };
    // Compare package *identity*, not the import path string: the object's
    // package comes from the type-checker, and the Package metadata's path is
    // not always the same spelling (it is empty under the unit-test harness).
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    obj.pkg(&artifacts.objects).is_some() && obj.pkg(&artifacts.objects) == pass.pkg().types
}

fn returns_error(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Some(typ) = type_of(pass, &Expr::CallExpr(call.clone())) else {
        return false;
    };
    match result_type_errors(pass, typ) {
        Some(flags) => flags.iter().any(|&b| b),
        None => false,
    }
}

fn result_type_errors(pass: &Pass<'_>, typ: TypeId) -> Option<Vec<bool>> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    match artifacts.types.get(typ) {
        TypeData::Tuple(t) => Some(
            (0..t.len())
                .map(|i| {
                    t.at(i)
                        .typ(&artifacts.objects)
                        .is_some_and(|rt| type_with_name(pass, rt, "error"))
                })
                .collect(),
        ),
        _ => Some(vec![type_with_name(pass, typ, "error")]),
    }
}

/// Upstream `lintUnhandledErrors.funcName`: for a selector call whose selected
/// object is a `*types.Func`, the object's `FullName()` with `(`, `)` and `*`
/// removed; for anything else, the callee as `go/printer` writes it — so a
/// plain call to a package-level function prints as the bare identifier, not
/// as `example.com/pkg.f`.
fn func_name(pass: &Pass<'_>, call: &CallExpr) -> String {
    if let Expr::SelectorExpr(sel) = call.fun.as_ref() {
        if let (Some(info), Some(artifacts)) =
            (pass.types_info(), pass.pkg().type_artifacts.as_ref())
        {
            if let Some(obj) = info.uses.get(&sel.sel.id).copied() {
                if matches!(
                    artifacts.objects.get(obj),
                    guff_types::arena::ObjectData::Func(_)
                ) {
                    let full = code::type_func_name(
                        &artifacts.types,
                        &artifacts.objects,
                        &artifacts.packages,
                        obj,
                    );
                    return full.replace(['(', ')', '*'], "");
                }
            }
        }
    }
    let mut buf: Vec<u8> = Vec::new();
    match guff::printer::fprint(&mut buf, pass.fset(), guff::printer::PrintNode::Expr(&call.fun)) {
        Ok(()) => String::from_utf8(buf).unwrap_or_default(),
        Err(_) => String::new(),
    }
}
