//! Method-set utilities and on-demand method resolution.
//!
//! Port of go/ssa's `methods.go`: [`Program::method_value`],
//! [`Program::object_method`], [`Program::lookup_method`], and the concrete
//! method-set cache used to avoid duplicate wrapper synthesis.

use std::collections::{HashMap, HashSet};

use guff_types::{
    alias::unalias_readonly, is_interface, is_pointer, lookup_field_or_method,
    named_method, named_num_methods, named_underlying, selection_type, struct_field,
    struct_num_fields, LookupResult, ObjectData, ObjectId, PackageId as TypePackageId,
    Selection, SelectionKind, TypeArena, TypeData, TypeId, ObjectArena,
};

use crate::create::create_function;
use crate::ids::FuncId;
use crate::program::Program;
use crate::value::Value;
use crate::wrappers::{create_wrapper, WrapperSelection};

/// Cache of type-checker-style method sets (`types.MethodSet` analog).
/// (Go: `typeutil.MethodSetCache`, stored on `Program.MethodSets`.)
#[derive(Default)]
pub struct MethodSetCache {
  entries: HashMap<TypeId, Vec<Selection>>,
}

/// Concrete SSA method set for a non-interface, non-parameterized type: maps each
/// method object to its SSA `Function` (which may be a wrapper).
/// (Go: `methodSet`.)
#[derive(Default)]
pub struct ConcreteMethodSet {
  pub mapping: HashMap<ObjectId, FuncId>,
}

impl Program {
  /// Returns the method set of concrete type `t` (cached). The selections use
  /// [`SelectionKind::MethodVal`] and describe methods callable as `x.m` on a
  /// value of type `t`. Interface and type-parameter types yield an empty set.
  /// (Go: `MethodSetCache.MethodSet`.)
  pub fn method_set(&mut self, t: TypeId) -> &[Selection] {
    if !self.method_set_cache.entries.contains_key(&t) {
      let sels = compute_method_set(self, t);
      self.method_set_cache.entries.insert(t, sels);
    }
    self.method_set_cache.entries.get(&t).unwrap()
  }

  /// Returns the SSA `Function` implementing method selection `sel`, building
  /// wrapper methods on demand. Returns `None` if `sel` denotes an interface or
  /// generic method. (Go: `(*Program).MethodValue`.)
  ///
  /// # Panics
  /// Panics if `sel.kind() != SelectionKind::MethodVal`.
  pub fn method_value(&mut self, sel: &Selection) -> Option<FuncId> {
    assert_eq!(
      sel.kind(),
      SelectionKind::MethodVal,
      "MethodValue requires MethodVal selection"
    );

    let t = sel.recv();
    if is_interface(&self.type_arena, t) {
      return None;
    }

    let sel_typ = selection_type(&mut self.type_arena, &mut self.object_arena, sel);
    if self.is_parameterized(&[t, sel_typ]) {
      return None;
    }

    let obj = sel.obj();
    let mset = self.concrete_method_set_for(t);
    if let Some(&fid) = self.concrete_method_sets.get(&mset).unwrap().mapping.get(&obj) {
      return Some(fid);
    }

    let rt = recv_type(self, obj)?;
    let needs_promotion = sel.index().len() > 1;
    let needs_indirection = !is_pointer(&self.type_arena, rt) && is_pointer(&self.type_arena, t);

    let fid = if needs_promotion || needs_indirection {
      let ws = WrapperSelection::from_selection(self, sel);
      let fid = create_wrapper(self, &ws, &[]);
      self.enqueue_build(fid);
      fid
    } else {
      self.object_method(obj, &[])
    };

    self.concrete_method_sets
      .get_mut(&mset)
      .unwrap()
      .mapping
      .insert(obj, fid);
    Some(fid)
  }

