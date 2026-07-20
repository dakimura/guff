//! Port of literal type-checking from `go/types/literals.go`
//! (`cmd/compile/internal/types2/literals.go`).
//!
//! **Chunk 31a**: function literals ([`Checker::func_lit`]). The signature is
//! built via [`Checker::func_type`] (chunk 24) — *not* `Checker::typ`, since
//! type-expression checking of `func(...)` types is still deferred (chunk 21) —
//! and the body is checked later via [`Checker::func_body`] (chunk 30e), the
//! same machinery used for named function declarations.
//!
//! Composite literals ([`Checker::composite_lit`]) land in 31b.
//!
//! ## Deferrals (chunk-31a, see §8)
//!
//! - `langCompat` (go1.13 numeric-literal version gate) and `basicLit`'s
//!   overflow recheck are handled in `expr.rs` (chunk 25b), not here.
//! - `describef` (Trace bookkeeping), `iota` capture for func literals inside
//!   const declarations (go.dev/issue/22345), and `Info` recording are omitted.

use std::collections::{HashMap, HashSet};

use guff::ast::{CompositeLit, Expr, FuncLit};
use guff_constant::{Kind as ConstKind, Value};
use guff_types_errors::Code;

use crate::arena::TypeData;
use crate::array::new_array;
use crate::array::{array_elem, array_len};
use crate::check::Checker;
use crate::lookup::{deref, field_index};
use crate::map::{map_elem, map_key};
use crate::operand::{Operand, OperandMode};
use crate::predicates::{identical, is_non_type_param_interface, is_valid};
use crate::r#struct::{struct_field, struct_num_fields};
use crate::slice::slice_elem;
use crate::under::common_under;
use crate::TypeId;

/// A hashable key derived from a constant map-literal key, mirroring Go's
/// `keyVal`. Unlike switch's `goVal`, every representable constant yields a key
/// (bool and complex included), and values are normalised complex→float→int so
/// that e.g. `1`, `1.0`, and `1.0+0i` collide as the same key.
#[derive(PartialEq, Eq, Hash)]
enum MapKey {
    Int(i64),
    Uint(u64),
    Float(u64),        // f64 bit pattern
    Complex(u64, u64), // (real bits, imag bits)
    Str(String),
    Bool(bool),
}

/// Equivalent to Go's `keyVal(constant.Value) any`. Returns `None` only for an
/// `Unknown` (non-representable) constant, which disables dedup for that key.
fn key_val(val: &Value) -> Option<MapKey> {
    let mut x = val.clone();
    // Complex: collapse to a real value when the imaginary part is zero.
    if x.kind() == ConstKind::Complex {
        let f = guff_constant::to_float(x.clone());
        if f.kind() != ConstKind::Float {
            let (r, _) = guff_constant::float64_val(&guff_constant::real(x.clone()));
            let (i, _) = guff_constant::float64_val(&guff_constant::imag(x));
            return Some(MapKey::Complex(r.to_bits(), i.to_bits()));
        }
        x = f;
    }
    // Float: collapse to an integer when the value is integral.
    if x.kind() == ConstKind::Float {
        let i = guff_constant::to_int(x.clone());
        if i.kind() != ConstKind::Int {
            let (v, _) = guff_constant::float64_val(&x);
            return Some(MapKey::Float(v.to_bits()));
        }
        x = i;
    }
    match x.kind() {
        ConstKind::Int => {
            let (v, ok) = guff_constant::int64_val(&x);
            if ok {
                return Some(MapKey::Int(v));
            }
            let (v, ok) = guff_constant::uint64_val(&x);
            if ok {
                return Some(MapKey::Uint(v));
            }
            None
        }
        ConstKind::String => Some(MapKey::Str(guff_constant::string_val(&x))),
        ConstKind::Bool => Some(MapKey::Bool(guff_constant::bool_val(&x))),
        _ => None,
    }
}

