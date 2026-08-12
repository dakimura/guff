//! Shared helpers for govet analyzers.

use std::collections::HashSet;

use guff::ast::{CallExpr, Expr, Field, FieldList, Ident, SelectorExpr};
use guff::token::Token;
use guff_analysis::code;
use guff_analysis::Pass;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::BasicKind;
use guff_types::named::named_obj;
use guff_types::selection::SelectionKind;
use guff_types::signature::{signature_params, signature_recv, signature_results};
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::TypeId;

use crate::expreq::unparen;

pub fn imports_package(pass: &Pass<'_>, path: &str) -> bool {
    pass.pkg().imports.contains_key(path) || package_imports_path(pass, path)
}

fn package_imports_path(pass: &Pass<'_>, path: &str) -> bool {
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::GenDecl(gd) = decl else {
                continue;
            };
            if gd.tok != Some(Token::IMPORT) {
                continue;
            }
            for spec in &gd.specs {
                let guff::ast::Spec::ImportSpec(is) = spec else {
                    continue;
                };
                if is.path.value.trim_matches('"') == path {
                    return true;
                }
            }
        }
    }
    false
}

pub fn is_type_named(pass: &Pass<'_>, typ: TypeId, pkg_path: &str, name: &str) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    // Named-ness lives on the type itself (after unalias). `underlying()` is the
    // struct/interface/etc. and is never `TypeData::Named` for a completed type —
    // checking it made every `is_type_named` call fail (e.g. slog.Attr FPs).
    let typ = guff_types::alias::unalias_readonly(&artifacts.types, typ);
    let TypeData::Named(_) = artifacts.types.get(typ) else {
        return false;
    };
    let obj = named_obj(&artifacts.types, typ);
    if obj.name(&artifacts.objects) != name {
        return false;
    }
    match obj.pkg(&artifacts.objects) {
        Some(pkg) => artifacts.packages.get(pkg).path() == pkg_path,
        None => false,
    }
}

pub fn receiver_named_type(
    pass: &Pass<'_>,
    typ: TypeId,
) -> Option<(TypeId, guff_types::ObjectId)> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let u = guff_types::alias::unalias_readonly(&artifacts.types, typ);
    let elem = match artifacts.types.get(u) {
        TypeData::Pointer(p) => {
            guff_types::alias::unalias_readonly(&artifacts.types, p.elem())
        }
        TypeData::Named(_) => u,
        _ => return None,
    };
    let TypeData::Named(_) = artifacts.types.get(elem) else {
        return None;
    };
    Some((elem, named_obj(&artifacts.types, elem)))
}

pub fn is_method_named(
    pass: &Pass<'_>,
    call: &CallExpr,
    pkg_path: &str,
    recv_type: &str,
    method: &str,
) -> bool {
    let Some(obj) = static_callee(pass, call) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let ObjectData::Func(_) = artifacts.objects.get(obj) else {
        return false;
    };
    if !method.is_empty() && obj.name(&artifacts.objects) != method {
        return false;
    }
    let Some(sig) = obj.typ(&artifacts.objects) else {
        return false;
    };
    let Some(recv) = signature_recv(&artifacts.types, sig) else {
        return false;
    };
    let Some(recv_typ) = recv.typ(&artifacts.objects) else {
        return false;
    };
    is_type_named(pass, recv_typ, pkg_path, recv_type)
}

pub fn is_function_named(pass: &Pass<'_>, call: &CallExpr, pkg_path: &str, name: &str) -> bool {
    let Some(obj) = static_callee(pass, call) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let ObjectData::Func(_) = artifacts.objects.get(obj) else {
        return false;
    };
    if obj.name(&artifacts.objects) != name {
        return false;
    }
    code::object_pkg_path(pass, obj).as_deref() == Some(pkg_path)
}

pub fn static_callee(pass: &Pass<'_>, call: &CallExpr) -> Option<guff_types::ObjectId> {
    code::call_target_object(pass, &call.fun)
}

