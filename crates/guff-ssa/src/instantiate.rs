//! Generic instantiation — instance function creation and caching.
//!
//! Port of go/ssa's `instantiate.go` (the `generic` instance cache,
//! `createInstance`, and `(*Function).instance`) plus the `targstr` helper from
//! `ssa.go`. Given a generic origin function and a list of type arguments,
//! [`Program::instance`] returns the SSA `Function` for that instantiation,
//! creating and caching it on first request. [`Program::create_instance`]
//! computes the instance's signature (via `types.Instantiate` + canonicalization)
//! and selects its build strategy.
//!
//! # Scope
//!
//! Both package-level generic **functions** and **methods** (generic methods,
//! methods on generic types, or both) are supported. Method instantiation uses
//! [`crate::canon::Canonizer::instantiate_method`] when receiver type arguments
//! are present.

use guff_types::{
    instantiate, signature_recv, signature_type_params, type_string, TypeId,
};

use crate::function::{BuildStrategy, Function};
use crate::ids::FuncId;
use crate::mode::BuilderMode;
use crate::program::Program;

/// Formats a type-argument list as `[T1, T2, …]` (empty string for no args),
/// used to build an instance function's name. (Go: `targstr`.)
pub fn targstr(
    arena: &guff_types::TypeArena,
    oarena: &guff_types::ObjectArena,
    parena: &guff_types::PackageArena,
    targs: &[TypeId],
) -> String {
    if targs.is_empty() {
        return String::new();
    }
    let mut sb = String::from("[");
    for (i, &t) in targs.iter().enumerate() {
        if i > 0 {
            sb.push_str(", ");
        }
        sb.push_str(&type_string(arena, oarena, parena, t, None));
    }
    sb.push(']');
    sb
}

impl Program {
    /// Returns the `Function` that is the instantiation of generic origin
    /// `fn_id` with receiver type arguments `rtargs` and type arguments
    /// `targs`, creating and caching it on first request. (Go:
    /// `(*Function).instance`.)
    ///
    /// The instance is cached on the origin, keyed by the canonical
    /// concatenation of `rtargs` and `targs`, so structurally-identical
    /// argument lists share one instance.
    pub fn instance(&mut self, fn_id: FuncId, rtargs: &[TypeId], targs: &[TypeId]) -> FuncId {
        let concat: Vec<TypeId> = rtargs.iter().chain(targs).copied().collect();
        let key = self.canon.canonical_list(
            &mut self.type_arena,
            &self.object_arena,
            &self.package_arena,
            &concat,
        );

        if let Some(k) = key {
            if let Some(&existing) = self.functions.get(fn_id).generic_instances.get(&k) {
                return existing;
            }
            let inst = self.create_instance(fn_id, rtargs, targs);
            self.functions.get_mut(fn_id).generic_instances.insert(k, inst);
            self.enqueue_build(inst);
            inst
        } else {
            // No type arguments at all — not a real instance; create without
            // caching (Go always concatenates to a non-nil key for instances).
            let inst = self.create_instance(fn_id, rtargs, targs);
            self.enqueue_build(inst);
            inst
        }
    }