impl Checker {
    /// Type-check a function literal `func(...) {...}`.
    ///
    /// Equivalent to `Checker.funcLit`. The body is queued with
    /// [`Checker::later`] so it is checked after the enclosing declaration's
    /// objects are in place (matching Go's deferral for func literals that may
    /// refer to a type still being defined).
    pub fn func_lit(&mut self, x: &mut Operand, e: &FuncLit) {
        // Build the signature directly (typexpr's func-type path is deferred).
        let sig = self.func_type(None, &e.ty);
        if matches!(self.types.get(sig), TypeData::Signature(_)) {
            let body = e.body.clone();
            let parent = self.env.scope;
            let ftid = e.ty.id;
            // A func literal inherits the enclosing package-level declaration so
            // identifiers in its body add dependency edges to the right node
            // (Go: `funcLit` captures `check.decl`).
            let decl = self.env.decl;
            // In IgnoreFuncBodies mode (dependency seed builds) skip queueing the
            // literal's body; its type is the signature, already set below.
            if !self.ignore_func_bodies {
                self.later(move |c| c.func_body(decl, sig, ftid, parent, &body));
            }
            x.mode = OperandMode::Value;
            x.typ = Some(sig);
        } else {
            self.error(
                e.ty.pos().0 as u32,
                Code::InvalidSyntaxTree,
                "invalid function literal",
            );
            x.mode = OperandMode::Invalid;
        }
    }

    /// Type-check a composite literal `T{...}` (or a typeless `{...}` element
    /// when `hint` gives the enclosing element type).
    ///
    /// Equivalent to `Checker.compositeLit`. Array/slice elements and map
    /// keys/values are now evaluated with an element-type hint (Go's
    /// `exprWithHint`), so a typeless inner `{...}` resolves against it
    /// (`[]T{{...}, {...}}`). Struct fields intentionally use plain `expr`
    /// (Go requires an explicit type for nested struct literals).
    /// **Deferrals**: `Info` recording of nested-literal element types.
    pub fn composite_lit(&mut self, x: &mut Operand, e: &CompositeLit, hint: Option<TypeId>) {
        // Open `[...]T` arrays may only appear as a composite-literal type;
        // handle them here so the general path needn't deal with `...`.
        if let Some(te) = e.ty.as_deref() {
            if let Expr::ArrayType(a) = te {
                if matches!(a.len.as_deref(), Some(Expr::Ellipsis(_))) {
                    let elem = self.typ(&a.elt);
                    let n = self.indexed_elts(&e.elts, elem, -1);
                    let arr = new_array(&mut self.types, elem, n);
                    x.mode = OperandMode::Value;
                    x.typ = Some(arr);
                    return;
                }
            }
        }

        // Determine the literal type and its base.
        let mut is_elem = false;
        let (typ, base): (TypeId, TypeId) = if let Some(te) = e.ty.as_deref() {
            let t = self.typ(te);
            (t, t)
        } else if let Some(h) = hint {
            // `*T` element type implies `&T{}`: deref the common underlying.
            let (u, _) = common_under(&mut self.types, &self.objects, &self.packages, h, None);
            let b = match u {
                Some(uu) => {
                    let (d, ok) = deref(&self.types, uu);
                    if ok {
                        d
                    } else {
                        h
                    }
                }
                None => h,
            };
            is_elem = true;
            (h, b)
        } else {
            self.error(
                e.lbrace.0 as u32,
                Code::UntypedLit,
                "missing type in composite literal",
            );
            let inv = self.invalid_type();
            (inv, inv)
        };

        // Switch on the common underlying type of `base`.
        let (cu, _) = common_under(&mut self.types, &self.objects, &self.packages, base, None);

        match cu.map(|u| (u, self.types.get(u))) {
            Some((u, TypeData::Struct(_))) => self.composite_struct(x, e, base, u),
            Some((u, TypeData::Array(_))) => {
                let elem = array_elem(&self.types, u);
                let len = array_len(&self.types, u);
                self.indexed_elts(&e.elts, elem, len);
            }
            Some((u, TypeData::Slice(_))) => {
                let elem = slice_elem(&self.types, u);
                self.indexed_elts(&e.elts, elem, -1);
            }
            Some((u, TypeData::Map(_))) => self.composite_map(x, e, u),
            _ => {
                // "Use" every element (unpacking key:value pairs) so they're
                // evaluated, then report an error if the type was otherwise OK.
                for el in &e.elts {
                    let target = match el {
                        Expr::KeyValueExpr(kv) => &kv.value,
                        other => other,
                    };
                    let mut tmp = Operand::invalid();
                    self.expr(&mut tmp, target);
                }
                if is_valid(&self.types, base) {
                    let qualifier = if is_elem { " element" } else { "" };
                    let ts = self.type_str(typ);
                    self.error(
                        e.lbrace.0 as u32,
                        Code::InvalidLit,
                        format!("invalid composite literal{} type {}", qualifier, ts),
                    );
                    x.mode = OperandMode::Invalid;
                    return;
                }
            }
        }

        x.mode = OperandMode::Value;
        x.typ = Some(typ);
    }

