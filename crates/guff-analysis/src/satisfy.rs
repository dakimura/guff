//! Port of `golang.org/x/tools/refactor/satisfy` (x/tools v0.50.0, `find.go`).
//!
//! [`find`] walks a type-checked package and returns the type constraints of
//! the form `(lhs, rhs)` where `lhs` is an interface with methods, `rhs` has
//! methods, and the package is only well-typed because `rhs` is assignable to
//! `lhs`. It is the subset of the "implements" relation that compiling the
//! package checks.
//!
//! The only consumer is printf's interface-method induction
//! (`methodImplementations`), which turns `var _ Logger = myLogger(0)` into
//! "`Logger.Logf` forwards to `myLogger.Logf`".
//!
//! Assignability conversions happen in these places, and the walk visits each:
//! - assignments `y = x`, `y := x`, `var y = x`;
//! - call arguments to formal parameters (and `append` / `delete`);
//! - return operands to result parameters;
//! - composite literal elements to their field / element / key type;
//! - `m[k]`, from `k` to the map's key type;
//! - comparisons `x == y` and `switch x { case y: }`;
//! - explicit conversions `T(x)`;
//! - sends `ch <- x`;
//! - type assertions `x.(T)` and `switch x.(type) { case T: }`.
//!
//! Like upstream, constant expressions are not descended into, and neither are
//! types. Upstream panics on ill-typed input; this port answers "no type"
//! instead and carries on, so a package with type errors gives a partial
//! result rather than a crash.
//!
//! Upstream's `Result` is a map, so its iteration order is random; [`find`]
//! returns the pairs in discovery order with duplicates removed. Its consumer
//! builds a set from them, so the order does not matter.

use std::collections::{HashMap, HashSet};

use guff::ast::{BlockStmt, CallExpr, Decl, Expr, File, Spec, Stmt, ValueSpec};
use guff::token::Token;
use guff_types::api::Info;
use guff_types::arena::{ObjectArena, ObjectData, PackageArena, TypeArena, TypeData, TypeId};
use guff_types::basic::BasicKind;
use guff_types::operand::OperandMode;
use guff_types::selection::SelectionKind;

/// One constraint: `rhs` does, and must, satisfy the interface `lhs`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Constraint {
    pub lhs: TypeId,
    pub rhs: TypeId,
}

/// `Finder.Find` over one package's files.
pub fn find(
    types: &mut TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    info: &Info,
    files: &[File],
) -> Vec<Constraint> {
    let mut f = Finder {
        types,
        objects,
        packages,
        info,
        sig: None,
        result: Vec::new(),
        seen: HashSet::new(),
        has_methods: HashMap::new(),
    };
    for file in files {
        for d in &file.decls {
            match d {
                Decl::GenDecl(g) => {
                    // Consts are ignored.
                    if g.tok == Some(Token::VAR) {
                        for spec in &g.specs {
                            if let Spec::ValueSpec(vs) = spec {
                                f.value_spec(vs);
                            }
                        }
                    }
                }
                Decl::FuncDecl(d) => {
                    if let Some(body) = &d.body {
                        if let Some(Some(obj)) = info.defs.get(&d.name.id).copied() {
                            f.sig = obj.typ(objects).filter(|&t| f.is_signature(t));
                        }
                        f.block(body);
                        f.sig = None;
                    }
                }
                _ => {}
            }
        }
    }
    f.result
}

struct Finder<'a> {
    types: &'a mut TypeArena,
    objects: &'a ObjectArena,
    packages: &'a PackageArena,
    info: &'a Info,
    /// The signature of the enclosing function, for `return`.
    sig: Option<TypeId>,
    result: Vec<Constraint>,
    seen: HashSet<Constraint>,
    /// `msetcache.MethodSet(t).Len() != 0`, memoized.
    has_methods: HashMap<TypeId, bool>,
}

/// `ast.Unparen`.
fn unparen(mut e: &Expr) -> &Expr {
    while let Expr::ParenExpr(p) = e {
        e = &p.x;
    }
    e
}

