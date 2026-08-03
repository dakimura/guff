//! Port of the `Checker.structType` builder from `go/types/struct.go`
//! (`cmd/compile/internal/types2/struct.go`).
//!
//! **Chunk 33a**: builds a `Struct` type from a `struct { ... }` type
//! expression — named and embedded fields, tags, and the duplicate-field-name
//! check (via [`ObjSet`]). Wired into [`Checker::typ`] (typexpr) so struct type
//! expressions and `type T struct{...}` declarations resolve (recovering the
//! chunk-21 deferral).
//!
//! ## Deferrals (chunk-33a, see §8)
//!
//! - **chunk 61**: the embedded-field validity check (`later`: an embedded
//!   type, after one optional pointer deref, must not be `unsafe.Pointer`, a
//!   pointer, a pointer to an interface, or a (pointer to a) type parameter —
//!   `InvalidPtrEmbed`/`MisplacedTypeParam`) is now implemented in
//!   [`Checker::check_embedded_field`].
//! - `Info` recording (`recordDef`) is a no-op.
//! - tag unquoting is minimal (strips matching outer quotes/backticks; no
//!   escape processing).

use guff::ast::{Expr, StructType};
use guff_types_errors::Code;

use crate::check::Checker;
use crate::object::var::new_field;
use crate::objset::ObjSet;
use crate::r#struct::new_struct;
use crate::{ObjectId, TypeId};

impl Checker {
    /// Build a `Struct` type from a `struct { ... }` type expression.
    ///
    /// Equivalent to `Checker.structType`.
    pub fn struct_type(&mut self, e: &StructType) -> TypeId {
        let list = &e.fields.list;
        if list.is_empty() {
            return new_struct(&mut self.types, Vec::new(), Vec::new());
        }

        let mut fields: Vec<ObjectId> = Vec::new();
        let mut tags: Vec<String> = Vec::new();
        let mut any_tag = false;
        let mut fset = ObjSet::new();

        for f in list {
            let typ = match &f.ty {
                Some(t) => self.typ(t),
                None => self.invalid_type(),
            };
            let tag = f
                .tag
                .as_ref()
                .map(|t| unquote_tag(&t.value))
                .unwrap_or_default();
            if !tag.is_empty() {
                any_tag = true;
            }

            if !f.names.is_empty() {
                // named fields
                for name in &f.names {
                    if let Some(fld) = self.add_field(
                        &mut fields,
                        &mut tags,
                        &mut fset,
                        &name.name,
                        typ,
                        false,
                        &tag,
                        name.pos().0 as u32,
                    ) {
                        // go/types `recordDef` on the field Ident.
                        self.record_def(name, Some(fld));
                    }
                }
            } else {
                // embedded field: the field name is the type's (final) name.
                let te = f.ty.as_ref();
                let pos = f.ty.as_ref().map(|t| t.pos().0 as u32).unwrap_or(0);
                match te.and_then(embedded_field_ident) {
                    Some(id) => {
                        if let Some(fld) = self.add_field(
                            &mut fields,
                            &mut tags,
                            &mut fset,
                            &id.name,
                            typ,
                            true,
                            &tag,
                            id.pos().0 as u32,
                        ) {
                            // go/types records Defs[typeNameIdent] = field Var so
                            // ObjectOf prefers the field over the TypeName use.
                            // SA1019 (and anything else using ObjectOf) relies on
                            // this to not treat embedding as a use of a deprecated type.
                            self.record_def(id, Some(fld));
                        }

                        // spec: "An embedded type must be specified as a type
                        // name T or as a pointer to a non-interface type name
                        // *T, and T itself may not be a pointer type." Delayed
                        // to the end so we don't instantiate a possibly
                        // incomplete underlying type early.
                        self.check_embedded_field(typ, pos);
                    }
                    None => {
                        self.error(
                            pos,
                            Code::InvalidSyntaxTree,
                            "embedded field type has no name",
                        );
                        let inv = self.invalid_type();
                        let _ = self.add_field(
                            &mut fields,
                            &mut tags,
                            &mut fset,
                            "_",
                            inv,
                            true,
                            "",
                            pos,
                        );
                    }
                }
            }
        }

        let tags = if any_tag { tags } else { Vec::new() };
        new_struct(&mut self.types, fields, tags)
    }