    /// Check the elements of a struct composite literal against `u`'s fields.
    fn composite_struct(&mut self, x: &mut Operand, e: &CompositeLit, base: TypeId, u: TypeId) {
        if e.elts.is_empty() {
            return;
        }
        // Snapshot the field list before evaluating elements (which mutate the
        // arenas).
        let n = struct_num_fields(&self.types, u);
        let field_objs: Vec<crate::ObjectId> =
            (0..n).map(|i| struct_field(&self.types, u, i)).collect();

        let keyed = matches!(e.elts.first(), Some(Expr::KeyValueExpr(_)));
        if keyed {
            // All elements must have keys.
            let mut visited = vec![false; field_objs.len()];
            for el in &e.elts {
                let kv = match el {
                    Expr::KeyValueExpr(kv) => kv,
                    _ => {
                        self.error(
                            el.pos().0 as u32,
                            Code::MixedStructLit,
                            "mixture of field:value and value elements in struct literal",
                        );
                        continue;
                    }
                };
                // Struct fields (keyed or positional) do NOT propagate the
                // element hint in Go — nested `{...}` needs an explicit type.
                let mut xe = Operand::invalid();
                self.expr(&mut xe, &kv.value);
                let key = match &*kv.key {
                    Expr::Ident(id) => id,
                    _ => {
                        self.error(
                            kv.key.pos().0 as u32,
                            Code::InvalidLitField,
                            "invalid field name in struct literal",
                        );
                        continue;
                    }
                };
                let i = match field_index(
                    &self.objects,
                    &self.packages,
                    &field_objs,
                    Some(self.pkg),
                    &key.name,
                    false,
                ) {
                    Some(i) => i,
                    None => {
                        let bs = self.type_str(base);
                        self.error(
                            kv.key.pos().0 as u32,
                            Code::MissingLitField,
                            format!(
                                "unknown field {} in struct literal of type {}",
                                key.name, bs
                            ),
                        );
                        continue;
                    }
                };
                let fld = field_objs[i];
                let etyp = fld
                    .typ(&self.objects)
                    .unwrap_or_else(|| self.invalid_type());
                self.assignment(&mut xe, Some(etyp), "struct literal");
                if visited[i] {
                    self.error(
                        kv.key.pos().0 as u32,
                        Code::DuplicateLitField,
                        format!("duplicate field name {} in struct literal", key.name),
                    );
                    continue;
                }
                visited[i] = true;
            }
        } else {
            // No element must have a key.
            for (i, el) in e.elts.iter().enumerate() {
                if let Expr::KeyValueExpr(_) = el {
                    self.error(
                        el.pos().0 as u32,
                        Code::MixedStructLit,
                        "mixture of field:value and value elements in struct literal",
                    );
                    continue;
                }
                let mut xe = Operand::invalid();
                self.expr(&mut xe, el);
                if i >= field_objs.len() {
                    let bs = self.type_str(base);
                    self.error(
                        el.pos().0 as u32,
                        Code::InvalidStructLit,
                        format!("too many values in struct literal of type {}", bs),
                    );
                    break;
                }
                let fld = field_objs[i];
                if !fld.exported(&self.objects) && fld.pkg(&self.objects) != Some(self.pkg) {
                    let (name, bs) = (fld.name(&self.objects).to_string(), self.type_str(base));
                    self.error(
                        el.pos().0 as u32,
                        Code::UnexportedLitField,
                        format!(
                            "implicit assignment to unexported field {} in struct literal of type {}",
                            name, bs
                        ),
                    );
                    continue;
                }
                let etyp = fld
                    .typ(&self.objects)
                    .unwrap_or_else(|| self.invalid_type());
                self.assignment(&mut xe, Some(etyp), "struct literal");
            }
            if e.elts.len() < field_objs.len() {
                let bs = self.type_str(base);
                self.error(
                    e.rbrace.0 as u32,
                    Code::InvalidStructLit,
                    format!("too few values in struct literal of type {}", bs),
                );
            }
        }
        let _ = x;
    }

