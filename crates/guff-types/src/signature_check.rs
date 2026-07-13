//! Checker portion of `signature.go` — building a `Signature` type from a
//! `FuncType` AST node (`go/ast`-shaped).
//!
//! **Chunk 24**: [`Checker::func_type`] (`Checker.funcType`), plus the helpers
//! `collect_params` (`collectParams`) and `collect_recv` (a simplified
//! `collectRecv`). Kept in its own `check_*`-style module so `signature.rs`
//! stays a pure data-structure port.
//!
//! ## Deferrals (chunk-24, see §8)
//!
//! - **Scopes**: the function scope (`openScope`/`closeScope`, `sig.scope`,
//!   parameter declaration via `declare`/`declareParams`) is not created.
//!   Parameters are built as `Var`s but not declared into any scope — the
//!   `Signature` struct has no `scope` field, and body-checking (stmt.go,
//!   chunk 30) is what needs declared parameters. → D20.
//! - **Generics**: function/method type parameters (`collectTypeParams`) and
//!   receiver type parameters (`rparams`, `collectRecv`'s unpacking of
//!   `T[P]`) are deferred; a method with a type-parameter list still reports
//!   `InvalidMethodTypeParams`.
//! - `recordImplicit`/`recordTypeAndValue` are no-ops.
//! - `varType`'s constraint-interface rejection is not applied (we call `typ`).

use guff::ast::{Expr, Field, FieldList, FuncType, Ident};
use guff_types_errors::Code;

use crate::arena::{ObjectData, TypeData};
use crate::check::Checker;
use crate::instantiate::instantiate;
use crate::object::var::{new_param, VarKind};
use crate::pointer::new_pointer;
use crate::predicates::is_valid;
use crate::signature::{
    new_signature_type, signature_set_recv_type_params, signature_set_type_params,
};
use crate::slice::new_slice;
use crate::stmt::unparen;
use crate::subst::{make_subst_map, subst};
use crate::tuple::new_tuple;
use crate::typelists::bind_tparams;
use crate::typeparam::{set_constraint, type_param_constraint};
use crate::{ObjectId, TypeId, TypeParamList};

impl Checker {
    /// Build a `Signature` type from an optional receiver field list and a
    /// function-type AST node.
    ///
    /// Equivalent to `Checker.funcType` (acyclic, non-generic subset). Returns
    /// the new `Signature`'s `TypeId`.
    pub fn func_type(&mut self, recv_par: Option<&FieldList>, ftyp: &FuncType) -> TypeId {
        // Open a function scope so type parameters (and, in the body, the named
        // parameters) resolve without leaking into the enclosing (file) scope.
        // The signature carries no scope field (D20), so this scope is used
        // only transiently for resolving parameter types; `func_body` opens a
        // fresh scope and re-declares the parameters and type parameters.
        self.open_scope(ftyp.func.0 as u32, 0, "function");

        // Collect the method receiver, if any.
        let mut recv: Option<ObjectId> = None;
        let mut rparams: Option<TypeParamList> = None;
        if let Some(rp) = recv_par {
            if !rp.list.is_empty() {
                if rp.list.len() > 1 {
                    let at = rp.list[rp.list.len() - 1].pos().0 as u32;
                    self.error(at, Code::InvalidRecv, "method has multiple receivers");
                    // continue with the first one
                }
                let (r, rp_list) = self.collect_recv(&rp.list[0]);
                recv = r;
                rparams = rp_list;
            }
        }

        // Collect and declare function type parameters (chunk 35c). Methods may
        // not have type parameters (the parser usually catches this, but we
        // diagnose it defensively).
        let mut tparams: Option<crate::TypeParamList> = None;
        if let Some(tp) = ftyp.type_params.as_ref().filter(|fl| !fl.list.is_empty()) {
            if recv_par.is_some() {
                self.error(
                    tp.pos().0 as u32,
                    Code::InvalidMethodTypeParams,
                    "methods cannot have type parameters",
                );
            }
            let (ids, field_of) = self.declare_type_params(tp);
            if !ids.is_empty() {
                let tlist = bind_tparams(&mut self.types, ids.clone())
                    .expect("non-empty type-parameter list");
                // A function bound cannot reference the function itself, so the
                // order relative to bound resolution is immaterial here.
                self.resolve_type_param_bounds(tp, &ids, &field_of);
                tparams = Some(tlist);
            }
        }
        // DEFERRED: receiver type parameters (`rparams` from a `T[P]` receiver).

        // Collect ordinary parameters and results (type parameters now in scope).
        let (params, variadic) = self.collect_params(VarKind::Param, ftyp.params.as_ref());
        let (results, _) = self.collect_params(VarKind::Result, ftyp.results.as_ref());

        self.close_scope();

        // DEFERRED: declare named receiver/params/results into the func scope
        // (done in func_body instead).

        let params_tuple = new_tuple(&mut self.types, &params);
        let results_tuple = new_tuple(&mut self.types, &results);
        let sig = new_signature_type(
            &mut self.types,
            recv,
            &[],
            &[],
            params_tuple,
            results_tuple,
            variadic,
        );
        if let Some(tl) = tparams {
            signature_set_type_params(&mut self.types, sig, tl);
        }
        if let Some(rl) = rparams {
            signature_set_recv_type_params(&mut self.types, sig, rl);
        }
        sig
    }

