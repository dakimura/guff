//! SSA construction CREATE phase.

use guff::ast::{Decl, FieldList, File, FuncDecl, FuncType, Ident, Spec};
use guff::token::Token;
use guff_types::{ObjectData, ObjectId, PackageId as TypePackageId, VarKind};
use crate::global::Global;
use crate::ids::{FuncId, PackageId};
use crate::member::MemberData;
use crate::program::Program;
use crate::package::Package;
use crate::function::{Function, Parameter};
use crate::value::Value;

/// create_package creates and returns an SSA Package from the specified
/// type-checked package.
/// (Go: `(*Program).CreatePackage`)
///
/// This allocates the Package shell only. Member population (vars, funcs,
/// consts, types) is performed separately by [`populate_package_members`],
/// because the current test harnesses drive the build phase by hand. Once the
/// Package.Build orchestration is wired, the two will merge (as in go/ssa's
/// `CreatePackage`).
pub fn create_package(prog: &mut Program, pkg_id: TypePackageId) -> PackageId {
    let id = prog.packages.alloc(Package::new(pkg_id));
    prog.package_map.insert(pkg_id, id);

    // Synthesized package initializer. go/ssa gives it an empty *Signature; we
    // leave `signature` as None, which the disassembler renders identically as
    // `func init():`. Its body is built later by build_package_init.
    let init_fid = create_function(prog, "init".to_string(), None, Some(id));
    {
        let f = prog.functions.get_mut(init_fid);
        f.synthetic = Some("package initializer".to_string());
    }
    {
        let pkg = prog.packages.get_mut(id);
        pkg.init = Some(init_fid);
        pkg.members.insert("init".to_string(), MemberData::Function(init_fid));
    }

    // Initializer guard variable `init$guard` (a package-level *bool), unless
    // BareInits was requested. It has no type-checker object; only build_package_init
    // and the Members map reference it. (Go: the anonymous Global in CreatePackage.)
    if !prog.mode.contains(crate::mode::BuilderMode::BARE_INITS) {
        if let Some(bool_ty) =
            guff_types::lookup_basic(&prog.type_arena, guff_types::BasicKind::Bool)
        {
            let ptr = guff_types::pointer::new_pointer(&mut prog.type_arena, bool_ty);
            let gid = prog.globals.alloc(Global::new("init$guard".to_string(), id, ptr));
            let pkg = prog.packages.get_mut(id);
            pkg.init_guard = Some(gid);
            pkg.members.insert("init$guard".to_string(), MemberData::Global(gid));
        }
    }

    if prog.mode.contains(crate::mode::BuilderMode::GLOBAL_DEBUG) {
        prog.packages.get_mut(id).set_debug_mode(true);
    }

    id
}

/// create_params populates a function's parameters from its signature: the
/// receiver (if any) followed by the ordinary parameters. Each parameter's
/// type-checker `Var` object is mapped to its SSA [`Value::Param`] in
/// `Function.objects`, so the builder can resolve identifier uses.
/// (Go: the parameter loop of `(*builder).createParams`, which is used for
/// synthetic functions such as wrappers and generic instances — it calls
/// `addParamVar` only, never spilling.)
///
/// Requires `Function.signature` to be set. It is a no-op otherwise.
///
/// Functions built from syntax use [`create_syntactic_params`] instead, which
/// spills named parameters to stack locals (Go: `createSyntacticParams`).
pub fn create_params(prog: &mut Program, fid: FuncId) {
    let sig = match prog.functions.get(fid).signature {
        Some(s) => s,
        None => return,
    };
    // Receiver (methods).
    if let Some(recv) = guff_types::signature::signature_recv(&prog.type_arena, sig) {
        add_param_var(prog, fid, recv);
    }
    // Ordinary parameters.
    if let Some(params) = guff_types::signature::signature_params(&prog.type_arena, sig) {
        let n = guff_types::tuple::tuple_len(&prog.type_arena, Some(params));
        for i in 0..n {
            let obj = guff_types::tuple::tuple_at(&prog.type_arena, params, i);
            add_param_var(prog, fid, obj);
        }
    }
}