impl Finder<'_> {
    fn tv_type(&self, e: &Expr) -> Option<TypeId> {
        self.info.types.get(&e.id()).map(|tv| tv.typ)
    }

    fn is_signature(&self, t: TypeId) -> bool {
        matches!(self.types.get(t), TypeData::Signature(_))
    }

    fn var_type(&self, v: guff_types::arena::ObjectId) -> Option<TypeId> {
        v.typ(self.objects)
    }

    /// The `i`'th element type of tuple `t`.
    fn tuple_at(&self, t: TypeId, i: usize) -> Option<TypeId> {
        match self.types.get(t) {
            TypeData::Tuple(tup) if i < tup.len() => self.var_type(tup.at(i)),
            _ => None,
        }
    }

    fn tuple_len(&self, t: Option<TypeId>) -> usize {
        guff_types::tuple::tuple_len(self.types, t)
    }

    /// `terms(t)`: the types in `t`'s type set. A type parameter (or any
    /// interface) contributes its normalized terms — none when its type set
    /// is unrestricted — and every other type is its own single term.
    fn terms(&mut self, t: TypeId) -> Vec<TypeId> {
        let u = t.underlying(self.types);
        let iface = match self.types.get(guff_types::alias::unalias_readonly(self.types, t)) {
            TypeData::TypeParam(_) => {
                let t = guff_types::alias::unalias_readonly(self.types, t);
                guff_types::type_param_iface(self.types, self.objects, self.packages, t)
            }
            _ if matches!(self.types.get(u), TypeData::Interface(_)) => u,
            _ => return vec![t],
        };
        let tset = guff_types::interface_typeset(self.types, self.objects, self.packages, iface);
        tset.term_types().unwrap_or_default()
    }

    /// `hasUnderlyingTermOf[*T]`: the underlying type of the first term in
    /// `t`'s type set that `pick` accepts.
    fn underlying_term(&mut self, t: Option<TypeId>, pick: fn(&TypeData) -> bool) -> Option<TypeId> {
        let t = t?;
        for term in self.terms(t) {
            let u = term.underlying(self.types);
            if pick(self.types.get(u)) {
                return Some(u);
            }
        }
        None
    }

    fn signature_of(&mut self, t: Option<TypeId>) -> Option<TypeId> {
        self.underlying_term(t, |d| matches!(d, TypeData::Signature(_)))
    }

    fn map_of(&mut self, t: Option<TypeId>) -> Option<TypeId> {
        self.underlying_term(t, |d| matches!(d, TypeData::Map(_)))
    }

    /// `f.assign(lhs, rhs)`: record the pair when `lhs` is an interface with
    /// methods, `rhs` has methods, and the two are not identical.
    fn assign(&mut self, lhs: Option<TypeId>, rhs: Option<TypeId>) {
        // An invalid type on either side is uninteresting: it is not an
        // interface, and it has no methods.
        let (Some(lhs), Some(rhs)) = (lhs, rhs) else {
            return;
        };
        if guff_types::predicates::identical(self.types, self.objects, self.packages, lhs, rhs) {
            return;
        }
        if !guff_types::predicates::is_interface(self.types, lhs) {
            return;
        }
        if !self.method_set_nonempty(lhs) || !self.method_set_nonempty(rhs) {
            return;
        }
        let c = Constraint { lhs, rhs };
        if self.seen.insert(c) {
            self.result.push(c);
        }
    }

    /// `MethodSet(t).Len() != 0`.
    fn method_set_nonempty(&mut self, t: TypeId) -> bool {
        if let Some(&b) = self.has_methods.get(&t) {
            return b;
        }
        let b = self.compute_method_set_nonempty(t);
        self.has_methods.insert(t, b);
        b
    }

    fn compute_method_set_nonempty(&mut self, t: TypeId) -> bool {
        let t = guff_types::alias::unalias_readonly(self.types, t);
        // `types.NewMethodSet`: a pointer to an interface has no methods; an
        // interface (and a type parameter, whose underlying type is its
        // constraint) has the methods of its type set.
        let (base, is_ptr) = match self.types.get(t) {
            TypeData::Pointer(p) => (guff_types::alias::unalias_readonly(self.types, p.elem()), true),
            _ => (t, false),
        };
        let u = base.underlying(self.types);
        if matches!(self.types.get(u), TypeData::Interface(_)) {
            if is_ptr {
                return false;
            }
            return guff_types::interface::interface_num_methods(self.types, self.objects, self.packages, u) > 0;
        }

        // A concrete type: is any method name it could have actually in its
        // method set? `lookup_field_or_method` with `addressable == false`
        // applies the value-receiver rule that `NewMethodSet(T)` applies.
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        self.collect_method_names(base, &mut names, &mut seen);
        for (name, pkg) in names {
            let r = guff_types::lookup::lookup_field_or_method(
                self.types,
                self.objects,
                self.packages,
                t,
                false,
                pkg,
                &name,
            );
            if let Some((obj, _, _)) = r.found() {
                if matches!(self.objects.get(obj), ObjectData::Func(_)) {
                    return true;
                }
            }
        }
        false
    }

    /// Every method name declared on `t` or reachable through its embedded
    /// fields, with the package that qualifies an unexported one.
    fn collect_method_names(
        &mut self,
        t: TypeId,
        names: &mut Vec<(String, Option<guff_types::arena::PackageId>)>,
        seen: &mut HashSet<TypeId>,
    ) {
        let t = guff_types::alias::unalias_readonly(self.types, t);
        if !seen.insert(t) {
            return;
        }
        match self.types.get(t) {
            TypeData::Pointer(p) => {
                let e = p.elem();
                self.collect_method_names(e, names, seen);
            }
            TypeData::Named(_) => {
                for i in 0..guff_types::named_num_methods(self.types, t) {
                    let m = guff_types::named_method(self.types, t, i);
                    names.push((m.name(self.objects).to_string(), m.pkg(self.objects)));
                }
                if let Some(u) = guff_types::named_underlying(self.types, t) {
                    self.collect_method_names(u, names, seen);
                }
            }
            TypeData::Struct(_) => {
                for i in 0..guff_types::struct_num_fields(self.types, t) {
                    let field = guff_types::struct_field(self.types, t, i);
                    let embedded = matches!(self.objects.get(field), ObjectData::Var(v) if v.embedded());
                    if !embedded {
                        continue;
                    }
                    if let Some(ft) = field.typ(self.objects) {
                        self.collect_method_names(ft, names, seen);
                    }
                }
            }
            TypeData::Interface(_) => {
                let n = guff_types::interface::interface_num_methods(self.types, self.objects, self.packages, t);
                for i in 0..n {
                    let m = guff_types::interface::interface_method(self.types, self.objects, self.packages, t, i);
                    names.push((m.name(self.objects).to_string(), m.pkg(self.objects)));
                }
            }
            _ => {}
        }
    }

    fn assignable_to(&mut self, v: TypeId, t: TypeId) -> bool {
        guff_types::api_predicates::api_assignable_to(self.types, self.objects, self.packages, v, t)
    }

    /// `typeAssert(I, T)`, called for each `x.(T)` where `x` has interface
    /// type `I`. An assertion may be impossible, so it only constrains when
    /// `T` is assignable to `I`.
    fn type_assert(&mut self, i: Option<TypeId>, t: Option<TypeId>) {
        let (Some(i), Some(t)) = (i, t) else { return };
        if self.assignable_to(t, i) {
            self.assign(Some(i), Some(t));
        }
    }

    /// `compare(x, y)`, called for each comparison `x == y`.
    fn compare(&mut self, x: Option<TypeId>, y: Option<TypeId>) {
        let (Some(x), Some(y)) = (x, y) else { return };
        if self.assignable_to(x, y) {
            self.assign(Some(y), Some(x));
        } else if self.assignable_to(y, x) {
            self.assign(Some(x), Some(y));
        }
    }

    /// `exprN`: an expression in a multi-value context. Returns its tuple type.
    fn expr_n(&mut self, e: &Expr) -> Option<TypeId> {
        let typ = self.tv_type(e).filter(|&t| matches!(self.types.get(t), TypeData::Tuple(_)));
        match e {
            Expr::ParenExpr(p) => return self.expr_n(&p.x),
            Expr::CallExpr(call) => {
                // x, err := f(args)
                let ft = self.expr(&call.fun);
                if let Some(sig) = self.signature_of(ft) {
                    self.call(sig, &call.args);
                }
            }
            Expr::IndexExpr(ix) => {
                // y, ok := x[i]
                let x = self.expr(&ix.x);
                let i = self.expr(&ix.index);
                if let Some(m) = self.map_of(x) {
                    let k = guff_types::map::map_key(self.types, m);
                    self.assign(i, Some(k));
                }
            }
            Expr::TypeAssertExpr(ta) => {
                // y, ok := x.(T)
                if typ.is_some_and(|t| self.tuple_len(Some(t)) > 0) {
                    let x = self.expr(&ta.x);
                    let t0 = typ.and_then(|t| self.tuple_at(t, 0));
                    self.type_assert(x, t0);
                } else {
                    self.expr(&ta.x);
                }
            }
            // y, ok := <-x
            Expr::UnaryExpr(u) => {
                self.expr(&u.x);
            }
            _ => {}
        }
        typ
    }

    fn call(&mut self, sig: TypeId, args: &[Expr]) {
        if args.is_empty() {
            return;
        }
        let params = guff_types::signature::signature_params(self.types, sig);
        let nparams = self.tuple_len(params);
        let param = |f: &Self, i: usize| params.and_then(|p| f.tuple_at(p, i));

        // Ellipsis call? `f(x, y, z...)`. (Go's parser records the `...` on
        // the CallExpr, not as an Ellipsis argument, so upstream never takes
        // this branch either; it is kept to mirror `find.go`.)
        if matches!(args.last(), Some(Expr::Ellipsis(_))) {
            for (i, arg) in args.iter().enumerate() {
                let a = self.expr(arg);
                self.assign(param(self, i), a);
            }
            return;
        }

        // The effective actual parameter types.
        let mut argtypes: Vec<Option<TypeId>> = Vec::new();
        let first = self.tv_type(&args[0]).filter(|&t| matches!(self.types.get(t), TypeData::Tuple(_)));
        if let Some(tuple) = first {
            // f(g()) where g has multiple results.
            self.expr(&args[0]);
            for i in 0..self.tuple_len(Some(tuple)) {
                argtypes.push(self.tuple_at(tuple, i));
            }
        } else {
            for arg in args {
                argtypes.push(self.expr(arg));
            }
        }

        // Assign the actuals to the formals.
        if !guff_types::signature::signature_variadic(self.types, sig) {
            for (i, at) in argtypes.into_iter().enumerate() {
                if i >= nparams {
                    break;
                }
                self.assign(param(self, i), at);
            }
        } else {
            // The first n-1 parameters are assigned normally; the rest are
            // elements of the variadic slice.
            let nnormals = nparams.saturating_sub(1);
            let elem = param(self, nnormals).and_then(|s| match self.types.get(s.underlying(self.types)) {
                TypeData::Slice(s) => Some(s.elem()),
                _ => None,
            });
            for (i, at) in argtypes.into_iter().enumerate() {
                if i < nnormals {
                    self.assign(param(self, i), at);
                } else {
                    self.assign(elem, at);
                }
            }
        }
    }

    /// `builtin`: the arguments of a call to the builtin named `name`.
    fn builtin(&mut self, name: &str, sig: Option<TypeId>, args: &[Expr]) {
        match name {
            "make" | "new" => {
                for (i, arg) in args.iter().enumerate() {
                    let is_type = self
                        .info
                        .types
                        .get(&arg.id())
                        .is_some_and(|tv| tv.mode == OperandMode::TypeExpr);
                    if i == 0 && is_type {
                        continue; // the type operand
                    }
                    self.expr(arg);
                }
            }
            "append" => {
                let Some(first) = args.first() else { return };
                let s = self.expr(first);
                if args.len() == 2 && matches!(args.last(), Some(Expr::Ellipsis(_))) {
                    // append(x, y...), including append([]byte, "foo"...)
                    self.expr(&args[1]);
                } else {
                    // append(x, y, z)
                    let elem = self
                        .underlying_term(s, |d| matches!(d, TypeData::Slice(_)))
                        .map(|s| guff_types::slice::slice_elem(self.types, s));
                    for arg in &args[1..] {
                        let a = self.expr(arg);
                        if elem.is_some() {
                            self.assign(elem, a);
                        }
                    }
                }
            }
            "delete" => {
                if args.len() < 2 {
                    for arg in args {
                        self.expr(arg);
                    }
                    return;
                }
                let m = self.expr(&args[0]);
                let k = self.expr(&args[1]);
                if let Some(m) = self.map_of(m) {
                    let key = guff_types::map::map_key(self.types, m);
                    self.assign(Some(key), k);
                }
            }
            // An ordinary call.
            _ => match sig {
                Some(sig) => self.call(sig, args),
                None => {
                    for arg in args {
                        self.expr(arg);
                    }
                }
            },
        }
    }

    fn value_spec(&mut self, spec: &ValueSpec) {
        let t = spec.ty.as_ref().and_then(|ty| self.tv_type(ty));
        if spec.values.len() == spec.names.len() {
            // var x, y = f(), g()
            for value in &spec.values {
                let v = self.expr(value);
                if t.is_some() {
                    self.assign(t, v);
                }
            }
        } else if spec.values.len() == 1 {
            // var x, y = f()
            let tuple = self.expr_n(&spec.values[0]);
            for i in 0..spec.names.len() {
                if t.is_some() {
                    let v = tuple.and_then(|tu| self.tuple_at(tu, i));
                    self.assign(t, v);
                }
            }
        }
    }

    /// `expr`: a true expression (not a type, not a defining identifier).
    /// Returns its type, `None` standing for `Typ[Invalid]`.
    fn expr(&mut self, e: &Expr) -> Option<TypeId> {
        let tv = self.info.types.get(&e.id());
        let tv_type = tv.map(|tv| tv.typ);
        if tv.is_some_and(|tv| tv.val.is_some()) {
            return tv_type; // a constant: prune the descent
        }

        match e {
            Expr::BadExpr(_) | Expr::BasicLit(_) => {}

            Expr::Ident(id) => {
                // Referring identifiers only.
                return self.info.uses.get(&id.id).and_then(|&obj| obj.typ(self.objects));
            }

            Expr::Ellipsis(el) => {
                if let Some(elt) = &el.elt {
                    self.expr(elt);
                }
            }

            Expr::FuncLit(lit) => {
                let saved = self.sig;
                self.sig = tv_type.filter(|&t| self.is_signature(t));
                self.block(&lit.body);
                self.sig = saved;
            }

            Expr::CompositeLit(cl) => {
                if let Some(t) = tv_type {
                    self.composite_lit(t, &cl.elts);
                }
            }

            Expr::ParenExpr(p) => {
                self.expr(&p.x);
            }

            Expr::SelectorExpr(sel) => match self.info.selections.get(&sel.id) {
                Some(seln) => {
                    // `interface{ m() }.m`: e.X is a type, not visited.
                    if seln.kind() != SelectionKind::MethodExpr {
                        self.expr(&sel.x);
                    }
                }
                None => {
                    // A qualified identifier.
                    return self.info.uses.get(&sel.sel.id).and_then(|&obj| obj.typ(self.objects));
                }
            },

            Expr::IndexExpr(ix) => {
                if self.is_instance(&ix.x) {
                    // f[T] or C[T]: a generic instantiation.
                } else {
                    // x[i] or m[k]: an index or lookup.
                    let x = self.expr(&ix.x);
                    let i = self.expr(&ix.index);
                    if let Some(m) = self.map_of(x) {
                        let k = guff_types::map::map_key(self.types, m);
                        self.assign(Some(k), i);
                    }
                }
            }

            // f[X, Y]: a generic instantiation.
            Expr::IndexListExpr(_) => {}

            Expr::SliceExpr(s) => {
                self.expr(&s.x);
                for part in [&s.low, &s.high, &s.max].into_iter().flatten() {
                    self.expr(part);
                }
            }

            Expr::TypeAssertExpr(ta) => {
                let x = self.expr(&ta.x);
                let t = ta.ty.as_ref().and_then(|ty| self.tv_type(ty));
                self.type_assert(x, t);
            }

            Expr::CallExpr(call) => self.call_expr(call),

            Expr::StarExpr(s) => {
                self.expr(&s.x);
            }

            Expr::UnaryExpr(u) => {
                self.expr(&u.x);
            }

            Expr::BinaryExpr(b) => {
                let x = self.expr(&b.x);
                let y = self.expr(&b.y);
                if b.op == Token::EQL || b.op == Token::NEQ {
                    self.compare(x, y);
                }
            }

            Expr::KeyValueExpr(kv) => {
                self.expr(&kv.key);
                self.expr(&kv.value);
            }

            // Types are not expressions; upstream panics here.
            Expr::ArrayType(_)
            | Expr::StructType(_)
            | Expr::FuncType(_)
            | Expr::InterfaceType(_)
            | Expr::MapType(_)
            | Expr::ChanType(_) => {}
        }

        tv_type
    }

    fn call_expr(&mut self, call: &CallExpr) {
        let fun_tv = self.info.types.get(&call.fun.id());
        if fun_tv.is_some_and(|tv| tv.mode == OperandMode::TypeExpr) {
            // A conversion T(x).
            let t = fun_tv.map(|tv| tv.typ);
            if let Some(arg0) = call.args.first() {
                let a = self.expr(arg0);
                self.assign(t, a);
            }
            return;
        }

        // A call to a function in unsafe is an ordinary call, except that its
        // signature cannot come from the function object.
        if let Expr::SelectorExpr(s) = unparen(&call.fun) {
            if let Some(&obj) = self.info.uses.get(&s.sel.id) {
                if matches!(self.objects.get(obj), ObjectData::Builtin(_)) {
                    match fun_tv.map(|tv| tv.typ).filter(|&t| self.is_signature(t)) {
                        Some(sig) => self.call(sig, &call.args),
                        None => {
                            for arg in &call.args {
                                self.expr(arg);
                            }
                        }
                    }
                    return;
                }
            }
        }

        // A call to a builtin.
        if let Expr::Ident(id) = unparen(&call.fun) {
            if let Some(&obj) = self.info.uses.get(&id.id) {
                if matches!(self.objects.get(obj), ObjectData::Builtin(_)) {
                    let sig = self
                        .info
                        .types
                        .get(&unparen(&call.fun).id())
                        .map(|tv| tv.typ)
                        .filter(|&t| self.is_signature(t));
                    let name = id.name.clone();
                    self.builtin(&name, sig, &call.args);
                    return;
                }
            }
        }

        // An ordinary call.
        let ft = self.expr(&call.fun);
        if let Some(sig) = self.signature_of(ft) {
            self.call(sig, &call.args);
        }
    }

    fn composite_lit(&mut self, t: TypeId, elts: &[Expr]) {
        // `typeparams.Deref(tv.Type)`: `&T{…}` is typed `*T`.
        let t = match self.types.get(t.underlying(self.types)) {
            TypeData::Pointer(p) => p.elem(),
            _ => t,
        };
        for term in self.terms(t) {
            let u = term.underlying(self.types);
            match self.types.get(u) {
                TypeData::Struct(_) => {
                    for (i, elem) in elts.iter().enumerate() {
                        if let Expr::KeyValueExpr(kv) = elem {
                            // In weird code the key might not be an identifier.
                            let field = match &*kv.key {
                                Expr::Ident(id) => self.info.uses.get(&id.id).copied(),
                                _ => None,
                            };
                            let v = self.expr(&kv.value);
                            if let Some(field) = field {
                                let ft = field.typ(self.objects);
                                self.assign(ft, v);
                            }
                        } else {
                            let v = self.expr(elem);
                            if i < guff_types::struct_num_fields(self.types, u) {
                                let ft = guff_types::struct_field(self.types, u, i).typ(self.objects);
                                self.assign(ft, v);
                            }
                        }
                    }
                }
                TypeData::Map(_) => {
                    let key = guff_types::map::map_key(self.types, u);
                    let val = guff_types::map::map_elem(self.types, u);
                    for elem in elts {
                        if let Expr::KeyValueExpr(kv) = elem {
                            let k = self.expr(&kv.key);
                            self.assign(Some(key), k);
                            let v = self.expr(&kv.value);
                            self.assign(Some(val), v);
                        } else {
                            self.expr(elem);
                        }
                    }
                }
                TypeData::Array(_) | TypeData::Slice(_) => {
                    let el = match self.types.get(u) {
                        TypeData::Array(a) => a.elem(),
                        TypeData::Slice(s) => s.elem(),
                        _ => unreachable!(),
                    };
                    for elem in elts {
                        // The key of `[...]T{k: v}` is ignored.
                        let v = match elem {
                            Expr::KeyValueExpr(kv) => self.expr(&kv.value),
                            _ => self.expr(elem),
                        };
                        self.assign(Some(el), v);
                    }
                }
                _ => {
                    // An invalid literal type: visit everything. (Upstream
                    // panics on any other type.)
                    for elem in elts {
                        if let Expr::KeyValueExpr(kv) = elem {
                            self.expr(&kv.key);
                            self.expr(&kv.value);
                        } else {
                            self.expr(elem);
                        }
                    }
                }
            }
        }
    }

    /// `instance(info, expr)`: does `expr` name an instantiated generic?
    fn is_instance(&self, e: &Expr) -> bool {
        let id = match e {
            Expr::Ident(id) => id,
            Expr::SelectorExpr(s) => &s.sel,
            _ => return false,
        };
        self.info.instances.contains_key(&id.id)
    }

    fn block(&mut self, b: &BlockStmt) {
        for s in &b.list {
            self.stmt(s);
        }
    }

    fn stmts(&mut self, list: &[Stmt]) {
        for s in list {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::BadStmt(_) | Stmt::EmptyStmt(_) | Stmt::BranchStmt(_) => {}

            Stmt::DeclStmt(d) => {
                if let Decl::GenDecl(g) = &d.decl {
                    // Consts are ignored.
                    if g.tok == Some(Token::VAR) {
                        for spec in &g.specs {
                            if let Spec::ValueSpec(vs) = spec {
                                self.value_spec(vs);
                            }
                        }
                    }
                }
            }

            Stmt::LabeledStmt(l) => self.stmt(&l.stmt),

            Stmt::ExprStmt(e) => {
                self.expr(&e.x);
            }

            Stmt::SendStmt(s) => {
                let ch = self.expr(&s.chan_);
                let val = self.expr(&s.value);
                if let Some(c) = self.underlying_term(ch, |d| matches!(d, TypeData::Chan(_))) {
                    let elem = guff_types::chan::chan_elem(self.types, c);
                    self.assign(Some(elem), val);
                }
            }

            Stmt::IncDecStmt(s) => {
                self.expr(&s.x);
            }

            Stmt::AssignStmt(s) => match s.tok {
                Some(Token::ASSIGN) | Some(Token::DEFINE) => {
                    // y := x  or  y = x
                    let rhs_tuple = if s.lhs.len() != s.rhs.len() {
                        match s.rhs.first() {
                            Some(r) => Some(self.expr_n(r)),
                            None => return,
                        }
                    } else {
                        None
                    };
                    for i in 0..s.lhs.len() {
                        let rhs = match rhs_tuple {
                            None => match s.rhs.get(i) {
                                Some(r) => self.expr(r), // 1:1 assignment
                                None => None,
                            },
                            Some(tuple) => tuple.and_then(|t| self.tuple_at(t, i)), // n:1
                        };
                        let mut lhs = None;
                        let mut defined = false;
                        if let Expr::Ident(id) = &s.lhs[i] {
                            if id.name != "_" {
                                if let Some(def) = self.info.defs.get(&id.id) {
                                    // A definition.
                                    defined = true;
                                    lhs = def.and_then(|obj| obj.typ(self.objects));
                                }
                            }
                        }
                        if !defined || lhs.is_none() {
                            lhs = self.expr(&s.lhs[i]); // an assignment
                        }
                        self.assign(lhs, rhs);
                    }
                }
                _ => {
                    // y op= x
                    if let Some(l) = s.lhs.first() {
                        self.expr(l);
                    }
                    if let Some(r) = s.rhs.first() {
                        self.expr(r);
                    }
                }
            },

            Stmt::GoStmt(g) => self.call_expr(&g.call),
            Stmt::DeferStmt(d) => self.call_expr(&d.call),

            Stmt::ReturnStmt(r) => {
                let Some(sig) = self.sig else {
                    for result in &r.results {
                        self.expr(result);
                    }
                    return;
                };
                let formals = guff_types::signature::signature_results(self.types, sig);
                let n = self.tuple_len(formals);
                let formal = |f: &Self, i: usize| formals.and_then(|t| f.tuple_at(t, i));
                if r.results.len() == n {
                    // 1:1
                    for (i, result) in r.results.iter().enumerate() {
                        let v = self.expr(result);
                        self.assign(formal(self, i), v);
                    }
                } else if r.results.len() == 1 {
                    // n:1
                    let tuple = self.expr_n(&r.results[0]);
                    for i in 0..n {
                        let v = tuple.and_then(|t| self.tuple_at(t, i));
                        self.assign(formal(self, i), v);
                    }
                }
            }

            Stmt::SelectStmt(s) => self.block(&s.body),

            Stmt::BlockStmt(b) => self.block(b),

            Stmt::IfStmt(s) => {
                if let Some(init) = &s.init {
                    self.stmt(init);
                }
                self.expr(&s.cond);
                self.block(&s.body);
                if let Some(e) = &s.else_ {
                    self.stmt(e);
                }
            }

            Stmt::SwitchStmt(s) => {
                if let Some(init) = &s.init {
                    self.stmt(init);
                }
                // A tagless switch compares against `untyped bool`, which is
                // never an interface and has no methods: nothing to record.
                let tag = s.tag.as_ref().map(|t| self.expr(t));
                for cc in &s.body.list {
                    let Stmt::CaseClause(cc) = cc else { continue };
                    if let Some(tag) = tag {
                        for cond in &cc.list {
                            let ct = self.tv_type(cond);
                            self.compare(tag, ct);
                        }
                    }
                    self.stmts(&cc.body);
                }
            }

            Stmt::TypeSwitchStmt(s) => {
                if let Some(init) = &s.init {
                    self.stmt(init);
                }
                let x = match &*s.assign {
                    // x.(type)
                    Stmt::ExprStmt(e) => match unparen(&e.x) {
                        Expr::TypeAssertExpr(ta) => Some(&ta.x),
                        _ => None,
                    },
                    // y := x.(type)
                    Stmt::AssignStmt(a) => match a.rhs.first().map(unparen) {
                        Some(Expr::TypeAssertExpr(ta)) => Some(&ta.x),
                        _ => None,
                    },
                    _ => None,
                };
                let i = x.and_then(|x| self.expr(x));
                for cc in &s.body.list {
                    let Stmt::CaseClause(cc) = cc else { continue };
                    for cond in &cc.list {
                        let ct = self.tv_type(cond);
                        let is_nil = ct.is_some_and(|t| {
                            matches!(self.types.get(t), TypeData::Basic(b) if b.kind() == BasicKind::UntypedNil)
                        });
                        if !is_nil {
                            self.type_assert(i, ct);
                        }
                    }
                    self.stmts(&cc.body);
                }
            }

            Stmt::CommClause(c) => {
                if let Some(comm) = &c.comm {
                    self.stmt(comm);
                }
                self.stmts(&c.body);
            }

            Stmt::CaseClause(c) => {
                // Only reached through a switch, which visits clauses itself.
                self.stmts(&c.body);
            }

            Stmt::ForStmt(s) => {
                if let Some(init) = &s.init {
                    self.stmt(init);
                }
                if let Some(cond) = &s.cond {
                    self.expr(cond);
                }
                if let Some(post) = &s.post {
                    self.stmt(post);
                }
                self.block(&s.body);
            }

            Stmt::RangeStmt(s) => {
                let x = self.expr(&s.x);
                // No conversions are involved when Tok == DEFINE.
                if s.tok == Some(Token::ASSIGN) {
                    let terms = match x {
                        Some(x) => self.terms(x),
                        None => Vec::new(),
                    };
                    if let Some(key) = &s.key {
                        let k = self.expr(key);
                        // Keys of arrays, slices and strings are ints:
                        // uninteresting.
                        for &term in &terms {
                            let u = term.underlying(self.types);
                            match self.types.get(u) {
                                TypeData::Chan(c) => {
                                    let e = c.elem();
                                    self.assign(k, Some(e));
                                }
                                TypeData::Map(_) => {
                                    let mk = guff_types::map::map_key(self.types, u);
                                    self.assign(k, Some(mk));
                                }
                                _ => {}
                            }
                        }
                    }
                    if let Some(value) = &s.value {
                        let val = self.expr(value);
                        // String values are runes: uninteresting.
                        for &term in &terms {
                            let u = term.underlying(self.types);
                            let elem = match self.types.get(u) {
                                TypeData::Pointer(p) => {
                                    // A pointer to an array.
                                    let pe = p.elem();
                                    for t in self.terms(pe) {
                                        let tu = t.underlying(self.types);
                                        if let TypeData::Array(a) = self.types.get(tu) {
                                            let e = a.elem();
                                            self.assign(val, Some(e));
                                        }
                                    }
                                    None
                                }
                                TypeData::Array(a) => Some(a.elem()),
                                TypeData::Map(_) => Some(guff_types::map::map_elem(self.types, u)),
                                TypeData::Slice(sl) => Some(sl.elem()),
                                _ => None,
                            };
                            if elem.is_some() {
                                self.assign(val, elem);
                            }
                        }
                    }
                }
                self.block(&s.body);
            }
        }
    }
}