    /// Collect the parameters (or results) described by `list`, returning the
    /// `Var`s and whether the list is variadic.
    ///
    /// Equivalent to `Checker.collectParams`. The variadic last parameter's
    /// type is wrapped as `[]T` here (Go wraps it after the loop).
    fn collect_params(&mut self, kind: VarKind, list: Option<&FieldList>) -> (Vec<ObjectId>, bool) {
        let list = match list {
            Some(l) => l,
            None => return (Vec::new(), false),
        };

        let mut params: Vec<ObjectId> = Vec::new();
        let mut variadic = false;
        let mut named = false;
        let mut anonymous = false;
        let n = list.list.len();

        for (i, field) in list.list.iter().enumerate() {
            // Unwrap a trailing `...T` (Ellipsis); detect variadic.
            let mut is_variadic_field = false;
            let ftype: Option<Expr> = match &field.ty {
                Some(Expr::Ellipsis(el)) => {
                    if kind == VarKind::Param && i == n - 1 && field.names.len() <= 1 {
                        variadic = true;
                        is_variadic_field = true;
                    } else {
                        let at = el.ellipsis.0 as u32;
                        self.error(at, Code::InvalidSyntaxTree, "invalid use of ...");
                    }
                    el.elt.as_deref().cloned()
                }
                other => other.clone(),
            };

            let base = match &ftype {
                Some(e) => self.typ(e),
                None => self.invalid_type(),
            };
            // For the variadic parameter, the Var's type is `[]base`.
            let typ = if is_variadic_field {
                new_slice(&mut self.types, base)
            } else {
                base
            };

            if !field.names.is_empty() {
                for name in &field.names {
                    if name.name.is_empty() {
                        self.error(
                            name.pos().0 as u32,
                            Code::InvalidSyntaxTree,
                            "anonymous parameter",
                        );
                    }
                    let par =
                        self.new_param_var(name.name.clone(), typ, kind, name.pos().0 as u32);
                    // Record the parameter/result definition (Go's declareParams
                    // → declare → recordDef). Recorded for `_` too.
                    self.record_def(name, Some(par));
                    params.push(par);
                }
                named = true;
            } else {
                let par = self.new_param_var(String::new(), typ, kind, field.pos().0 as u32);
                // Record the anonymous parameter/result's implicit Var on its
                // field node (Go `recordImplicit(field, par)`).
                self.record_implicit(field.id, par);
                params.push(par);
                anonymous = true;
            }
        }

        if named && anonymous {
            self.error(
                list.pos().0 as u32,
                Code::InvalidSyntaxTree,
                "list contains both named and anonymous parameters",
            );
        }

        (params, variadic)
    }

