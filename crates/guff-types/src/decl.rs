//! Port of declaration type-checking from `go/types/decl.go`
//! (`cmd/compile/internal/types2/decl.go`).
//!
//! **Chunk 23a**: the [`Checker::obj_decl`] dispatcher (simplified state
//! machine), [`Checker::type_decl`] for defined types and simple aliases, and
//! [`Checker::collect_methods`] (attaching collected methods to a named type).
//! As the AST crate is `go/ast`-shaped, the port follows `go/types/decl.go`.
//!
//! ## Deferrals (chunk-23a, see §8)
//!
//! - **const / var declarations** (`constDecl`/`varDecl`) need `Checker.expr`
//!   (chunk 25) to evaluate initializers and `initConst`/`initVar` (the
//!   Checker side of `assignments.go`). The resolver (chunk 22) already gave
//!   every package-level `Const`/`Var` a `Typ[Invalid]` placeholder (D18), so
//!   `obj_decl` treats them as already-resolved (the black-state check fires)
//!   until chunk 23b. → D18.
//! - **func declarations** (`funcDecl`) build the signature via
//!   [`Checker::func_type`] (chunk 24); the body is checked later (deferred to
//!   stmt.go, chunk 30).
//! - **generics**: type-parameter lists on a `type` decl, and generic aliases,
//!   are not handled — they need `collectTypeParams`/`bound` (a later chunk).
//! - **cycle detection**: the full grey/`objPathIdx`/`validCycle` machinery
//!   (`cycles.go`, chunk 33) is reduced to the black-state check plus a guard
//!   that maps a `Named`/`Alias` RHS-underlying to `Typ[Invalid]` (so a
//!   self-referential `type T T` can't panic in `set_underlying`).
//! - `validType` scheduled via `later` is omitted (the recursive-expansion
//!   safety check); recovered with the cycles chunk.
//! - `checkFieldUniqueness` (struct field-vs-method name clash) inside
//!   `collect_methods` is deferred.

use guff::ast::{BinaryExpr, Decl, Expr, Spec, TypeSpec, UnaryExpr};
use guff::token::Token;
use guff_constant::make_int64;
use guff_types_errors::Code;

use crate::alias::new_alias;
use crate::arena::{ObjectData, TypeData, TypeId};
use crate::check::Checker;
use crate::named::{
    add_method, named_method, named_num_methods, named_set_type_params, new_named, set_underlying,
};
use crate::object::const_::new_const;
use crate::object::type_name::{new_type_name, type_name_set_typ};
use crate::object::var::{new_var, VarKind};
use crate::objset::ObjSet;
use crate::operand::Operand;
use crate::predicates::{is_const_type, is_type_param, is_valid};
use crate::typelists::bind_tparams;
use crate::typeparam::{new_type_param, set_constraint};
use crate::ObjectId;
use guff::ast::{Field, FieldList, Ident, InterfaceType};

/// Which kind of declaration `obj_decl` is dispatching.
enum DeclKind {
    Type,
    Func,
    Const,
    Var,
}

