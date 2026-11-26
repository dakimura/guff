//! Port of `cmd/compile/internal/types2/typestring.go` — human-readable type
//! printing (`TypeString` / `WriteType` / `WriteSignature`).
//!
//! ## Scope (chunk 17)
//!
//! Only the **non-hashing** path is ported — the `qf`-driven, reader-facing
//! renderer. Go's `typeWriter` doubles as a *type hasher* (`ctxt != nil`,
//! `newTypeHasher`, `typeSet` canonicalisation) used by `Context` for
//! instance dedup; we don't need it because our `Context` keys on stable
//! arena `TypeId`s (chunk 9). Dropping the hasher means this whole module is
//! **read-only** over the arenas (the non-`ctxt` interface branch walks the
//! explicit `methods`/`embeddeds`, never the lazily-computed type set).
//!
//! ## Faithful-but-simplified output
//!
//! These Go niceties are intentionally omitted (each is a hashing concern or
//! a disambiguation detail that affects prettiness, not type identity):
//!
//! - the `universeAny` / `universeComparable` pointer-identity special cases
//!   (an empty interface prints `interface{}`; the predeclared `any` alias
//!   still prints `any` via the `Alias` path);
//! - struct-field `/* package … */` annotations (`pkgInfo`);
//! - type-parameter subscripts and the `/* with X declared at … */` hint;
//! - the type-hashing `$<index>` placeholder for local type parameters.
//!
//! Everything structural (arrays, slices, structs incl. tags, pointers,
//! tuples, signatures incl. type-parameter lists with bound-sharing, unions,
//! interfaces, maps, channels incl. the `chan (<-chan T)` parenthesisation,
//! named/alias instances and parameterised forms) is faithful.

use crate::arena::{ObjectArena, ObjectData, PackageArena, PackageId, TypeArena, TypeData, TypeId};
use crate::chan::ChanDir;
use crate::object::is_exported;

/// Controls how package-level objects are qualified. `None` is equivalent to
/// using each package's import path (Go's nil `Qualifier`); a closure may
/// return `""` to print the bare object name.
///
/// Equivalent to `types2.Qualifier` (adapted to the arena: the closure takes
/// the [`PackageId`] and the package arena instead of a `*Package`).
pub type Qualifier<'q> = Option<&'q dyn Fn(PackageId, &PackageArena) -> String>;

/// Returns the string representation of `typ`.
///
/// Equivalent to `TypeString`.
pub fn type_string(
    arena: &TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    typ: TypeId,
    qf: Qualifier<'_>,
) -> String {
    let mut w = TypeWriter::new(arena, oarena, parena, qf);
    w.typ(typ);
    w.buf
}

/// Writes the representation of signature `sig` (without a leading `func`).
///
/// Equivalent to `WriteSignature`.
pub fn signature_string(
    arena: &TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    sig: TypeId,
    qf: Qualifier<'_>,
) -> String {
    let mut w = TypeWriter::new(arena, oarena, parena, qf);
    w.signature(sig);
    w.buf
}

const TERM_SEP: &str = " | ";

struct TypeWriter<'a> {
    buf: String,
    arena: &'a TypeArena,
    oarena: &'a ObjectArena,
    parena: &'a PackageArena,
    qf: Qualifier<'a>,
    seen: Vec<TypeId>,
    param_names: bool,
}

impl<'a> TypeWriter<'a> {
    fn new(
        arena: &'a TypeArena,
        oarena: &'a ObjectArena,
        parena: &'a PackageArena,
        qf: Qualifier<'a>,
    ) -> Self {
        TypeWriter {
            buf: String::new(),
            arena,
            oarena,
            parena,
            qf,
            seen: Vec::new(),
            param_names: true,
        }
    }

    fn byte(&mut self, b: u8) {
        // (We are never in hashing mode, so no ' ' -> '#' rewrite.)
        self.buf.push(b as char);
        if b == b',' || b == b';' {
            self.buf.push(' ');
        }
    }

    fn string(&mut self, s: &str) {
        self.buf.push_str(s);
    }

    fn error(&mut self, msg: &str) {
        self.buf.push('<');
        self.buf.push_str(msg);
        self.buf.push('>');
    }

    fn typ(&mut self, typ: TypeId) {
        if self.seen.contains(&typ) {
            let name = go_kind_name(self.arena, typ);
            self.error(&format!("cycle to {}", name));
            return;
        }
        self.seen.push(typ);
        self.typ_inner(typ);
        if let Some(pos) = self.seen.iter().rposition(|&t| t == typ) {
            self.seen.remove(pos); // mirror Go's `defer delete(w.seen, typ)`
        }
    }