pub fn is_builtin_named(pass: &Pass<'_>, expr: &Expr, name: &str) -> bool {
    let Expr::Ident(id) = unparen(expr) else {
        return false;
    };
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(obj) = info.uses.get(&id.id).copied() else {
        return false;
    };
    match artifacts.objects.get(obj) {
        ObjectData::Builtin(b) => b.name() == name,
        _ => false,
    }
}

/// Render an expression the way upstream diagnostics do.
///
/// Every `x/tools` pass that names an expression in its message builds the
/// string with `analysisutil.Format`, i.e. `go/printer` over the real fileset.
/// Matching a couple of node kinds and using a fixed word for the rest reads as
/// harmless and is not: `shift` used the literal `"x"`, so **every** shift whose
/// operand was not a bare identifier reported the letter x — `s.f << 10` and
/// `a[0] << 10` and `(i) << 10` all rendered identically, and none of them
/// matched upstream. `printf` had already hit this and fixed it locally
/// (`describe_arg`); this is that fix, shared.
pub fn format_expr(pass: &Pass<'_>, expr: &Expr) -> String {
    let mut buf: Vec<u8> = Vec::new();
    if guff::printer::fprint(&mut buf, pass.fset(), guff::printer::PrintNode::Expr(expr)).is_ok() {
        if let Ok(text) = String::from_utf8(buf) {
            return text;
        }
    }
    // Only reached if the printer or UTF-8 conversion fails.
    match expr {
        Expr::BasicLit(lit) => lit.value.clone(),
        Expr::Ident(id) => id.name.clone(),
        _ => String::new(),
    }
}

/// Port of `typesinternal.NoEffects`: can this expression be evaluated without
/// observable side effects?
///
/// Upstream `assign` consults it before calling `x = x` a self-assignment, so
/// `a[f()] = a[f()]` is left alone — the two calls are two separate calls, and
/// deleting the statement would delete them. guff had no equivalent and
/// reported it.
pub fn no_effects(pass: &Pass<'_>, expr: &Expr) -> bool {
    use guff::walk::{inspect, NodeRef};

    let mut ok = true;
    inspect(guff::walk::expr_ref(expr), |n| {
        let Some(n) = n else {
            return true; // post-order visit
        };
        match n {
            NodeRef::Ident(_)
            | NodeRef::BasicLit(_)
            | NodeRef::BinaryExpr(_)
            | NodeRef::ParenExpr(_)
            | NodeRef::SelectorExpr(_)
            | NodeRef::IndexExpr(_)
            | NodeRef::IndexListExpr(_)
            | NodeRef::SliceExpr(_)
            | NodeRef::TypeAssertExpr(_)
            | NodeRef::StarExpr(_)
            | NodeRef::CompositeLit(_)
            | NodeRef::KeyValueExpr(_)
            | NodeRef::FieldList(_)
            | NodeRef::Field(_)
            | NodeRef::Ellipsis(_) => {}

            // Type syntax: no effects, and no need to descend.
            NodeRef::ArrayType(_)
            | NodeRef::StructType(_)
            | NodeRef::ChanType(_)
            | NodeRef::FuncType(_)
            | NodeRef::MapType(_)
            | NodeRef::InterfaceType(_) => return false,

            // A receive `<-ch` has an effect; the other unary operators do not.
            NodeRef::UnaryExpr(u) => {
                if u.op == Token::ARROW {
                    ok = false;
                }
            }

            // A conversion `T(x)` has no effects; a call generally does, except
            // for the pure builtins.
            NodeRef::CallExpr(call) => {
                let is_conversion = pass
                    .types_info()
                    .and_then(|i| i.types.get(&call.fun.id()))
                    .map(|tv| tv.mode == guff_types::operand::OperandMode::TypeExpr)
                    .unwrap_or(false);
                if !is_conversion && !calls_pure_builtin(pass, call) {
                    ok = false;
                }
            }

            // A func literal is a value, not a call — but do not descend into
            // the effects in its body.
            NodeRef::FuncLit(_) => return false,

            _ => ok = false,
        }
        ok
    });
    ok
}

