//! Chunk-7 tests: Scope, Package, objset, ObjectMeta (parent/pkg/order/id).

use guff_types::{
    init_universe_full, is_exported, lookup_chain, new_package, new_scope, new_var, object_id,
    scope_insert, scope_lookup, BasicKind, BuiltinId, ObjSet, ObjectArena, PackageArena,
    ScopeArena,
};

#[test]
fn is_exported_basics() {
    assert!(is_exported("Foo"));
    assert!(is_exported("X"));
    assert!(!is_exported("foo"));
    assert!(!is_exported("_"));
    assert!(!is_exported(""));
    // Non-ASCII uppercase counts as exported.
    assert!(is_exported("Αlpha"));
}

#[test]
fn scope_insert_lookup_and_parent_backfill() {
    let mut o_arena = ObjectArena::new();
    let mut s_arena = ScopeArena::new();
    let _p_arena = PackageArena::new();

    let pkg_scope = new_scope(&mut s_arena, None, None, 0, 0, "pkg");

    let (_, table) = guff_types::init_universe();
    // Build a stand-alone Var (no real arena cross-wire; we just need
    // *an* ObjectId to insert).
    // Use the chunk-1 init_universe TypeArena only for the basic types,
    // but the Var lives in our local ObjectArena.
    let int = table[BasicKind::Int as usize];
    let v = new_var(&mut o_arena, "x", int);

    let alt = scope_insert(&mut s_arena, &mut o_arena, pkg_scope, v);
    assert!(alt.is_none(), "first insert succeeds");
    assert_eq!(scope_lookup(&s_arena, pkg_scope, "x"), Some(v));

    // Inserting a different object with the same name returns the original.
    let v2 = new_var(&mut o_arena, "x", int);
    let alt = scope_insert(&mut s_arena, &mut o_arena, pkg_scope, v2);
    assert_eq!(alt, Some(v));

    // The first inserted Var got its parent set to pkg_scope.
    assert_eq!(v.parent(&o_arena), Some(pkg_scope));
    // The rejected v2 didn't get a parent.
    assert_eq!(v2.parent(&o_arena), None);
}

#[test]
fn lookup_chain_walks_to_parent() {
    let mut o_arena = ObjectArena::new();
    let mut s_arena = ScopeArena::new();

    let parent = new_scope(&mut s_arena, None, None, 0, 0, "parent");
    let child = new_scope(&mut s_arena, Some(parent), None, 0, 0, "child");

    let (_, table) = guff_types::init_universe();
    let int = table[BasicKind::Int as usize];

    let outer = new_var(&mut o_arena, "x", int);
    scope_insert(&mut s_arena, &mut o_arena, parent, outer);

    // child has no "x" locally — chain walks to parent.
    assert_eq!(scope_lookup(&s_arena, child, "x"), None);
    assert_eq!(lookup_chain(&s_arena, child, "x"), Some(outer));
}

#[test]
fn package_carries_own_scope_under_universe() {
    let mut o_arena = ObjectArena::new();
    let mut s_arena = ScopeArena::new();
    let mut p_arena = PackageArena::new();

    let universe = new_scope(&mut s_arena, None, None, 0, 0, "universe");
    let pkg = new_package(
        &mut p_arena,
        &mut s_arena,
        universe,
        "example.com/foo",
        "foo",
    );

    let p = p_arena.get(pkg);
    assert_eq!(p.path(), "example.com/foo");
    assert_eq!(p.name(), "foo");
    assert!(!p.complete());

    let scope = p.scope();
    assert_eq!(s_arena.get(scope).parent(), Some(universe));

    // Insert a Var into the package scope and look it up via chain
    // walking — falls through to universe if not present.
    let (_, table) = guff_types::init_universe();
    let int = table[BasicKind::Int as usize];
    let x = new_var(&mut o_arena, "x", int);
    scope_insert(&mut s_arena, &mut o_arena, scope, x);
    assert_eq!(lookup_chain(&s_arena, scope, "x"), Some(x));
    assert!(lookup_chain(&s_arena, scope, "not_there").is_none());

    // Mark the package complete.
    p_arena.get_mut(pkg).mark_complete();
    assert!(p_arena.get(pkg).complete());
}

#[test]
fn object_id_dispatch_for_pkg_qualified_names() {
    let mut o_arena = ObjectArena::new();
    let mut s_arena = ScopeArena::new();
    let mut p_arena = PackageArena::new();

    let universe = new_scope(&mut s_arena, None, None, 0, 0, "universe");
    let pkg = new_package(
        &mut p_arena,
        &mut s_arena,
        universe,
        "github.com/foo/bar",
        "bar",
    );

    let (_, table) = guff_types::init_universe();
    let int = table[BasicKind::Int as usize];

    let exported = new_var(&mut o_arena, "X", int);
    let unexported = new_var(&mut o_arena, "x", int);
    let lonely = new_var(&mut o_arena, "y", int); // no pkg

    exported.set_pkg(&mut o_arena, pkg);
    unexported.set_pkg(&mut o_arena, pkg);

    assert_eq!(exported.id(&o_arena, &p_arena), "X");
    assert_eq!(unexported.id(&o_arena, &p_arena), "github.com/foo/bar.x");
    // No pkg → "_.name".
    assert_eq!(lonely.id(&o_arena, &p_arena), "_.y");

    // Free-function `object_id` matches the method.
    let n = lonely.name(&o_arena).to_string();
    assert_eq!(object_id(&p_arena, None, &n), "_.y");
}

