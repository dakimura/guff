//! Tests for object source-position integration (D07, chunk 82).
//!
//! Go passes a `syntax.Pos` to every object constructor (`NewVar(pos, ..)` and
//! friends); our constructors default it to `nopos` and the checker fills it in
//! at each declaration site via [`ObjectId::set_pos`]. These tests verify that
//! package-level `const`/`var`/`type`/`func` objects carry the byte offset of
//! their declaring identifier — the information a linter needs to report a
//! diagnostic against a declared symbol.
//!
//! The `Info.Defs` map is keyed on the declaring `Ident::id()`, so for each
//! recorded def we can recover the source identifier node and check that the
//! object's stored position matches the identifier's position.

use std::collections::HashMap;

use guff::ast::File;
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff::walk::{inspect, NodeRef};

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

/// Map from `Ident::id()` to that identifier's source position (byte offset),
/// gathered by walking the file.
fn ident_positions(file: &File) -> HashMap<u32, u32> {
    let mut out = HashMap::new();
    inspect(NodeRef::File(file), |n| {
        if let Some(NodeRef::Ident(id)) = n {
            out.insert(id.id(), id.pos().0 as u32);
        }
        true
    });
    out
}

/// For every recorded def, the object's stored position must equal the position
/// of its declaring identifier.
#[test]
fn package_level_defs_carry_declaring_ident_position() {
    let src = "package p\nconst c = 1\nvar v = 2\ntype T int\nfunc f() {}\n";
    let (check, file) = check_src(src);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let id_pos = ident_positions(&file);
    let mut checked = 0;
    for (ident_id, obj) in &check.info.defs {
        let obj = match obj {
            Some(o) => *o,
            None => continue,
        };
        let name = obj.name(&check.objects);
        // Only the four top-level declarations here.
        if !["c", "v", "T", "f"].contains(&name) {
            continue;
        }
        let want = id_pos.get(ident_id).copied().expect("ident node present");
        assert_ne!(want, 0, "declaring ident of {name} has no position");
        assert_eq!(
            obj.pos(&check.objects),
            want,
            "object {name} position mismatch"
        );
        checked += 1;
    }
    assert_eq!(checked, 4, "expected to check all four package-level decls");
}

/// The offset must actually distinguish the declarations (a sanity check that
/// we're storing per-object positions, not a single shared value).
#[test]
fn distinct_decls_have_distinct_positions() {
    let src = "package p\nvar a = 1\nvar b = 2\n";
    let (check, _file) = check_src(src);
    assert!(check.errors.is_empty(), "errors: {:?}", check.errors);

    let mut positions: Vec<(String, u32)> = check
        .info
        .defs
        .values()
        .filter_map(|o| o.map(|id| (id.name(&check.objects).to_string(), id.pos(&check.objects))))
        .filter(|(n, _)| n == "a" || n == "b")
        .collect();
    positions.sort();
    assert_eq!(positions.len(), 2);
    assert_ne!(positions[0].1, positions[1].1, "a and b share a position");
    assert!(positions[0].1 > 0 && positions[1].1 > 0);
}

/// Named function parameters, results, and method receivers carry the position
/// of their declaring identifier (chunk 83).
#[test]
fn params_results_and_receiver_carry_positions() {
    let src = "package p\n\
               func f(a int, b string) (r int) { return 0 }\n\
               type T int\n\
               func (recv T) m() {}\n";
    let (check, file) = check_src(src);
    assert!(check.errors.is_empty(), "errors: {:?}", check.errors);

    let id_pos = ident_positions(&file);
    let want_names = ["a", "b", "r", "recv"];
    let mut seen = Vec::new();
    for (ident_id, obj) in &check.info.defs {
        let obj = match obj {
            Some(o) => *o,
            None => continue,
        };
        let name = obj.name(&check.objects).to_string();
        if !want_names.contains(&name.as_str()) {
            continue;
        }
        let want = id_pos.get(ident_id).copied().expect("ident node present");
        assert_ne!(want, 0);
        assert_eq!(obj.pos(&check.objects), want, "position mismatch for {name}");
        seen.push(name);
    }
    seen.sort();
    assert_eq!(seen, vec!["a", "b", "r", "recv"]);
}

