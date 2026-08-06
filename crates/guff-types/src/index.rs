//! Port of index/slice expression type-checking from
//! `cmd/compile/internal/types2/index.go`.
//!
//! **Chunk 28**: [`Checker::index_expr`] (`a[i]`), [`Checker::slice_expr`]
//! (`a[lo:hi]` / `a[lo:hi:max]`), and the index helpers [`Checker::index`] /
//! [`Checker::is_valid_index`].
//!
//! Our AST is go/ast-shaped, so an `IndexExpr` carries a *single* `index`
//! expression (multi-index generic instantiation is a separate `IndexListExpr`)
//! — Go's `singleIndex` / `ListExpr` unpacking is therefore unnecessary.
//!
//! ## Deferrals (chunk-28, see §8)
//!
//! - **Function instantiation** `f[int]` (chunk 71): [`Checker::index_expr`]
//!   returns `true` for a generic-function operand so the caller runs
//!   [`Checker::func_inst`]. Generic *type* instantiation `T[int]` in a pure
//!   expression position still invalidates (types go through `typexpr`).
//! - **Type-parameter operands** are handled by [`Checker::index_type_param`]
//!   and [`Checker::slice_common_under`] (Go's `Interface`/`underIs` branch):
//!   every type in the type set must be indexable/sliceable the same way.
//! - Constant string indexing computes the length from the value, but the
//!   3-index-slice-of-string and constant-string-length niceties match Go.
//! - `record` / `hasCallOrRecv` are omitted (Info recording deferred, §18b).

use guff::ast::{Expr, IndexExpr, SliceExpr};
use guff_constant::{int64_val, sign, string_val, Kind, Value};
use guff_types_errors::Code;

use crate::arena::{TypeData, TypeId};
use crate::array::{array_elem, array_len};
use crate::basic::BasicKind;
use crate::check::Checker;
use crate::map::{map_elem, map_key};
use crate::operand::{Operand, OperandMode};
use crate::pointer::pointer_elem;
use crate::predicates::{is_integer, is_string, is_type_param, is_valid};
use crate::slice::{new_slice, slice_elem};

/// The element/length classification of an indexable operand's underlying type.
#[derive(Clone, Copy)]
enum Indexable {
    /// A string — indexing yields a `byte` value.
    Str,
    /// An array with element type and length.
    Array(TypeId, i64),
    /// A pointer to an array.
    PtrArray(TypeId, i64),
    /// A slice with element type.
    Slice(TypeId),
    /// A map with key and element type.
    Map(TypeId, TypeId),
    /// Not indexable.
    None,
}

/// Classify `under` (an already-underlying type) as an indexable operand.
fn classify_indexable(types: &crate::arena::TypeArena, under: TypeId) -> Indexable {
    match types.get(under) {
        TypeData::Basic(_) if is_string(types, under) => Indexable::Str,
        TypeData::Array(_) => Indexable::Array(array_elem(types, under), array_len(types, under)),
        TypeData::Pointer(_) => {
            let base = pointer_elem(types, under);
            let base_u = base.underlying(types);
            if matches!(types.get(base_u), TypeData::Array(_)) {
                Indexable::PtrArray(array_elem(types, base_u), array_len(types, base_u))
            } else {
                Indexable::None
            }
        }
        TypeData::Slice(_) => Indexable::Slice(slice_elem(types, under)),
        TypeData::Map(_) => Indexable::Map(map_key(types, under), map_elem(types, under)),
        _ => Indexable::None,
    }
}