  /// Returns the SSA `Function` for method symbol `obj`. The object must be a
  /// method (`signature` has a receiver). If the method belongs to a created
  /// package it is returned from `objects`; otherwise a synthetic external
  /// function is created on demand. (Go: `(*Program).objectMethod`.)
  ///
  /// Generic instantiation (`targs` non-empty, or the receiver is an
  /// instantiated named type) delegates to the origin method's SSA instance.
  pub fn object_method(&mut self, obj: ObjectId, targs: &[TypeId]) -> FuncId {
    let sig = obj
      .typ(&self.object_arena)
      .expect("method object must have a type");
    assert!(
      guff_types::signature::signature_recv(&self.type_arena, sig).is_some(),
      "object_method: not a method: {:?}",
      obj.name(&self.object_arena)
    );

    let rtargs = receiver_type_args(self, obj);
    if !targs.is_empty() || !rtargs.is_empty() {
      let origin_fid = self
        .func_value(obj)
        .or_else(|| {
          // Method object may already be an instantiated signature on a generic
          // receiver; fall back to the package member with the same name.
          let name = obj.name(&self.object_arena);
          let pkg = obj.pkg(&self.object_arena)?;
          let ssa_pkg = *self.package_map.get(&pkg)?;
          self.packages.get(ssa_pkg).objects.iter().find_map(|(o, v)| {
            if o.name(&self.object_arena) == name {
              if let Value::Function(fid) = v {
                Some(*fid)
              } else {
                None
              }
            } else {
              None
            }
          })
        })
        .expect("generic method must have an SSA origin function");
      return self.instance(origin_fid, &rtargs, targs);
    }

    if let Some(fid) = self.func_value(obj) {
      return fid;
    }

    if let Some(&fid) = self.object_methods.get(&obj) {
      return fid;
    }

    let name = obj.name(&self.object_arena).to_string();
    let fid = create_function(self, name, None, None);
    {
      let f = self.functions.get_mut(fid);
      f.object = Some(obj);
      f.signature = Some(sig);
      f.synthetic = Some("from type information (on demand)".to_string());
    }
    self.object_methods.insert(obj, fid);
    fid
  }

  /// Returns the implementation of the method of type `t` identified by
  /// `(pkg, name)`. Returns `None` if the method exists but is an interface or
  /// generic method. Panics if `t` has no such method. (Go:
  /// `(*Program).LookupMethod`.)
  pub fn lookup_method(
    &mut self,
    t: TypeId,
    pkg: Option<TypePackageId>,
    name: &str,
  ) -> Option<FuncId> {
    let candidates: Vec<Selection> = self.method_set(t).to_vec();
    let sel = candidates
      .iter()
      .find(|s| {
        s.obj().name(&self.object_arena) == name
          && method_pkg(s.obj(), &self.object_arena) == pkg
      })
      .cloned()
    .or_else(|| {
        let result = lookup_field_or_method(
          &mut self.type_arena,
          &self.object_arena,
          &self.package_arena,
          t,
          true,
          pkg,
          name,
        );
        match result {
          LookupResult::Found { obj, index, indirect } => {
            if matches!(self.object_arena.get(obj), ObjectData::Func(_)) {
              Some(Selection::new(SelectionKind::MethodVal, t, obj, index, indirect))
            } else {
              None
            }
          }
          LookupResult::NotFound => {
            panic!("{t:?} has no method {name}");
          }
          other => panic!("lookup_method: ambiguous or invalid selection: {other:?}"),
        }
      });

    let sel = sel.expect("lookup_method: method not in set");
    self.method_value(&sel)
  }
}

impl Program {
  /// Key for the concrete method-set map (receiver type of the selection).
  fn concrete_method_set_for(&mut self, t: TypeId) -> TypeId {
    // Ensure the method-set entries exist before we allocate a ConcreteMethodSet.
    self.method_set(t);
    if !self.concrete_method_sets.contains_key(&t) {
      self.concrete_method_sets.insert(t, ConcreteMethodSet::default());
    }
    t
  }

  pub(crate) fn build_wrapper(&mut self, fid: FuncId) {
    crate::wrappers::build_wrapper(self, fid);
  }

  pub(crate) fn build_bound(&mut self, fid: FuncId) {
    crate::wrappers::build_bound(self, fid);
  }
}

/// Returns the receiver type of method object `obj`. (Go: `recvType`.)
///
/// Returns `None` when `obj` is not a method (no signature, or signature
/// without a receiver) — callers must fall back instead of panicking during
/// buildir.
pub(crate) fn recv_type(prog: &Program, obj: ObjectId) -> Option<TypeId> {
    recv_type_from_objects(&prog.type_arena, &prog.object_arena, obj)
}

