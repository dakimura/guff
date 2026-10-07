//! The operand checks of x/tools v0.50's printf: `types.go` (`matchArgType`,
//! `argMatcher`, `isConvertibleToString`) and the predicates `okPrintfArg`
//! and `checkPrint` make around it (`isFormatter`, `isFunctionValue`,
//! `recursiveStringer`).
//!
//! Method questions go through `LookupFieldOrMethod` with `addressable =
//! false`, as upstream's do, so a `String` declared on `*T` does not make a
//! `T` value a Stringer, and a method promoted through an embedded field does.
//! guff used to look only at the methods declared on the named type itself and
//! accept any struct with an embedded field instead; both under-reported.

use std::collections::HashSet;

use guff::ast::{Decl, Expr, UnaryExpr};
use guff::token::Token;
use guff_analysis::Pass;
use guff_types::api_predicates::api_convertible_to;
use guff_types::alias::unalias_readonly;
use guff_types::arena::{ObjectData, ObjectId, TypeArena, TypeData};
use guff_types::basic::BasicKind;
use guff_types::lookup::lookup_field_or_method;
use guff_types::signature::{signature_params, signature_recv, signature_results};
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::TypeId;

use crate::govet_util::expr_type;

// `printfArgType`, bit for bit.
pub(crate) const ARG_BOOL: u32 = 1 << 0;
pub(crate) const ARG_BYTE: u32 = 1 << 1;
pub(crate) const ARG_INT: u32 = 1 << 2;
pub(crate) const ARG_RUNE: u32 = 1 << 3;
pub(crate) const ARG_STRING: u32 = 1 << 4;
pub(crate) const ARG_FLOAT: u32 = 1 << 5;
pub(crate) const ARG_COMPLEX: u32 = 1 << 6;
pub(crate) const ARG_POINTER: u32 = 1 << 7;
pub(crate) const ARG_ERROR: u32 = 1 << 8;
pub(crate) const ANY_TYPE: u32 = u32::MAX;

/// A scratch copy of the package's type arena.
///
/// `LookupFieldOrMethod` and `ConvertibleTo` take the arena mutably (they may
/// intern a pointer type on the way), and the package's own is shared. One
/// clone per pass serves every call site; ids from the package arena stay
/// valid in it.
#[derive(Default)]
pub(crate) struct Scratch(Option<TypeArena>);

impl Scratch {
    fn types<'s>(&'s mut self, pass: &Pass<'_>) -> Option<&'s mut TypeArena> {
        let art = pass.pkg().type_artifacts.as_ref()?;
        Some(self.0.get_or_insert_with(|| art.types.clone()))
    }
}

/// `matchArgType`: whether `arg` suits a verb accepting `t`, and the reason
/// upstream appends to the diagnostic when it does not.
pub(crate) fn match_arg_type(
    pass: &Pass<'_>,
    scratch: &mut Scratch,
    t: u32,
    arg: &Expr,
) -> (Option<String>, bool) {
    // %v, %T accept any argument type.
    if t == ANY_TYPE {
        return (None, true);
    }
    let Some(typ) = expr_type(pass, arg) else {
        return (None, true); // probably a type check problem
    };
    let mut m = ArgMatcher {
        pass,
        scratch,
        t,
        seen: HashSet::new(),
        reason: None,
    };
    let ok = m.matches(typ, true);
    (m.reason, ok)
}

/// `argMatcher`.
struct ArgMatcher<'p, 'a, 's> {
    pass: &'p Pass<'a>,
    scratch: &'s mut Scratch,
    t: u32,
    seen: HashSet<TypeId>,
    reason: Option<String>,
}