impl Checker {
    /// Type-check the declaration of `obj` in its (file) environment.
    ///
    /// Equivalent to `Checker.objDecl` — reduced to the acyclic path. An
    /// object whose type is already set is "black" (done); otherwise its
    /// declaration is processed under the declaring file's scope.
    pub fn obj_decl(&mut self, obj: ObjectId) {
        // black: already type-checked. `Const`/`Var` carry a `Typ[Invalid]`
        // placeholder from the resolver; treat that as "not yet checked" so we
        // run their declaration (chunk 23b).
        let placeholder = self.invalid_type();
        match obj.typ(&self.objects) {
            Some(t) if t != placeholder => return, // genuinely black
            Some(_)
                if !matches!(
                    self.objects.get(obj),
                    ObjectData::Const(_) | ObjectData::Var(_)
                ) =>
            {
                return
            }
            _ => {}
        }

        // Classify without holding an arena borrow across the mutating calls
        // below.
        let kind = match self.objects.get(obj) {
            ObjectData::TypeName(_) => DeclKind::Type,
            ObjectData::Func(_) => DeclKind::Func,
            ObjectData::Const(_) => DeclKind::Const,
            ObjectData::Var(_) => DeclKind::Var,
            _ => return,
        };

        // grey: mark as being-checked for cycle reporting (full validCycle is
        // deferred to cycles.go, chunk 33).
        self.push(obj);

        // Save/restore the environment, set it up from the declaration info.
        let (file_scope, version) = match self.obj_map.get(&obj) {
            Some(d) => (d.file_scope, d.version.clone()),
            None => {
                self.pop();
                return;
            }
        };
        let saved_scope = self.env.scope;
        let saved_version = std::mem::take(&mut self.env.version);
        let saved_decl = self.env.decl;
        self.env.scope = file_scope;
        self.env.version = version;

        // Const and var declarations must not have initialization cycles. We
        // track them by remembering the current declaration in `env.decl`, so
        // that identifiers in their init expressions add dependency edges via
        // `add_decl_dep` (see `expr::ident`). Functions are recursive, so their
        // dependencies are tracked from `func_body` instead, not here.
        match kind {
            DeclKind::Const | DeclKind::Var => self.env.decl = Some(obj),
            DeclKind::Type | DeclKind::Func => {}
        }

        match kind {
            DeclKind::Type => {
                if let Some(tdecl) = self.obj_map.get(&obj).and_then(|d| d.tdecl.clone()) {
                    self.type_decl(obj, &tdecl);
                    // methods can only be added to top-level types.
                    self.collect_methods(obj);
                }
            }
            DeclKind::Func => self.func_decl(obj),
            DeclKind::Const => {
                let d = self.obj_map.get(&obj);
                let (vtyp, init, inherited) = match d {
                    Some(d) => (d.vtyp.clone(), d.init.clone(), d.inherited),
                    None => (None, None, false),
                };
                self.const_decl(obj, vtyp.as_ref(), init.as_ref(), inherited);
            }
            DeclKind::Var => {
                let d = self.obj_map.get(&obj);
                let (vtyp, init, lhs) = match d {
                    Some(d) => (d.vtyp.clone(), d.init.clone(), d.lhs.clone()),
                    None => (None, None, Vec::new()),
                };
                self.var_decl(obj, &lhs, vtyp.as_ref(), init.as_ref());
            }
        }

        self.env.scope = saved_scope;
        self.env.version = saved_version;
        self.env.decl = saved_decl;
        self.pop();
    }

    /// Type-check a constant declaration.
    ///
    /// Equivalent to `Checker.constDecl`. The `iota` environment is set from
    /// the const's stored value (the resolver seeded it). `errpos` handling for
    /// inherited initializers is simplified.
    fn const_decl(
        &mut self,
        obj: ObjectId,
        typ: Option<&Expr>,
        init: Option<&Expr>,
        _inherited: bool,
    ) {
        // Set iota for this constant (resolver stored it as the const's value).
        let saved_iota = self.env.iota.take();
        if let ObjectData::Const(c) = self.objects.get(obj) {
            self.env.iota = Some(c.val().clone());
        }

        // Provide a valid (unknown) value under all circumstances.
        if let ObjectData::Const(c) = self.objects.get_mut(obj) {
            c.set_val(guff_constant::make_unknown());
        }

        // Determine the declared type, if any.
        if let Some(te) = typ {
            let t = self.typ(te);
            if !is_const_type(&self.types, t) {
                if is_valid(&self.types, t.underlying(&self.types)) {
                    let ts = self.type_str(t);
                    self.error(
                        te.pos().0 as u32,
                        Code::InvalidConstType,
                        format!("invalid constant type {}", ts),
                    );
                }
                let invalid = self.invalid_type();
                if let ObjectData::Const(c) = self.objects.get_mut(obj) {
                    c.set_typ(invalid);
                }
                self.env.iota = saved_iota;
                return;
            }
            if let ObjectData::Const(c) = self.objects.get_mut(obj) {
                c.set_typ(t);
            }
        }

        // Check the initialization expression.
        let mut x = Operand::invalid();
        if let Some(ie) = init {
            self.expr(&mut x, ie);
        }
        self.init_const(obj, &mut x);

        self.env.iota = saved_iota;
    }

