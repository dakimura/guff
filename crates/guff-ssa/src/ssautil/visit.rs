//! SSA program traversal utilities — port of go/ssa/ssautil/visit.go.

use crate::hash::HashSet;

use guff_types::{
    is_interface, named, new_pointer, signature_type_params, SelectionKind, TypeData, TypeId,
};

use crate::ids::{FuncId, PackageId};
use crate::member::MemberData;
use crate::program::Program;
use crate::value::Value;

/// Finds and returns the set of functions potentially needed by `prog`, using a
/// simple reachability walk from package members, exported-type methods, and
/// functions referenced as operands. (Go: `ssautil.AllFunctions`.)
///
/// Precondition: packages that should contribute have been built.
pub fn all_functions(prog: &mut Program) -> HashSet<FuncId> {
    let mut seen = HashSet::default();

    let pkg_ids = prog.all_packages();
    let mut root_fns = Vec::new();
    let mut exported_types = Vec::new();

    for pkg_id in pkg_ids {
        let mut member_pairs: Vec<_> = prog.packages.get(pkg_id).members.clone().into_iter().collect();
        // FxHash (and previously SipHash) iteration order must not affect the
        // order in which we seed reachability / lazily create methods.
        member_pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
        for (_, mem) in member_pairs {
            match mem {
                MemberData::Function(fid) => root_fns.push(fid),
                MemberData::Type(t) => exported_types.push((pkg_id, t)),
                _ => {}
            }
        }
    }

    for fid in root_fns {
        visit_fn(prog, fid, &mut seen);
    }

    for (pkg_id, t) in exported_types {
        exported_type_hack(prog, pkg_id, t, &mut seen);
    }

    let runtime = prog.runtime_types();
    for t in runtime {
        methods_of(prog, t, &mut seen);
    }

    seen
}

/// Returns the subset of `pkgs` named `main` that define a `main` function.
/// (Go: `ssautil.MainPackages`.)
pub fn main_packages(prog: &Program, pkgs: &[PackageId]) -> Vec<PackageId> {
    let mut mains = Vec::new();
    for &pkg_id in pkgs {
        let pkg = prog.packages.get(pkg_id);
        if pkg.name(prog) == "main" && pkg.func("main").is_some() {
            mains.push(pkg_id);
        }
    }
    mains
}

fn visit_fn(prog: &Program, fid: FuncId, seen: &mut HashSet<FuncId>) {
    if !seen.insert(fid) {
        return;
    }
    let f = prog.functions.get(fid);
    for (_, block) in f.blocks.iter() {
        for &instr_id in &block.instrs {
            let instr = f.instrs.get(instr_id);
            instr.for_each_operand(|op| {
                if let Value::Function(callee) = op {
                    visit_fn(prog, *callee, seen);
                }
            });
        }
    }
}

fn methods_of(prog: &mut Program, t: TypeId, seen: &mut HashSet<FuncId>) {
    if is_interface(&prog.type_arena, t) {
        return;
    }
    let sels: Vec<_> = prog.method_set(t).to_vec();
    for sel in sels {
        if sel.kind() != SelectionKind::MethodVal {
            continue;
        }
        let obj = sel.obj();
        if let Some(sig) = obj.typ(&prog.object_arena) {
            if signature_type_params(&prog.type_arena, sig)
                .map(|p| !p.list().is_empty())
                .unwrap_or(false)
            {
                continue;
            }
        }
        if let Some(callee) = prog.method_value(&sel) {
            visit_fn(prog, callee, seen);
        }
    }
}

fn exported_type_hack(prog: &mut Program, pkg_id: PackageId, typ: TypeId, seen: &mut HashSet<FuncId>) {
    let pkg = prog.packages.get(pkg_id);
    if !pkg.is_syntactic() {
        return;
    }
    let name = type_export_name(prog, typ);
    if name.is_empty() || !is_exported(&name) || is_interface(&prog.type_arena, typ) {
        return;
    }
    if let TypeData::Named(n) = prog.type_arena.get(typ) {
        if n.type_params().map(|p| !p.list().is_empty()).unwrap_or(false) {
            return;
        }
        methods_of(prog, typ, seen);
        let ptr = new_pointer(&mut prog.type_arena, typ);
        methods_of(prog, ptr, seen);
    }
}

fn is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_uppercase())
}

fn type_export_name(prog: &Program, t: TypeId) -> String {
    match prog.type_arena.get(t) {
        TypeData::Named(_) => named::named_obj(&prog.type_arena, t)
            .name(&prog.object_arena)
            .to_string(),
        _ => String::new(),
    }
}