    fn typ_inner(&mut self, typ: TypeId) {
        // Note: every arm extracts the values it needs from the arena *before*
        // calling any `&mut self` method, so no shared borrow of `self.arena`
        // is held across a recursive `self.typ(..)` / `self.byte(..)` call.
        match kind_of(self.arena, typ) {
            Kind::Basic => {
                let name = basic_name(self.arena, typ);
                // Exported basic types live in package unsafe (currently just
                // unsafe.Pointer); qf is ignored for unsafe here.
                if is_exported(&name) {
                    self.string("unsafe.");
                }
                self.string(&name);
            }

            Kind::Array => {
                let len = crate::array::array_len(self.arena, typ);
                let elem = crate::array::array_elem(self.arena, typ);
                self.byte(b'[');
                self.string(&len.to_string());
                self.byte(b']');
                self.typ(elem);
            }

            Kind::Slice => {
                let elem = crate::slice::slice_elem(self.arena, typ);
                self.string("[]");
                self.typ(elem);
            }

            Kind::Struct => self.write_struct(typ),

            Kind::Pointer => {
                let base = crate::pointer::pointer_elem(self.arena, typ);
                self.byte(b'*');
                self.typ(base);
            }

            Kind::Tuple => self.tuple(Some(typ), false),

            Kind::Signature => {
                self.string("func");
                self.signature(typ);
            }

            Kind::Union => self.write_union(typ),

            Kind::Interface => self.write_interface(typ),

            Kind::Map => {
                let key = crate::map::map_key(self.arena, typ);
                let elem = crate::map::map_elem(self.arena, typ);
                self.string("map[");
                self.typ(key);
                self.byte(b']');
                self.typ(elem);
            }

            Kind::Chan => {
                let dir = crate::chan::chan_dir(self.arena, typ);
                let elem = crate::chan::chan_elem(self.arena, typ);
                self.write_chan(dir, elem);
            }

            Kind::Named => self.write_named(typ),

            Kind::TypeParam => self.write_type_param(typ),

            Kind::Alias => self.write_alias(typ),
        }
    }

    fn write_struct(&mut self, typ: TypeId) {
        let n = crate::r#struct::struct_num_fields(self.arena, typ);
        self.string("struct{");
        for i in 0..n {
            if i > 0 {
                self.byte(b';');
            }
            let field = crate::r#struct::struct_field(self.arena, typ, i);
            let (name, embedded) = match self.oarena.get(field) {
                ObjectData::Var(v) => (v.name().to_string(), v.embedded()),
                _ => (String::new(), false),
            };
            let ftyp = field.typ(self.oarena);
            if !embedded {
                self.string(&name);
                self.byte(b' ');
            }
            match ftyp {
                Some(ft) => self.typ(ft),
                None => self.error("nil"),
            }
            let tag = crate::r#struct::struct_tag(self.arena, typ, i).to_string();
            if !tag.is_empty() {
                self.byte(b' ');
                let q = quote(&tag);
                self.string(&q);
            }
        }
        self.byte(b'}');
    }

    fn write_union(&mut self, typ: TypeId) {
        let n = crate::union::union_len(self.arena, typ);
        if n == 0 {
            self.error("empty union");
            return;
        }
        for i in 0..n {
            if i > 0 {
                self.string(TERM_SEP);
            }
            let (tilde, tt) = {
                let term = crate::union::union_term(self.arena, typ, i);
                (term.tilde(), term.typ())
            };
            if tilde {
                self.byte(b'~');
            }
            self.typ(tt);
        }
    }

    fn write_interface(&mut self, typ: TypeId) {
        let implicit = crate::interface::interface_is_implicit(self.arena, typ);
        let n_methods = crate::interface::interface_num_explicit_methods(self.arena, typ);
        let n_embeds = crate::interface::interface_num_embeddeds(self.arena, typ);

        if implicit {
            if n_methods == 0 && n_embeds == 1 {
                let e = crate::interface::interface_embedded_type(self.arena, typ, 0);
                self.typ(e);
                return;
            }
            // Something's wrong with the implicit interface; flag and continue.
            self.string("/* implicit */ ");
        }

        self.string("interface{");
        let mut first = true;
        for i in 0..n_methods {
            if !first {
                self.byte(b';');
            }
            first = false;
            let m = crate::interface::interface_explicit_method(self.arena, typ, i);
            let mname = m.name(self.oarena).to_string();
            let msig = m.typ(self.oarena);
            self.string(&mname);
            if let Some(msig) = msig {
                self.signature(msig);
            }
        }
        for i in 0..n_embeds {
            if !first {
                self.byte(b';');
            }
            first = false;
            let e = crate::interface::interface_embedded_type(self.arena, typ, i);
            self.typ(e);
        }
        self.byte(b'}');
    }