impl ArgMatcher<'_, '_, '_> {
    /// `match`. `top_level` is true for the operand's own type, the only
    /// place a pointer to a composite is followed.
    fn matches(&mut self, typ: TypeId, top_level: bool) -> bool {
        let pass = self.pass;
        let Some(art) = pass.pkg().type_artifacts.as_ref() else {
            return true;
        };

        // %w accepts only errors.
        if self.t == ARG_ERROR {
            return convertible_to_error(pass, self.scratch, typ);
        }

        // If the type implements fmt.Formatter, we have nothing to check.
        if is_formatter(pass, self.scratch, typ) {
            return true;
        }

        // If we can use a string, might arg (dynamically) implement the
        // Stringer or Error interface?
        if self.t & ARG_STRING != 0 && is_convertible_to_string(pass, self.scratch, typ) {
            return true;
        }

        let unaliased = unalias_readonly(&art.types, typ);
        if matches!(art.types.get(unaliased), TypeData::TypeParam(_)) {
            // Avoid infinite recursion through type parameters.
            if !self.seen.insert(unaliased) {
                return true;
            }
            // `typeparams.StructuralTerms`: the constraint's normalized terms.
            let Some(types) = self.scratch.types(pass) else {
                return true;
            };
            let iface = guff_types::type_param_iface(types, &art.objects, &art.packages, unaliased);
            let tset = guff_types::interface_typeset(types, &art.objects, &art.packages, iface);
            let Some(term_types) = tset.term_types() else {
                // No restrictions on the underlying of typ. Type parameters
                // implementing error, fmt.Formatter, or fmt.Stringer were
                // handled above, so some element of the type set violates
                // one of the arg type checks below.
                return false;
            };
            if term_types.is_empty() {
                return true; // invalid type (possibly an empty type set)
            }
            let mut terms = Vec::new();
            tset.is(|tilde, t| {
                if let Some(t) = t {
                    terms.push((tilde, t));
                }
                true
            });

            // Only report a reason if typ is the argument type, otherwise it
            // won't make sense.
            let report_reason = self.seen.len() == 1;

            for (tilde, term) in terms {
                if !self.matches(term, top_level) {
                    if report_reason {
                        let name = type_string(pass, term);
                        self.reason = Some(if tilde {
                            format!("contains ~{name}")
                        } else {
                            format!("contains {name}")
                        });
                    }
                    return false;
                }
            }
            return true;
        }

        let typ = typ.underlying(&art.types);
        if !self.seen.insert(typ) {
            // We've already considered typ, or are in the process of
            // considering it.
            return true;
        }

        match art.types.get(typ) {
            TypeData::Signature(_) => self.t == ARG_POINTER,

            TypeData::Map(m) => {
                if self.t == ARG_POINTER {
                    return true;
                }
                // Recur: map[int]int matches %d.
                let (key, elem) = (m.key(), m.elem());
                self.matches(key, false) && self.matches(elem, false)
            }

            TypeData::Chan(_) => self.t & ARG_POINTER != 0,

            TypeData::Array(a) => {
                // Same as slice.
                let elem = a.elem();
                if is_byte(&art.types, elem.underlying(&art.types)) && self.t & ARG_STRING != 0 {
                    return true; // %s matches []byte
                }
                // Recur: []int matches %d.
                self.matches(elem, false)
            }

            TypeData::Slice(s) => {
                // Same as array.
                let elem = s.elem();
                if is_byte(&art.types, elem.underlying(&art.types)) && self.t & ARG_STRING != 0 {
                    return true; // %s matches []byte
                }
                if self.t == ARG_POINTER {
                    return true; // %p prints a slice's 0th element
                }
                // Recur: []int matches %d.
                self.matches(elem, false)
            }

            TypeData::Pointer(p) => {
                let elem = p.elem();
                // A known pointer to an invalid type, probably something from
                // a failed import.
                if matches!(art.types.get(elem), TypeData::Basic(b) if b.kind() == BasicKind::Invalid)
                {
                    return true;
                }
                // If it's actually a pointer with %p, it prints as one.
                if self.t == ARG_POINTER {
                    return true;
                }
                if matches!(
                    art.types.get(unalias_readonly(&art.types, elem)),
                    TypeData::TypeParam(_)
                ) {
                    return true; // We don't know whether the logic below applies. Give up.
                }
                let under = elem.underlying(&art.types);
                match art.types.get(under) {
                    TypeData::Struct(_)
                    | TypeData::Array(_)
                    | TypeData::Slice(_)
                    | TypeData::Map(_) => {}
                    _ => {
                        // Check whether the rest can print pointers.
                        self.pointer_reason();
                        return self.t & ARG_POINTER != 0;
                    }
                }
                // A top-level pointer to a struct, array, slice or map prints
                // as what it points to. Pointers in nested levels are not
                // supported "to minimize fmt running into loops".
                if !top_level {
                    return false;
                }
                self.matches(under, false)
            }

            TypeData::Struct(_) => {
                // All the elements of the struct must match the expected type.
                let n = guff_types::r#struct::struct_num_fields(&art.types, typ);
                for i in 0..n {
                    let f = guff_types::r#struct::struct_field(&art.types, typ, i);
                    let ObjectData::Var(v) = art.objects.get(f) else {
                        continue;
                    };
                    let field_type = v.typ();
                    if !self.matches(field_type, false) {
                        return false;
                    }
                    // Issue #17798: unexported Stringer or error cannot be
                    // properly formatted.
                    if self.t & ARG_STRING != 0
                        && !is_exported(v.name())
                        && is_convertible_to_string(pass, self.scratch, field_type)
                    {
                        return false;
                    }
                }
                true
            }

            // There's little we can do. Whether any particular verb is valid
            // depends on the argument.
            TypeData::Interface(_) => true,

            TypeData::Basic(b) => {
                let t = self.t;
                match b.kind() {
                    BasicKind::UntypedBool | BasicKind::Bool => t & ARG_BOOL != 0,
                    BasicKind::Uint8 => t & (ARG_INT | ARG_BYTE) != 0,
                    BasicKind::Int32 | BasicKind::UntypedRune => t & (ARG_INT | ARG_RUNE) != 0,
                    BasicKind::UntypedInt
                    | BasicKind::Int
                    | BasicKind::Int8
                    | BasicKind::Int16
                    | BasicKind::Int64
                    | BasicKind::Uint
                    | BasicKind::Uint16
                    | BasicKind::Uint32
                    | BasicKind::Uint64
                    | BasicKind::Uintptr => t & ARG_INT != 0,
                    BasicKind::UntypedFloat | BasicKind::Float32 | BasicKind::Float64 => {
                        t & ARG_FLOAT != 0
                    }
                    BasicKind::UntypedComplex | BasicKind::Complex64 | BasicKind::Complex128 => {
                        t & ARG_COMPLEX != 0
                    }
                    BasicKind::UntypedString | BasicKind::String => t & ARG_STRING != 0,
                    BasicKind::UnsafePointer => {
                        self.pointer_reason();
                        t & ARG_POINTER != 0
                    }
                    BasicKind::UntypedNil => false,
                    // Probably a type check problem.
                    BasicKind::Invalid => true,
                }
            }

            _ => false,
        }
    }

    /// `m.t&argPointer == 0 && m.t&argInt != 0`: an integer verb that is not
    /// also a pointer verb, looking at a pointer.
    fn pointer_reason(&mut self) {
        if self.t & ARG_POINTER == 0 && self.t & ARG_INT != 0 {
            self.reason = Some("use %p for a pointer".to_string());
        }
    }
}