    /// Collect a method receiver `Var` and, for a parameterized receiver
    /// (`func (r T[P]) M()`), its receiver type-parameter list.
    ///
    /// Simplified `Checker.collectRecv`. For a non-generic receiver the type is
    /// resolved via `typ` (which handles `*T`). For a parameterized receiver
    /// the base generic type is resolved, the receiver type parameters are
    /// declared in the (already-open) function scope with bounds copied from
    /// the base type's parameters, and the base is instantiated with them.
    ///
    /// **Deferred**: `validRecv` (later), methods on generic *aliases* error,
    /// `mono.recordCanon`, Info recording.
    fn collect_recv(&mut self, field: &Field) -> (Option<ObjectId>, Option<TypeParamList>) {
        let rtyp = match field.ty.as_ref() {
            Some(t) => t.clone(),
            None => return (None, None),
        };
        let (ptr, base, rtparam_names) = self.unpack_recv(&rtyp);
        let name = field
            .names
            .first()
            .map(|n| n.name.clone())
            .unwrap_or_default();

        if rtparam_names.is_empty() {
            // No receiver type parameters: typecheck the receiver type directly.
            let typ = self.typ(&rtyp);
            let recv = self.new_param_var(name, typ, VarKind::Recv, field.pos().0 as u32);
            // Record the receiver definition (Go declares a named receiver via
            // `check.declare(.., recvPar.Name, recv, ..)` → recordDef). An
            // anonymous receiver has no ident, so record it as implicit instead
            // (Go `recordImplicit(rparam, recv)`).
            if let Some(id) = field.names.first() {
                self.record_def(id, Some(recv));
            } else {
                self.record_implicit(field.id, recv);
            }
            return (Some(recv), None);
        }

        // Parameterized receiver: rbase must denote a generic base type.
        // Resolve it *before* declaring the receiver type parameters (which may
        // share its name — go.dev/issue/52038).
        let base_typ = self.generic_type(&base);
        let base_named = if matches!(self.types.get(base_typ), TypeData::Named(_)) {
            Some(base_typ)
        } else {
            // Alias / invalid base: leave the receiver type invalid (but still
            // declare the type parameters below for scope hygiene).
            None
        };

        // Declare the receiver type parameters in the current scope.
        let scope = self.env.scope.expect("function scope is open in func_type");
        let scope_pos = base.pos().0 as u32;
        let recv_tparams: Vec<TypeId> = rtparam_names
            .iter()
            .map(|n| self.declare_type_param(n, scope, scope_pos))
            .collect();
        let rparams_list = bind_tparams(&mut self.types, recv_tparams.clone());

        let mut recv_type = self.invalid_type();
        if let Some(base_named) = base_named {
            // Copy the bounds from the base type's parameters, substituting the
            // base parameters with the receiver's (so cross-references resolve).
            let base_tparams: Vec<TypeId> = match self.types.get(base_named) {
                TypeData::Named(n) => n
                    .type_params()
                    .map(|l| l.list().to_vec())
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            if base_tparams.len() == recv_tparams.len() {
                let smap = make_subst_map(&base_tparams, &recv_tparams);
                for (i, &recv_tp) in recv_tparams.iter().enumerate() {
                    if let Some(b) = type_param_constraint(&self.types, base_tparams[i]) {
                        let nb = subst(
                            &mut self.types,
                            &mut self.objects,
                            &smap,
                            None,
                            &mut self.ctxt,
                            b,
                        );
                        set_constraint(&mut self.types, recv_tp, nb);
                    }
                }

                // The receiver type parameters also serve as the type arguments
                // of the receiver type: instantiate the base.
                recv_type = instantiate(
                    &mut self.types,
                    &mut self.objects,
                    &mut self.ctxt,
                    base_named,
                    recv_tparams.clone(),
                );
                if ptr && is_valid(&self.types, recv_type) {
                    recv_type = new_pointer(&mut self.types, recv_type);
                }
            } else {
                self.error(
                    base.pos().0 as u32,
                    Code::BadRecv,
                    format!(
                        "receiver declares {} type parameters, but receiver base type declares {}",
                        recv_tparams.len(),
                        base_tparams.len()
                    ),
                );
            }
        }

        let recv = self.new_param_var(name, recv_type, VarKind::Recv, field.pos().0 as u32);
        if let Some(id) = field.names.first() {
            self.record_def(id, Some(recv));
        } else {
            self.record_implicit(field.id, recv);
        }
        (Some(recv), rparams_list)
    }

    /// Unpack a receiver type expression `[*]B[P...]` into its pointer flag,
    /// base type expression `B`, and receiver type-parameter name idents.
    ///
    /// Equivalent to `Checker.unpackRecv` (go/ast-shaped: `*B` is a `StarExpr`,
    /// `B[P]` an `IndexExpr`, `B[P, Q]` an `IndexListExpr`).
    fn unpack_recv(&mut self, rtyp: &Expr) -> (bool, Expr, Vec<Ident>) {
        // Unwrap parentheses and a single leading `*`.
        let mut base = unparen(rtyp);
        let mut ptr = false;
        if let Expr::StarExpr(s) = base {
            ptr = true;
            base = unparen(&s.x);
        }

        let mut tparams: Vec<Ident> = Vec::new();
        let new_base = match base {
            Expr::IndexExpr(ix) => {
                tparams.push(self.recv_tparam_ident(&ix.index));
                (*ix.x).clone()
            }
            Expr::IndexListExpr(ix) => {
                for arg in &ix.indices {
                    tparams.push(self.recv_tparam_ident(arg));
                }
                (*ix.x).clone()
            }
            other => other.clone(),
        };
        (ptr, new_base, tparams)
    }

    /// A receiver type parameter must be an identifier; anything else is a
    /// `BadDecl`, replaced with a blank `_` to keep parameter counts aligned.
    fn recv_tparam_ident(&mut self, arg: &Expr) -> Ident {
        match arg {
            Expr::Ident(id) => id.clone(),
            other => {
                self.error(
                    other.pos().0 as u32,
                    Code::BadDecl,
                    "receiver type parameter must be an identifier",
                );
                Ident::new_ident("_")
            }
        }
    }

    /// Allocate a parameter/result/receiver `Var` of the given `kind`, with
    /// the checker's package set.
    fn new_param_var(&mut self, name: String, typ: TypeId, kind: VarKind, pos: u32) -> ObjectId {
        let par = new_param(&mut self.objects, name, typ);
        par.set_pkg(&mut self.objects, self.pkg);
        par.set_pos(&mut self.objects, pos);
        if let ObjectData::Var(v) = self.objects.get_mut(par) {
            v.set_kind(kind);
        }
        par
    }
}
