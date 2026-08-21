//! Port of built-in function type-checking from
//! `cmd/compile/internal/types2/builtins.go`.
//!
//! **Chunk 29a**: the [`Checker::builtin`] entry + the common preamble (`...`
//! restriction, argument evaluation, arg-count check) and the value-argument
//! builtins `append` / `len` / `cap` / `copy`.
//!
//! Later sub-chunks add `make`/`new`/`delete`/`clear` (29b) and
//! `complex`/`real`/`imag`/`close`/`panic`/`recover`/`print`/`min`/`max`/
//! `unsafe.*` (29c); until then those ids fall through to a DEFERRED arm that
//! leaves the operand invalid.
//!
//! ## Deferrals (chunk-29a, see §8)
//!
//! - **Type-parameter arguments** (Go's `underIs` / `sliceElem`-over-typeset
//!   branches) are reduced to the operand's underlying type.
//! - `hasCallOrRecv` tracking is omitted, so `len`/`cap` of an array is always
//!   treated as a constant (Go also makes it constant when there is no call /
//!   receive — the common case).
//! - `recordBuiltinType` is a no-op. `verifyVersionf` version gates
//!   (clear/min/max → go1.21, unsafe.Add/Slice → go1.17,
//!   unsafe.SliceData/String/StringData → go1.20) are applied at the dispatch
//!   site in [`Checker::builtin`]; the `new(expr)` value form (go1.26) is gated
//!   inside `builtin_new`, which only reaches the gate once the argument itself
//!   checks out (Go reports the version error last, too).

use guff::ast::{CallExpr, Expr};
use guff::token::Token;
use guff_constant::{
    binary_op, compare, imag, make_imag, make_int64, real, sign, string_val, to_float, Value,
};
use guff_types_errors::Code;

use crate::arena::{ObjectData, TypeData, TypeId};
use crate::basic::BasicKind;
use crate::check::Checker;
use crate::lookup::{as_named, deref_struct_ptr, lookup_field_or_method, LookupResult};
use crate::object::builtin::{builtin_info, BuiltinId};
use crate::operand::{Operand, OperandMode};
use crate::pointer::{new_pointer, pointer_elem};
use crate::predicates::{is_integer_or_float, is_numeric, is_string, is_valid};
use crate::r#struct::{struct_field, struct_num_fields};
use crate::selection::SelectionKind;
use crate::signature::new_signature_type;
use crate::sizes::{default_sizes, Sizes};
use crate::slice::{new_slice, slice_elem};
use crate::under::common_under;
use crate::version::{go1_17, go1_20, go1_21, go1_26};

impl Checker {
    /// Type-check a call to built-in `id`, recording the result in `x`.
    /// Returns `true` if the call checked out, `false` (operand left invalid by
    /// the caller) otherwise.
    ///
    /// Equivalent to `Checker.builtin` (chunk-29a subset).
    pub fn builtin<'a>(&mut self, x: &mut Operand<'a>, call: &'a CallExpr, id: BuiltinId) -> bool {
        let bin = builtin_info(id);
        let has_dots = crate::util::has_dots(call);

        // `append` is the only built-in that permits `...` for the last arg.
        if has_dots && id != BuiltinId::Append {
            self.error(
                call.pos().0 as u32,
                Code::InvalidDotDotDot,
                format!("invalid use of ... with built-in {}", bin.name),
            );
            self.use_exprs(&call.args);
            return false;
        }

        // Evaluate value arguments. Built-ins with special argument handling
        // (make/new — their first argument is a type; offsetof — its argument
        // is a selector evaluated specially) skip this and evaluate their
        // arguments themselves.
        let special = matches!(id, BuiltinId::Make | BuiltinId::New | BuiltinId::Offsetof);
        let mut args: Vec<Operand> = Vec::new();
        let nargs;
        if special {
            nargs = call.args.len();
        } else {
            args.reserve(call.args.len());
            if call.args.len() == 1 {
                // Go's `exprList` routes a lone argument through `multiExpr`,
                // so a multi-valued call spreads across the parameters
                // (`println(two())` is two arguments, not one tuple) and
                // `single_value` must not reduce it first.
                let a = &call.args[0];
                let mut op = Operand::invalid();
                self.raw_expr(&mut op, a, None);
                let tuple = op.typ.filter(|_| op.mode != OperandMode::Invalid).filter(
                    |t| matches!(self.types.get(*t), crate::arena::TypeData::Tuple(_)),
                );
                match tuple {
                    Some(t) => {
                        for i in 0..crate::tuple::tuple_len(&self.types, Some(t)) {
                            let v = crate::tuple::tuple_at(&self.types, t, i);
                            let mut e = Operand::invalid();
                            e.mode = OperandMode::Value;
                            e.typ = v.typ(&self.objects);
                            e.expr = op.expr;
                            args.push(e);
                        }
                    }
                    None => args.push(op),
                }
            } else {
                for a in &call.args {
                    let mut op = Operand::invalid();
                    self.expr(&mut op, a);
                    args.push(op);
                }
            }
            nargs = args.len();
            for a in &args {
                if a.mode == OperandMode::Invalid {
                    return false;
                }
            }
            // The first argument is always in x.
            if nargs > 0 {
                *x = args[0].clone();
            }
        }

        // Argument count.
        let too_few = nargs < bin.nargs as usize;
        let too_many = !bin.variadic && nargs > bin.nargs as usize;
        if too_few || too_many {
            let msg = if too_few { "not enough" } else { "too many" };
            self.error(
                call.pos().0 as u32,
                Code::WrongArgCount,
                format!(
                    "{} arguments for built-in {} (expected {}, found {})",
                    msg, bin.name, bin.nargs, nargs
                ),
            );
            return false;
        }

