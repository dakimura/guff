//! Utilities for working with source positions and source-level named
//! entities ("objects"). (Go: `source.go`)
//!
//! This chunk (D06) ports the pieces that the current foundations support:
//! [`Program::package`] (type-checker package → SSA package) and
//! [`Function::value_for_expr`] (source expression → SSA value, via the
//! DebugRef pseudo-instructions emitted in D05).
//!
//! The remaining source.go entry points are deferred because they depend on
//! machinery that the CREATE phase does not yet build:
//!
//! - `EnclosingFunction` / `HasEnclosingFunction` /
//!   `findEnclosingPackageLevelFunction` / `findNamedFunc` need populated
//!   package members, the synthesized `pkg.init` function, `Function.AnonFuncs`,
//!   and named-type method sets.
//! - `packageLevelMember` / `FuncValue` / `ConstValue` need `pkg.objects`
//!   populated by CREATE.
//! - `VarValue` builds on `EnclosingFunction` and the above.
//!
//! These cross into Milestone E; see the deferral table in the migration plan.

use crate::const_val::Const;
use crate::function::Function;
use crate::ids::{FuncId, PackageId};
use crate::instr::InstrData;
use crate::member::MemberData;
use crate::program::Program;
use crate::value::Value;
use guff::ast::{Decl, Expr, Node};
use guff::token::Token;
use guff_types::{ObjectData, ObjectId, PackageId as TypePackageId, TypeId};

impl Program {
    /// package returns the SSA Package corresponding to the specified
    /// type-checker package. It returns `None` if no such Package was created
    /// by a prior call to [`crate::create::create_package`].
    /// (Go: `(*Program).Package`)
    pub fn package(&self, pkg: TypePackageId) -> Option<PackageId> {
        self.package_map.get(&pkg).copied()
    }

    /// package_level_member returns the package-level member corresponding to
    /// the object `obj`, which may be a package-level const ([`Value::Const`]),
    /// var ([`Value::Global`]) or func/method ([`Value::Function`]) of some
    /// package in the program. It returns `None` if the object belongs to a
    /// package that was not created (or is not a package-level value object,
    /// e.g. a type name — types are members but not values).
    /// (Go: `(*Program).packageLevelMember`)
    ///
    /// Requires that the owning package's members were populated (see
    /// [`crate::create::populate_package_members`]).
    pub fn package_level_member(&self, obj: ObjectId) -> Option<Value> {
        let type_pkg = obj.pkg(&self.object_arena)?;
        let ssa_pkg = *self.package_map.get(&type_pkg)?;
        self.packages.get(ssa_pkg).objects.get(&obj).copied()
    }

    /// func_value returns the SSA function or (non-interface) method denoted by
    /// the specified func object. It returns `None` if the symbol denotes an
    /// interface method, or belongs to a package that was not created.
    /// (Go: `(*Program).FuncValue`)
    pub fn func_value(&self, obj: ObjectId) -> Option<FuncId> {
        match self.package_level_member(obj)? {
            Value::Function(fid) => Some(fid),
            _ => None,
        }
    }

    /// const_value returns the SSA constant denoted by the specified const
    /// object. For a package-level named constant it returns the constant
    /// created during member population; for a universal constant
    /// (`true`/`false`/`nil`) or any other const it reconstructs the value from
    /// the object. Returns `None` if `obj` is not a const object.
    /// (Go: `(*Program).ConstValue`)
    ///
    /// Divergence from go/ssa: go returns a `*Const` (a [`Value`]); we return
    /// the [`Const`] data by value, since our constants live in the program
    /// arena and a fresh universal constant would otherwise need `&mut self`.
    /// A caller wanting a [`Value`] handle can pass this to
    /// [`Program::emit_const`].
    pub fn const_value(&self, obj: ObjectId) -> Option<Const> {
        // Package-level named constant? Return the already-created Const.
        if let Some(Value::Const(cid)) = self.package_level_member(obj) {
            let c = self.constants.get(cid);
            return Some(Const::new(c.val.clone(), c.typ));
        }
        // Universal constant, or a const in an uncreated package: reconstruct.
        match self.object_arena.get(obj) {
            ObjectData::Const(c) => Some(Const::new(Some(c.val().clone()), c.typ())),
            _ => None,
        }
    }