fn is_byte(types: &TypeArena, t: TypeId) -> bool {
    matches!(types.get(t), TypeData::Basic(b) if b.kind() == BasicKind::Uint8)
}

fn is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

pub(crate) fn type_string(pass: &Pass<'_>, typ: TypeId) -> String {
    let Some(art) = pass.pkg().type_artifacts.as_ref() else {
        return "?".into();
    };
    guff_types::typestring::type_string(&art.types, &art.objects, &art.packages, typ, None)
}

/// The predeclared `error` type.
fn universe_error(pass: &Pass<'_>) -> Option<TypeId> {
    let art = pass.pkg().type_artifacts.as_ref()?;
    for oid in art.objects.ids() {
        let ObjectData::TypeName(tn) = art.objects.get(oid) else {
            continue;
        };
        if tn.name() != "error" || oid.pkg(&art.objects).is_some() {
            continue;
        }
        return tn.typ();
    }
    None
}

/// `types.ConvertibleTo(typ, errorType)`.
fn convertible_to_error(pass: &Pass<'_>, scratch: &mut Scratch, typ: TypeId) -> bool {
    let Some(art) = pass.pkg().type_artifacts.as_ref() else {
        return true;
    };
    let Some(err) = universe_error(pass) else {
        return true;
    };
    let Some(types) = scratch.types(pass) else {
        return true;
    };
    api_convertible_to(types, &art.objects, &art.packages, typ, err)
}