    /// Creates the instantiation of generic origin `fn_id` using `rtargs` and
    /// `targs`, computes its signature, selects a build strategy, and returns the
    /// new instance `FuncId`. (Go: `createInstance`.)
    pub fn create_instance(&mut self, fn_id: FuncId, rtargs: &[TypeId], targs: &[TypeId]) -> FuncId {
        let origin = self.functions.get(fn_id);
        let orig_name = origin.name.clone();
        let orig_sig = origin.signature.expect("generic origin has a signature");
        let orig_object = origin.object;
        let orig_from_syntax = origin.from_syntax;
        let orig_syntax_decl = origin.syntax_decl.clone();
        let orig_recv_tparams = if origin.recv_type_params.is_empty() {
            guff_types::signature::signature_recv_type_params(&self.type_arena, orig_sig)
                .map(|l| l.list().to_vec())
                .unwrap_or_default()
        } else {
            origin.recv_type_params.clone()
        };
        let orig_tparams = if origin.type_params.is_empty() {
            signature_type_params(&self.type_arena, orig_sig)
                .map(|l| l.list().to_vec())
                .unwrap_or_default()
        } else {
            origin.type_params.clone()
        };

        // Compute the instance signature and (for methods) the instantiated object.
        // Non-methods ignore rtargs (go/ssa createInstance does not assert; hybrid
        // name-collision fallbacks can pass spurious receiver args).
        let is_method = signature_recv(&self.type_arena, orig_sig).is_some();
        let rtargs = if is_method { rtargs } else { &[] };

        let (inst_object, sig) = if is_method {
            // Method: instantiate receiver and/or method type parameters.
            let obj = if !rtargs.is_empty() {
                self.canon.instantiate_method(
                    &mut self.type_arena,
                    &mut self.object_arena,
                    &self.package_arena,
                    orig_object.expect("method origin has object"),
                    rtargs,
                    &mut self.ctxt,
                )
            } else {
                orig_object.expect("method origin has object")
            };
            let sig = if !targs.is_empty() {
                let obj_sig = obj.typ(&self.object_arena).expect("method has type");
                instantiate(
                    &mut self.type_arena,
                    &mut self.object_arena,
                    &mut self.ctxt,
                    obj_sig,
                    targs.to_vec(),
                )
            } else {
                obj.typ(&self.object_arena).expect("method has type")
            };
            (Some(obj), sig)
        } else {
            if targs.is_empty() {
                return fn_id;
            }
            let inst_sig = instantiate(
                &mut self.type_arena,
                &mut self.object_arena,
                &mut self.ctxt,
                orig_sig,
                targs.to_vec(),
            );
            let sig = self.canon.canonical_type(
                &mut self.type_arena,
                &self.object_arena,
                &self.package_arena,
                inst_sig,
            );
            (orig_object, sig)
        };

        // Choose strategy: a fully-concrete instance under InstantiateGenerics
        // is built directly; otherwise it goes through an instantiation wrapper.
        let concrete =
            self.mode.contains(BuilderMode::INSTANTIATE_GENERICS) && !self.is_parameterized(&concat_owned(rtargs, targs));
        let (synthetic, strategy, subst) = if concrete {
            let syn = format!("instance of {}", orig_name);
            if orig_from_syntax {
                let subst = crate::subst::Subster::with_recv_and_type(
                    &orig_recv_tparams,
                    rtargs,
                    &orig_tparams,
                    targs,
                );
                (syn, BuildStrategy::FromSyntax, Some(subst))
            } else {
                (syn, BuildStrategy::ParamsOnly, None)
            }
        } else {
            (
                format!("instantiation wrapper of {}", orig_name),
                BuildStrategy::InstantiationWrapper,
                None,
            )
        };

        // Instance name: origin name + `[targs]` (method receiver args are not
        // reflected in the name).
        let mut name = orig_name;
        if !targs.is_empty() {
            name.push_str(&targstr(
                &self.type_arena,
                &self.object_arena,
                &self.package_arena,
                targs,
            ));
        }

        // Pkg is nil for instances (Go: `Pkg: nil`); parent is None.
        let mut inst = Function::new(name, None, None);
        inst.object = inst_object;
        inst.signature = Some(sig);
        inst.synthetic = Some(synthetic);
        inst.from_syntax = orig_from_syntax; // shares origin syntax (Go: `syntax: fn.syntax`)
        inst.syntax_decl = orig_syntax_decl;
        inst.top_level_origin = Some(fn_id);
        inst.recv_type_params = orig_recv_tparams;
        inst.type_params = orig_tparams;
        inst.type_args = targs.to_vec();
        inst.recv_type_args = rtargs.to_vec();
        inst.build_strategy = strategy;
        inst.subst = subst;

        self.functions.alloc(inst)
    }