    /// Type-check a variable declaration. `lhs` is the full left-hand-side list
    /// for an n:1 (multiple variables, single multi-valued init) declaration,
    /// or empty/single for the ordinary case.
    ///
    /// Equivalent to `Checker.varDecl`.
    fn var_decl(&mut self, obj: ObjectId, lhs: &[ObjectId], typ: Option<&Expr>, init: Option<&Expr>) {
        // Determine the declared type, if any.
        if let Some(te) = typ {
            let t = self.typ(te);
            if let ObjectData::Var(v) = self.objects.get_mut(obj) {
                v.set_typ(t);
            }
        }

        // No initializer: type must have been given (else arityMatch erred).
        let init = match init {
            Some(e) => e,
            None => {
                if typ.is_none() {
                    let invalid = self.invalid_type();
                    if let ObjectData::Var(v) = self.objects.get_mut(obj) {
                        v.set_typ(invalid);
                    }
                }
                return;
            }
        };

        // Ordinary single-variable declaration.
        if lhs.len() <= 1 {
            let mut x = Operand::invalid();
            self.expr(&mut x, init);
            self.init_var(obj, &mut x, "variable declaration");
            return;
        }

        // n:1 — multiple variables share one multi-valued init expression.
        // Give every lhs variable the same declared type (if one was given),
        // otherwise each adopts its init value's type (go.dev/issue/15755).
        // `init_vars` then unpacks the tuple and checks each assignment; when
        // a sibling is processed later, `obj_decl` sees its type already set
        // and skips it, so this runs only once.
        if typ.is_some() {
            let t = obj.typ(&self.objects).unwrap_or_else(|| self.invalid_type());
            for &l in lhs {
                self.set_var_typ(l, t);
            }
        }
        self.init_vars(lhs, std::slice::from_ref(init), false);
    }