    /// find_named_func returns the package-level named function whose declaring
    /// identifier is at source position `pos` (a byte offset; `0` = no
    /// position). (Go: `findNamedFunc`)
    ///
    /// DEFERRED: the `*Type` branch, which scans a named type's method set for
    /// a method at `pos`. It needs `Program.MethodSets`; until then, methods are
    /// not resolvable by position.
    pub fn find_named_func(&mut self, pkg_id: PackageId, pos: u32) -> Option<FuncId> {
        if pos == 0 {
            return None;
        }
        let pkg = self.packages.get(pkg_id);
        let type_members: Vec<TypeId> = pkg
            .members
            .values()
            .filter_map(|m| match m {
                MemberData::Type(t) => Some(*t),
                _ => None,
            })
            .collect();
        for member in pkg.members.values() {
            if let MemberData::Function(fid) = member {
                let f = self.functions.get(*fid);
                if let Some(obj) = f.object {
                    if obj.pos(&self.object_arena) == pos {
                        return Some(*fid);
                    }
                }
            }
        }
        for typ in type_members {
            let ptr = guff_types::pointer::new_pointer(&mut self.type_arena, typ);
            let sels: Vec<guff_types::Selection> = self.method_set(ptr).to_vec();
            for sel in sels {
                let mobj = sel.obj();
                if mobj.pos(&self.object_arena) == pos {
                    if let Some(Value::Function(fid)) = self.package_level_member(mobj) {
                        return Some(fid);
                    }
                }
            }
        }
        None
    }

    /// find_enclosing_package_level_function returns the package-level function
    /// enclosing the AST node denoted by `path` (innermost first, as produced by
    /// an enclosing-interval search). (Go: `findEnclosingPackageLevelFunction`)
    ///
    /// DEFERRED: package-level `var` initializers and explicit `init` functions
    /// are enclosed by the synthesized `pkg.init`, which does not exist yet;
    /// those cases return `None` for now.
    fn find_enclosing_package_level_function(&mut self, pkg_id: PackageId, path: &[Node]) -> Option<FuncId> {
        let n = path.len();
        if n >= 2 {
            // path is [... {Gen,Func}Decl File]
            if let Node::Decl(decl) = &path[n - 2] {
                match decl {
                    Decl::GenDecl(gd) => {
                        if gd.tok == Some(Token::VAR) && n >= 3 {
                            // Package-level 'var' initializer -> pkg.init.
                            return None; // DEFERRED: pkg.init
                        }
                    }
                    Decl::FuncDecl(fd) => {
                        if fd.recv.is_none() && fd.name.name == "init" {
                            // Explicit init() function -> pkg.init.
                            return None; // DEFERRED: pkg.init
                        }
                        // Declared function/method.
                        return self.find_named_func(pkg_id, fd.name.name_pos.0 as u32);
                    }
                    Decl::BadDecl(_) => {}
                }
            }
        }
        None // not in any function
    }

    /// has_enclosing_function reports whether the AST node denoted by `path` is
    /// contained within the declaration of some function or package-level
    /// variable. Unlike [`Program::enclosing_function`], its result does not
    /// depend on whether SSA bodies have been built. (Go: `HasEnclosingFunction`)
    ///
    /// Partial: until `pkg.init` exists, package-level var initializers are not
    /// yet reported (see [`Program::find_enclosing_package_level_function`]).
    pub fn has_enclosing_function(&mut self, pkg_id: PackageId, path: &[Node]) -> bool {
        self.find_enclosing_package_level_function(pkg_id, path).is_some()
    }

    /// enclosing_function returns the function that contains the syntax node
    /// denoted by `path`. Returns `None` if the node is not enclosed by any
    /// function. (Go: `EnclosingFunction`)
    ///
    /// DEFERRED: descent into nested anonymous functions (`ast.FuncLit`) needs
    /// `Function.AnonFuncs`, which the builder does not record yet; a path that
    /// passes through a `FuncLit` therefore yields `None`, matching go/ssa's
    /// "SSA function not found" fallback.
    pub fn enclosing_function(&mut self, pkg_id: PackageId, path: &[Node]) -> Option<FuncId> {
        // Start with the package-level function...
        let fid = self.find_enclosing_package_level_function(pkg_id, path)?;

        // ...then walk down the nested anonymous functions.
        let n = path.len();
        for i in 0..n {
            if let Node::Expr(Expr::FuncLit(_)) = &path[n - 1 - i] {
                // The enclosing SSA function is a not-yet-created anonymous
                // function (AnonFuncs unbuilt).
                return None; // DEFERRED: Function.AnonFuncs
            }
        }
        Some(fid)
    }

