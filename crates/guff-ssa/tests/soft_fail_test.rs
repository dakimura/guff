//! Soft-fail paths for incomplete hybrid type info.
//!
//! When the checker leaves expressions as `Typ[Invalid]` (or SSA resolves the
//! wrong origin for a generic method), the builder must degrade to Invalid
//! placeholders / skip lowering rather than aborting the package build.

use guff::ast::{Decl, Expr, Stmt};
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::block::BasicBlock;
use guff_ssa::builder::build_function;
use guff_ssa::create::{create_function, create_package, populate_package_members};
use guff_ssa::emit::emit_extract;
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_ssa::value::Value;
use guff_ssa::ArenaId;
use guff_types::basic::{init_universe, lookup_basic, BasicKind};
use guff_types::{
    bind_tparams, new_param,
    object::type_name::new_type_name,
    signature::{new_signature_type, signature_set_type_params},
    tuple::new_tuple,
    typeparam::new_type_param,
    Checker, Config, Info, ObjectArena, OperandMode, PackageArena, TypeAndValue, TypeId,
};

fn build_all_funcs(src: &str, mutate: impl FnOnce(&mut Info, &guff::ast::File, TypeId)) {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", src.as_bytes(), Mode::NONE).expect("parse");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    let invalid = lookup_basic(&check.types, BasicKind::Invalid).expect("Invalid basic");

    let mut info = check.info;
    mutate(&mut info, &file, invalid);

    let mut prog = Program::new(
        BuilderMode::INSTANTIATE_GENERICS,
        info,
        check.types,
        check.objects,
        check.packages,
    );
    let ssa_pkg = create_package(&mut prog, check.pkg);
    populate_package_members(&mut prog, ssa_pkg, &[file.clone()]);

    let names: Vec<String> = prog
        .packages
        .get(ssa_pkg)
        .members
        .iter()
        .filter_map(|(n, m)| match m {
            MemberData::Function(_) => Some(n.clone()),
            _ => None,
        })
        .collect();
    for name in names {
        let Some(MemberData::Function(fid)) = prog.packages.get(ssa_pkg).members.get(&name).copied()
        else {
            continue;
        };
        let Some(fd) = file.decls.iter().find_map(|d| match d {
            Decl::FuncDecl(fd) if fd.name.name == name => Some(fd.clone()),
            _ => None,
        }) else {
            continue;
        };
        build_function(&mut prog, fid, &fd);
    }
    prog.drain_build_queue();
}

fn poison_range_x(info: &mut Info, file: &guff::ast::File, invalid: TypeId) {
    for decl in &file.decls {
        let Decl::FuncDecl(fd) = decl else { continue };
        let Some(body) = &fd.body else { continue };
        for stmt in &body.list {
            let Stmt::RangeStmt(rs) = stmt else { continue };
            info.types.insert(
                rs.x.id(),
                TypeAndValue {
                    mode: OperandMode::Value,
                    typ: invalid,
                    val: None,
                },
            );
        }
    }
}

fn poison_first_call(info: &mut Info, file: &guff::ast::File, invalid: TypeId) {
    fn walk_expr(e: &Expr, info: &mut Info, invalid: TypeId) -> bool {
        match e {
            Expr::CallExpr(c) => {
                info.types.insert(
                    c.id,
                    TypeAndValue {
                        mode: OperandMode::Value,
                        typ: invalid,
                        val: None,
                    },
                );
                true
            }
            Expr::ParenExpr(p) => walk_expr(&p.x, info, invalid),
            _ => false,
        }
    }
    for decl in &file.decls {
        let Decl::FuncDecl(fd) = decl else { continue };
        let Some(body) = &fd.body else { continue };
        for stmt in &body.list {
            if let Stmt::AssignStmt(a) = stmt {
                if a.rhs.iter().any(|e| walk_expr(e, info, invalid)) {
                    return;
                }
            }
        }
    }
}

#[test]
fn range_over_invalid_basic_soft_skips() {
    const SRC: &str = r#"
package p

func walk(s []int) {
	for _, v := range s {
		_ = v
	}
}
"#;
    build_all_funcs(SRC, poison_range_x);
}

#[test]
fn multi_value_assign_from_invalid_soft_extracts() {
    const SRC: &str = r#"
package p

func pair() (int, int) { return 1, 2 }

func use() {
	a, b := pair()
	_, _ = a, b
}
"#;
    build_all_funcs(SRC, poison_first_call);
}

#[test]
fn emit_extract_on_non_tuple_returns_invalid_zero() {
    let (arena, _) = init_universe();
    let mut prog = Program::new(
        BuilderMode::default(),
        Info::default(),
        arena,
        ObjectArena::new(),
        PackageArena::new(),
    );
    let fid = create_function(&mut prog, "f".to_string(), None, None);
    let entry = {
        let f = prog.functions.get_mut(fid);
        f.blocks.alloc(BasicBlock::new(0, fid))
    };
    let invalid_ty = prog.basic_type(BasicKind::Invalid);
    let placeholder = prog.emit_const(None, invalid_ty);
    let v = emit_extract(&mut prog, fid, entry, placeholder, 0);
    match v {
        Value::Const(_) => {}
        other => panic!("expected Invalid const placeholder, got {other:?}"),
    }
}

#[test]
fn create_instance_ignores_rtargs_on_non_method() {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let mut objs = ObjectArena::new();

    let t_obj = new_type_name(&mut objs, "T", None);
    let tparam = new_type_param(&mut arena, t_obj, None);
    let tlist = bind_tparams(&mut arena, vec![tparam]).expect("tparams");
    let x = new_param(&mut objs, "x", tparam);
    let params = new_tuple(&mut arena, &[x]);
    let r = new_param(&mut objs, "", tparam);
    let results = new_tuple(&mut arena, &[r]);
    let sig = new_signature_type(&mut arena, None, &[], &[], params, results, false);
    signature_set_type_params(&mut arena, sig, tlist);

    let mut prog = Program::new(
        BuilderMode::INSTANTIATE_GENERICS,
        Info::default(),
        arena,
        objs,
        PackageArena::new(),
    );
    let origin = create_function(&mut prog, "F".to_string(), None, None);
    {
        let f = prog.functions.get_mut(origin);
        f.signature = Some(sig);
        f.from_syntax = true;
    }

    // Spurious rtargs on a package-level (non-method) generic function must not panic.
    let inst = prog.create_instance(origin, &[int_ty], &[int_ty]);
    assert_ne!(inst.index(), origin.index());
    assert_eq!(prog.functions.get(inst).type_args, vec![int_ty]);

    // Empty targs + spurious rtargs → return origin.
    let same = prog.create_instance(origin, &[int_ty], &[]);
    assert_eq!(same, origin);
}