    /// Type-check a local declaration statement (`const` / `var` / `type`
    /// inside a function body). Objects are created, checked, and declared into
    /// the current block scope.
    ///
    /// Equivalent to `Checker.declStmt` (`decl.go`). Local declarations are
    /// always a `GenDecl`; `import`/`func` cannot appear here.
    ///
    /// **Deferred**: the n:1 (multi-valued single rhs) `var` spread needs
    /// `initVars` (chunk 30c); generic local `type` decls; `Info` recording.
    pub fn decl_stmt(&mut self, d: &Decl) {
        let gd = match d {
            Decl::GenDecl(gd) => gd,
            _ => {
                self.error(
                    d.pos().0 as u32,
                    Code::InvalidSyntaxTree,
                    "unknown declaration in statement context",
                );
                return;
            }
        };

        let scope = match self.env.scope {
            Some(s) => s,
            None => return, // no current scope: nothing to declare into
        };

        match gd.tok {
            Some(Token::CONST) => {
                // `iota` value + inheritance of the previous spec's type/values.
                let mut last_type: Option<Expr> = None;
                let mut last_values: Vec<Expr> = Vec::new();
                let mut have_last = false;

                for (iota, spec) in gd.specs.iter().enumerate() {
                    let vs = match spec {
                        Spec::ValueSpec(vs) => vs,
                        _ => continue,
                    };

                    let mut inherited = true;
                    if vs.ty.is_some() || !vs.values.is_empty() {
                        last_type = vs.ty.clone();
                        last_values = vs.values.clone();
                        have_last = true;
                        inherited = false;
                    } else if !have_last {
                        last_type = None;
                        last_values = Vec::new();
                        have_last = true;
                        inherited = false;
                    }

                    let top = self.delayed.len();
                    let iota_val = make_int64(iota as i64);
                    let mut lhs: Vec<ObjectId> = Vec::with_capacity(vs.names.len());
                    for (i, name) in vs.names.iter().enumerate() {
                        let invalid = self.invalid_type();
                        let obj = new_const(
                            &mut self.objects,
                            name.name.clone(),
                            invalid,
                            iota_val.clone(),
                        );
                        obj.set_pkg(&mut self.objects, self.pkg);
                        obj.set_pos(&mut self.objects, name.pos().0 as u32);
                        lhs.push(obj);
                        let init = last_values.get(i).cloned();
                        self.const_decl(obj, last_type.as_ref(), init.as_ref(), inherited);
                    }

                    // process function literals in init expressions before
                    // scope changes
                    self.process_delayed(top);

                    let scope_pos = spec.end().0 as u32;
                    for obj in lhs {
                        self.declare(scope, obj, scope_pos);
                    }
                }
            }

            Some(Token::VAR) => {
                for spec in &gd.specs {
                    let vs = match spec {
                        Spec::ValueSpec(vs) => vs,
                        _ => continue,
                    };

                    let top = self.delayed.len();
                    let n = vs.names.len();
                    let v = vs.values.len();
                    let mut lhs: Vec<ObjectId> = Vec::with_capacity(n);
                    for name in &vs.names {
                        let invalid = self.invalid_type();
                        let obj = new_var(&mut self.objects, name.name.clone(), invalid);
                        obj.set_pkg(&mut self.objects, self.pkg);
                        obj.set_pos(&mut self.objects, name.pos().0 as u32);
                        if let ObjectData::Var(var) = self.objects.get_mut(obj) {
                            var.set_kind(VarKind::Local);
                        }
                        // Record the defining identifier (Go's `declare` calls
                        // `recordDef`). SSA needs `Info.defs` to find the local's
                        // object; package-level vars are recorded in the resolver.
                        self.record_def(name, Some(obj));
                        lhs.push(obj);
                    }

                    if v == n {
                        // n:n — each value initializes its variable.
                        for (i, &obj) in lhs.iter().enumerate() {
                            self.var_decl(obj, &[], vs.ty.as_ref(), vs.values.get(i));
                        }
                    } else if v == 0 {
                        // declared type only, no initializers.
                        for &obj in &lhs {
                            self.var_decl(obj, &[], vs.ty.as_ref(), None);
                        }
                    } else {
                        // n:1 multi-valued spread (`var a, b = f()`) or a count
                        // mismatch — apply a declared type to each lhs, then let
                        // `init_vars` unpack the tuple (or report the mismatch).
                        if let Some(te) = vs.ty.as_ref() {
                            let t = self.typ(te);
                            for &obj in &lhs {
                                self.set_var_typ(obj, t);
                            }
                        }
                        self.init_vars(&lhs, &vs.values, false);
                    }

                    self.process_delayed(top);

                    let scope_pos = spec.end().0 as u32;
                    for obj in lhs {
                        self.declare(scope, obj, scope_pos);
                    }
                }
            }

            Some(Token::TYPE) => {
                for spec in &gd.specs {
                    let ts = match spec {
                        Spec::TypeSpec(ts) => ts,
                        _ => continue,
                    };
                    let obj = new_type_name(&mut self.objects, ts.name.name.clone(), None);
                    obj.set_pkg(&mut self.objects, self.pkg);
                    obj.set_pos(&mut self.objects, ts.name.pos().0 as u32);
                    // spec: the scope of a local type identifier begins at the
                    // identifier in the TypeSpec.
                    let scope_pos = ts.name.pos().0 as u32;
                    self.declare(scope, obj, scope_pos);
                    self.push(obj); // grey
                    self.type_decl(obj, ts);
                    self.pop();
                }
            }

            _ => {
                self.error(
                    gd.tok_pos.0 as u32,
                    Code::InvalidSyntaxTree,
                    "invalid declaration in statement context",
                );
            }
        }
    }