/// Creates parameters for a synthetic wrapper/thunk: spills `recv_obj` to a
/// stack local, then adds regular parameters starting at tuple index
/// `param_start` (0 for wrappers with a receiver, 1 for thunks whose first
/// param is the receiver). (Go: `addSpilledParam` + `createParams`.)
pub(crate) fn create_wrapper_params(
    prog: &mut Program,
    fid: FuncId,
    entry: crate::ids::BlockId,
    recv_obj: ObjectId,
    param_start: usize,
) {
    add_spilled_param(prog, fid, entry, recv_obj);
    let sig = prog.functions.get(fid).signature.expect("wrapper has signature");
    if let Some(params) = guff_types::signature::signature_params(&prog.type_arena, sig) {
        let n = guff_types::tuple::tuple_len(&prog.type_arena, Some(params));
        for i in param_start..n {
            let obj = guff_types::tuple::tuple_at(&prog.type_arena, params, i);
            add_param_var(prog, fid, obj);
        }
    }
}

/// create_syntactic_params populates the parameters of a function built from
/// syntax, spilling each *named* parameter (and named receiver) to a stack
/// local so the body can take its address and mutate it. Anonymous (unnamed or
/// blank `_`) parameters cannot be referenced, so they are added by value with
/// no spill. Emits the spill `Alloc`s and `Store`s into `entry`.
/// (Go: the parameter portion of `(*Function).createSyntacticParams`.)
///
/// Precondition: `entry` is the function's (current) entry block, already
/// created. Requires `Function.signature` to be set.
///
/// Named *result* variables also get a stack local (recorded in
/// `Function.named_results`), so a `return` can spill through them.
///
/// DEFERRED vs go/ssa: blank `_` parameters are treated as anonymous (go
/// resolves them through `identVar`, which we approximate by name).
pub fn create_syntactic_params(prog: &mut Program, fid: FuncId, entry: crate::ids::BlockId) {
    let sig = match prog.functions.get(fid).signature {
        Some(s) => s,
        None => return,
    };
    // Receiver (methods).
    if let Some(recv) = guff_types::signature::signature_recv(&prog.type_arena, sig) {
        maybe_spill_param(prog, fid, entry, recv);
    }
    // Ordinary parameters.
    if let Some(params) = guff_types::signature::signature_params(&prog.type_arena, sig) {
        let n = guff_types::tuple::tuple_len(&prog.type_arena, Some(params));
        for i in 0..n {
            let obj = guff_types::tuple::tuple_at(&prog.type_arena, params, i);
            maybe_spill_param(prog, fid, entry, obj);
        }
    }
    // Named results: allocate a local for each and record it in
    // `named_results`. Go requires a function's results to be all-named or
    // all-anonymous, so it suffices to test the first result; a named-but-blank
    // result (`func() (_ int)`) still has a non-empty name ("_") and so is
    // treated, faithfully, as a named result.
    if let Some(results) = guff_types::signature::signature_results(&prog.type_arena, sig) {
        let n = guff_types::tuple::tuple_len(&prog.type_arena, Some(results));
        let first_named = n > 0 && {
            let r0 = guff_types::tuple::tuple_at(&prog.type_arena, results, 0);
            !r0.name(&prog.object_arena).is_empty()
        };
        if first_named {
            for i in 0..n {
                let obj = guff_types::tuple::tuple_at(&prog.type_arena, results, i);
                add_result_var(prog, fid, entry, obj);
            }
        }
    }
}

/// ident_var returns the variable defined by `id`. (Go: `identVar`.)
fn ident_var(prog: &Program, id: &Ident) -> ObjectId {
    prog.info
        .defs
        .get(&id.id)
        .expect("no Defs entry for ident")
        .expect("ident Var def should have object")
}

/// create_syntactic_params_from_decl populates parameters and named results
/// using the names declared in `fd`'s syntax, binding each identifier to its
/// type-checker `Var` object via [`ident_var`]. This matches go/ssa's
/// `createSyntacticParams` and is required for generic instances, whose
/// instantiated signature carries fresh `Var` objects that differ from the
/// checker objects referenced by the origin syntax.
pub fn create_syntactic_params_from_decl(
    prog: &mut Program,
    fid: FuncId,
    entry: crate::ids::BlockId,
    fd: &FuncDecl,
) {
    create_syntactic_params_from_functype(prog, fid, entry, fd.recv.as_ref(), &fd.ty);
}