    /// Records `fid` for body construction if it has a build strategy but no
    /// blocks yet. (Go: `(*builder).enqueue`.)
    pub fn enqueue_build(&mut self, fid: FuncId) {
        let needs = {
            let f = self.functions.get(fid);
            f.build_strategy != BuildStrategy::Unset && f.blocks.is_empty()
        };
        if needs && !self.pending_builds.contains(&fid) {
            self.pending_builds.push(fid);
        }
    }

    /// Builds every function queued by [`Self::enqueue_build`] until the queue
    /// is drained. New instances created during a build append to the queue, so
    /// this loop converges like go/ssa's `(*builder).iterate`. (Go:
    /// `buildFunction` over the enqueue list.)
    pub fn drain_build_queue(&mut self) {
        const LIMIT: usize = 4096;
        let mut i = 0usize;
        while i < self.pending_builds.len() && i < LIMIT {
            let fid = self.pending_builds[i];
            if self.functions.get(fid).blocks.is_empty() {
                self.build_instance(fid);
            }
            i += 1;
        }
        self.pending_builds.clear();
    }
}

impl Program {
    /// Builds the body of instance/wrapper function `fid` according to its
    /// recorded [`BuildStrategy`]. This is the sequential analog of the strategy
    /// dispatch go/ssa performs via the `fn.build` function pointer (invoked
    /// from `(*builder).buildFunction`).
    ///
    /// - [`BuildStrategy::InstantiationWrapper`] → [`build_instantiation_wrapper`].
    /// - [`BuildStrategy::ParamsOnly`] → parameters only, no body.
    /// - [`BuildStrategy::FromSyntax`] → rebuild from the origin's syntax with
    ///   type substitution applied via [`Program::function_typ`].
    /// - [`BuildStrategy::Unset`] is a no-op.
    ///
    /// [`build_instantiation_wrapper`]: crate::wrappers::build_instantiation_wrapper
    pub fn build_instance(&mut self, fid: FuncId) {
        match self.functions.get(fid).build_strategy {
            BuildStrategy::InstantiationWrapper => {
                crate::wrappers::build_instantiation_wrapper(self, fid)
            }
            BuildStrategy::ParamsOnly => self.build_params_only(fid),
            BuildStrategy::FromSyntax => self.build_from_syntax(fid),
            BuildStrategy::Wrapper => crate::wrappers::build_wrapper(self, fid),
            BuildStrategy::Bound => crate::wrappers::build_bound(self, fid),
            BuildStrategy::YieldFunc => {}
            BuildStrategy::Unset => {}
        }
    }

    /// Creates the function's parameters (receiver + regular) from its signature
    /// but no body — used for instances with no available syntax (imported
    /// generic origins). (Go: `(*builder).buildParamsOnly`.)
    fn build_params_only(&mut self, fid: FuncId) {
        crate::create::create_params(self, fid);
        self.functions.get_mut(fid).subst = None;
    }

    /// Builds a concrete generic instance from the origin's syntax, applying
    /// type-parameter substitution while translating expressions. (Go:
    /// `(*builder).buildFromSyntax` on an instance function.)
    fn build_from_syntax(&mut self, fid: FuncId) {
        let fd = self
            .functions
            .get(fid)
            .syntax_decl
            .clone()
            .or_else(|| {
                let origin = self.functions.get(fid).top_level_origin?;
                self.functions.get(origin).syntax_decl.clone()
            })
            .expect("FromSyntax instance requires origin syntax_decl");

        crate::builder::build_syntactic_body(self, fid, Some(&fd), fd.body.as_ref());
        self.functions.get_mut(fid).subst = None;
    }
}

/// `rtargs` followed by `targs`, as an owned `Vec` (Go: `slices.Concat`).
fn concat_owned(rtargs: &[TypeId], targs: &[TypeId]) -> Vec<TypeId> {
    rtargs.iter().chain(targs).copied().collect()
}