#[test]
fn objset_dedupes_by_id() {
    let mut o_arena = ObjectArena::new();
    let mut p_arena = PackageArena::new();
    let mut s_arena = ScopeArena::new();

    let universe = new_scope(&mut s_arena, None, None, 0, 0, "universe");
    let pkg_a = new_package(&mut p_arena, &mut s_arena, universe, "pkg/a", "a");
    let pkg_b = new_package(&mut p_arena, &mut s_arena, universe, "pkg/b", "b");

    let (_, table) = guff_types::init_universe();
    let int = table[BasicKind::Int as usize];

    // Two unexported `x` in different packages → different ids.
    let xa = new_var(&mut o_arena, "x", int);
    xa.set_pkg(&mut o_arena, pkg_a);
    let xb = new_var(&mut o_arena, "x", int);
    xb.set_pkg(&mut o_arena, pkg_b);

    let mut set = ObjSet::new();
    assert!(set.insert(&o_arena, &p_arena, xa).is_none());
    // Different package → distinct id → inserts.
    assert!(set.insert(&o_arena, &p_arena, xb).is_none());
    assert_eq!(set.len(), 2);

    // Inserting xa again returns the original.
    let alt = set.insert(&o_arena, &p_arena, xa);
    assert_eq!(alt, Some(xa));

    // Two exported `X` (no package qualifier in id) collide.
    let big_xa = new_var(&mut o_arena, "X", int);
    big_xa.set_pkg(&mut o_arena, pkg_a);
    let big_xb = new_var(&mut o_arena, "X", int);
    big_xb.set_pkg(&mut o_arena, pkg_b);

    let mut set2 = ObjSet::new();
    assert!(set2.insert(&o_arena, &p_arena, big_xa).is_none());
    assert_eq!(set2.insert(&o_arena, &p_arena, big_xb), Some(big_xa));
}

#[test]
fn order_must_be_positive() {
    let mut o_arena = ObjectArena::new();
    let (_, table) = guff_types::init_universe();
    let int = table[BasicKind::Int as usize];
    let v = new_var(&mut o_arena, "x", int);

    assert_eq!(v.order(&o_arena), 0);
    v.set_order(&mut o_arena, 7);
    assert_eq!(v.order(&o_arena), 7);
}

#[test]
fn universe_routes_exported_names_to_unsafe_package() {
    let u = init_universe_full();

    // Exported builtins / TypeNames go into unsafe.
    assert!(u.lookup_unsafe("Add").is_some(), "Add → unsafe");
    assert!(u.lookup_unsafe("Pointer").is_some(), "Pointer → unsafe");
    assert!(u.lookup_unsafe("Sizeof").is_some(), "Sizeof → unsafe");

    // Same names absent from universe scope (strict Go-style).
    assert!(u.lookup_universe("Add").is_none());
    assert!(u.lookup_universe("Pointer").is_none());

    // Non-exported names go into universe.
    assert!(u.lookup_universe("int").is_some());
    assert!(u.lookup_universe("len").is_some());
    assert!(u.lookup_universe("true").is_some());
    assert!(u.lookup_universe("nil").is_some());

    // The convenience `lookup` finds both.
    assert!(u.lookup("Add").is_some());
    assert!(u.lookup("int").is_some());

    // Exported objects have their pkg set to unsafe.
    let add = u.lookup_unsafe("Add").unwrap();
    assert_eq!(add.pkg(&u.object_arena), Some(u.unsafe_pkg));
    // Builtin id matches.
    if let guff_types::ObjectData::Builtin(b) = u.object_arena.get(add) {
        assert_eq!(b.id(), BuiltinId::Add);
    } else {
        panic!("Add should be a Builtin");
    }

    // Universe-bound objects have no pkg.
    let int_obj = u.lookup_universe("int").unwrap();
    assert_eq!(int_obj.pkg(&u.object_arena), None);
}

#[test]
fn universe_scope_has_parent_set_on_inserted_objects() {
    let u = init_universe_full();
    let int_obj = u.lookup_universe("int").unwrap();
    assert_eq!(int_obj.parent(&u.object_arena), Some(u.universe_scope));

    let add = u.lookup_unsafe("Add").unwrap();
    let unsafe_scope = u.package_arena.get(u.unsafe_pkg).scope();
    assert_eq!(add.parent(&u.object_arena), Some(unsafe_scope));
}

#[test]
fn unsafe_package_scope_parent_is_universe() {
    let u = init_universe_full();
    let unsafe_scope = u.package_arena.get(u.unsafe_pkg).scope();
    assert_eq!(
        u.scope_arena.get(unsafe_scope).parent(),
        Some(u.universe_scope)
    );
}
