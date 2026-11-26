//! Tests for `Info.Implicits` recording (port of the `recordImplicit` parts of
//! `recording.go`, chunk 72c).
//!
//! Currently the only recorded implicit is the case-specific variable of a type
//! switch with a binding (`switch v := x.(type)`): each `CaseClause` maps to the
//! narrowed `Var` declared in its scope. Keyed on the `CaseClause` node id.

use guff::ast::{Decl, File, Stmt};
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::{Checker, Config};

fn parse(src: &str) -> File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

fn check_src(src: &str) -> (Checker, File) {
    let file = parse(src);
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    (check, file)
}

fn func_body(file: &File) -> &guff::ast::BlockStmt {
    for d in &file.decls {
        if let Decl::FuncDecl(fd) = d {
            return fd.body.as_ref().expect("function has a body");
        }
    }
    panic!("no function declaration found");
}

/// The first top-level function declaration named `name`.
fn func_decl<'a>(file: &'a File, name: &str) -> &'a guff::ast::FuncDecl {
    for d in &file.decls {
        if let Decl::FuncDecl(fd) = d {
            if fd.name.name == name {
                return fd;
            }
        }
    }
    panic!("no function {name} found");
}

/// `switch v := x.(type)` records, per case clause, the narrowed variable in
/// `Info.implicits`, with the case's type.
#[test]
fn type_switch_binding_records_implicit_var_per_case() {
    let src = "package p\nfunc f(x interface{}) {\n\
               \tswitch v := x.(type) {\n\
               \tcase int:\n\
               \t\t_ = v\n\
               \tcase string:\n\
               \t\t_ = v\n\
               \t}\n\
               }\n";
    let (check, file) = check_src(src);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let body = func_body(&file);
    let Stmt::TypeSwitchStmt(ts) = &body.list[0] else {
        panic!("expected type switch");
    };

    let mut narrowed: Vec<String> = Vec::new();
    for c in &ts.body.list {
        let Stmt::CaseClause(cc) = c else { continue };
        let obj = *check
            .info
            .implicits
            .get(&cc.id)
            .expect("case clause has an implicit var");
        // Every implicit is the binding variable `v`.
        assert_eq!(obj.name(&check.objects), "v");
        let t = obj.typ(&check.objects).expect("implicit var has a type");
        narrowed.push(check.type_str(t));
    }
    narrowed.sort();
    assert_eq!(narrowed, vec!["int".to_string(), "string".to_string()]);
}

/// A type switch WITHOUT a binding (`switch x.(type)`) records no implicits.
#[test]
fn type_switch_without_binding_records_no_implicit() {
    let src = "package p\nfunc f(x interface{}) {\n\
               \tswitch x.(type) {\n\
               \tcase int:\n\
               \tdefault:\n\
               \t}\n\
               }\n";
    let (check, file) = check_src(src);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let body = func_body(&file);
    let Stmt::TypeSwitchStmt(ts) = &body.list[0] else {
        panic!("expected type switch");
    };
    for c in &ts.body.list {
        if let Stmt::CaseClause(cc) = c {
            assert!(
                !check.info.implicits.contains_key(&cc.id),
                "no-binding type switch case should have no implicit"
            );
        }
    }
}

/// An anonymous parameter (`func g(int)`) records an implicit `Var` on its
/// field node; a named parameter does not.
#[test]
fn anonymous_parameter_records_implicit_var() {
    let src = "package p\nfunc g(int) {}\nfunc h(x int) {}\n";
    let (check, file) = check_src(src);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    // g: anonymous parameter → implicit unnamed Var of type int.
    let g = func_decl(&file, "g");
    let g_field = &g.ty.params.as_ref().unwrap().list[0];
    let obj = *check
        .info
        .implicits
        .get(&g_field.id)
        .expect("anonymous param has an implicit var");
    assert_eq!(obj.name(&check.objects), "");
    let t = obj.typ(&check.objects).expect("implicit var has a type");
    assert_eq!(check.type_str(t), "int");

    // h: named parameter → no implicit.
    let h = func_decl(&file, "h");
    let h_field = &h.ty.params.as_ref().unwrap().list[0];
    assert!(!check.info.implicits.contains_key(&h_field.id));
}

/// An unnamed method receiver (`func (T) M()`) records an implicit recv `Var`
/// on its receiver field node.
#[test]
fn unnamed_receiver_records_implicit_var() {
    let src = "package p\ntype T int\nfunc (T) M() {}\nfunc (r T) N() {}\n";
    let (check, file) = check_src(src);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    // M: unnamed receiver → implicit recv Var of type T.
    let m = func_decl(&file, "M");
    let m_recv = &m.recv.as_ref().unwrap().list[0];
    let obj = *check
        .info
        .implicits
        .get(&m_recv.id)
        .expect("unnamed receiver has an implicit var");
    assert_eq!(obj.name(&check.objects), "");
    let t = obj.typ(&check.objects).expect("implicit recv has a type");
    assert_eq!(check.type_str(t), "T");

    // N: named receiver → no implicit.
    let n = func_decl(&file, "N");
    let n_recv = &n.recv.as_ref().unwrap().list[0];
    assert!(!check.info.implicits.contains_key(&n_recv.id));
}

/// The first `ImportSpec` in `file`.
fn import_spec(file: &File) -> &guff::ast::ImportSpec {
    for d in &file.decls {
        if let Decl::GenDecl(gd) = d {
            for sp in &gd.specs {
                if let guff::ast::Spec::ImportSpec(is) = sp {
                    return is;
                }
            }
        }
    }
    panic!("no import spec found");
}

/// A name-less `import "unsafe"` records the synthesised `PkgName` as the
/// import spec's implicit.
#[test]
fn nameless_unsafe_import_records_implicit_pkgname() {
    let src = "package p\nimport \"unsafe\"\nvar _ = unsafe.Sizeof(0)\n";
    let (check, file) = check_src(src);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let is = import_spec(&file);
    assert!(is.name.is_none(), "this import has no explicit alias");
    let obj = *check
        .info
        .implicits
        .get(&is.id)
        .expect("name-less import records an implicit PkgName");
    assert_eq!(obj.name(&check.objects), "unsafe");
}

/// An aliased `import u "unsafe"` records the name via `Info.Defs` on the alias
/// identifier, not as an implicit.
#[test]
fn aliased_unsafe_import_records_def_not_implicit() {
    let src = "package p\nimport u \"unsafe\"\nvar _ = u.Sizeof(0)\n";
    let (check, file) = check_src(src);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let is = import_spec(&file);
    let alias = is.name.as_ref().expect("explicit alias");
    // Not an implicit …
    assert!(!check.info.implicits.contains_key(&is.id));
    // … but recorded as a Def on the alias ident.
    let obj = check
        .info
        .defs
        .get(&alias.id())
        .copied()
        .flatten()
        .expect("alias ident recorded in Info.defs");
    assert_eq!(obj.name(&check.objects), "u");
}
