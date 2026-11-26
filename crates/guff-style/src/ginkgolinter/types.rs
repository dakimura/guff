//! go/types queries ginkgolinter makes, over one package's type artifacts.
//!
//! Upstream asks `types.Implements` against three kinds of interface: the
//! universe `error`, Gomega's own `Assertion` / `AsyncAssertion` /
//! `GomegaMatcher` found in the package's import graph, and two it builds by
//! hand (`internal/typecheck`: a `GomegaMatcher` look-alike and four
//! one-method `testing.TB` slices). The hand-built ones are checked here by
//! method lookup — the same question `missingMethod` asks of a synthetic
//! interface — because the type arena cannot grow new objects for them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use guff::ast::Expr;
use guff::position::FileSet;
use guff_analysis::Pass;
use guff_constant::Value;
use guff_types::api::{Info, TypeAndValue};
use guff_types::api_predicates::{api_identical, api_implements};
use guff_types::arena::{ObjectArena, ObjectData, PackageArena, PackageId, ScopeArena, TypeArena, TypeData, TypeId};
use guff_types::basic::BasicKind;
use guff_types::lookup::{lookup_field_or_method, LookupResult};
use guff_types::operand::OperandMode;

pub(super) struct Tc<'p> {
    info: Option<&'p Info>,
    fset: &'p Arc<FileSet>,
    scopes: &'p ScopeArena,
    base: &'p TypeArena,
    /// A copy of `base`, made on the first query: `api_implements` and method
    /// lookup take `&mut TypeArena` (computing a method set can intern), and
    /// a package that imports neither Ginkgo nor Gomega never asks.
    types: RefCell<Option<TypeArena>>,
    objects: &'p ObjectArena,
    packages: &'p PackageArena,
    pkg: PackageId,
    error: Option<TypeId>,
    any: Option<TypeId>,
    boolean: Option<TypeId>,
    string: Option<TypeId>,
    memo: RefCell<HashMap<(TypeId, TypeId), bool>>,
}

impl<'p> Tc<'p> {
    pub(super) fn new(pass: &'p Pass<'_>) -> Option<Self> {
        let artifacts = pass.pkg().type_artifacts.as_ref()?;
        let mut tc = Tc {
            info: pass.types_info(),
            fset: pass.fset(),
            scopes: &artifacts.scopes,
            base: &artifacts.types,
            types: RefCell::new(None),
            objects: &artifacts.objects,
            packages: &artifacts.packages,
            pkg: artifacts.type_pkg,
            error: None,
            any: None,
            boolean: None,
            string: None,
            memo: RefCell::new(HashMap::new()),
        };
        tc.error = tc.universe_type("error");
        tc.any = tc.universe_type("any");
        tc.boolean = tc.universe_type("bool");
        tc.string = tc.universe_type("string");
        Some(tc)
    }