    /// Type-check a function/method declaration: build its signature. The
    /// body is checked later (deferred to stmt.go, chunk 30).
    ///
    /// Equivalent to `Checker.funcDecl` (signature half).
    fn func_decl(&mut self, obj: ObjectId) {
        let fdecl = match self.obj_map.get(&obj).and_then(|d| d.fdecl.clone()) {
            Some(f) => f,
            None => return,
        };

        // DEFERRED: Go pre-creates an empty Signature and sets obj.typ before
        // funcType to guard against cycles; we build the signature atomically
        // and set it afterward (cycles through a signature are rare; full
        // handling is chunk 33).
        let sig = self.func_type(fdecl.recv.as_ref(), &fdecl.ty);
        if let ObjectData::Func(f) = self.objects.get_mut(obj) {
            f.set_typ(sig);
        }

        // Check the body later (after all package objects are declared), so it
        // can refer to forward-declared package-level names. The body scope is
        // parented at the declaring file scope captured now.
        if let Some(body) = fdecl.body.clone() {
            let parent = self.env.scope;
            let ftid = fdecl.ty.id;
            self.later(move |check| check.func_body(Some(obj), sig, ftid, parent, &body));
        }

        // DEFERRED: sig.scope extent and the "generic function is missing
        // body" soft error.
    }

    /// Type-check a `type` declaration: a defined (named) type, or a simple
    /// type alias.
    ///
    /// Equivalent to `Checker.typeDecl` (non-generic subset).
    fn type_decl(&mut self, obj: ObjectId, tdecl: &TypeSpec) {
        // alias declaration: `type T = U`.
        if tdecl.assign.is_valid() {
            // DEFERRED: generic aliases (type parameters on an alias).
            let rhs = self.typ(&tdecl.ty);
            // new_alias back-fills obj.typ and memoises the resolved actual.
            new_alias(&mut self.types, &mut self.objects, obj, Some(rhs));
            return;
        }

        // type definition or generic type declaration: `type T[P ...] U`.
        // Create the named type up-front (it back-fills obj.typ) so the RHS can
        // refer to T recursively.
        let named = new_named(&mut self.types, &mut self.objects, obj, None, Vec::new());

        // Collect type parameters (chunk 35a-decl). Their scope starts at the
        // beginning of the type-parameter list and extends over the RHS, so we
        // open a fresh scope and keep it current while checking the RHS.
        let has_tparams = tdecl
            .type_params
            .as_ref()
            .map_or(false, |fl| !fl.list.is_empty());
        if let Some(fl) = tdecl.type_params.as_ref().filter(|_| has_tparams) {
            let scope = self.open_scope(fl.pos().0 as u32, fl.closing.0 as u32, "type parameters");
            // Record the type-parameter scope under the TypeSpec node
            // (Go `openScope(tdecl, "type parameters")`).
            self.record_scope(tdecl.id, scope);
            self.collect_type_params(named, fl);
        }

        let rhs = self.typ(&tdecl.ty);

        // spec: "In a type definition the given type cannot be a type
        // parameter." (go.dev/issue/45639)
        if is_type_param(&self.types, rhs) {
            self.error(
                tdecl.ty.pos().0 as u32,
                Code::MisplacedTypeParam,
                "cannot use a type parameter as RHS in type declaration",
            );
            let invalid = self.invalid_type();
            set_underlying(&mut self.types, named, invalid);
            if has_tparams {
                self.close_scope();
            }
            return;
        }

        // Resolve the underlying of the RHS (`type T U` ⇒ T and U share an
        // underlying). If that resolves to another `Named`/`Alias`, the RHS is
        // incomplete or cyclic — map to `Typ[Invalid]` (full cycle handling is
        // chunk 33). `set_underlying` requires a non-`Named` underlying.
        let u = rhs.underlying(&self.types);
        let u = if matches!(self.types.get(u), TypeData::Named(_) | TypeData::Alias(_)) {
            self.invalid_type()
        } else {
            u
        };
        set_underlying(&mut self.types, named, u);

        if has_tparams {
            self.close_scope();
        }

        // Detect invalid recursive types (e.g. `type T struct { x T }`), which
        // have no finite layout. Deferred so the type is fully set up first.
        // `valid_type` invalidates the type and returns the offending cycle.
        self.later(move |c| {
            let invalid = c.invalid_type();
            if let crate::validtype::ValidResult::Cycle { path } =
                crate::validtype::valid_type(&mut c.types, &c.objects, &c.packages, named, invalid)
            {
                let name = obj.name(&c.objects).to_string();
                let msg = if path.len() <= 1 {
                    format!("invalid recursive type: {} refers to itself", name)
                } else {
                    format!("invalid recursive type {}", name)
                };
                c.error(obj.pos(&c.objects), Code::InvalidDeclCycle, msg);
            }
        });
    }