        // Version gates: report (and continue) when the effective Go version
        // predates a feature. Go calls `check.verifyVersionf` inside each
        // handler and ignores the result; we do them up-front at the dispatch
        // site since `call`/`id` are both in scope. Equivalent builtins.go
        // gates: clear/min/max → go1.21, unsafe.Add/Slice → go1.17,
        // unsafe.SliceData/String/StringData → go1.20. (`new(expr)` → go1.26 is
        // gated in `builtin_new`, after the argument has been checked.)
        let fpos = call.fun.pos().0 as u32;
        match id {
            BuiltinId::Clear => {
                self.verify_versionf(fpos, &go1_21(), "clear");
            }
            BuiltinId::Min => {
                self.verify_versionf(fpos, &go1_21(), "built-in min");
            }
            BuiltinId::Max => {
                self.verify_versionf(fpos, &go1_21(), "built-in max");
            }
            BuiltinId::Add => {
                self.verify_versionf(fpos, &go1_17(), "unsafe.Add");
            }
            BuiltinId::Slice => {
                self.verify_versionf(fpos, &go1_17(), "unsafe.Slice");
            }
            BuiltinId::SliceData => {
                self.verify_versionf(fpos, &go1_20(), "unsafe.SliceData");
            }
            BuiltinId::String => {
                self.verify_versionf(fpos, &go1_20(), "unsafe.String");
            }
            BuiltinId::StringData => {
                self.verify_versionf(fpos, &go1_20(), "unsafe.StringData");
            }
            _ => {}
        }

