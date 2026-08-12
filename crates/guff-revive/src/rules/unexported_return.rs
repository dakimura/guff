//! `unexported-return` — exported functions should not return unexported types.

use guff::ast::FuncDecl;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;
use guff_types::arena::TypeData;
use guff_types::ObjectId;

use crate::failure::Failure;
use crate::util::{is_importable_package, receiver_type_key, type_string};

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
}

impl<'a> Checker<'a> {
    pub fn try_new(pass: &'a Pass<'a>) -> Option<Self> {
        if !is_importable_package(&pass.pkg().name) {
            return None;
        }
        Some(Self {
            pass,
            failures: Vec::new(),
        })
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::FuncDecl(f) = n else {
            return;
        };
        check_func(self.pass, f, &mut self.failures);
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let Some(mut c) = Checker::try_new(pass) else {
        return Vec::new();
    };
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}

fn check_func(pass: &Pass<'_>, f: &FuncDecl, failures: &mut Vec<Failure>) {
    let Some(results) = &f.ty.results else {
        return;
    };
    if !f.name.is_exported() {
        return;
    }
    let thing = if f.recv.is_some() {
        if let Some(recv) = f.recv.as_ref().and_then(|r| r.list.first()) {
            if let Some(ty) = &recv.ty {
                let key = receiver_type_key(ty);
                if !guff::ast::ast_is_exported(&key) {
                    return;
                }
            }
        }
        "method"
    } else {
        "func"
    };
    for ret in &results.list {
        let Some(ty_expr) = &ret.ty else {
            continue;
        };
        // Must use type info — a lowercase Ident may be a predeclared builtin
        // (`string`, `error`, `int`, …) which is not an "unexported" package type.
        let Some(typ) = crate::util::type_of(pass, ty_expr) else {
            continue;
        };
        if exported_type(pass, typ) {
            continue;
        }
        failures.push(Failure {
            rule: "unexported-return",
            pos: ty_expr.pos().0 as u32,
            message: format!(
                "exported {thing} {} returns unexported type {}, which can be annoying to use",
                f.name.name,
                type_string(pass, typ)
            ),
            ..Failure::default()
        });
        break;
    }
}

fn exported_type(pass: &Pass<'_>, typ: guff_types::TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return true;
    };
    let types = &artifacts.types;
    let objects = &artifacts.objects;
    match types.get(typ) {
        TypeData::Named(n) => return named_exported(objects, types, n.obj()),
        TypeData::Alias(a) => {
            let obj = a.obj();
            if obj.pkg(objects).is_none() {
                return true;
            }
            if obj.exported(objects) {
                return true;
            }
            let Some(rhs) = a.rhs() else {
                return true;
            };
            return matches!(types.get(rhs.underlying(types)), TypeData::Interface(_));
        }
        TypeData::Pointer(p) => return exported_type(pass, p.elem()),
        TypeData::Slice(s) => return exported_type(pass, s.elem()),
        TypeData::Array(a) => return exported_type(pass, a.elem()),
        TypeData::Map(m) => {
            return exported_type(pass, m.key()) && exported_type(pass, m.elem());
        }
        TypeData::Chan(c) => return exported_type(pass, c.elem()),
        _ => {}
    }
    let u = typ.underlying(types);
    match types.get(u) {
        TypeData::Basic(_) => true,
        TypeData::Named(n) => named_exported(objects, types, n.obj()),
        TypeData::Pointer(p) => exported_type(pass, p.elem()),
        TypeData::Slice(s) => exported_type(pass, s.elem()),
        TypeData::Array(a) => exported_type(pass, a.elem()),
        TypeData::Map(m) => exported_type(pass, m.key()) && exported_type(pass, m.elem()),
        TypeData::Chan(c) => exported_type(pass, c.elem()),
        _ => true,
    }
}

fn named_exported(
    objects: &guff_types::ObjectArena,
    types: &guff_types::TypeArena,
    obj: ObjectId,
) -> bool {
    if obj.pkg(objects).is_none() {
        return true;
    }
    if obj.exported(objects) {
        return true;
    }
    let typ = match objects.get(obj) {
        guff_types::arena::ObjectData::TypeName(tn) => tn.typ(),
        _ => return true,
    };
    let Some(typ) = typ else {
        return true;
    };
    let u = typ.underlying(types).underlying(types);
    matches!(types.get(u), TypeData::Interface(_))
}