    /// var_value returns the SSA value that corresponds to a specific
    /// identifier (`ref[0]`) denoting the var object `obj`, together with
    /// `is_addr` (whether the value is the variable's address). `pkg` is the
    /// package enclosing the reference. (Go: `(*Program).VarValue`)
    ///
    /// Returns `None` if no value was found (e.g. the package was not built,
    /// debug information was not requested, or the value was optimized away).
    ///
    /// `ref` is the enclosing-interval path to an `ast.Ident` that must resolve
    /// to `obj`.
    pub fn var_value(
        &mut self,
        obj: ObjectId,
        pkg_id: PackageId,
        r#ref: &[Node],
    ) -> Option<(Value, bool)> {
        // All references to a var are local to some function, possibly init.
        let fid = self.enclosing_function(pkg_id, r#ref)?;

        // ref[0] must be the referring identifier.
        let id = match r#ref.first() {
            Some(Node::Expr(Expr::Ident(id))) => id,
            _ => return None,
        };
        let id_pos = id.name_pos.0 as u32;

        let f = self.functions.get(fid);

        // Defining ident of a parameter?
        if id_pos == obj.pos(&self.object_arena) {
            for (pid, param) in f.params.iter() {
                if param.object == Some(obj) {
                    return Some((Value::Param(pid), false));
                }
            }
        }

        // Other ident? Look for a DebugRef at the identifier's position.
        for (_, block) in f.blocks.iter() {
            for &instr_id in &block.instrs {
                if let InstrData::DebugRef(dr) = f.instrs.get(instr_id) {
                    if f.pos(instr_id).0 as u32 == id_pos {
                        return Some((dr.x, dr.is_addr));
                    }
                }
            }
        }

        // Defining ident of a package-level var?
        if let Some(v @ Value::Global(_)) = self.package_level_member(obj) {
            return Some((v, true));
        }

        None // e.g. debug info not requested, or var optimized away
    }
}

impl Function {
    /// value_for_expr returns the SSA [`Value`] that corresponds to the
    /// non-constant expression `e`, together with `is_addr`: if `e` is an
    /// addressable expression used in an lvalue context, the returned value is
    /// the address `e` denotes and `is_addr` is `true`.
    ///
    /// It returns `None` if no value was found, e.g. because:
    ///   - the expression is not lexically contained within this function;
    ///   - the function was not built with debug information (no DebugRefs);
    ///   - `e` is a constant expression (no debug info is stored for constants);
    ///   - `e` refers to nil or a built-in function; or
    ///   - the value was optimized away.
    ///
    /// (Go: `(*Function).ValueForExpr`)
    ///
    /// Where go/ssa matches the stored `DebugRef.Expr` against `e` by pointer
    /// identity, we match on the expression's stable AST node id (see
    /// [`Expr::id`]), which survives the AST clone the builder operates on.
    pub fn value_for_expr(&self, e: &Expr) -> Option<(Value, bool)> {
        let target = unparen(e).id();
        if target == 0 {
            // Unstamped / hand-built node: it could never have been recorded.
            return None;
        }
        for (_, block) in self.blocks.iter() {
            for &instr_id in &block.instrs {
                if let InstrData::DebugRef(dr) = self.instrs.get(instr_id) {
                    if dr.expr_id == target {
                        return Some((dr.x, dr.is_addr));
                    }
                }
            }
        }
        None
    }
}

/// unparen strips any enclosing parentheses from an expression.
/// (Go: `ast.Unparen`)
fn unparen(e: &Expr) -> &Expr {
    let mut e = e;
    while let Expr::ParenExpr(p) = e {
        e = &p.x;
    }
    e
}