/// Like [`recv_type`] but takes the arenas directly (for use from `canon.rs`).
pub(crate) fn recv_type_from_objects(
    arena: &TypeArena,
    oarena: &ObjectArena,
    obj: ObjectId,
) -> Option<TypeId> {
    let sig = obj.typ(oarena)?;
    let recv = guff_types::signature::signature_recv(arena, sig)?;
    recv.typ(oarena)
}

/// Returns the type arguments to a method's receiver named type, or an empty
/// slice if the receiver is not an instantiated named type. (Go:
/// `receiverTypeArgs`.)
pub fn receiver_type_args(prog: &Program, method: ObjectId) -> Vec<TypeId> {
    let Some(recv_typ) = recv_type(prog, method) else {
        return Vec::new();
    };
    let recv_typ = guff_types::alias::unalias_readonly(&prog.type_arena, recv_typ);
    match prog.type_arena.get(recv_typ) {
        TypeData::Pointer(p) => receiver_type_args_on_type(prog, p.elem()),
        _ => receiver_type_args_on_type(prog, recv_typ),
    }
}

fn receiver_type_args_on_type(prog: &Program, t: TypeId) -> Vec<TypeId> {
    let t = guff_types::alias::unalias_readonly(&prog.type_arena, t);
    match prog.type_arena.get(t) {
        TypeData::Named(_) => guff_types::named::named_type_args(&prog.type_arena, t)
            .map(|l| l.list().to_vec())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn method_pkg(obj: ObjectId, oarena: &guff_types::ObjectArena) -> Option<TypePackageId> {
  obj.pkg(oarena)
}

/// Enumerates the methods of `t` by collecting candidate names from embedded
/// named types / struct fields and resolving each with `lookup_field_or_method`.
fn compute_method_set(prog: &mut Program, t: TypeId) -> Vec<Selection> {
  if is_interface(&prog.type_arena, t) {
    return Vec::new();
  }

  let mut names = HashSet::new();
  let mut seen_types = HashSet::new();
  collect_method_names(prog, t, &mut names, &mut seen_types);

  let mut sels = Vec::new();
  let mut seen_objs = HashSet::new();
  for name in names {
    let result = lookup_field_or_method(
      &mut prog.type_arena,
      &prog.object_arena,
      &prog.package_arena,
      t,
      true,
      None,
      &name,
    );
    if let LookupResult::Found { obj, index, indirect } = result {
      if matches!(prog.object_arena.get(obj), ObjectData::Func(_)) && seen_objs.insert(obj) {
        sels.push(Selection::new(
          SelectionKind::MethodVal,
          t,
          obj,
          index,
          indirect,
        ));
      }
    }
  }
  sels.sort_by(|a, b| {
    a.obj()
      .name(&prog.object_arena)
      .cmp(b.obj().name(&prog.object_arena))
  });
  sels
}

fn collect_method_names(
  prog: &Program,
  t: TypeId,
  names: &mut HashSet<String>,
  seen: &mut HashSet<TypeId>,
) {
  let u = unalias_readonly(&prog.type_arena, t);
  if !seen.insert(u) {
    return;
  }
  match prog.type_arena.get(u) {
    TypeData::Pointer(p) => collect_method_names(prog, p.elem(), names, seen),
    TypeData::Named(_) => {
      let n = named_num_methods(&prog.type_arena, u);
      for i in 0..n {
        let m = named_method(&prog.type_arena, u, i);
        names.insert(m.name(&prog.object_arena).to_string());
      }
      if let Some(under) = named_underlying(&prog.type_arena, u) {
        collect_method_names(prog, under, names, seen);
      }
    }
    TypeData::Struct(_) => {
      let n = struct_num_fields(&prog.type_arena, u);
      for i in 0..n {
        let field = struct_field(&prog.type_arena, u, i);
        if let Some(ft) = field.typ(&prog.object_arena) {
          collect_method_names(prog, ft, names, seen);
        }
      }
    }
    _ => {}
  }
}