    /// Check the elements of a map composite literal against `u`'s key/value.
    fn composite_map(&mut self, x: &mut Operand, e: &CompositeLit, u: TypeId) {
        let key_t = map_key(&self.types, u);
        let elem_t = map_elem(&self.types, u);
        // When the key type is an interface (but not a type parameter), two
        // constant keys with the same underlying value but different types are
        // distinct, so we must also compare types. For a concrete key type the
        // underlying value alone determines duplication.
        let key_is_interface = is_non_type_param_interface(&self.types, key_t);
        let mut visited: HashMap<MapKey, Vec<TypeId>> = HashMap::new();
        for el in &e.elts {
            let kv = match el {
                Expr::KeyValueExpr(kv) => kv,
                _ => {
                    self.error(
                        el.pos().0 as u32,
                        Code::MissingLitKey,
                        "missing key in map literal",
                    );
                    continue;
                }
            };
            let mut xk = Operand::invalid();
            self.expr_with_hint(&mut xk, &kv.key, key_t);
            self.assignment(&mut xk, Some(key_t), "map literal");
            if xk.mode != OperandMode::Invalid {
                if let (OperandMode::Constant, Some(val)) = (xk.mode, xk.val.as_ref()) {
                    if let Some(key) = key_val(val) {
                        let ktyp = xk.typ.unwrap_or_else(|| self.invalid_type());
                        let duplicate = if key_is_interface {
                            // Snapshot to avoid holding the `visited` borrow
                            // across `Identical` calls on `self.types`.
                            let prev: Vec<TypeId> = visited.get(&key).cloned().unwrap_or_default();
                            let dup = prev.iter().any(|&vt| {
                                identical(&mut self.types, &self.objects, &self.packages, vt, ktyp)
                            });
                            visited.entry(key).or_default().push(ktyp);
                            dup
                        } else {
                            let dup = visited.contains_key(&key);
                            visited.entry(key).or_default();
                            dup
                        };
                        if duplicate {
                            let ks = self.operand_str(&xk);
                            self.error(
                                kv.key.pos().0 as u32,
                                Code::DuplicateLitKey,
                                format!("duplicate key {} in map literal", ks),
                            );
                            continue;
                        }
                    }
                }
            }
            let mut xv = Operand::invalid();
            self.expr_with_hint(&mut xv, &kv.value, elem_t);
            self.assignment(&mut xv, Some(elem_t), "map literal");
        }
        let _ = x;
    }

    /// Check the elements of an array/slice composite literal against the
    /// element type `typ`, validating indices against `length` (if `>= 0`).
    /// Returns the literal length (max index + 1).
    ///
    /// Equivalent to `Checker.indexedElts`.
    fn indexed_elts(&mut self, elts: &[Expr], typ: TypeId, length: i64) -> i64 {
        let mut visited: HashSet<i64> = HashSet::new();
        let mut index: i64 = 0;
        let mut max: i64 = 0;
        for el in elts {
            let (eval, kv_key): (&Expr, Option<&Expr>) = match el {
                Expr::KeyValueExpr(kv) => (&kv.value, Some(&kv.key)),
                other => (other, None),
            };

            let mut valid_index = false;
            if let Some(key) = kv_key {
                let (it, i) = self.index(key, length);
                if is_valid(&self.types, it) {
                    if i >= 0 {
                        index = i;
                        valid_index = true;
                    } else {
                        self.error(
                            key.pos().0 as u32,
                            Code::InvalidLitIndex,
                            "index must be integer constant",
                        );
                    }
                }
            } else if length >= 0 && index >= length {
                self.error(
                    el.pos().0 as u32,
                    Code::OversizeArrayLit,
                    format!("index {} is out of bounds (>= {})", index, length),
                );
            } else {
                valid_index = true;
            }

            if valid_index {
                if visited.contains(&index) {
                    self.error(
                        el.pos().0 as u32,
                        Code::DuplicateLitKey,
                        format!("duplicate index {} in array or slice literal", index),
                    );
                }
                visited.insert(index);
            }
            index += 1;
            if index > max {
                max = index;
            }

            let mut xe = Operand::invalid();
            self.expr_with_hint(&mut xe, eval, typ);
            self.assignment(&mut xe, Some(typ), "array or slice literal");
        }
        max
    }
}