/// create_syntactic_params_from_functype is the shared implementation for
/// [`create_syntactic_params_from_decl`] and future `FuncLit` support.
fn create_syntactic_params_from_functype(
    prog: &mut Program,
    fid: FuncId,
    entry: crate::ids::BlockId,
    recv: Option<&FieldList>,
    functype: &FuncType,
) {
    let sig = prog.functions.get(fid).signature.expect("function has signature");

    if let Some(recv) = recv {
        for field in &recv.list {
            for name in &field.names {
                maybe_spill_param(prog, fid, entry, ident_var(prog, name));
            }
            if field.names.is_empty() {
                if let Some(r) = guff_types::signature::signature_recv(&prog.type_arena, sig) {
                    add_param_var(prog, fid, r);
                }
            }
        }
    }

    let param_base = prog.functions.get(fid).params.len();
    if let Some(params) = &functype.params {
        for field in &params.list {
            for name in &field.names {
                maybe_spill_param(prog, fid, entry, ident_var(prog, name));
            }
            if field.names.is_empty() {
                if let Some(ptuple) =
                    guff_types::signature::signature_params(&prog.type_arena, sig)
                {
                    let i = prog.functions.get(fid).params.len() - param_base;
                    let obj = guff_types::tuple::tuple_at(&prog.type_arena, ptuple, i);
                    add_param_var(prog, fid, obj);
                }
            }
        }
    }

    if let Some(results) = &functype.results {
        for field in &results.list {
            for name in &field.names {
                add_result_var(prog, fid, entry, ident_var(prog, name));
            }
            if field.names.is_empty() {
                if let Some(rtuple) =
                    guff_types::signature::signature_results(&prog.type_arena, sig)
                {
                    let i = prog.functions.get(fid).named_results.len();
                    let obj = guff_types::tuple::tuple_at(&prog.type_arena, rtuple, i);
                    add_result_var(prog, fid, entry, obj);
                }
            }
        }
    }
}

/// add_result_var declares a named result variable as a stack local: it
/// allocates a local for `obj` (binding `obj` in `objects` to that cell, so
/// body references resolve to it) and appends the cell to `named_results`.
/// (Go: `addNamedLocal` for a result, plus the `namedResults` append in
/// `createSyntacticParams`.)
fn add_result_var(prog: &mut Program, fid: FuncId, entry: crate::ids::BlockId, obj: ObjectId) {
    let local = crate::emit::emit_local_var(prog, fid, entry, obj);
    prog.functions.get_mut(fid).named_results.push(local);
}

/// Spill `obj` if it is a named parameter, else add it by value.
fn maybe_spill_param(prog: &mut Program, fid: FuncId, entry: crate::ids::BlockId, obj: ObjectId) {
    let name = obj.name(&prog.object_arena);
    if name.is_empty() || name == "_" {
        add_param_var(prog, fid, obj); // anonymous: no need to spill
    } else {
        add_spilled_param(prog, fid, entry, obj);
    }
}

/// add_spilled_param declares a parameter pre-spilled to a stack local: it adds
/// the `Parameter`, allocates a local for `obj` (rebinding `obj` in `objects`
/// to that cell), and stores the incoming parameter value into it. Subsequent
/// lifting eliminates the spill where the local is never addressed.
/// (Go: `(*Function).addSpilledParam`.)
pub(crate) fn add_spilled_param(prog: &mut Program, fid: FuncId, entry: crate::ids::BlockId, obj: ObjectId) {
    let param = add_param_var(prog, fid, obj);
    // emit_local_var rebinds objects[obj] from the Parameter to the spill cell.
    let spill = crate::emit::emit_local_var(prog, fid, entry, obj);
    crate::emit::emit_store(prog.functions.get_mut(fid), entry, spill, param, guff::NO_POS);
}

/// add_param_var allocates one [`Parameter`] for `obj`, records the
/// object → `Value::Param` mapping, and returns that value.
/// (Go: `(*Function).addParamVar`)
pub(crate) fn add_param_var(prog: &mut Program, fid: FuncId, obj: ObjectId) -> Value {
    let name = {
        let n = obj.name(&prog.object_arena);
        if n.is_empty() {
            format!("arg{}", prog.functions.get(fid).params.len())
        } else {
            n.to_string()
        }
    };
    let typ = obj.typ(&prog.object_arena).expect("parameter has a type");
    let f = prog.functions.get_mut(fid);
    let p_id = f.params.alloc(Parameter {
        name,
        typ,
        parent: fid,
        object: Some(obj),
    });
    let v = Value::Param(p_id);
    f.objects.insert(obj, v);
    v
}

/// create_function creates a function or method.
/// (Go: `createFunction`)
pub fn create_function(
    prog: &mut Program,
    name: String,
    parent: Option<FuncId>,
    pkg: Option<PackageId>,
) -> FuncId {
    let fn_obj = Function::new(name, parent, pkg);
    prog.functions.alloc(fn_obj)
}