/// Locals declared with `:=` and range loop variables carry the position of
/// their declaring identifier (chunk 84).
#[test]
fn short_var_and_range_locals_carry_positions() {
    let src = "package p\n\
               func f() {\n\
               \tn := 1\n\
               \ts := []int{1, 2}\n\
               \tfor i, x := range s {\n\
               \t\t_ = i\n\
               \t\t_ = x\n\
               \t}\n\
               \t_ = n\n\
               }\n";
    let (check, file) = check_src(src);
    assert!(check.errors.is_empty(), "errors: {:?}", check.errors);

    let id_pos = ident_positions(&file);
    let want_names = ["n", "s", "i", "x"];
    let mut seen = Vec::new();
    for (ident_id, obj) in &check.info.defs {
        let obj = match obj {
            Some(o) => *o,
            None => continue,
        };
        let name = obj.name(&check.objects).to_string();
        if !want_names.contains(&name.as_str()) {
            continue;
        }
        let want = id_pos.get(ident_id).copied().expect("ident node present");
        assert_ne!(want, 0);
        assert_eq!(obj.pos(&check.objects), want, "position mismatch for {name}");
        seen.push(name);
    }
    seen.sort();
    assert_eq!(seen, vec!["i", "n", "s", "x"]);
}

/// An aliased import binds a `PkgName` whose position is the alias identifier
/// (chunk 85).
#[test]
fn import_alias_pkgname_carries_position() {
    let src = "package p\nimport u \"unsafe\"\nvar _ = u.Sizeof(0)\n";
    let (check, file) = check_src(src);
    assert!(check.errors.is_empty(), "errors: {:?}", check.errors);

    let id_pos = ident_positions(&file);
    let mut found = false;
    for (ident_id, obj) in &check.info.defs {
        let obj = match obj {
            Some(o) => *o,
            None => continue,
        };
        if obj.name(&check.objects) != "u" {
            continue;
        }
        let want = id_pos.get(ident_id).copied().expect("alias ident present");
        assert_ne!(want, 0);
        assert_eq!(obj.pos(&check.objects), want, "pkg alias position mismatch");
        found = true;
    }
    assert!(found, "alias `u` should be recorded in Info.Defs");
}

/// A duplicate struct field is reported at the offending field's position — a
/// direct observation that struct fields now carry positions (chunk 85).
#[test]
fn duplicate_struct_field_error_points_at_field() {
    // Two `a` fields; the redeclaration error must point at the second one.
    let src = "package p\ntype T struct {\n\ta int\n\ta int\n}\n";
    let (check, _file) = check_src(src);

    let dup: Vec<u32> = check
        .errors
        .iter()
        .filter(|e| e.msg.contains("redeclared"))
        .map(|e| e.pos)
        .collect();
    assert_eq!(dup.len(), 1, "expected one redeclaration error: {:?}", check.errors);
    // The second `a` sits on the fourth line; its offset must be non-zero and
    // strictly greater than the struct keyword.
    assert!(dup[0] > 0, "redeclaration error has no position");
    // Locate the two `a` field identifiers by source scan and confirm the error
    // points at the *second* one.
    let a_offsets: Vec<usize> = src
        .match_indices("a int")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(a_offsets.len(), 2);
    // Pos is 1-based (FileSet base 1), so offset+1.
    assert_eq!(dup[0] as usize, a_offsets[1] + 1, "should point at second field");
}

/// Multi-name specs (`const a, b = …`, `var x, y int`) give each name its own
/// position.
#[test]
fn multi_name_specs_position_each_name() {
    let src = "package p\nconst a, b = 1, 2\nvar x, y int\n";
    let (check, file) = check_src(src);
    assert!(check.errors.is_empty(), "errors: {:?}", check.errors);

    let id_pos = ident_positions(&file);
    let mut names_seen = Vec::new();
    for (ident_id, obj) in &check.info.defs {
        let obj = match obj {
            Some(o) => *o,
            None => continue,
        };
        let name = obj.name(&check.objects).to_string();
        if !["a", "b", "x", "y"].contains(&name.as_str()) {
            continue;
        }
        let want = id_pos.get(ident_id).copied().expect("ident node present");
        assert_eq!(obj.pos(&check.objects), want, "position mismatch for {name}");
        names_seen.push(name);
    }
    names_seen.sort();
    assert_eq!(names_seen, vec!["a", "b", "x", "y"]);
}