    /// Declare the type parameters of a generic type declaration, bind them
    /// onto `named`, then resolve their constraint bounds.
    ///
    /// Equivalent to `Checker.collectTypeParams`. The type parameters are
    /// declared in the current (already-open) type-parameter scope. They are
    /// bound onto the type *before* the bounds are resolved so a bound may
    /// refer to the parameterized type (go.dev/issue/47887).
    ///
    /// **Note**: go/ast groups shared bounds — `[A, B any]` is a single
    /// `Field` with two names. We flatten that so each name becomes its own
    /// `TypeParam` sharing the field's bound.
    fn collect_type_params(&mut self, named: TypeId, list: &FieldList) {
        let (tparams, field_of) = self.declare_type_params(list);
        if tparams.is_empty() {
            return;
        }

        // Bind the parameters onto the type before collecting constraints, so
        // a constraint may reference the parameterized type itself
        // (go.dev/issue/47887).
        let tlist =
            bind_tparams(&mut self.types, tparams.clone()).expect("non-empty type-parameter list");
        named_set_type_params(&mut self.types, named, tlist);

        self.resolve_type_param_bounds(list, &tparams, &field_of);
    }

    /// Declare each type parameter named in `list` into the current scope,
    /// returning the parameters' `TypeId`s and, for each, the index of the
    /// `Field` it came from (go/ast groups shared bounds: `[A, B any]` is one
    /// `Field` with two names).
    ///
    /// The constraints are left as placeholder `Typ[Invalid]` — fill them in
    /// with [`Checker::resolve_type_param_bounds`] after binding.
    pub(crate) fn declare_type_params(&mut self, list: &FieldList) -> (Vec<TypeId>, Vec<usize>) {
        let scope = self.env.scope.expect("type-parameter scope must be open");
        let scope_pos = list.list.first().map(|f| f.pos().0 as u32).unwrap_or(0);

        let mut tparams: Vec<TypeId> = Vec::new();
        let mut field_of: Vec<usize> = Vec::new();
        for (fi, f) in list.list.iter().enumerate() {
            for name in &f.names {
                let tp = self.declare_type_param(name, scope, scope_pos);
                tparams.push(tp);
                field_of.push(fi);
            }
        }
        (tparams, field_of)
    }

    /// Resolve and assign the constraint bounds of already-declared type
    /// parameters, re-using the previous bound for grouped names
    /// (`[A, B any]`). A bound that is itself a type parameter is reported as
    /// `MisplacedTypeParam`.
    pub(crate) fn resolve_type_param_bounds(
        &mut self,
        list: &FieldList,
        tparams: &[TypeId],
        field_of: &[usize],
    ) {
        let invalid = self.invalid_type();
        let mut bound: TypeId = invalid;
        let mut last_fi: Option<usize> = None;
        for (i, &tp) in tparams.iter().enumerate() {
            let fi = field_of[i];
            if last_fi != Some(fi) {
                bound = match &list.list[fi].ty {
                    Some(t) => self.bound(t),
                    None => invalid,
                };
                if is_type_param(&self.types, bound) {
                    let pos = list.list[fi]
                        .ty
                        .as_ref()
                        .map(|t| t.pos().0 as u32)
                        .unwrap_or(0);
                    self.error(
                        pos,
                        Code::MisplacedTypeParam,
                        "cannot use a type parameter as constraint",
                    );
                    bound = invalid;
                }
                last_fi = Some(fi);
            }
            set_constraint(&mut self.types, tp, bound);
        }
    }