/// populate_package_members allocates package-level members (vars, funcs,
/// consts and types) for each declaration in `files`, mirroring go/ssa's
/// `CreatePackage` member loop (`membersFromDecl` / `memberFromObject`).
///
/// It fills `Package.members` (name → [`MemberData`]) and `Package.objects`
/// (object → [`Value`] for consts/vars/funcs), which the source-level lookups
/// (`Program::package_level_member` etc.) and the builder's ident resolution
/// consult.
///
/// DEFERRED vs. go/ssa: the synthesized `init` function, the `init$guard`
/// variable, per-decl syntax/goversion recording (used only by the build
/// phase), and the export-data (no-syntax) path for imported packages.
pub fn populate_package_members(prog: &mut Program, pkg_id: PackageId, files: &[File]) {
    if !files.is_empty() {
        prog.packages.get_mut(pkg_id).has_syntax = true;
    }
    for file in files {
        for decl in &file.decls {
            members_from_decl(prog, pkg_id, decl);
        }
    }
}

/// members_from_decl populates `pkg` with members for each type-checker object
/// (var, func, const or type) associated with `decl`. (Go: `membersFromDecl`)
fn members_from_decl(prog: &mut Program, pkg_id: PackageId, decl: &Decl) {
    match decl {
        Decl::GenDecl(gd) => match gd.tok {
            Some(Token::CONST) | Some(Token::TYPE) | Some(Token::VAR) => {
                for spec in &gd.specs {
                    match spec {
                        Spec::ValueSpec(vs) => {
                            // const or var: one member per declared name.
                            for id in &vs.names {
                                if let Some(obj) = def_object(prog, id) {
                                    member_from_object(prog, pkg_id, obj);
                                }
                            }
                        }
                        Spec::TypeSpec(ts) => {
                            if let Some(obj) = def_object(prog, &ts.name) {
                                member_from_object(prog, pkg_id, obj);
                            }
                        }
                        Spec::ImportSpec(_) => {}
                    }
                }
            }
            _ => {}
        },
        Decl::FuncDecl(fd) => {
            if let Some(obj) = def_object(prog, &fd.name) {
                member_from_object(prog, pkg_id, obj);
            }
        }
        Decl::BadDecl(_) => {}
    }
}

/// def_object returns the type-checker object defined by identifier `id`
/// (`Info.Defs[id]`), or `None` if the checker recorded no object (e.g. the
/// blank identifier `_` in some positions). (Go: `pkg.info.Defs[id]`)
fn def_object(prog: &Program, id: &Ident) -> Option<ObjectId> {
    prog.info.defs.get(&id.id).copied().flatten()
}

/// member_from_object populates `pkg` with a member for the type-checker object
/// `obj`. (Go: `memberFromObject`)
fn member_from_object(prog: &mut Program, pkg_id: PackageId, obj: ObjectId) {
    let name = obj.name(&prog.object_arena).to_string();
    match prog.object_arena.get(obj) {
        ObjectData::TypeName(_) => {
            if name != "_" {
                if let Some(typ) = obj.typ(&prog.object_arena) {
                    let pkg = prog.packages.get_mut(pkg_id);
                    pkg.members.insert(name, MemberData::Type(typ));
                }
            }
        }

        ObjectData::Const(c) => {
            let val = c.val().clone();
            let typ = c.typ();
            let v = prog.emit_const(Some(val), typ); // Value::Const(id)
            let const_id = match v {
                Value::Const(id) => id,
                _ => unreachable!("emit_const returns Value::Const"),
            };
            let pkg = prog.packages.get_mut(pkg_id);
            pkg.objects.insert(obj, v);
            if name != "_" {
                pkg.members.insert(name, MemberData::NamedConst(const_id));
            }
        }

        ObjectData::Var(v) => {
            let elem = v.typ();
            // A Global's type is a *pointer* to the variable's type (its address).
            let ptr = guff_types::pointer::new_pointer(&mut prog.type_arena, elem);
            let mut g = Global::new(name.clone(), pkg_id, ptr);
            g.object = Some(obj);
            let gid = prog.globals.alloc(g);
            let pkg = prog.packages.get_mut(pkg_id);
            pkg.objects.insert(obj, Value::Global(gid));
            if name != "_" {
                pkg.members.insert(name, MemberData::Global(gid));
            }
        }

        ObjectData::Func(_) => {
            let sig = obj.typ(&prog.object_arena);
            // Detect methods: a signature with a receiver is not a package-level
            // member (go/ssa registers only non-method funcs in Members).
            let is_method = sig
                .map(|s| guff_types::signature::signature_recv(&prog.type_arena, s).is_some())
                .unwrap_or(false);

            // Explicit init() functions get unique member names init#N.
            let member_name = if !is_method && name == "init" {
                let pkg = prog.packages.get_mut(pkg_id);
                pkg.ninit += 1;
                format!("init#{}", pkg.ninit)
            } else {
                name.clone()
            };

            let fid = create_function(prog, member_name.clone(), None, Some(pkg_id));
            {
                let f = prog.functions.get_mut(fid);
                f.signature = sig;
                f.object = Some(obj);
            }
            let pkg = prog.packages.get_mut(pkg_id);
            pkg.objects.insert(obj, Value::Function(fid));
            if member_name != "_" && !is_method {
                pkg.members.insert(member_name, MemberData::Function(fid));
            }
        }

        // Builtins live only in package "unsafe"; nil/pkgname are never
        // package members. (Go: memberFromObject panics on these here.)
        ObjectData::Builtin(_) | ObjectData::Nil(_) | ObjectData::PkgName(_) => {}
    }
}