impl Checker {
    /// Index a value whose type is a type parameter: every type in the type set
    /// must be indexable, all element types must be identical, and either all
    /// or none of them must be maps (with identical key types).
    ///
    /// Equivalent to the `*Interface` / `underIs` branch of `Checker.indexExpr`.
    /// Returns `false` like the ordinary path (never a generic func inst).
    fn index_type_param<'a>(
        &mut self,
        x: &mut Operand<'a>,
        e: &'a IndexExpr,
        t: TypeId,
    ) -> bool {
        let byte_t = self.basic(BasicKind::Uint8);
        let mut unders: Vec<Option<TypeId>> = Vec::new();
        crate::under::all(&mut self.types, &self.objects, &self.packages, t, |_, u| {
            unders.push(u);
            true
        });

        let mut length: i64 = -1;
        let mut key: Option<TypeId> = None;
        let mut elem: Option<TypeId> = None;
        // Result mode for the non-map case; a string element, or an array in a
        // non-addressable operand, downgrades it to a plain value.
        let mut mode = OperandMode::Variable;
        let mut ok = !unders.is_empty();

        for u in unders {
            let Some(u) = u else {
                ok = false;
                break;
            };
            let (l, k, el) = match classify_indexable(&self.types, u) {
                Indexable::Str => {
                    mode = OperandMode::Value;
                    (-1, None, Some(byte_t))
                }
                Indexable::Array(el, l) => {
                    if x.mode != OperandMode::Variable {
                        mode = OperandMode::Value;
                    }
                    (l, None, Some(el))
                }
                Indexable::PtrArray(el, l) => (l, None, Some(el)),
                Indexable::Slice(el) => (-1, None, Some(el)),
                Indexable::Map(k, el) => (-1, Some(k), Some(el)),
                Indexable::None => (-1, None, None),
            };
            let Some(el) = el else {
                ok = false;
                break;
            };
            match elem {
                None => {
                    length = l;
                    key = k;
                    elem = Some(el);
                }
                Some(prev_elem) => {
                    // Maps may not be mixed with anything else, and their key
                    // types must agree.
                    let keys_match = match (key, k) {
                        (None, None) => true,
                        (Some(a), Some(b)) => {
                            crate::predicates::identical(
                                &mut self.types,
                                &self.objects,
                                &self.packages,
                                a,
                                b,
                            )
                        }
                        _ => false,
                    };
                    if !keys_match
                        || !crate::predicates::identical(
                            &mut self.types,
                            &self.objects,
                            &self.packages,
                            prev_elem,
                            el,
                        )
                    {
                        ok = false;
                        break;
                    }
                    // Track the minimal array length across the type set.
                    if l >= 0 && (length < 0 || l < length) {
                        length = l;
                    }
                }
            }
        }

        let elem = match (ok, elem) {
            (true, Some(el)) => el,
            _ => {
                let xs = self.operand_str(x);
                self.error(
                    e.x.pos().0 as u32,
                    Code::NonSliceableOperand,
                    format!("cannot index {}", xs),
                );
                self.use1(&e.index);
                x.mode = OperandMode::Invalid;
                x.typ = Some(self.invalid_type());
                return false;
            }
        };

        if let Some(key) = key {
            let mut k = Operand::invalid();
            self.expr(&mut k, &e.index);
            self.assignment(&mut k, Some(key), "map index");
            // OK to continue even if indexing failed — the element type is known.
            x.mode = OperandMode::MapIndex;
            x.typ = Some(elem);
            return false;
        }

        x.mode = mode;
        x.typ = Some(elem);
        self.index(&e.index, length);
        false
    }

    /// The common underlying type shared by every type in a type parameter's
    /// type set, for the purpose of slicing. Strings are normalised to
    /// `[]byte` so a `~string | ~[]byte` constraint is sliceable; if any term
    /// was a string the result is `string` again (Go's `hasString` fixup).
    ///
    /// Returns `None` (after reporting) when the type set is empty or its
    /// members disagree.
    fn slice_common_under(
        &mut self,
        x: &Operand<'_>,
        e: &SliceExpr,
        t: TypeId,
    ) -> Option<TypeId> {
        let byte_slice = {
            let byte_t = self.basic(BasicKind::Uint8);
            new_slice(&mut self.types, byte_t)
        };
        let mut pairs: Vec<Option<TypeId>> = Vec::new();
        crate::under::all(&mut self.types, &self.objects, &self.packages, t, |_, u| {
            pairs.push(u);
            true
        });

        let mut cu: Option<TypeId> = None;
        let mut has_string = false;
        for u in pairs {
            let mut u = match u {
                Some(u) => u,
                None => {
                    let xs = self.operand_str(x);
                    self.error(
                        e.x.pos().0 as u32,
                        Code::NonSliceableOperand,
                        format!("cannot slice {}: no specific type in its type set", xs),
                    );
                    return None;
                }
            };
            if is_string(&self.types, u) {
                u = byte_slice;
                has_string = true;
            }
            match cu {
                None => cu = Some(u),
                Some(prev) => {
                    if !crate::predicates::identical(
                        &mut self.types,
                        &self.objects,
                        &self.packages,
                        prev,
                        u,
                    ) {
                        let xs = self.operand_str(x);
                        self.error(
                            e.x.pos().0 as u32,
                            Code::NonSliceableOperand,
                            format!("cannot slice {}: types in its type set have different underlying types", xs),
                        );
                        return None;
                    }
                }
            }
        }

        if cu.is_none() {
            let xs = self.operand_str(x);
            self.error(
                e.x.pos().0 as u32,
                Code::NonSliceableOperand,
                format!("cannot slice {}: no specific type in its type set", xs),
            );
            return None;
        }
        if has_string {
            // Proceed with the string type; a type parameter is always typed,
            // so this never turns an untyped string typed.
            return Some(self.basic(BasicKind::String));
        }
        cu
    }

    /// Type-check an index expression `x[i]`, recording the result in `x`.
    ///
    /// Returns `true` (without mutating `x`) when the operand is a generic
    /// function value — the caller then runs [`Checker::func_inst`] with the
    /// explicit type arguments (Go's `isFuncInst`). Otherwise it fully checks
    /// the index expression and returns `false`.
    ///
    /// Equivalent to `Checker.indexExpr` (chunk-28 subset, chunk-71 func inst).
    pub fn index_expr<'a>(&mut self, x: &mut Operand<'a>, e: &'a IndexExpr) -> bool {
        self.expr(x, &e.x);

        match x.mode {
            OperandMode::Invalid => {
                self.use1(&e.index);
                return false;
            }
            OperandMode::TypeExpr => {
                // Type instantiation `T[int]` — DEFERRED (generics).
                x.mode = OperandMode::Invalid;
                x.typ = Some(self.invalid_type());
                return false;
            }
            OperandMode::Value => {
                // Function instantiation `f[int]`: a generic function value
                // signals the caller (`call_expr` / `expr_internal`) to run
                // `func_inst` with the explicit type arguments. Leave `x`
                // unchanged (it still holds the generic signature).
                if self.is_generic_func_value(x) {
                    return true;
                }
            }
            _ => {}
        }

        let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
        let under = xtyp.underlying(&self.types);

        // A value of type-parameter type is indexable when *every* type in its
        // type set is, and they agree on the element (and, for maps, the key)
        // type — `func Head[S ~[]E, E any](s S) E { return s[0] }`.
        if is_type_param(&self.types, xtyp) {
            return self.index_type_param(x, e, xtyp);
        }

        // Classify the operand's underlying type (snapshot before mutating).
        let kind = classify_indexable(&self.types, under);

        let mut length: i64 = -1;
        match kind {
            Indexable::Str => {
                if x.mode == OperandMode::Constant {
                    if let Some(v) = &x.val {
                        length = string_val(v).len() as i64;
                    }
                }
                // An indexed string yields a (non-constant) byte value.
                x.mode = OperandMode::Value;
                x.typ = Some(self.basic(BasicKind::Uint8));
            }
            Indexable::Array(elem, len) => {
                length = len;
                if x.mode != OperandMode::Variable {
                    x.mode = OperandMode::Value;
                }
                x.typ = Some(elem);
            }
            Indexable::PtrArray(elem, len) => {
                length = len;
                x.mode = OperandMode::Variable;
                x.typ = Some(elem);
            }
            Indexable::Slice(elem) => {
                x.mode = OperandMode::Variable;
                x.typ = Some(elem);
            }
            Indexable::Map(key, elem) => {
                let mut k = Operand::invalid();
                self.expr(&mut k, &e.index);
                self.assignment(&mut k, Some(key), "map index");
                // OK to continue even if indexing failed — the element type is
                // known.
                x.mode = OperandMode::MapIndex;
                x.typ = Some(elem);
                // expr set by raw_expr / caller
                return false;
            }
            Indexable::None => {
                if is_valid(&self.types, under) {
                    let xs = self.operand_str(x);
                    self.error(
                        e.x.pos().0 as u32,
                        Code::NonSliceableOperand,
                        format!("cannot index {}", xs),
                    );
                }
                self.use1(&e.index);
                x.mode = OperandMode::Invalid;
                x.typ = Some(self.invalid_type());
                return false;
            }
        }

        // In pathological cases the element type may be unset; keep it valid.
        if x.typ.is_none() {
            x.typ = Some(self.invalid_type());
        }

        self.index(&e.index, length);
        // expr set by raw_expr / caller
        false
    }

    /// Type-check a slice expression `x[lo:hi]` / `x[lo:hi:max]`.
    ///
    /// Equivalent to `Checker.sliceExpr` (chunk-28 subset). The type-set
    /// iteration over type parameters is reduced to the operand's underlying
    /// type (the common concrete case).
    pub fn slice_expr<'a>(&mut self, x: &mut Operand<'a>, e: &'a SliceExpr) {
        self.expr(x, &e.x);
        if x.mode == OperandMode::Invalid {
            self.use_slice_indices(e);
            return;
        }

        let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
        // For a type-parameter operand every type in the type set must share
        // one underlying type — `func Tail[S ~[]E, E any](s S) S { return s[1:] }`.
        // The result type stays the type parameter itself, so only `cu` differs
        // from the ordinary path.
        let cu = if is_type_param(&self.types, xtyp) {
            match self.slice_common_under(x, e, xtyp) {
                Some(cu) => cu,
                None => {
                    x.mode = OperandMode::Invalid;
                    x.typ = Some(self.invalid_type());
                    return;
                }
            }
        } else {
            xtyp.underlying(&self.types)
        };

        let mut valid = false;
        let mut length: i64 = -1;

        match self.types.get(cu) {
            TypeData::Basic(_) if is_string(&self.types, cu) => {
                if e.slice3 {
                    self.error(
                        e.lbrack.0 as u32,
                        Code::InvalidSliceExpr,
                        "invalid operation: 3-index slice of string".to_string(),
                    );
                    x.mode = OperandMode::Invalid;
                    return;
                }
                valid = true;
                if x.mode == OperandMode::Constant {
                    if let Some(v) = &x.val {
                        length = string_val(v).len() as i64;
                    }
                }
                // For untyped string operands the result is a non-constant
                // value of type string.
                if !crate::predicates::is_typed(&self.types, xtyp) {
                    x.typ = Some(self.basic(BasicKind::String));
                }
            }
            TypeData::Array(_) => {
                length = array_len(&self.types, cu);
                if x.mode != OperandMode::Variable {
                    let xs = self.operand_str(x);
                    self.error(
                        e.x.pos().0 as u32,
                        Code::NonSliceableOperand,
                        format!("cannot slice unaddressable value {}", xs),
                    );
                    x.mode = OperandMode::Invalid;
                    return;
                }
                valid = true;
                let elem = array_elem(&self.types, cu);
                x.typ = Some(new_slice(&mut self.types, elem));
            }
            TypeData::Pointer(_) => {
                let base = pointer_elem(&self.types, cu);
                let base_u = base.underlying(&self.types);
                if matches!(self.types.get(base_u), TypeData::Array(_)) {
                    valid = true;
                    length = array_len(&self.types, base_u);
                    let elem = array_elem(&self.types, base_u);
                    x.typ = Some(new_slice(&mut self.types, elem));
                }
            }
            TypeData::Slice(_) => {
                valid = true;
                // x.typ doesn't change.
            }
            _ => {}
        }

        if !valid {
            let xs = self.operand_str(x);
            self.error(
                e.x.pos().0 as u32,
                Code::NonSliceableOperand,
                format!("cannot slice {}", xs),
            );
            x.mode = OperandMode::Invalid;
            return;
        }

        x.mode = OperandMode::Value;

        // Check the (up to three) indices, collecting their constant values.
        let exprs = [e.low.as_deref(), e.high.as_deref(), e.max.as_deref()];
        let mut ind: [i64; 3] = [-1, -1, -1];
        for (i, slot) in exprs.iter().enumerate() {
            let mut v: i64 = -1;
            match slot {
                Some(expr) => {
                    // Capacity is statically known (== length) for strings,
                    // arrays, and pointers to arrays.
                    let max = if length >= 0 { length + 1 } else { -1 };
                    let (_, val) = self.index(expr, max);
                    if val >= 0 {
                        v = val;
                    }
                }
                None if i == 0 => v = 0,
                None if length >= 0 => v = length,
                None => {}
            }
            ind[i] = v;
        }

        // Constant indices must be in non-decreasing order.
        'outer: for i in 0..ind.len() - 1 {
            let x_i = ind[i];
            if x_i > 0 {
                for (j, &y) in ind[i + 1..].iter().enumerate() {
                    if y >= 0 && y < x_i {
                        let at = exprs[i + 1 + j].map(|x| x.pos().0 as u32).unwrap_or(0);
                        self.error(
                            at,
                            Code::SwappedSliceIndices,
                            format!("invalid slice indices: {} < {}", y, x_i),
                        );
                        break 'outer;
                    }
                }
            }
        }
    }

    /// Check an index expression for validity. If `max >= 0` it is the upper
    /// bound. Returns the index's (named) integer type and, if a non-negative
    /// constant, its value (else `-1`).
    ///
    /// Equivalent to `Checker.index`.
    pub fn index(&mut self, index: &Expr, max: i64) -> (TypeId, i64) {
        let invalid = self.invalid_type();
        let mut x = Operand::invalid();
        self.expr(&mut x, index);
        if !self.is_valid_index(&mut x, Code::InvalidIndex, "index", false) {
            return (invalid, -1);
        }
        if x.mode != OperandMode::Constant {
            return (x.typ.unwrap_or(invalid), -1);
        }
        match &x.val {
            Some(v) if v.kind() == Kind::Unknown => (invalid, -1),
            Some(v) => {
                let (val, _) = int64_val(v);
                if max >= 0 && val >= max {
                    self.error(
                        index.pos().0 as u32,
                        Code::InvalidIndex,
                        format!("index {} out of bounds [0:{}]", val, max),
                    );
                    return (invalid, -1);
                }
                (x.typ.unwrap_or(invalid), val)
            }
            None => (invalid, -1),
        }
    }

    /// Check whether operand `x` satisfies the criteria for an integer index.
    /// Reports an error (using `what` as context) and returns `false` if not.
    ///
    /// Equivalent to `Checker.isValidIndex`.
    pub fn is_valid_index(
        &mut self,
        x: &mut Operand,
        code: Code,
        what: &str,
        allow_negative: bool,
    ) -> bool {
        if x.mode == OperandMode::Invalid {
            return false;
        }
        // spec: "a constant index that is untyped is given type int"
        let int_t = self.basic(BasicKind::Int);
        self.convert_untyped(x, int_t);
        if x.mode == OperandMode::Invalid {
            return false;
        }
        let xt = x.typ.unwrap_or_else(|| self.invalid_type());
        // spec: "the index x must be of integer type or an untyped constant"
        if !is_integer(&self.types, xt) {
            let xs = self.operand_str(x);
            self.error(
                x.pos() as u32,
                code,
                format!("{} {} must be integer", what, xs),
            );
            return false;
        }
        if x.mode == OperandMode::Constant {
            if let Some(v) = x.val.clone() {
                // spec: "a constant index must be non-negative ..."
                if !allow_negative && sign(&v) < 0 {
                    let xs = self.operand_str(x);
                    self.error(
                        x.pos() as u32,
                        code,
                        format!("{} {} must not be negative", what, xs),
                    );
                    return false;
                }
                // spec: "... and representable by a value of type int"
                if !is_representable_int(&v) {
                    let xs = self.operand_str(x);
                    self.error(
                        x.pos() as u32,
                        code,
                        format!("{} {} overflows int", what, xs),
                    );
                    return false;
                }
            }
        }
        true
    }

    /// Evaluate an expression for its side effects, discarding the result.
    /// Single-expression form of `Checker.use`.
    fn use1(&mut self, e: &Expr) {
        let mut op = Operand::invalid();
        self.expr(&mut op, e);
    }

    /// Evaluate all present slice indices (for the invalid-operand path).
    fn use_slice_indices(&mut self, e: &SliceExpr) {
        for slot in [e.low.as_deref(), e.high.as_deref(), e.max.as_deref()] {
            if let Some(expr) = slot {
                self.use1(expr);
            }
        }
    }
}

/// Whether a constant value is representable by a 64-bit `int` (our `int` is
/// 64-bit until `sizes.go` lands). Mirrors the `representableConst(.., Int)`
/// overflow check in `isValidIndex`.
fn is_representable_int(v: &Value) -> bool {
    let (_, ok) = int64_val(v);
    ok
}