/// Port of `typesinternal.CallsPureBuiltin`. The excluded names matter as much
/// as the included ones: `append` and `copy` mutate, `panic` diverges.
fn calls_pure_builtin(pass: &Pass<'_>, call: &CallExpr) -> bool {
    const PURE: [&str; 9] = [
        "len", "cap", "complex", "imag", "real", "make", "new", "max", "min",
    ];
    PURE.iter().any(|n| is_builtin_named(pass, &call.fun, n))
}

pub fn expr_type(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    info.types.get(&expr.id()).map(|tv| tv.typ)
}

pub fn expr_string_const(pass: &Pass<'_>, expr: &Expr) -> Option<String> {
    code::expr_to_string(pass, expr)
}

pub fn has_basic_kind(pass: &Pass<'_>, expr: &Expr, kind: BasicKind) -> bool {
    let Some(typ) = expr_type(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    matches!(
        artifacts.types.get(typ.underlying(&artifacts.types)),
        TypeData::Basic(b) if b.kind() == kind
    )
}

pub fn is_unsafe_pointer_type(pass: &Pass<'_>, expr: &Expr) -> bool {
    has_basic_kind(pass, expr, BasicKind::UnsafePointer)
}

pub fn is_uintptr_type(pass: &Pass<'_>, expr: &Expr) -> bool {
    has_basic_kind(pass, expr, BasicKind::Uintptr)
}

pub fn root_ident<'a>(expr: &'a Expr) -> Option<&'a Ident> {
    match unparen(expr) {
        Expr::SelectorExpr(sel) => root_ident(&sel.x),
        Expr::Ident(id) => Some(id),
        _ => None,
    }
}

pub fn is_c_call(fun: &Expr) -> Option<&str> {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(fun) else {
        return None;
    };
    match x.as_ref() {
        Expr::Ident(id) if id.name == "C" => Some(&sel.name),
        _ => None,
    }
}

pub fn cgo_base_type(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let expr = unparen(expr);
    match expr {
        Expr::CallExpr(call) if call.args.len() == 1 && is_unsafe_pointer_type(pass, &call.fun) => {
            cgo_base_type(pass, &call.args[0])
        }
        Expr::StarExpr(star) => {
            let Expr::CallExpr(call) = unparen(&star.x) else {
                return expr_type(pass, expr);
            };
            if call.args.len() != 1 {
                return expr_type(pass, expr);
            }
            let Some(fun_typ) = expr_type(pass, &call.fun) else {
                return expr_type(pass, expr);
            };
            let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                return expr_type(pass, expr);
            };
            let u = fun_typ.underlying(&artifacts.types);
            let TypeData::Pointer(p) = artifacts.types.get(u) else {
                return expr_type(pass, expr);
            };
            let elem = p.elem().underlying(&artifacts.types);
            if !matches!(artifacts.types.get(elem), TypeData::Basic(b) if b.kind() == BasicKind::UnsafePointer) {
                return expr_type(pass, expr);
            }
            let Expr::CallExpr(inner) = unparen(&call.args[0]) else {
                return expr_type(pass, expr);
            };
            if inner.args.len() != 1 || !is_unsafe_pointer_type(pass, &inner.fun) {
                return expr_type(pass, expr);
            }
            let Expr::UnaryExpr(u) = unparen(&inner.args[0]) else {
                return expr_type(pass, expr);
            };
            if u.op != Token::AND {
                return expr_type(pass, expr);
            }
            cgo_base_type(pass, &u.x)
        }
        _ => expr_type(pass, expr),
    }
}