    /// Declare a single type parameter `name` in `scope`.
    ///
    /// Equivalent to `Checker.declareTypeParam`: allocates a `TypeName` whose
    /// type is a fresh `TypeParam` (with a placeholder `Typ[Invalid]`
    /// constraint), then declares the name in the scope.
    pub(crate) fn declare_type_param(
        &mut self,
        name: &Ident,
        scope: crate::ScopeId,
        scope_pos: u32,
    ) -> TypeId {
        let invalid = self.invalid_type();
        let tname = new_type_name(&mut self.objects, name.name.clone(), None);
        let tpar = new_type_param(&mut self.types, tname, Some(invalid));
        // newTypeParam assigns the type to the TypeName as a side effect.
        type_name_set_typ(&mut self.objects, tname, tpar);
        tname.set_pkg(&mut self.objects, self.pkg);
        tname.set_pos(&mut self.objects, name.pos().0 as u32);
        self.declare(scope, tname, scope_pos);
        tpar
    }

    /// Resolve a type-parameter constraint expression.
    ///
    /// Equivalent to `Checker.bound`. A bare type-set literal (`~T` or `A|B`)
    /// may only appear as a constraint, so it is wrapped in an implicit
    /// `interface{ ... }` and handed to `interface_type`; any other expression
    /// is an ordinary type.
    pub(crate) fn bound(&mut self, x: &Expr) -> TypeId {
        let is_constraint_lit = matches!(
            x,
            Expr::UnaryExpr(UnaryExpr { op, .. }) if *op == Token::TILDE
        ) || matches!(
            x,
            Expr::BinaryExpr(BinaryExpr { op, .. }) if *op == Token::OR
        );
        if is_constraint_lit {
            let iface = InterfaceType {
                interface_: x.pos(),
                methods: FieldList {
                    opening: x.pos(),
                    list: vec![Field {
                        doc: None,
                        names: Vec::new(),
                        ty: Some(x.clone()),
                        tag: None,
                        comment: None,
                        id: 0,
                    }],
                    closing: x.end(),
                },
                incomplete: false,
                id: 0, // synthetic node — not recorded in Info.Types
            };
            return self.interface_type(&iface);
        }
        self.typ(x)
    }

    /// Attach the methods collected for `obj` (during object collection) to its
    /// named type, reporting duplicate method names.
    ///
    /// Equivalent to `Checker.collectMethods` (minus `checkFieldUniqueness`).
    fn collect_methods(&mut self, obj: ObjectId) {
        let methods = match self.methods.remove(&obj) {
            Some(m) => m,
            None => return,
        };

        // The base type must be the `Named` for obj (aliases never carry
        // methods — guaranteed by resolveBaseTypeName in the resolver).
        let base = match obj.typ(&self.objects) {
            Some(t) if matches!(self.types.get(t), TypeData::Named(_)) => t,
            _ => return,
        };

        // Use an objset to detect duplicate method names. Seed it with any
        // pre-existing methods (additional package files may add to an
        // already type-checked type).
        let mut mset = ObjSet::new();
        let n = named_num_methods(&self.types, base);
        for i in 0..n {
            let m = named_method(&self.types, base, i);
            mset.insert(&self.objects, &self.packages, m);
        }

        for m in methods {
            if let Some(_alt) = mset.insert(&self.objects, &self.packages, m) {
                let on = obj.name(&self.objects).to_string();
                let mn = m.name(&self.objects).to_string();
                self.error(
                    m.pos(&self.objects),
                    Code::DuplicateMethod,
                    format!("method {}.{} already declared", on, mn),
                );
                continue;
            }
            add_method(&mut self.types, &self.objects, base, m);
        }

        // DEFERRED: checkFieldUniqueness(base) via check.later — struct
        // field-vs-method name clash detection.
    }
}

// ===== DEFERRED (forward pointers) =====
// Go: Checker.constDecl / varDecl (decl.go) — need Checker.expr (chunk 25) for
//   initializer evaluation and initConst/initVar (assignments.go Checker side).
//   Land in chunk 23b. The resolver gives Const/Var a Typ[Invalid] placeholder
//   in the meantime (D18).
// Go: Checker.funcBody (stmt.go) — the function body, queued via later from
//   func_decl, needs stmt.go (chunk 30).
// Go: Checker.declStmt (local declarations) — needs stmt.go (chunk 30).