pub fn is_package_level_object(prog: &Program, obj: ObjectId) -> bool {
    match prog.object_arena.get(obj) {
        ObjectData::Func(_) | ObjectData::Const(_) | ObjectData::TypeName(_) => true,
        ObjectData::Var(v) => v.kind() == VarKind::Package,
        _ => false,
    }
}

/// Ensures an imported package member has an SSA value, creating it on demand.
pub fn ensure_package_member(prog: &mut Program, obj: ObjectId) -> Option<Value> {
    if !is_package_level_object(prog, obj) {
        return None;
    }
    let type_pkg = obj.pkg(&prog.object_arena)?;
    let ssa_pkg = if let Some(&ssa_pkg) = prog.package_map.get(&type_pkg) {
        ssa_pkg
    } else {
        create_package(prog, type_pkg)
    };
    if let Some(&v) = prog.packages.get(ssa_pkg).objects.get(&obj) {
        return Some(v);
    }
    match prog.object_arena.get(obj) {
        ObjectData::Func(_)
        | ObjectData::Const(_)
        | ObjectData::TypeName(_) => member_from_object(prog, ssa_pkg, obj),
        ObjectData::Var(_) => member_from_object(prog, ssa_pkg, obj),
        _ => return None,
    }
    prog.packages.get(ssa_pkg).objects.get(&obj).copied()
}

/// Type packages referenced by objects in the arena (excluding `main`).
pub fn imported_type_packages(
    prog: &Program,
    main_type_pkg: TypePackageId,
) -> Vec<TypePackageId> {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    for obj in prog.object_arena.ids() {
        if let Some(pkg) = obj.pkg(&prog.object_arena) {
            if pkg != main_type_pkg {
                seen.insert(pkg);
            }
        }
    }
    seen.into_iter().collect()
}

/// Populates SSA members for imported packages (e.g. test stubs registered via
/// `add_dependency_source`) so cross-package calls resolve during `buildir`.
pub fn populate_imported_package_members(prog: &mut Program, main_type_pkg: TypePackageId) {
    use std::collections::{HashMap, HashSet};

    // Group every arena object by its owning package in a single pass. The old
    // code rescanned the entire object arena once per imported package *and*
    // re-derived the package list per package (`imported_type_packages` itself
    // scans all objects), making this O(packages × objects). On multi-package
    // runs with a large shared type arena that dominated `buildir` (~80s on
    // Prometheus `tsdb/...`). Grouping once is O(objects); `order` keeps the
    // deterministic arena-id ordering the previous HashSet walk did not.
    let mut order: Vec<TypePackageId> = Vec::new();
    let mut by_pkg: HashMap<TypePackageId, Vec<ObjectId>> = HashMap::new();
    for obj in prog.object_arena.ids() {
        let Some(pkg) = obj.pkg(&prog.object_arena) else {
            continue;
        };
        if pkg == main_type_pkg {
            continue;
        }
        by_pkg
            .entry(pkg)
            .or_insert_with(|| {
                order.push(pkg);
                Vec::new()
            })
            .push(obj);
    }

    for imp in order {
        let Some(&ssa_pkg) = prog.package_map.get(&imp) else {
            continue;
        };
        let existing: HashSet<ObjectId> =
            prog.packages.get(ssa_pkg).objects.keys().copied().collect();
        let objects: Vec<ObjectId> = by_pkg
            .remove(&imp)
            .unwrap_or_default()
            .into_iter()
            .filter(|obj| !existing.contains(obj))
            .filter(|obj| is_package_level_object(prog, *obj))
            .collect();
        for obj in objects {
            member_from_object(prog, ssa_pkg, obj);
        }
    }
}
