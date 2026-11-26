use guff::position::Pos;
use guff_packages::{go_available, load, Config, LoadMode};

#[test]
fn errors_package_typechecks() {
    if !go_available() {
        return;
    }
    if !go_available() {
        return;
    }
    let cfg = Config {
        mode: LoadMode::LOAD_SYNTAX,
        ..Default::default()
    };
    let pkgs = load(&cfg, &["errors".into()]).expect("load");
    let pkg = pkgs.first().expect("errors");
    let fset = pkg.fset.as_ref().expect("fset");
    for e in &pkg.errors {
        let pos: u32 = e.pos.parse().unwrap_or(0);
        let p = fset.position(Pos(pos as i64));
        eprintln!("{}:{}:{}: {}", p.filename, p.line, p.column, e.msg);
    }
    assert!(!pkg.ill_typed, "typecheck failed");
}