    fn ta(&self) -> std::cell::RefMut<'_, TypeArena> {
        let mut b = self.types.borrow_mut();
        if b.is_none() {
            *b = Some(self.base.clone());
        }
        std::cell::RefMut::map(b, |o| o.as_mut().expect("arena copy"))
    }

    fn universe_type(&self, name: &str) -> Option<TypeId> {
        for oid in self.objects.ids() {
            let ObjectData::TypeName(tn) = self.objects.get(oid) else {
                continue;
            };
            if tn.name() == name && oid.pkg(self.objects).is_none() {
                return oid.typ(self.objects);
            }
        }
        None
    }

    pub(super) fn fset(&self) -> &'p Arc<FileSet> {
        self.fset
    }

    /// `pass.TypesInfo.Types[e]`.
    pub(super) fn tv(&self, e: &Expr) -> Option<&'p TypeAndValue> {
        self.tv_id(e.id())
    }

    pub(super) fn tv_id(&self, id: u32) -> Option<&'p TypeAndValue> {
        if id == 0 {
            return None;
        }
        self.info?.types.get(&id)
    }

    /// `pass.TypesInfo.TypeOf(e)`: the recorded type, or for an identifier
    /// the type of the object it defines or uses.
    pub(super) fn type_of(&self, e: &Expr) -> Option<TypeId> {
        if let Some(tv) = self.tv(e) {
            return Some(tv.typ);
        }
        let Expr::Ident(id) = e else {
            return None;
        };
        let info = self.info?;
        let obj = info
            .uses
            .get(&id.id)
            .copied()
            .or_else(|| info.defs.get(&id.id).copied().flatten())?;
        obj.typ(self.objects)
    }

    pub(super) fn data(&self, t: TypeId) -> TypeData {
        self.ta().get(t).clone()
    }

    pub(super) fn underlying(&self, t: TypeId) -> TypeId {
        t.underlying(&*self.ta())
    }

    pub(super) fn unalias(&self, t: TypeId) -> TypeId {
        guff_types::alias::unalias(&mut *self.ta(), t)
    }

    /// `types.TypeString(t, nil)`, which is what `%s` of a `types.Type` prints.
    pub(super) fn type_string(&self, t: Option<TypeId>) -> String {
        match t {
            Some(t) => guff_types::typestring::type_string(
                &*self.ta(),
                self.objects,
                self.packages,
                t,
                None,
            ),
            None => "%!s(<nil>)".to_string(),
        }
    }

    pub(super) fn string_type(&self) -> Option<TypeId> {
        self.string
    }

    pub(super) fn identical(&self, a: Option<TypeId>, b: Option<TypeId>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => api_identical(
                &mut *self.ta(),
                self.objects,
                self.packages,
                a,
                b,
            ),
            _ => false,
        }
    }

    /// `types.IsInterface`.
    pub(super) fn is_interface(&self, t: Option<TypeId>) -> bool {
        t.is_some_and(|t| matches!(self.data(self.underlying(t)), TypeData::Interface(_)))
    }

    pub(super) fn is_pointer(&self, t: Option<TypeId>) -> bool {
        t.is_some_and(|t| matches!(self.data(t), TypeData::Pointer(_)))
    }

    pub(super) fn is_named(&self, t: TypeId) -> bool {
        matches!(self.data(t), TypeData::Named(_))
    }

    pub(super) fn pointer_elem(&self, t: TypeId) -> Option<TypeId> {
        match self.data(t) {
            TypeData::Pointer(p) => Some(p.elem()),
            _ => None,
        }
    }

    pub(super) fn basic_kind(&self, t: TypeId) -> Option<(BasicKind, guff_types::basic::BasicInfo, String)> {
        match self.data(t) {
            TypeData::Basic(b) => Some((b.kind(), b.info(), b.name().to_string())),
            _ => None,
        }
    }

    /// The elements of a tuple type, `None` when `t` is not a tuple.
    pub(super) fn tuple(&self, t: TypeId) -> Option<Vec<TypeId>> {
        if !matches!(self.data(t), TypeData::Tuple(_)) {
            return None;
        }
        Some(self.tuple_types(Some(t)))
    }

    fn tuple_types(&self, t: Option<TypeId>) -> Vec<TypeId> {
        let types = self.ta();
        let n = guff_types::tuple::tuple_len(&types, t);
        let Some(t) = t else {
            return Vec::new();
        };
        (0..n)
            .filter_map(|i| guff_types::tuple::tuple_at(&types, t, i).typ(self.objects))
            .collect()
    }

    /// `(params, results, variadic)` of a signature type — `t` itself, not its
    /// underlying type, as upstream's `t.(*types.Signature)` asks.
    pub(super) fn signature(&self, t: TypeId) -> Option<(Vec<TypeId>, Vec<TypeId>, bool)> {
        let TypeData::Signature(sig) = self.data(t) else {
            return None;
        };
        Some((
            self.tuple_types(sig.params()),
            self.tuple_types(sig.results()),
            sig.variadic(),
        ))
    }

    /// `types.Implements(v, iface)` for an interface the package can name.
    pub(super) fn implements(&self, v: TypeId, iface: TypeId) -> bool {
        if let Some(hit) = self.memo.borrow().get(&(v, iface)) {
            return *hit;
        }
        let iface_u = self.underlying(iface);
        let answer = api_implements(
            &mut *self.ta(),
            self.objects,
            self.packages,
            v,
            iface_u,
        );
        self.memo.borrow_mut().insert((v, iface), answer);
        answer
    }

    /// `typecheck.ImplementsError`.
    pub(super) fn implements_error(&self, t: TypeId) -> bool {
        self.error.is_some_and(|e| self.implements(t, e))
    }

    /// Does `t`'s method set hold `name` with exactly this signature?
    fn has_method(&self, t: TypeId, name: &str, params: &[Want], results: &[Want], variadic: bool) -> bool {
        let found = lookup_field_or_method(
            &mut *self.ta(),
            self.objects,
            self.packages,
            t,
            false,
            None,
            name,
        );
        let LookupResult::Found { obj, .. } = found else {
            return false;
        };
        if !matches!(self.objects.get(obj), ObjectData::Func(_)) {
            return false;
        }
        let Some(sig) = obj.typ(self.objects) else {
            return false;
        };
        let Some((p, r, v)) = self.signature(sig) else {
            return false;
        };
        v == variadic
            && p.len() == params.len()
            && r.len() == results.len()
            && p.iter().zip(params).all(|(t, w)| self.is_want(*t, *w))
            && r.iter().zip(results).all(|(t, w)| self.is_want(*t, *w))
    }

    fn is_want(&self, t: TypeId, w: Want) -> bool {
        let want = match w {
            Want::Any => self.any,
            Want::Bool => self.boolean,
            Want::String => self.string,
            Want::Error => self.error,
            Want::SliceOfAny => {
                return match self.data(t) {
                    TypeData::Slice(s) => self.is_want(s.elem(), Want::Any),
                    _ => false,
                };
            }
        };
        self.identical(Some(t), want)
    }

    /// `typecheck.ImplementsGomegaMatcher`: the three-method interface
    /// upstream spells out by hand.
    pub(super) fn implements_gomega_matcher(&self, t: Option<TypeId>) -> bool {
        let Some(t) = t else {
            return false;
        };
        self.has_method(t, "Match", &[Want::Any], &[Want::Bool, Want::Error], false)
            && self.has_method(t, "FailureMessage", &[Want::Any], &[Want::String], false)
            && self.has_method(t, "NegatedFailureMessage", &[Want::Any], &[Want::String], false)
    }

    /// `typecheck.ImplementsTB`: any one of `Error(...any)`,
    /// `Errorf(string, ...any)`, `Fatal(...any)`, `Fatalf(string, ...any)`.
    pub(super) fn implements_tb(&self, t: TypeId) -> bool {
        self.has_method(t, "Error", &[Want::SliceOfAny], &[], true)
            || self.has_method(t, "Errorf", &[Want::String, Want::SliceOfAny], &[], true)
            || self.has_method(t, "Fatal", &[Want::SliceOfAny], &[], true)
            || self.has_method(t, "Fatalf", &[Want::String, Want::SliceOfAny], &[], true)
    }

    /// `gomegahandler.getPackage`: search the import graph for a package whose
    /// path ends in `suffix`.
    pub(super) fn find_package(&self, suffix: &str) -> Option<PackageId> {
        let mut seen = std::collections::HashSet::new();
        // Depth-first in import order, like upstream's recursion.
        fn rec(
            packages: &PackageArena,
            pid: PackageId,
            suffix: &str,
            seen: &mut std::collections::HashSet<PackageId>,
        ) -> Option<PackageId> {
            if !seen.insert(pid) {
                return None;
            }
            let p = packages.get(pid);
            if p.path().ends_with(suffix) {
                return Some(pid);
            }
            for imp in p.imports() {
                if let Some(found) = rec(packages, *imp, suffix, seen) {
                    return Some(found);
                }
            }
            None
        }
        rec(self.packages, self.pkg, suffix, &mut seen)
    }

    /// `lookupInterface`: a package-scope type whose underlying type is an
    /// interface.
    pub(super) fn lookup_interface(&self, pkg: PackageId, name: &str) -> Option<TypeId> {
        let scope = self.packages.get(pkg).scope();
        let obj = guff_types::scope::lookup(self.scopes, scope, name)?;
        let t = obj.typ(self.objects)?;
        matches!(self.data(self.underlying(t)), TypeData::Interface(_)).then_some(t)
    }
}

#[derive(Clone, Copy)]
enum Want {
    Any,
    Bool,
    String,
    Error,
    SliceOfAny,
}

/// `TypeAndValue.IsNil`.
pub(super) fn tv_is_nil(tv: &TypeAndValue) -> bool {
    tv.mode == OperandMode::NilValue
}

/// `TypeAndValue.IsValue`.
pub(super) fn tv_is_value(tv: &TypeAndValue) -> bool {
    matches!(
        tv.mode,
        OperandMode::Constant
            | OperandMode::Variable
            | OperandMode::MapIndex
            | OperandMode::Value
            | OperandMode::NilValue
            | OperandMode::CommaOk
            | OperandMode::CommaErr
    )
}

/// `constant.Int64Val` — `None` unless the value is an exact int64.
pub(super) fn int64(v: &Value) -> Option<i64> {
    if v.kind() != guff_constant::Kind::Int {
        return None;
    }
    let (n, exact) = guff_constant::int64_val(v);
    exact.then_some(n)
}