/// `types.LookupFieldOrMethod(typ, false, nil, name)` narrowed to a method:
/// the method and its signature.
fn lookup_method(
    pass: &Pass<'_>,
    scratch: &mut Scratch,
    typ: TypeId,
    name: &str,
) -> Option<(ObjectId, TypeId)> {
    let art = pass.pkg().type_artifacts.as_ref()?;
    let types = scratch.types(pass)?;
    let (obj, _, _) =
        lookup_field_or_method(types, &art.objects, &art.packages, typ, false, None, name)
            .found()?;
    let ObjectData::Func(f) = art.objects.get(obj) else {
        return None;
    };
    Some((obj, f.typ()?))
}

/// The `i`th parameter (or result) type of a tuple.
fn tuple_type(pass: &Pass<'_>, tuple: Option<TypeId>, i: usize) -> Option<TypeId> {
    let art = pass.pkg().type_artifacts.as_ref()?;
    let obj = tuple_at(&art.types, tuple?, i);
    match art.objects.get(obj) {
        ObjectData::Var(v) => Some(v.typ()),
        _ => None,
    }
}

/// `isStringer`: `func() string`, with the result exactly the predeclared
/// `string`.
fn is_stringer_sig(pass: &Pass<'_>, sig: TypeId) -> bool {
    let Some(art) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let (params, results) = (
        signature_params(&art.types, sig),
        signature_results(&art.types, sig),
    );
    if tuple_len(&art.types, params) != 0 || tuple_len(&art.types, results) != 1 {
        return false;
    }
    tuple_type(pass, results, 0).is_some_and(|t| {
        let t = unalias_readonly(&art.types, t);
        matches!(art.types.get(t), TypeData::Basic(b) if b.kind() == BasicKind::String)
    })
}

/// `isFormatter`: could a value of this type implement `fmt.Formatter`?
///
/// Any interface could — the value it holds might — except a type
/// parameter, whose constraint says what it can be. Otherwise it needs a
/// `Format(fmt.State, rune)` method.
pub(crate) fn is_formatter(pass: &Pass<'_>, scratch: &mut Scratch, typ: TypeId) -> bool {
    let Some(art) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let is_type_param = matches!(
        art.types.get(unalias_readonly(&art.types, typ)),
        TypeData::TypeParam(_)
    );
    if !is_type_param
        && matches!(
            art.types.get(typ.underlying(&art.types)),
            TypeData::Interface(_)
        )
    {
        return true;
    }
    let Some((_, sig)) = lookup_method(pass, scratch, typ, "Format") else {
        return false;
    };
    let (params, results) = (
        signature_params(&art.types, sig),
        signature_results(&art.types, sig),
    );
    tuple_len(&art.types, params) == 2
        && tuple_len(&art.types, results) == 0
        && tuple_type(pass, params, 0)
            .is_some_and(|t| crate::govet_util::is_type_named(pass, t, "fmt", "State"))
        && tuple_type(pass, params, 1).is_some_and(|t| {
            let t = unalias_readonly(&art.types, t);
            matches!(art.types.get(t), TypeData::Basic(b) if b.kind() == BasicKind::Int32)
        })
}

/// `isConvertibleToString`: does the value print through `Error()` or
/// `String()`?
fn is_convertible_to_string(pass: &Pass<'_>, scratch: &mut Scratch, typ: TypeId) -> bool {
    let Some(art) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    // Not untyped nil, which is convertible to both of the interfaces below,
    // as it would just panic anyway.
    let t = unalias_readonly(&art.types, typ);
    if matches!(art.types.get(t), TypeData::Basic(b) if b.kind() == BasicKind::UntypedNil) {
        return false;
    }
    if convertible_to_error(pass, scratch, typ) {
        return true; // via .Error()
    }
    // Does it implement fmt.Stringer?
    lookup_method(pass, scratch, typ, "String").is_some_and(|(_, sig)| is_stringer_sig(pass, sig))
}

/// `isFunctionValue`: the expression is a function, not a call of one. The
/// type itself, not its underlying: a named func type with a `String` method
/// is fine.
pub(crate) fn is_function_value(pass: &Pass<'_>, e: &Expr) -> bool {
    let (Some(art), Some(typ)) = (pass.pkg().type_artifacts.as_ref(), expr_type(pass, e)) else {
        return false;
    };
    matches!(art.types.get(typ), TypeData::Signature(_))
}