    fn write_chan(&mut self, dir: ChanDir, elem: TypeId) {
        let mut parens = false;
        let s = match dir {
            ChanDir::SendRecv => {
                // chan (<-chan T) requires parentheses
                if matches!(self.arena.get(elem), TypeData::Chan(_))
                    && crate::chan::chan_dir(self.arena, elem) == ChanDir::RecvOnly
                {
                    parens = true;
                }
                "chan "
            }
            ChanDir::SendOnly => "chan<- ",
            ChanDir::RecvOnly => "<-chan ",
        };
        self.string(s);
        if parens {
            self.byte(b'(');
        }
        self.typ(elem);
        if parens {
            self.byte(b')');
        }
    }

    fn write_named(&mut self, typ: TypeId) {
        let obj = crate::named::named_obj(self.arena, typ);
        self.type_name(obj);
        // instantiated type -> targs; else parameterised type -> tparams
        let targs: Vec<TypeId> = crate::named::named_type_args(self.arena, typ)
            .map(|l| l.list().to_vec())
            .unwrap_or_default();
        if !targs.is_empty() {
            self.type_list(&targs);
            return;
        }
        let tparams: Vec<TypeId> = match self.arena.get(typ) {
            TypeData::Named(n) => n
                .type_params()
                .map(|l| l.list().to_vec())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        if !tparams.is_empty() {
            self.tparam_list(&tparams);
        }
    }

    fn write_alias(&mut self, typ: TypeId) {
        let obj = crate::alias::alias_obj(self.arena, typ);
        self.type_name(obj);
        let (targs, tparams): (Vec<TypeId>, Vec<TypeId>) = match self.arena.get(typ) {
            TypeData::Alias(a) => (
                a.type_args().map(|l| l.list().to_vec()).unwrap_or_default(),
                a.type_params()
                    .map(|l| l.list().to_vec())
                    .unwrap_or_default(),
            ),
            _ => (Vec::new(), Vec::new()),
        };
        if !targs.is_empty() {
            self.type_list(&targs);
        } else if !tparams.is_empty() {
            self.tparam_list(&tparams);
        }
    }

    fn write_type_param(&mut self, typ: TypeId) {
        let obj = crate::typeparam::type_param_obj(self.arena, typ);
        // (We drop Go's $<index> hashing form, subscripts, and the
        // "declared at" predeclared-name hint — all output niceties.)
        let name = obj.name(self.oarena).to_string();
        if name.is_empty() {
            self.error("unnamed type parameter");
        } else {
            self.string(&name);
        }
    }

    fn type_list(&mut self, list: &[TypeId]) {
        self.byte(b'[');
        for (i, &t) in list.iter().enumerate() {
            if i > 0 {
                self.byte(b',');
            }
            self.typ(t);
        }
        self.byte(b']');
    }

    fn tparam_list(&mut self, list: &[TypeId]) {
        self.byte(b'[');
        let mut prev: Option<TypeId> = None;
        for (i, &tpar) in list.iter().enumerate() {
            let bound = crate::typeparam::type_param_constraint(self.arena, tpar);
            if i > 0 {
                if bound != prev {
                    // bound changed — write the previous one before advancing
                    if let Some(p) = prev {
                        self.byte(b' ');
                        self.typ(p);
                    }
                }
                self.byte(b',');
            }
            prev = bound;
            self.typ(tpar);
        }
        if let Some(p) = prev {
            self.byte(b' ');
            self.typ(p);
        }
        self.byte(b']');
    }

    fn type_name(&mut self, obj: crate::arena::ObjectId) {
        let prefix = self.package_prefix(obj.pkg(self.oarena));
        self.string(&prefix);
        let name = obj.name(self.oarena).to_string();
        self.string(&name);
    }

    fn package_prefix(&self, pkg: Option<PackageId>) -> String {
        let pkg = match pkg {
            Some(p) => p,
            None => return String::new(),
        };
        let s = match self.qf {
            Some(f) => f(pkg, self.parena),
            None => self.parena.get(pkg).path().to_string(),
        };
        if s.is_empty() {
            String::new()
        } else {
            format!("{}.", s)
        }
    }

    fn tuple(&mut self, tup: Option<TypeId>, variadic: bool) {
        self.byte(b'(');
        if let Some(tup) = tup {
            let len = crate::tuple::tuple_len(self.arena, Some(tup));
            for i in 0..len {
                if i > 0 {
                    self.byte(b',');
                }
                let v = crate::tuple::tuple_at(self.arena, tup, i);
                let name = v.name(self.oarena).to_string();
                if !name.is_empty() && self.param_names {
                    self.string(&name);
                    self.byte(b' ');
                }
                let vtyp = match v.typ(self.oarena) {
                    Some(t) => t,
                    None => {
                        self.error("nil");
                        continue;
                    }
                };
                if variadic && i == len - 1 {
                    if matches!(self.arena.get(vtyp), TypeData::Slice(_)) {
                        let elem = crate::slice::slice_elem(self.arena, vtyp);
                        self.string("...");
                        self.typ(elem);
                    } else {
                        // append(slice, str...)-style irregular notation
                        self.typ(vtyp);
                        self.string("...");
                    }
                } else {
                    self.typ(vtyp);
                }
            }
        }
        self.byte(b')');
    }

    fn signature(&mut self, sig: TypeId) {
        let tparams: Vec<TypeId> = match self.arena.get(sig) {
            TypeData::Signature(s) => s
                .type_params()
                .map(|l| l.list().to_vec())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        if !tparams.is_empty() {
            self.tparam_list(&tparams);
        }

        let params = crate::signature::signature_params(self.arena, sig);
        let variadic = crate::signature::signature_variadic(self.arena, sig);
        self.tuple(params, variadic);

        let results = crate::signature::signature_results(self.arena, sig);
        let n = crate::tuple::tuple_len(self.arena, results);
        if n == 0 {
            return;
        }
        self.byte(b' ');
        if n == 1 {
            // single unnamed result -> no parentheses
            let results_id = results.expect("n == 1 implies Some");
            let v = crate::tuple::tuple_at(self.arena, results_id, 0);
            if v.name(self.oarena).is_empty() {
                match v.typ(self.oarena) {
                    Some(t) => self.typ(t),
                    None => self.error("nil"),
                }
                return;
            }
        }
        // multiple or named result(s)
        self.tuple(results, false);
    }
}

#[derive(Copy, Clone)]
enum Kind {
    Basic,
    Array,
    Slice,
    Struct,
    Pointer,
    Tuple,
    Signature,
    Union,
    Interface,
    Map,
    Chan,
    Named,
    TypeParam,
    Alias,
}

fn kind_of(arena: &TypeArena, typ: TypeId) -> Kind {
    match arena.get(typ) {
        TypeData::Basic(_) => Kind::Basic,
        TypeData::Array(_) => Kind::Array,
        TypeData::Slice(_) => Kind::Slice,
        TypeData::Struct(_) => Kind::Struct,
        TypeData::Pointer(_) => Kind::Pointer,
        TypeData::Tuple(_) => Kind::Tuple,
        TypeData::Signature(_) => Kind::Signature,
        TypeData::Union(_) => Kind::Union,
        TypeData::Interface(_) => Kind::Interface,
        TypeData::Map(_) => Kind::Map,
        TypeData::Chan(_) => Kind::Chan,
        TypeData::Named(_) => Kind::Named,
        TypeData::TypeParam(_) => Kind::TypeParam,
        TypeData::Alias(_) => Kind::Alias,
    }
}

fn basic_name(arena: &TypeArena, typ: TypeId) -> String {
    match arena.get(typ) {
        TypeData::Basic(b) => b.name().to_string(),
        _ => String::new(),
    }
}

/// A short Go-style kind label used only inside `<cycle to …>` messages.
fn go_kind_name(arena: &TypeArena, typ: TypeId) -> &'static str {
    match arena.get(typ) {
        TypeData::Basic(_) => "Basic",
        TypeData::Array(_) => "Array",
        TypeData::Slice(_) => "Slice",
        TypeData::Struct(_) => "Struct",
        TypeData::Pointer(_) => "Pointer",
        TypeData::Tuple(_) => "Tuple",
        TypeData::Signature(_) => "Signature",
        TypeData::Union(_) => "Union",
        TypeData::Interface(_) => "Interface",
        TypeData::Map(_) => "Map",
        TypeData::Chan(_) => "Chan",
        TypeData::Named(_) => "Named",
        TypeData::TypeParam(_) => "TypeParam",
        TypeData::Alias(_) => "Alias",
    }
}

/// Minimal Go-`strconv.Quote`-style quoting for struct tags: wrap in double
/// quotes and escape `"`, `\`, and the common control chars. (Full Go quoting
/// also escapes arbitrary non-printable runes; struct tags are printable
/// ASCII in practice.)
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