        match id {
            BuiltinId::Append => self.builtin_append(x, call, &args, nargs),
            BuiltinId::Len | BuiltinId::Cap => self.builtin_len_cap(x, id, bin.name),
            BuiltinId::Copy => self.builtin_copy(x, &args),
            BuiltinId::Make => self.builtin_make(x, call, nargs),
            BuiltinId::New => self.builtin_new(x, call),
            BuiltinId::Delete => self.builtin_delete(x, &args),
            BuiltinId::Clear => self.builtin_clear(x),
            BuiltinId::Close => self.builtin_close(x),
            BuiltinId::Complex => self.builtin_complex(x, &args, call),
            BuiltinId::Real | BuiltinId::Imag => self.builtin_real_imag(x, id),
            BuiltinId::Min | BuiltinId::Max => self.builtin_min_max(x, &args, id, bin.name),
            BuiltinId::Panic => self.builtin_panic(x),
            BuiltinId::Print | BuiltinId::Println => {
                let ok = self.builtin_print(&mut args);
                if ok {
                    x.mode = OperandMode::NoValue;
                }
                ok
            }
            BuiltinId::Recover => self.builtin_recover(x),
            BuiltinId::Sizeof => self.builtin_sizeof(x),
            BuiltinId::Alignof => self.builtin_alignof(x),
            BuiltinId::Offsetof => self.builtin_offsetof(x, call),
            BuiltinId::Add => self.builtin_add(x, &args),
            BuiltinId::Slice => self.builtin_slice(x, &args),
            BuiltinId::SliceData => self.builtin_slice_data(x),
            BuiltinId::String => self.builtin_string(x, &args),
            BuiltinId::StringData => self.builtin_string_data(x),
            // DEFERRED: test-only assert/trace.
            _ => {
                x.mode = OperandMode::Invalid;
                x.typ = Some(self.invalid_type());
                false
            }
        }
    }

    /// `close(c)` — `c` must be a sendable channel.
    fn builtin_close(&mut self, x: &mut Operand) -> bool {
        let typ = x.typ.unwrap_or_else(|| self.invalid_type());
        // `commonUnder(x.typ, …)`: a type parameter constrained to channels is
        // closeable, and its own underlying type is the constraint interface.
        let u = common_under(&mut self.types, &self.objects, &self.packages, typ, None)
            .0
            .unwrap_or_else(|| self.invalid_type());
        match self.types.get(u) {
            TypeData::Chan(_) => {
                if crate::chan::chan_dir(&self.types, u) == crate::chan::ChanDir::RecvOnly {
                    let xs = self.operand_str(x);
                    self.error(
                        x.pos() as u32,
                        Code::InvalidClose,
                        format!("cannot close receive-only channel {}", xs),
                    );
                    return false;
                }
            }
            _ => {
                let xs = self.operand_str(x);
                self.error(
                    x.pos() as u32,
                    Code::InvalidClose,
                    format!("cannot close non-channel {}", xs),
                );
                return false;
            }
        }
        x.mode = OperandMode::NoValue;
        true
    }

    /// `complex(x, y floatT) complexT`.
    fn builtin_complex<'a>(&mut self, x: &mut Operand<'a>, args: &[Operand<'a>], _call: &CallExpr) -> bool {
        let mut y = args[1].clone();

        // Convert or check untyped arguments.
        let xt = x.typ.unwrap_or_else(|| self.invalid_type());
        let yt = y.typ.unwrap_or_else(|| self.invalid_type());
        let d = (!crate::predicates::is_typed(&self.types, xt) as u8)
            | (((!crate::predicates::is_typed(&self.types, yt)) as u8) << 1);
        match d {
            0 => {}
            1 => self.convert_untyped(x, yt),
            2 => {
                let xt2 = x.typ.unwrap_or_else(|| self.invalid_type());
                self.convert_untyped(&mut y, xt2);
            }
            _ => {
                // Both untyped.
                if x.mode == OperandMode::Constant && y.mode == OperandMode::Constant {
                    let uf = self.basic(BasicKind::UntypedFloat);
                    if let Some(v) = &x.val {
                        if is_numeric(&self.types, xt) && sign(&imag(v.clone())) == 0 {
                            x.typ = Some(uf);
                        }
                    }
                    if let Some(v) = &y.val {
                        if is_numeric(&self.types, yt) && sign(&imag(v.clone())) == 0 {
                            y.typ = Some(uf);
                        }
                    }
                } else {
                    let f64t = self.basic(BasicKind::Float64);
                    self.convert_untyped(x, f64t);
                    self.convert_untyped(&mut y, f64t);
                }
            }
        }
        if x.mode == OperandMode::Invalid || y.mode == OperandMode::Invalid {
            return false;
        }

        let xt = x.typ.unwrap_or_else(|| self.invalid_type());
        let yt = y.typ.unwrap_or_else(|| self.invalid_type());
        if !crate::predicates::identical(&mut self.types, &self.objects, &self.packages, xt, yt) {
            let (xs, ys) = (self.type_str(xt), self.type_str(yt));
            self.error(
                x.pos() as u32,
                Code::InvalidComplex,
                format!("complex: mismatched types {} and {}", xs, ys),
            );
            return false;
        }

        // float → complex result type. (Type parameters deferred.)
        let res = match self.basic_kind(xt) {
            Some(BasicKind::Float32) => Some(self.basic(BasicKind::Complex64)),
            Some(BasicKind::Float64) => Some(self.basic(BasicKind::Complex128)),
            Some(BasicKind::UntypedFloat) => Some(self.basic(BasicKind::UntypedComplex)),
            _ => None,
        };
        let res = match res {
            Some(t) => t,
            None => {
                let xs = self.type_str(xt);
                self.error(
                    x.pos() as u32,
                    Code::InvalidComplex,
                    format!("arguments have type {}, expected floating-point", xs),
                );
                return false;
            }
        };

        if x.mode == OperandMode::Constant && y.mode == OperandMode::Constant {
            if let (Some(xv), Some(yv)) = (&x.val, &y.val) {
                x.val = Some(binary_op(
                    to_float(xv.clone()),
                    Token::ADD,
                    make_imag(to_float(yv.clone())),
                ));
            }
        } else {
            x.mode = OperandMode::Value;
        }
        x.typ = Some(res);
        true
    }

    /// `real(complexT) floatT` / `imag(complexT) floatT`.
    fn builtin_real_imag(&mut self, x: &mut Operand, id: BuiltinId) -> bool {
        let xt = x.typ.unwrap_or_else(|| self.invalid_type());
        if !crate::predicates::is_typed(&self.types, xt) {
            if x.mode == OperandMode::Constant {
                if is_numeric(&self.types, xt) {
                    x.typ = Some(self.basic(BasicKind::UntypedComplex));
                }
            } else {
                let c128 = self.basic(BasicKind::Complex128);
                self.convert_untyped(x, c128);
                if x.mode == OperandMode::Invalid {
                    return false;
                }
            }
        }

        let xt = x.typ.unwrap_or_else(|| self.invalid_type());
        let res = match self.basic_kind(xt) {
            Some(BasicKind::Complex64) => Some(self.basic(BasicKind::Float32)),
            Some(BasicKind::Complex128) => Some(self.basic(BasicKind::Float64)),
            Some(BasicKind::UntypedComplex) => Some(self.basic(BasicKind::UntypedFloat)),
            _ => None,
        };
        let res = match res {
            Some(t) => t,
            None => {
                let code = if id == BuiltinId::Real {
                    Code::InvalidReal
                } else {
                    Code::InvalidImag
                };
                let xs = self.type_str(xt);
                self.error(
                    x.pos() as u32,
                    code,
                    format!("argument has type {}, expected complex type", xs),
                );
                return false;
            }
        };

        if x.mode == OperandMode::Constant {
            if let Some(v) = &x.val {
                x.val = Some(if id == BuiltinId::Real {
                    real(v.clone())
                } else {
                    imag(v.clone())
                });
            }
        } else {
            x.mode = OperandMode::Value;
        }
        x.typ = Some(res);
        true
    }

    /// `min(x, ...)` / `max(x, ...)`.
    fn builtin_min_max<'a>(
        &mut self,
        x: &mut Operand<'a>,
        args: &[Operand<'a>],
        id: BuiltinId,
        name: &str,
    ) -> bool {
        let op = if id == BuiltinId::Max {
            Token::GTR
        } else {
            Token::LSS
        };

        for (i, a) in args.iter().enumerate() {
            let mut a = a.clone();
            if a.mode == OperandMode::Invalid {
                return false;
            }
            let at = a.typ.unwrap_or_else(|| self.invalid_type());
            if !crate::predicates::all_ordered(&mut self.types, &self.objects, &self.packages, at)
            {
                let as_ = self.operand_str(&a);
                self.error(
                    a.pos() as u32,
                    Code::InvalidMinMaxOperand,
                    format!("{} cannot be ordered", as_),
                );
                return false;
            }
            // The first argument is already in x.
            if i > 0 {
                self.match_types(x, &mut a);
                if x.mode == OperandMode::Invalid {
                    return false;
                }
                let xt = x.typ.unwrap_or_else(|| self.invalid_type());
                let at = a.typ.unwrap_or_else(|| self.invalid_type());
                if !crate::predicates::identical(
                    &mut self.types,
                    &self.objects,
                    &self.packages,
                    xt,
                    at,
                ) {
                    let (xs, as_) = (self.type_str(xt), self.type_str(at));
                    self.error(
                        a.pos() as u32,
                        Code::MismatchedTypes,
                        format!("mismatched types {} (previous argument) and {}", xs, as_),
                    );
                    return false;
                }
                if x.mode == OperandMode::Constant && a.mode == OperandMode::Constant {
                    if let (Some(xv), Some(av)) = (&x.val, &a.val) {
                        if compare(av.clone(), op, xv.clone()) {
                            *x = a.clone();
                        }
                    }
                } else {
                    x.mode = OperandMode::Value;
                }
            }
        }

        // If x is still untyped (e.g. min of one untyped constant treated as a
        // value), give it its default type.
        if x.mode != OperandMode::Constant {
            x.mode = OperandMode::Value;
            let any = self.universe_any;
            self.assignment(x, Some(any), &format!("argument to built-in {}", name));
            if x.mode == OperandMode::Invalid {
                return false;
            }
        }
        true
    }

    /// `panic(x)`.
    fn builtin_panic(&mut self, x: &mut Operand) -> bool {
        let any = self.universe_any;
        self.assignment(x, Some(any), "argument to panic");
        if x.mode == OperandMode::Invalid {
            return false;
        }
        x.mode = OperandMode::NoValue;
        true
    }

    /// `print(x, ...)` / `println(x, ...)`.
    fn builtin_print(&mut self, args: &mut [Operand]) -> bool {
        for a in args.iter_mut() {
            self.assignment(a, None, "argument to built-in print");
            if a.mode == OperandMode::Invalid {
                return false;
            }
        }
        // The result of print is recorded on the first operand by the caller;
        // we set it below in builtin() via the returned NoValue mode.
        true
    }

    /// `recover() interface{}`.
    fn builtin_recover(&mut self, x: &mut Operand) -> bool {
        x.mode = OperandMode::Value;
        x.typ = Some(self.universe_any);
        true
    }

    /// `unsafe.Sizeof(x T) uintptr`. `x` already holds the (evaluated) argument.
    fn builtin_sizeof(&mut self, x: &mut Operand) -> bool {
        self.assignment(x, None, "argument to unsafe.Sizeof");
        if x.mode == OperandMode::Invalid {
            return false;
        }
        let t = x.typ.unwrap_or_else(|| self.invalid_type());
        if self.has_var_size(t) {
            // Size depends on a type parameter — not a compile-time constant.
            x.mode = OperandMode::Value;
        } else {
            let size = self.conf_sizeof(t);
            if size < 0 {
                let xs = self.operand_str(x);
                self.error(
                    x.pos() as u32,
                    Code::TypeTooLarge,
                    format!("{} is too large", xs),
                );
                return false;
            }
            x.mode = OperandMode::Constant;
            x.val = Some(make_int64(size));
        }
        x.typ = Some(self.basic(BasicKind::Uintptr));
        true
    }

    /// `unsafe.Alignof(x T) uintptr`. `x` already holds the (evaluated) argument.
    fn builtin_alignof(&mut self, x: &mut Operand) -> bool {
        self.assignment(x, None, "argument to unsafe.Alignof");
        if x.mode == OperandMode::Invalid {
            return false;
        }
        let t = x.typ.unwrap_or_else(|| self.invalid_type());
        if self.has_var_size(t) {
            x.mode = OperandMode::Value;
        } else {
            // alignof always returns a value >= 1.
            x.mode = OperandMode::Constant;
            x.val = Some(make_int64(self.conf_alignof(t)));
        }
        x.typ = Some(self.basic(BasicKind::Uintptr));
        true
    }

    /// `unsafe.Offsetof(x.f) uintptr`, where the argument must be a selector
    /// (its operand is *not* pre-evaluated — handled here).
    fn builtin_offsetof<'a>(&mut self, x: &mut Operand<'a>, call: &'a CallExpr) -> bool {
        let arg0 = &call.args[0];
        let selx = match unparen_expr(arg0) {
            Expr::SelectorExpr(s) => s,
            _ => {
                self.error(
                    arg0.pos().0 as u32,
                    Code::BadOffsetofSyntax,
                    "invalid argument: argument to unsafe.Offsetof is not a selector expression"
                        .to_string(),
                );
                self.use_exprs(std::slice::from_ref(arg0));
                return false;
            }
        };

        // Evaluate the selector's base operand (`x` in `x.f`).
        self.expr(x, &selx.x);
        if x.mode == OperandMode::Invalid {
            return false;
        }

        let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
        let base = deref_struct_ptr(&self.types, xtyp);
        let sel = selx.sel.name.clone();
        let result = lookup_field_or_method(
            &mut self.types,
            &self.objects,
            &self.packages,
            base,
            false,
            Some(self.pkg),
            &sel,
        );

        let (obj, index, indirect) = match result {
            LookupResult::Found {
                obj,
                index,
                indirect,
            } => (obj, index, indirect),
            // Ambiguous / pointer-receiver / not found: there is no single field.
            _ => {
                let bs = self.type_str(base);
                self.error(
                    x.pos() as u32,
                    Code::MissingFieldOrMethod,
                    format!("invalid argument: {} has no single field {}", bs, sel),
                );
                return false;
            }
        };

        // A method (value) is not a field.
        if matches!(self.objects.get(obj), ObjectData::Func(_)) {
            self.error(
                arg0.pos().0 as u32,
                Code::InvalidOffsetof,
                format!("invalid argument: {} is a method value", sel),
            );
            return false;
        }
        if indirect {
            let bs = self.type_str(base);
            self.error(
                x.pos() as u32,
                Code::InvalidOffsetof,
                format!(
                    "invalid argument: field {} is embedded via a pointer in {}",
                    sel, bs
                ),
            );
            return false;
        }

        // recordSelection(FieldVal) for the (non-method, non-indirect) field.
        // `index` is still needed by `conf_offsetof` below, so clone it.
        self.record_selection(
            selx,
            SelectionKind::FieldVal,
            base,
            obj,
            index.clone(),
            false,
        );

        if self.has_var_size(base) {
            x.mode = OperandMode::Value;
        } else {
            let offs = self.conf_offsetof(base, &index);
            if offs < 0 {
                let xs = self.operand_str(x);
                self.error(
                    x.pos() as u32,
                    Code::TypeTooLarge,
                    format!("{} is too large", xs),
                );
                return false;
            }
            x.mode = OperandMode::Constant;
            x.val = Some(make_int64(offs));
        }
        x.typ = Some(self.basic(BasicKind::Uintptr));
        true
    }

    /// `unsafe.Add(ptr unsafe.Pointer, len IntegerType) unsafe.Pointer`.
    /// `x` already holds the first (evaluated) argument; `args[1]` is the
    /// length. The go1.17 version gate is applied at the dispatch site (see
    /// `builtin`).
    fn builtin_add<'a>(&mut self, x: &mut Operand<'a>, args: &[Operand<'a>]) -> bool {
        let usp = self.basic(BasicKind::UnsafePointer);
        self.assignment(x, Some(usp), "argument to unsafe.Add");
        if x.mode == OperandMode::Invalid {
            return false;
        }
        let mut y = args[1].clone();
        if !self.is_valid_index(&mut y, Code::InvalidUnsafeAdd, "length", true) {
            return false;
        }
        x.mode = OperandMode::Value;
        x.typ = Some(usp);
        true
    }

    /// `unsafe.Slice(ptr *T, len IntegerType) []T`. The go1.17 version gate is
    /// applied at the dispatch site (see `builtin`).
    fn builtin_slice<'a>(&mut self, x: &mut Operand<'a>, args: &[Operand<'a>]) -> bool {
        let xt = x.typ.unwrap_or_else(|| self.invalid_type());
        let (u, _) = common_under(&mut self.types, &self.objects, &self.packages, xt, None);
        let base = u.and_then(|uid| match self.types.get(uid) {
            TypeData::Pointer(_) => Some(pointer_elem(&self.types, uid)),
            _ => None,
        });
        let base = match base {
            Some(b) => b,
            None => {
                let xs = self.operand_str(x);
                self.error(
                    x.pos() as u32,
                    Code::InvalidUnsafeSlice,
                    format!("invalid argument: {} is not a pointer", xs),
                );
                return false;
            }
        };
        let mut y = args[1].clone();
        if !self.is_valid_index(&mut y, Code::InvalidUnsafeSlice, "length", false) {
            return false;
        }
        x.mode = OperandMode::Value;
        x.typ = Some(new_slice(&mut self.types, base));
        true
    }

    /// `unsafe.SliceData(slice []T) *T`. The go1.20 version gate is applied at
    /// the dispatch site (see `builtin`).
    fn builtin_slice_data(&mut self, x: &mut Operand) -> bool {
        let xt = x.typ.unwrap_or_else(|| self.invalid_type());
        let (u, _) = common_under(&mut self.types, &self.objects, &self.packages, xt, None);
        let elem = u.and_then(|uid| match self.types.get(uid) {
            TypeData::Slice(_) => Some(slice_elem(&self.types, uid)),
            _ => None,
        });
        let elem = match elem {
            Some(e) => e,
            None => {
                let xs = self.operand_str(x);
                self.error(
                    x.pos() as u32,
                    Code::InvalidUnsafeSliceData,
                    format!("invalid argument: {} is not a slice", xs),
                );
                return false;
            }
        };
        x.mode = OperandMode::Value;
        x.typ = Some(new_pointer(&mut self.types, elem));
        true
    }

    /// `unsafe.String(ptr *byte, len IntegerType) string`. The go1.20 version
    /// gate is treated as satisfied.
    fn builtin_string<'a>(&mut self, x: &mut Operand<'a>, args: &[Operand<'a>]) -> bool {
        let byte_t = self.basic(BasicKind::Uint8);
        let ptr_byte = new_pointer(&mut self.types, byte_t);
        self.assignment(x, Some(ptr_byte), "argument to unsafe.String");
        if x.mode == OperandMode::Invalid {
            return false;
        }
        let mut y = args[1].clone();
        if !self.is_valid_index(&mut y, Code::InvalidUnsafeString, "length", false) {
            return false;
        }
        x.mode = OperandMode::Value;
        x.typ = Some(self.basic(BasicKind::String));
        true
    }

    /// `unsafe.StringData(str string) *byte`. The go1.20 version gate is
    /// applied at the dispatch site (see `builtin`).
    fn builtin_string_data(&mut self, x: &mut Operand) -> bool {
        let str_t = self.basic(BasicKind::String);
        self.assignment(x, Some(str_t), "argument to unsafe.StringData");
        if x.mode == OperandMode::Invalid {
            return false;
        }
        x.mode = OperandMode::Value;
        let byte_t = self.basic(BasicKind::Uint8);
        x.typ = Some(new_pointer(&mut self.types, byte_t));
        true
    }

    // ---- sizes plumbing (Config.Sizes or the gc/amd64 default) -------------

    /// The effective [`Sizes`]: `conf.sizes` if set, else the `gc`/`amd64`
    /// default (Go's `stdSizes`).
    fn effective_sizes(&self) -> Sizes {
        self.conf.sizes.unwrap_or_else(default_sizes)
    }

    /// `(conf *Config) sizeof(T)`.
    pub(crate) fn conf_sizeof(&self, t: TypeId) -> i64 {
        self.effective_sizes()
            .sizeof(&self.types, &self.objects, &self.packages, t)
    }

    /// `(conf *Config) alignof(T)` — always >= 1.
    fn conf_alignof(&self, t: TypeId) -> i64 {
        self.effective_sizes()
            .alignof(&self.types, &self.objects, &self.packages, t)
    }

    /// `(conf *Config) offsetof(T, index)`. Walks the field-index path,
    /// accumulating per-struct field offsets. A negative result means the type
    /// is too large.
    fn conf_offsetof(&self, base: TypeId, index: &[i32]) -> i64 {
        let sizes = self.effective_sizes();
        let mut offs = 0i64;
        let mut t = base;
        for &i in index {
            let i = i as usize;
            let u = t.underlying(&self.types);
            let n = struct_num_fields(&self.types, u);
            let fields: Vec<_> = (0..n).map(|j| struct_field(&self.types, u, j)).collect();
            let offsets = sizes.offsetsof(&self.types, &self.objects, &self.packages, &fields);
            let d = offsets[i];
            if d < 0 {
                return -1;
            }
            offs += d;
            if offs < 0 {
                return -1;
            }
            t = fields[i]
                .typ(&self.objects)
                .expect("struct field has a type");
        }
        offs
    }

    /// `hasVarSize(t)` — reports whether `t`'s size depends on a type parameter
    /// (and is therefore not a compile-time constant). Cycles through `Named`
    /// types are broken via the `seen` set.
    fn has_var_size(&self, t: TypeId) -> bool {
        self.has_var_size_inner(t, &mut Vec::new())
    }

    fn has_var_size_inner(&self, t: TypeId, seen: &mut Vec<TypeId>) -> bool {
        // Cycles are only possible through Named types.
        if let Some(named) = as_named(&self.types, t) {
            if seen.contains(&named) {
                return false;
            }
            seen.push(named);
        }
        let u = t.underlying(&self.types);
        match self.types.get(u) {
            TypeData::Array(_) => {
                self.has_var_size_inner(crate::array::array_elem(&self.types, u), seen)
            }
            TypeData::Struct(_) => {
                let n = struct_num_fields(&self.types, u);
                for i in 0..n {
                    let f = struct_field(&self.types, u, i);
                    let ftyp = f.typ(&self.objects).expect("struct field has a type");
                    if self.has_var_size_inner(ftyp, seen) {
                        return true;
                    }
                }
                false
            }
            TypeData::Interface(_) => crate::predicates::is_type_param(&self.types, t),
            _ => false,
        }
    }

    /// The `BasicKind` of `t` if its underlying type is a `Basic`, else `None`.
    fn basic_kind(&self, t: TypeId) -> Option<BasicKind> {
        match self.types.get(t.underlying(&self.types)) {
            TypeData::Basic(b) => Some(b.kind()),
            _ => None,
        }
    }

    /// `make(T, n)` / `make(T, n, m)`.
    fn builtin_make(&mut self, x: &mut Operand, call: &CallExpr, nargs: usize) -> bool {
        let t = self.typ(&call.args[0]);
        if !is_valid(&self.types, t) {
            return false;
        }
        // DEFERRED: commonUnder over a type parameter's type set — use the
        // underlying type directly.
        let u = t.underlying(&self.types);
        let min = match self.types.get(u) {
            TypeData::Slice(_) => 2,
            TypeData::Map(_) | TypeData::Chan(_) => 1,
            _ => {
                let ts = self.type_str(t);
                self.error(
                    call.args[0].pos().0 as u32,
                    Code::InvalidMake,
                    format!("cannot make {}: type must be slice, map, or channel", ts),
                );
                return false;
            }
        };
        if nargs < min || min + 1 < nargs {
            self.error(
                call.pos().0 as u32,
                Code::WrongArgCount,
                format!(
                    "make expects {} or {} arguments; found {}",
                    min,
                    min + 1,
                    nargs
                ),
            );
            return false;
        }

        // The size arguments must be valid integer indices.
        let mut sizes: Vec<i64> = Vec::new();
        for arg in &call.args[1..] {
            let (_, size) = self.index(arg, -1);
            if size >= 0 {
                sizes.push(size);
            }
        }
        if sizes.len() == 2 && sizes[0] > sizes[1] {
            self.error(
                call.args[1].pos().0 as u32,
                Code::SwappedMakeArgs,
                "length and capacity swapped".to_string(),
            );
            // safe to continue
        }

        x.mode = OperandMode::Value;
        x.typ = Some(t);
        true
    }

    /// `new(T)` — yields a `*T`.
    fn builtin_new<'a>(&mut self, x: &mut Operand<'a>, call: &'a CallExpr) -> bool {
        let arg = &call.args[0];

        // `new` takes either a type (`new(T)`) or, since go1.26, a value
        // (`new(x)` allocates a *T initialised to x). Go distinguishes the two
        // with `exprOrType`; our `expr` cannot evaluate composite type syntax
        // (`[]int`, `map[k]v`, `struct{...}`), so probe the argument as a type
        // first and roll back the "is not a type" diagnostics that probe emits
        // when it turns out to be a value.
        let mark = self.errors.len();
        let t = self.typ(arg);
        if is_valid(&self.types, t) {
            x.mode = OperandMode::Value;
            x.typ = Some(crate::pointer::new_pointer(&mut self.types, t));
            return true;
        }
        self.errors.truncate(mark);

        // new(expr): the operand's type is the allocated type.
        self.expr(x, arg);
        if x.mode == OperandMode::Invalid {
            x.typ = Some(self.invalid_type());
            return false;
        }
        if matches!(
            x.mode,
            OperandMode::NoValue | OperandMode::Builtin | OperandMode::TypeExpr
        ) {
            let xs = self.operand_str(x);
            let msg = match x.mode {
                OperandMode::NoValue => format!("{} used as value", xs),
                _ => format!("invalid argument: {} is not an expression", xs),
            };
            self.error(arg.pos().0 as u32, Code::NotAnExpr, msg);
            x.mode = OperandMode::Invalid;
            x.typ = Some(self.invalid_type());
            return false;
        }
        // An untyped operand takes its default type; this also rejects
        // untyped nil and constant overflow (Go: `check.assignment(x, nil, ..)`).
        let untyped = x
            .typ
            .map(|t| crate::predicates::is_untyped(&self.types, t))
            .unwrap_or(false);
        if untyped {
            self.assignment(x, None, "argument to new");
            if x.mode == OperandMode::Invalid {
                x.typ = Some(self.invalid_type());
                return false;
            }
        }
        // Report the version error only once the argument itself checks out,
        // matching Go's ordering.
        self.verify_versionf(call.fun.pos().0 as u32, &go1_26(), "new(expr)");

        let elem = x.typ.unwrap_or_else(|| self.invalid_type());
        x.mode = OperandMode::Value;
        x.typ = Some(crate::pointer::new_pointer(&mut self.types, elem));
        true
    }

    /// `delete(m, k)`.
    fn builtin_delete<'a>(&mut self, x: &mut Operand<'a>, args: &[Operand<'a>]) -> bool {
        let map_typ = x.typ.unwrap_or_else(|| self.invalid_type());
        // `commonUnder`, for the same reason as `close`.
        let u = common_under(&mut self.types, &self.objects, &self.packages, map_typ, None)
            .0
            .unwrap_or_else(|| self.invalid_type());
        let key = match self.types.get(u) {
            TypeData::Map(_) => crate::map::map_key(&self.types, u),
            _ => {
                let xs = self.operand_str(x);
                self.error(
                    x.pos() as u32,
                    Code::InvalidDelete,
                    format!("invalid argument: {} is not a map", xs),
                );
                return false;
            }
        };

        let mut k = args[1].clone();
        self.assignment(&mut k, Some(key), "argument to delete");
        if k.mode == OperandMode::Invalid {
            return false;
        }
        x.mode = OperandMode::NoValue;
        true
    }

    /// `clear(m)` / `clear(s)` — clears a map or slice.
    fn builtin_clear(&mut self, x: &mut Operand) -> bool {
        let typ = x.typ.unwrap_or_else(|| self.invalid_type());
        // The message already says "(or constrained by)"; now it is true.
        let u = common_under(&mut self.types, &self.objects, &self.packages, typ, None)
            .0
            .unwrap_or_else(|| self.invalid_type());
        if !matches!(self.types.get(u), TypeData::Map(_) | TypeData::Slice(_)) {
            let xs = self.operand_str(x);
            self.error(
                x.pos() as u32,
                Code::InvalidClear,
                format!(
                    "cannot clear {}: argument must be (or constrained by) map or slice",
                    xs
                ),
            );
            return false;
        }
        x.mode = OperandMode::NoValue;
        true
    }

    /// `append(s S, x ...E) S`.
    fn builtin_append<'a>(
        &mut self,
        x: &mut Operand<'a>,
        call: &'a CallExpr,
        args: &[Operand<'a>],
        nargs: usize,
    ) -> bool {
        // The first argument must be a slice; E is its element type.
        let s_typ = x.typ.unwrap_or_else(|| self.invalid_type());
        let elem = match self.slice_elem_of(s_typ) {
            Some(e) => e,
            None => {
                let xs = self.operand_str(x);
                self.error(
                    x.pos() as u32,
                    Code::InvalidAppend,
                    format!("invalid append: argument {} is not a slice", xs),
                );
                return false;
            }
        };

        // Special case: append([]byte, string...) appends the string's bytes.
        // go/types: nargs==2 && dots, first assignable to []byte, second is string.
        let uint8 = self.basic(BasicKind::Uint8);
        let byte_slice = new_slice(&mut self.types, uint8);
        if nargs == 2 && crate::util::has_dots(call) {
            let y = &args[1];
            let y_typ = y.typ.unwrap_or_else(|| self.invalid_type());
            if crate::predicates::all_string(&mut self.types, &self.objects, &self.packages, y_typ)
                && self.assignable_to(x, byte_slice).ok
            {
                let s_param = crate::object::var::new_param(&mut self.objects, "", s_typ);
                // Variadic last param is the string type itself (not []string),
                // matching go/types Signature special case for append.
                let rest_param = crate::object::var::new_param(&mut self.objects, "", y_typ);
                let params = crate::tuple::new_tuple(&mut self.types, &[s_param, rest_param]);
                let result_var = crate::object::var::new_param(&mut self.objects, "", s_typ);
                let results = crate::tuple::new_tuple(&mut self.types, &[result_var]);
                let sig =
                    new_signature_type(&mut self.types, None, &[], &[], params, results, true);
                self.arguments(call, sig, &[]);
                x.mode = OperandMode::Value;
                x.typ = Some(s_typ);
                return true;
            }
        }

        // Build a custom variadic signature `func(s S, rest ...E) S` and run
        // it through the ordinary argument checker.
        let s_param = crate::object::var::new_param(&mut self.objects, "", s_typ);
        let elem_slice = new_slice(&mut self.types, elem);
        let rest_param = crate::object::var::new_param(&mut self.objects, "", elem_slice);
        let params = crate::tuple::new_tuple(&mut self.types, &[s_param, rest_param]);
        let result_var = crate::object::var::new_param(&mut self.objects, "", s_typ);
        let results = crate::tuple::new_tuple(&mut self.types, &[result_var]);
        let sig = new_signature_type(&mut self.types, None, &[], &[], params, results, true);
        // Discard the result — we already know the result type is S.
        self.arguments(call, sig, &[]);

        x.mode = OperandMode::Value;
        x.typ = Some(s_typ); // unchanged
        true
    }

    /// `len(x)` / `cap(x)`.
    fn builtin_len_cap(&mut self, x: &mut Operand, id: BuiltinId, name: &str) -> bool {
        let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
        let under = xtyp.underlying(&self.types);
        let t = self.array_ptr_deref(under);

        let mut mode = OperandMode::Invalid;
        let mut val: Option<Value> = None;

        match self.types.get(t) {
            TypeData::Basic(_) if is_string(&self.types, t) && id == BuiltinId::Len => {
                if x.mode == OperandMode::Constant {
                    mode = OperandMode::Constant;
                    if let Some(v) = &x.val {
                        val = Some(make_int64(string_val(v).len() as i64));
                    }
                } else {
                    mode = OperandMode::Value;
                }
            }
            TypeData::Array(_) => {
                // spec: len/cap of an array (without calls/receives) is a
                // constant. We don't track hasCallOrRecv, so treat as constant.
                mode = OperandMode::Constant;
                let len = crate::array::array_len(&self.types, t);
                val = Some(if len >= 0 {
                    make_int64(len)
                } else {
                    Value::Unknown
                });
            }
            TypeData::Slice(_) | TypeData::Chan(_) => mode = OperandMode::Value,
            TypeData::Map(_) if id == BuiltinId::Len => mode = OperandMode::Value,
            // DEFERRED: type-parameter (Interface/underIs) operands.
            _ => {}
        }

        if mode == OperandMode::Invalid {
            if is_valid(&self.types, under) {
                let code = if id == BuiltinId::Len {
                    Code::InvalidLen
                } else {
                    Code::InvalidCap
                };
                let xs = self.operand_str(x);
                self.error(
                    x.pos() as u32,
                    code,
                    format!("invalid argument: {} for built-in {}", xs, name),
                );
            }
            return false;
        }

        x.mode = mode;
        x.typ = Some(self.basic(BasicKind::Int));
        x.val = val;
        true
    }

    /// `copy(dst, src []E) int`.
    fn builtin_copy<'a>(&mut self, x: &mut Operand<'a>, args: &[Operand<'a>]) -> bool {
        let y = &args[1];
        let dst_typ = x.typ.unwrap_or_else(|| self.invalid_type());
        let src_typ = y.typ.unwrap_or_else(|| self.invalid_type());

        let dst_e = match self.slice_elem_of(dst_typ) {
            Some(e) => e,
            None => {
                let xs = self.operand_str(x);
                self.error(
                    x.pos() as u32,
                    Code::InvalidCopy,
                    format!("invalid copy: destination {} is not a slice", xs),
                );
                return false;
            }
        };
        // copy([]byte, string) special case: a string source copies its bytes.
        let src_is_string = is_string(&self.types, src_typ.underlying(&self.types));
        let src_e = match self.slice_elem_of(src_typ) {
            Some(e) => e,
            None if src_is_string => self.basic(BasicKind::Uint8),
            None => {
                let ys = self.operand_str(y);
                self.error(
                    y.pos() as u32,
                    Code::InvalidCopy,
                    format!("invalid copy: source {} is not a slice or string", ys),
                );
                return false;
            }
        };

        if !crate::predicates::identical(
            &mut self.types,
            &self.objects,
            &self.packages,
            dst_e,
            src_e,
        ) {
            let (de, se) = (self.type_str(dst_e), self.type_str(src_e));
            self.error(
                x.pos() as u32,
                Code::InvalidCopy,
                format!(
                    "invalid copy: arguments have different element types {} and {}",
                    de, se
                ),
            );
            return false;
        }

        x.mode = OperandMode::Value;
        x.typ = Some(self.basic(BasicKind::Int));
        true
    }

    /// The element type of a slice operand's type, or `None` if it isn't a
    /// slice. Simplified `sliceElem` (type-parameter type sets deferred).
    /// Element type of `t` when `t` is a slice — **or is constrained to be one**.
    ///
    /// `append`, `copy` and friends ask go/types for `coreType(S)`, not
    /// `under(S)`: a type parameter's underlying type is its constraint
    /// interface, and only its *type set* says whether every member is a slice.
    /// syncthing's `func without[E comparable, S ~[]E](s S, e E) S` is the shape
    /// that makes the difference — with `under` the `append(s[:i], …)` inside it
    /// is "argument S is not a slice", the package is ill-typed, and every
    /// analyzer that refuses ill-typed packages goes quiet for the whole of
    /// `lib/model`.
    fn slice_elem_of(&mut self, t: TypeId) -> Option<TypeId> {
        let (u, _) = common_under(&mut self.types, &self.objects, &self.packages, t, None);
        let u = u?;
        match self.types.get(u) {
            TypeData::Slice(_) => Some(slice_elem(&self.types, u)),
            _ => None,
        }
    }

    /// If `t` is a pointer to an array, return that array type; else `t`.
    /// `t` is expected to be an underlying type. Mirrors `arrayPtrDeref`.
    fn array_ptr_deref(&self, t: TypeId) -> TypeId {
        if matches!(self.types.get(t), TypeData::Pointer(_)) {
            let base = pointer_elem(&self.types, t).underlying(&self.types);
            if matches!(self.types.get(base), TypeData::Array(_)) {
                return base;
            }
        }
        t
    }

    /// Evaluate each expression for its side effects (so variables are marked
    /// used and errors surface) discarding the result.
    fn use_exprs(&mut self, args: &[Expr]) {
        for a in args {
            let mut op = Operand::invalid();
            self.expr(&mut op, a);
        }
    }
}

/// Whether `t` supports ordering (integers, floats, strings) — used by
/// `min`/`max`. Mirrors `allOrdered` for the non-type-parameter case.
fn is_ordered(arena: &crate::arena::TypeArena, t: TypeId) -> bool {
    is_integer_or_float(arena, t) || is_string(arena, t)
}

/// Strips enclosing parentheses from an expression. Mirrors `syntax.Unparen` /
/// `ast.Unparen`.
fn unparen_expr(e: &Expr) -> &Expr {
    let mut cur = e;
    while let Expr::ParenExpr(p) = cur {
        cur = &p.x;
    }
    cur
}
