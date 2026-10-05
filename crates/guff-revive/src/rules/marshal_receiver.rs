//! `marshal-receiver` (revive v1.17.0) — `Marshal*` methods should take a value
//! receiver and `Unmarshal*` methods a pointer receiver.

use guff::ast::{Decl, Expr};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::receiver_type_key;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else { continue };
            let Some(recv) = f.recv.as_ref().and_then(|r| r.list.first()) else {
                continue;
            };
            let name = f.name.name.as_str();
            let is_marshal = matches!(name, "MarshalJSON" | "MarshalText" | "MarshalYAML");
            let is_unmarshal =
                !is_marshal && matches!(name, "UnmarshalJSON" | "UnmarshalText" | "UnmarshalYAML");
            if !is_marshal && !is_unmarshal {
                continue;
            }
            let Some(recv_ty) = &recv.ty else { continue };
            let is_ptr = matches!(recv_ty, Expr::StarExpr(_));
            let msg = match (is_marshal, is_ptr) {
                (true, true) => "method should use a value receiver, not a pointer receiver",
                (false, false) => "method should use a pointer receiver, not a value receiver",
                _ => continue,
            };
            failures.push(Failure {
                rule: "marshal-receiver",
                // `Node: decl`: a FuncDecl's Pos() is its `func` keyword.
                pos: f.ty.pos().0 as u32,
                // `typeparams.ReceiverType(fn) + "." + name`.
                message: format!("{}.{name} {msg}", receiver_type_key(recv_ty)),
                ..Failure::default()
            });
        }
    }
    failures
}