    /// Queue the delayed validity check for an embedded field of type `typ`
    /// declared at `pos`.
    ///
    /// Equivalent to the `check.later(...)` closure at the end of
    /// `structType`'s embedded-field branch. An embedded field's type, after
    /// one optional pointer dereference, must not be `unsafe.Pointer`, a
    /// pointer, a pointer to an interface, or a (pointer to a) type parameter.
    fn check_embedded_field(&mut self, typ: TypeId, pos: u32) {
        self.later(move |c| {
            // t = deref(typ); is_ptr tracks whether one pointer was removed.
            let (t, is_ptr) = crate::lookup::deref(&c.types, typ);
            let u = t.underlying(&c.types);
            match c.types.get(u) {
                crate::arena::TypeData::Basic(_) => {
                    if !crate::predicates::is_valid(&c.types, t) {
                        return; // error was reported before
                    }
                    // unsafe.Pointer is treated like a regular pointer.
                    if crate::basic::basic_kind(&c.types, u)
                        == crate::basic::BasicKind::UnsafePointer
                    {
                        c.error(
                            pos,
                            Code::InvalidPtrEmbed,
                            "embedded field type cannot be unsafe.Pointer".to_string(),
                        );
                    }
                }
                crate::arena::TypeData::Pointer(_) => {
                    c.error(
                        pos,
                        Code::InvalidPtrEmbed,
                        "embedded field type cannot be a pointer".to_string(),
                    );
                }
                crate::arena::TypeData::Interface(_) => {
                    if crate::predicates::is_type_param(&c.types, t) {
                        // The error code is intentionally inconsistent with the
                        // other invalid-embedding codes (this restriction may be
                        // relaxed in the future).
                        c.error(
                            pos,
                            Code::MisplacedTypeParam,
                            "embedded field type cannot be a (pointer to a) type parameter"
                                .to_string(),
                        );
                    } else if is_ptr {
                        c.error(
                            pos,
                            Code::InvalidPtrEmbed,
                            "embedded field type cannot be a pointer to an interface".to_string(),
                        );
                    }
                }
                _ => {}
            }
        });
    }

    /// Create a field, run the duplicate-name check, and (if accepted) append
    /// it with its tag. Equivalent to `structType`'s inner `add` closure.
    /// Returns the field object when it was accepted into the struct.
    fn add_field(
        &mut self,
        fields: &mut Vec<ObjectId>,
        tags: &mut Vec<String>,
        fset: &mut ObjSet,
        name: &str,
        typ: TypeId,
        embedded: bool,
        tag: &str,
        pos: u32,
    ) -> Option<ObjectId> {
        let fld = new_field(&mut self.objects, name.to_string(), typ, embedded);
        fld.set_pkg(&mut self.objects, self.pkg);
        fld.set_pos(&mut self.objects, pos);

        // spec: "Within a struct, non-blank field names must be unique."
        if name != "_" {
            if fset.insert(&self.objects, &self.packages, fld).is_some() {
                self.error(
                    fld.pos(&self.objects),
                    Code::DuplicateDecl,
                    format!("{} redeclared", name),
                );
                return None;
            }
        }
        fields.push(fld);
        tags.push(tag.to_string());
        Some(fld)
    }
}

/// The identifier naming an embedded field's type, if any.
///
/// Equivalent to `embeddedFieldIdent`. Handles `T`, `*T` (not `**T`),
/// `pkg.T`, and the generic-instance forms `T[...]`.
fn embedded_field_ident(e: &Expr) -> Option<&guff::ast::Ident> {
    match e {
        Expr::Ident(id) => Some(id),
        Expr::StarExpr(s) => {
            // *T is valid, but **T is not.
            if matches!(&*s.x, Expr::StarExpr(_)) {
                None
            } else {
                embedded_field_ident(&s.x)
            }
        }
        Expr::SelectorExpr(s) => Some(&s.sel),
        Expr::IndexExpr(ie) => embedded_field_ident(&ie.x),
        Expr::IndexListExpr(ie) => embedded_field_ident(&ie.x),
        _ => None,
    }
}

/// Strip matching outer quotes/backticks from a struct tag literal.
fn unquote_tag(lit: &str) -> String {
    let bytes = lit.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'`' && last == b'`') {
            return lit[1..lit.len() - 1].to_string();
        }
    }
    lit.to_string()
}