/// `recursiveStringer`: is `e` the receiver (or `&receiver`) of the
/// `String` / `Error` method whose body it appears in? Returns that method's
/// full name.
pub(crate) fn recursive_stringer(
    pass: &Pass<'_>,
    scratch: &mut Scratch,
    e: &Expr,
) -> Option<String> {
    let art = pass.pkg().type_artifacts.as_ref()?;
    let info = pass.types_info()?;
    let typ = expr_type(pass, e)?;

    // It's unlikely to be a recursive stringer if it has a Format method.
    if is_formatter(pass, scratch, typ) {
        return None;
    }

    // Does e allow e.String() or e.Error()? Is the expression within the body
    // of that method?
    let pos = e.pos().0;
    let mut method = None;
    for name in ["String", "Error"] {
        let Some((obj, sig)) = lookup_method(pass, scratch, typ, name) else {
            continue;
        };
        if obj.pkg(&art.objects) != Some(art.type_pkg) {
            continue;
        }
        let Some(declared) = declared_method(pass, scratch, obj) else {
            continue;
        };
        if let Some(decl) = method_decl(pass, declared).filter(|d| d.0 <= pos && pos < d.1) {
            method = Some((obj, sig, decl.2));
            break;
        }
    }
    let (obj, sig, recv) = method?;
    if !is_stringer_sig(pass, sig) {
        return None;
    }

    // Is it the receiver r, or &r?
    let e = match e {
        Expr::UnaryExpr(UnaryExpr { op: Token::AND, x, .. }) => &**x,
        _ => e,
    };
    let Expr::Ident(id) = e else {
        return None;
    };
    // `Uses[id] == sig.Recv().Origin()`: the declared receiver, not the copy
    // an instantiated signature carries.
    if recv.is_none() || info.uses.get(&id.id).copied() != recv {
        return None;
    }
    crate::printf_wrappers::names_of(pass, obj).map(|(full, _)| full)
}

/// The method `obj` as the source declares it: `Func.Origin()`.
///
/// A method looked up on an instance whose arguments are the receiver's own
/// type parameters — `u` inside `func (u U[T]) String()` — is a copy guff
/// does not link back, so that case goes through the receiver's generic
/// type and takes its method of the same name.
fn declared_method(pass: &Pass<'_>, scratch: &mut Scratch, obj: ObjectId) -> Option<ObjectId> {
    let art = pass.pkg().type_artifacts.as_ref()?;
    let origin = guff_types::object::func::func_origin(&art.objects, obj);
    if origin != obj {
        return Some(origin);
    }
    let types = scratch.types(pass)?;
    let ObjectData::Func(f) = art.objects.get(obj) else {
        return None;
    };
    let ObjectData::Var(recv) = art.objects.get(signature_recv(types, f.typ()?)?) else {
        return None;
    };
    let mut t = unalias_readonly(types, recv.typ());
    if let TypeData::Pointer(p) = types.get(t) {
        t = unalias_readonly(types, p.elem());
    }
    if !matches!(types.get(t), TypeData::Named(_)) {
        return Some(obj);
    }
    let generic = guff_types::named::named_origin(types, t);
    let name = f.name();
    (0..guff_types::named::named_num_methods(types, generic))
        .map(|i| guff_types::named::named_method(types, generic, i))
        .find(|&m| m.name(&art.objects) == name)
        .or(Some(obj))
}

/// The source extent of the declaration of method `obj` — its `Scope()` —
/// and its declared receiver.
fn method_decl(pass: &Pass<'_>, obj: ObjectId) -> Option<(i64, i64, Option<ObjectId>)> {
    let info = pass.types_info()?;
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            if info.defs.get(&fd.name.id).and_then(|o| *o) != Some(obj) {
                continue;
            }
            let recv = fd
                .recv
                .as_ref()
                .and_then(|r| r.list.first())
                .and_then(|f| f.names.first())
                .and_then(|n| info.defs.get(&n.id).and_then(|o| *o));
            // A function's scope runs from its type to the end of its body.
            let end = fd.body.as_ref().map_or_else(|| fd.ty.end(), |b| b.end());
            return Some((fd.ty.pos().0, end.0, recv));
        }
    }
    None
}
