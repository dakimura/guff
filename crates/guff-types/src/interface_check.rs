//! Port of the `Checker.interfaceType` builder from `go/types/interface.go`
//! (`cmd/compile/internal/types2/interface.go`).
//!
//! **Chunk 33b**: builds an `Interface` type from an `interface { ... }` type
//! expression — explicit methods plus embedded elements (interfaces, and
//! `~T`/`A | B` type-constraint unions). Wired into [`Checker::typ`] so
//! interface type expressions and `type T interface{...}` resolve.
//!
//! ## Deferrals (chunk-33b, see §8)
//!
//! - interface-method receivers are left unset (`sig.recv == None`); Go sets a
//!   `RecvVar` of the interface/named type for error messages and method-set
//!   semantics. Our `implements`/`missingMethod` compare method signatures
//!   ignoring the receiver, so this is safe for now.
//! - `sortMethods` (API-stability sort) is skipped; method dedup is by
//!   `Object.id` in `compute_interface_type_set`.
//! - method type-parameter rejection, `Info` recording, and `def`-based
//!   receiver naming are omitted.

use guff::ast::{Expr, InterfaceType};
use guff::token::Token;
use guff_types_errors::Code;

use crate::arena::TypeData;
use crate::check::Checker;
use crate::interface::{interface_compute_typeset, new_interface_type};
use crate::object::func::new_func;
use crate::predicates::is_valid;
use crate::union::{new_term, new_union};
use crate::{ObjectId, TypeId};

impl Checker {
    /// Build an `Interface` type from an `interface { ... }` type expression.
    ///
    /// Equivalent to `Checker.interfaceType` (minus the deferrals above).
    pub fn interface_type(&mut self, e: &InterfaceType) -> TypeId {
        let mut methods: Vec<ObjectId> = Vec::new();
        let mut embeddeds: Vec<TypeId> = Vec::new();

        for f in &e.methods.list {
            if f.names.is_empty() {
                // Embedded element: an interface, or a type-constraint union.
                if let Some(te) = &f.ty {
                    let emb = self.parse_interface_embedded(te);
                    embeddeds.push(emb);
                }
                continue;
            }

            // Named method.
            let name = &f.names[0];
            if name.name == "_" {
                self.error(
                    name.pos().0 as u32,
                    Code::BlankIfaceMethod,
                    "methods must have a unique non-blank name",
                );
                continue;
            }
            let typ = match &f.ty {
                Some(t) => self.typ(t),
                None => continue,
            };
            if !matches!(self.types.get(typ), TypeData::Signature(_)) {
                if is_valid(&self.types, typ) {
                    self.error(
                        f.ty.as_ref().map(|t| t.pos().0 as u32).unwrap_or(0),
                        Code::InvalidSyntaxTree,
                        "is not a method signature",
                    );
                }
                continue;
            }
            // DEFERRED: set sig.recv to a RecvVar of the interface/named type.
            let m = new_func(&mut self.objects, name.name.clone(), Some(typ));
            m.set_pkg(&mut self.objects, self.pkg);
            methods.push(m);
        }

        let iface = new_interface_type(&mut self.types, methods, embeddeds);
        // Compute the type set now so any errors surface here (Go defers this
        // with `later`; we compute eagerly since we run serially).
        interface_compute_typeset(&mut self.types, &self.objects, &self.packages, iface);
        iface
    }

    /// Parse an embedded interface element: a plain type, or a type-constraint
    /// union `~T | U | ...`. A single non-`~` term yields that type directly;
    /// otherwise a `Union` is built. Simplified `parseUnion`.
    fn parse_interface_embedded(&mut self, e: &Expr) -> TypeId {
        let mut terms: Vec<(bool, &Expr)> = Vec::new();
        collect_union_terms(e, &mut terms);

        if terms.len() == 1 && !terms[0].0 {
            // Plain embedded type (e.g. another interface).
            return self.typ(terms[0].1);
        }

        let mut union_terms = Vec::with_capacity(terms.len());
        for (tilde, te) in terms {
            let t = self.typ(te);
            union_terms.push(new_term(tilde, t));
        }
        new_union(&mut self.types, union_terms)
    }
}

/// Collect the `|`-separated terms of a type-constraint element, recording for
/// each whether it carried a leading `~`.
fn collect_union_terms<'a>(e: &'a Expr, out: &mut Vec<(bool, &'a Expr)>) {
    match e {
        Expr::BinaryExpr(b) if b.op == Token::OR => {
            collect_union_terms(&b.x, out);
            collect_union_terms(&b.y, out);
        }
        Expr::UnaryExpr(u) if u.op == Token::TILDE => {
            out.push((true, &u.x));
        }
        other => out.push((false, other)),
    }
}