pub fn type_ok_for_cgo_call(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return true;
    };
    fn walk(
        types: &guff_types::arena::TypeArena,
        objects: &guff_types::arena::ObjectArena,
        typ: TypeId,
        seen: &mut HashSet<TypeId>,
    ) -> bool {
        if !seen.insert(typ) {
            return true;
        }
        let u = typ.underlying(types);
        match types.get(u) {
            TypeData::Chan(_) | TypeData::Map(_) | TypeData::Slice(_) => false,
            TypeData::Signature(_) => false,
            TypeData::Pointer(p) => walk(types, objects, p.elem(), seen),
            TypeData::Array(a) => walk(types, objects, a.elem(), seen),
            TypeData::Struct(s) => (0..s.num_fields()).all(|i| {
                s.field(i)
                    .typ(objects)
                    .is_none_or(|t| walk(types, objects, t, seen))
            }),
            _ => true,
        }
    }
    walk(
        &artifacts.types,
        &artifacts.objects,
        typ,
        &mut HashSet::new(),
    )
}

pub fn selection_kind(pass: &Pass<'_>, sel: &SelectorExpr) -> Option<SelectionKind> {
    let info = pass.types_info()?;
    info.selections.get(&sel.id).map(|s| s.kind())
}

pub fn method_expr_call(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Expr::SelectorExpr(sel) = unparen(&call.fun) else {
        return false;
    };
    selection_kind(pass, sel) == Some(SelectionKind::MethodExpr)
}

pub fn tuple_type_at(pass: &Pass<'_>, tuple: Option<TypeId>, i: usize) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let tuple = tuple?;
    let param = tuple_at(&artifacts.types, tuple, i);
    param.typ(&artifacts.objects)
}

pub fn tuple_len_of(pass: &Pass<'_>, tuple: Option<TypeId>) -> usize {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return 0;
    };
    tuple_len(&artifacts.types, tuple)
}

pub fn is_testing_type(pass: &Pass<'_>, typ: TypeId, name: &str) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let u = typ.underlying(&artifacts.types);
    let TypeData::Pointer(p) = artifacts.types.get(u) else {
        return false;
    };
    is_type_named(pass, p.elem(), "testing", name)
}

pub fn field_idents(field: &Field) -> &[Ident] {
    &field.names
}

pub fn file_go_version_before(pass: &Pass<'_>, threshold: &str) -> bool {
    // Empty `File.go_version` means "use the module go line", not "pre-1.22".
    let module = code::module_go_version(pass);
    for file in pass.files() {
        let v = file.go_version.trim();
        let fv = if v.is_empty() {
            module.clone()
        } else if v.starts_with("go") {
            v.to_string()
        } else {
            format!("go{v}")
        };
        if code::version_compare(&fv, threshold) < 0 {
            return true;
        }
    }
    false
}

pub fn is_in_test_file(pass: &Pass<'_>, pos: u32) -> bool {
    code::is_in_test_at(pass, pos)
}

pub fn is_main_package(pass: &Pass<'_>) -> bool {
    code::is_main(pass)
}

pub fn ident_def(pass: &Pass<'_>, id: &Ident) -> Option<guff_types::ObjectId> {
    let info = pass.types_info()?;
    info.defs.get(&id.id).and_then(|o| *o)
}

pub fn func_result_tuple(pass: &Pass<'_>, obj: guff_types::ObjectId) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let sig = obj.typ(&artifacts.objects)?;
    signature_results(&artifacts.types, sig)
}

pub fn func_param_tuple(pass: &Pass<'_>, obj: guff_types::ObjectId) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let sig = obj.typ(&artifacts.objects)?;
    signature_params(&artifacts.types, sig)
}

pub fn is_empty_interface(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let u = typ.underlying(&artifacts.types);
    if let TypeData::Interface(i) = artifacts.types.get(u) {
        i.num_explicit_methods() == 0 && i.num_embeddeds() == 0
    } else {
        false
    }
}

pub fn is_interface_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    matches!(
        artifacts.types.get(typ.underlying(&artifacts.types)),
        TypeData::Interface(_)
    )
}

pub fn object_pkg_path(pass: &Pass<'_>, obj: guff_types::ObjectId) -> Option<String> {
    code::object_pkg_path(pass, obj)
}

pub fn first_field_type(ft: &FieldList) -> Option<&Expr> {
    ft.list.first().and_then(|f| f.ty.as_ref())
}
