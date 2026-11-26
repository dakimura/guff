//! Port of `cmd/compile/internal/types2/cycles.go` — direct cycle detection
//! among package-level type declarations.
//!
//! A *direct* cycle exists when the path from a type name's declaration RHS
//! leads from type name to type name and eventually back onto that path — with
//! no type literals or basic types on the way, and without ending in an
//! undeclared object. Examples:
//!
//! ```text
//! type A B; type B A        // mutual, via regular declarations
//! type A = B; type B = A    // mutual, via aliases
//! type A A                  // self reference
//! ```
//!
//! Such cycles are *not* caught by [`valid_type`](crate::validtype::valid_type)
//! (chunk 38): that pass only inspects a defined type's *underlying* structure,
//! but on a pure name chain our [`Checker::type_decl`](crate::Checker) maps the
//! underlying to `Typ[Invalid]` before `valid_type` ever runs, so the cycle
//! would otherwise be silently invalidated with no diagnostic.
//!
//! `direct_cycles` runs between `sort_objects` and `package_objects` in
//! [`Checker::check_files`](crate::Checker::check_files), mirroring Go's
//! `checkFiles` ordering. It marks the type at the start of each detected cycle
//! `Typ[Invalid]`, which causes `obj_decl` to treat it as already-checked
//! (black) and skip re-processing.
//!
//! **Deferred**:
//! - `finiteSize` (cycles.go) — needs the `Named` finite-size state machine
//!   (`hasFinite`/`finite`) and `objPathIdx`, neither of which is ported.
//! - The multi-line `cycleError` detail (one "X refers to Y" line per edge,
//!   with per-object positions) — we report a single concise message (D07).
//! - The alias `fromRHS`/`validAlias` reset — we set the `TypeName`'s type to
//!   `Typ[Invalid]` directly instead.

use crate::hash::HashMap;

use guff::ast::Expr;
use guff_types_errors::Code;

use crate::arena::ObjectData;
use crate::check::Checker;
use crate::object::type_name::type_name_set_typ;
use crate::scope;
use crate::ObjectId;

impl Checker {
    /// Search for direct cycles among package-level type declarations.
    ///
    /// Equivalent to `Checker.directCycles`.
    pub(crate) fn direct_cycles(&mut self) {
        // An entry in `path_idx` is in one of three states (white/grey/black):
        //   - absent        : not seen yet (white)
        //   - value >= 0     : seen, not done (grey); value is the path index
        //   - value <  0     : seen and done (black)
        let mut path_idx: HashMap<ObjectId, i64> = HashMap::default();

        // `obj_list` is the source-sorted package-level object list (filled by
        // `sort_objects`). Snapshot it because `direct_cycle` mutates `self`.
        let objs = self.obj_list.clone();
        for obj in objs {
            if matches!(self.objects.get(obj), ObjectData::TypeName(_)) {
                self.direct_cycle(obj, &mut path_idx);
            }
        }
    }

    /// Check whether the declaration of the type named by `tname` contains a
    /// direct cycle, following the name chain through the package scope.
    ///
    /// Equivalent to `Checker.directCycle`. On return, every type name on the
    /// path starting at `tname` is marked black, so each is traversed only once.
    fn direct_cycle(&mut self, mut tname: ObjectId, path_idx: &mut HashMap<ObjectId, i64>) {
        let mut path: Vec<ObjectId> = Vec::new();
        loop {
            match path_idx.get(&tname).copied() {
                // tname is black — do not traverse it again.
                Some(start) if start < 0 => break,
                // tname is grey — a cycle on the path beginning at `start`.
                Some(start) => {
                    // Mark the cycle-start type invalid (so obj_decl skips it).
                    let invalid = self.invalid_type();
                    type_name_set_typ(&mut self.objects, tname, invalid);

                    let cycle: Vec<ObjectId> = path[start as usize..].to_vec();
                    self.cycle_error(&cycle);
                    break;
                }
                // tname is white — mark it grey and add it to the path.
                None => {
                    path_idx.insert(tname, path.len() as i64);
                    path.push(tname);

                    // For direct-cycle detection an alias vs. defined type makes
                    // no difference. If the RHS is not a bare name we are at the
                    // end of the path and done.
                    let rhs_name = match self.obj_map.get(&tname).and_then(|d| d.tdecl) {
                        Some(tdecl_id) => match self.syntax.type_spec(tdecl_id) {
                            Some(tdecl) => match &tdecl.ty {
                                Expr::Ident(id) => id.name.clone(),
                                _ => break,
                            },
                            None => break,
                        },
                        None => break,
                    };

                    // Determine the RHS type. If it is not a type name in the
                    // package scope, then either it lives elsewhere (universe or
                    // file scope via dot-import — no cycle possible) or it is an
                    // error reported later; in all cases we can stop.
                    let pkg_scope = self.packages.get(self.pkg).scope();
                    let next = match scope::lookup(&self.scopes, pkg_scope, &rhs_name) {
                        Some(o) if matches!(self.objects.get(o), ObjectData::TypeName(_)) => o,
                        _ => break,
                    };
                    tname = next;
                }
            }
        }

        // Mark all traversed type names black (no grey entries left behind).
        for t in path {
            path_idx.insert(t, -1);
        }
    }

    /// Report a type-declaration cycle.
    ///
    /// Simplified port of `Checker.cycleError`: we emit a single
    /// [`Code::InvalidDeclCycle`] message. For multi-type cycles we render the
    /// "X refers to Y refers to … refers to X" chain inline rather than as Go's
    /// per-edge multi-line diagnostic (which needs per-object positions, D07).
    fn cycle_error(&mut self, cycle: &[ObjectId]) {
        let start = first_in_src(self, cycle);
        let obj = cycle[start];
        let pos = obj.pos(&self.objects);
        let name = obj.name(&self.objects).to_string();

        let msg = if cycle.len() == 1 {
            format!("invalid recursive type: {} refers to itself", name)
        } else {
            // cycle[start] -> cycle[start+1] -> ... -> cycle[start] (wrap)
            let mut chain: Vec<String> = Vec::with_capacity(cycle.len() + 1);
            for i in 0..cycle.len() {
                let cur = cycle[(start + i) % cycle.len()];
                chain.push(cur.name(&self.objects).to_string());
            }
            chain.push(name.clone()); // close the loop
            format!(
                "invalid recursive type {}: {}",
                name,
                chain.join(" refers to ")
            )
        };

        self.error(pos, Code::InvalidDeclCycle, msg);
    }
}

/// Return the index of the object in `path` with the "smallest" source
/// position, so cycle errors are reported deterministically at the earliest
/// declaration. Equivalent to `firstInSrc` (decl.go).
///
/// With positions still stubbed (D07: all `0`), this picks index `0`.
fn first_in_src(check: &Checker, path: &[ObjectId]) -> usize {
    let mut fst = 0usize;
    let mut pos = path[0].pos(&check.objects);
    for (i, &t) in path.iter().enumerate().skip(1) {
        let p = t.pos(&check.objects);
        if p < pos {
            fst = i;
            pos = p;
        }
    }
    fst
}
