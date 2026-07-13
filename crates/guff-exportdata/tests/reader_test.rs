//! Integration tests for reading compiler export data.

use std::fs;
use std::path::PathBuf;

use guff::position::FileSet;
use guff_exportdata::{new_reader, read};
use guff_types::arena::{ObjectArena, PackageArena, ScopeArena, TypeArena};
use guff_types::importer::{ImportCtx, Importer};
use guff_types::init_universe_full;
use guff_types::scope::lookup as scope_lookup;

struct NoopImporter;

impl Importer for NoopImporter {
    fn import(&mut self, _ctx: &mut ImportCtx<'_>, _path: &str) -> Option<guff_types::PackageId> {
        None
    }
}

fn simple_archive() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/export/simple/simple.a");
    fs::read(&path).expect("simple.a fixture")
}

#[test]
fn read_simple_export_lists_x_t_f() {
    let archive = simple_archive();
    let data = new_reader(&archive).expect("extract export data from archive");

    let mut types = TypeArena::default();
    let mut objects = ObjectArena::default();
    let mut scopes = ScopeArena::default();
    let mut packages = PackageArena::default();
    let universe = init_universe_full();

    let mut ctx = ImportCtx {
        types: &mut types,
        objects: &mut objects,
        scopes: &mut scopes,
        packages: &mut packages,
        universe_scope: universe.universe_scope,
    };

    let fset = FileSet::new();
    let mut noop = NoopImporter;
    let pkg = read(
        &mut noop,
        &mut ctx,
        &universe,
        data,
        "example.com/simple",
        &fset,
    )
    .expect("read unified export data");

    let scope = ctx.packages.get(pkg).scope();
    for name in ["X", "T", "F"] {
        assert!(
            scope_lookup(ctx.scopes, scope, name).is_some(),
            "missing exported name {name}"
        );
    }
}
