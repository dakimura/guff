use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff::ast::Decl;
use guff_types::{Checker, Config};
use guff_ssa::builder::build_function;
use guff_ssa::program::Program;
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;

#[test]
fn test_golden_basic() {
    let src = "package p\nfunc f(x int) int { return x + 1 }";
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse failed");
    
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    
    let mut prog = Program::new(
        BuilderMode::default(),
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    
    let type_pkg_id = check.pkg;
    let ssa_pkg_id = guff_ssa::create::create_package(&mut prog, type_pkg_id);
    
    for decl in &file.decls {
        if let Decl::FuncDecl(fd) = decl {
            let fn_id = guff_ssa::create::create_function(&mut prog, fd.name.name.clone(), None, Some(ssa_pkg_id));

            // Full build: params + body + post-construction passes.
            build_function(&mut prog, fn_id, fd);

            let f_obj = prog.functions.get(fn_id);
            let output = disassemble_function(f_obj, &prog);
            println!("{}", output);

            // Header now renders the recorded signature (D03).
            assert!(output.contains("func f(x int) int:"), "header:\n{output}");
            assert!(output.contains("t0 = x + 1"));
            assert!(output.contains("return t0"));
        }
    }
}
