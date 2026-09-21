//! Port of [`github.com/sanposhiho/wastedassign`](https://github.com/sanposhiho/wastedassign).
//!
//! Finds local variable assignments whose value is never read before the next
//! assignment or function exit. Builds NaiveForm SSA internally (upstream
//! requires it; shared `buildir` uses GlobalDebug instead).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, Stmt};
use guff::node_mask;
use guff::walk::{expr_ref, preorder, NodeRef};
use guff_analysis::code::object_of;
use guff_analysis::passes::buildir::collect_src_funcs_with_methods;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Diagnostic, RunError, RunFn, Pass};
use guff_ssa::function::Function;
use guff_ssa::ids::{BlockId, InstrId};
use guff_ssa::instr::{Alloc, InstrData};
use guff_ssa::mode::BuilderMode;
use guff_ssa::ssautil::build_package_for_analysis;
use guff_ssa::value::Value;
use guff_types::arena::TypeData;
use guff_types::ObjectId;

#[derive(Clone, Copy, PartialEq, Eq)]
enum WastedReason {
    NoUseUntilReturn,
    ReassignedSoon,
    NotWasted,
}

fn format_reason(reason: WastedReason, comment: &str) -> Option<String> {
    match reason {
        WastedReason::NoUseUntilReturn => Some(format!(
            "assigned to {comment}, but never used afterwards"
        )),
        WastedReason::ReassignedSoon => Some(format!(
            "assigned to {comment}, but reassigned without using the value"
        )),
        WastedReason::NotWasted => None,
    }
}


fn collect_type_switch_lines(pass: &Pass<'_>) -> HashSet<i64> {
    let mut lines = HashSet::new();
    let Some(inspect) = pass.result_of::<inspect::InspectResult>(inspect::analyzer()) else {
        return lines;
    };
    let fset = pass.fset();
    inspect.preorder_typed(node_mask!(TypeSwitchStmt), pass.files(), |n| {
        if let NodeRef::TypeSwitchStmt(stmt) = n {
            lines.insert(fset.as_ref().position(stmt.switch).line);
        }
    });
    lines
}

/// Locals assigned in `IfStmt.Init` and read in that same `IfStmt.Cond`
/// (e.g. `if fi, err := os.Stat(dir); err == nil && fi.IsDir()`).
///
/// NaiveForm often keeps the Extract value in a register for the condition and
/// never Loads the spilled local — SSA then looks like a wasted store.
/// The stores an `if` init performs whose value its condition then reads.
///
/// NaiveForm does not Load a local that the condition reads through a
/// register-lifted Extract, so `is_next_operation_to_op_is_store` calls those
/// stores wasted and this set excuses them. go/ssa emits the Load and upstream
/// never sees the question.
///
/// Positions, not objects. Keyed on the object, the set excused *every* store
/// to that variable in the function — so in
///
/// ```go
/// dir, exists := tree, false        // ← upstream reports this one
/// for _, item := range components {
///     if dir, exists = dir[item]; !exists { … }
/// }
/// ```
///
/// the earlier store, which has nothing to do with the `if`, went unreported
/// too (beats `auditbeat/.../filetree.go:117`, and three more like it).
fn if_init_cond_read_stores(pass: &Pass<'_>) -> HashSet<u32> {
    let mut out = HashSet::new();
    let Some(inspect) = pass.result_of::<inspect::InspectResult>(inspect::analyzer()) else {
        return out;
    };
    inspect.preorder_typed(node_mask!(IfStmt), pass.files(), |n| {
        let NodeRef::IfStmt(ifs) = n else {
            return;
        };
        let Some(Stmt::AssignStmt(init)) = ifs.init.as_deref() else {
            return;
        };
        let mut used = HashSet::new();
        collect_used_objs(pass, &ifs.cond, &mut used);
        for lhs in &init.lhs {
            if let Expr::Ident(id) = lhs {
                if object_of(pass, id).is_some_and(|obj| used.contains(&obj)) {
                    out.insert(id.name_pos.0 as u32);
                }
            }
        }
    });
    out
}

/// Source lines of body `i++`/`i--` on a surrounding `for`'s loop variable.
///
/// Those stores look unused under NaiveForm because the next read is the header
/// (earlier in the file). Match by line — Store pos may sit on `tok_pos` or the
/// Ident depending on builder mode.
fn for_loop_var_body_incdec_lines(pass: &Pass<'_>) -> HashSet<i64> {
    let mut out = HashSet::new();
    let Some(inspect) = pass.result_of::<inspect::InspectResult>(inspect::analyzer()) else {
        return out;
    };
    let fset = pass.fset();
    inspect.preorder_typed(node_mask!(ForStmt), pass.files(), |n| {
        let NodeRef::ForStmt(fs) = n else {
            return;
        };
        let mut header = HashSet::new();
        if let Some(init) = fs.init.as_deref() {
            header.extend(objs_assigned_in_stmt(pass, init));
            collect_used_objs_in_stmt(pass, init, &mut header);
        }
        if let Some(cond) = fs.cond.as_ref() {
            collect_used_objs(pass, cond, &mut header);
        }
        if let Some(post) = fs.post.as_deref() {
            header.extend(objs_assigned_in_stmt(pass, post));
            collect_used_objs_in_stmt(pass, post, &mut header);
            if let Stmt::IncDecStmt(inc) = post {
                if let Expr::Ident(id) = unparen(&inc.x) {
                    if let Some(obj) = object_of(pass, id) {
                        header.insert(obj);
                    }
                }
            }
        }
        if header.is_empty() {
            return;
        }
        preorder(NodeRef::BlockStmt(&fs.body), |n| {
            let NodeRef::IncDecStmt(inc) = n else {
                return true;
            };
            let Expr::Ident(id) = unparen(&inc.x) else {
                return true;
            };
            if let Some(obj) = object_of(pass, id) {
                if header.contains(&obj) {
                    out.insert(fset.as_ref().position(inc.tok_pos).line);
                }
            }
            true
        });
    });
    out
}

fn unparen(expr: &Expr) -> &Expr {
    match expr {
        Expr::ParenExpr(p) => unparen(&p.x),
        other => other,
    }
}

fn collect_used_objs_in_stmt(pass: &Pass<'_>, stmt: &Stmt, out: &mut HashSet<ObjectId>) {
    preorder(match stmt {
        Stmt::AssignStmt(a) => NodeRef::AssignStmt(a),
        Stmt::IncDecStmt(i) => NodeRef::IncDecStmt(i),
        Stmt::ExprStmt(e) => NodeRef::ExprStmt(e),
        Stmt::DeclStmt(d) => NodeRef::DeclStmt(d),
        _ => return,
    }, |n| {
        if let NodeRef::Ident(id) = n {
            if let Some(obj) = object_of(pass, id) {
                out.insert(obj);
            }
        }
        true
    });
}

fn objs_assigned_in_stmt(pass: &Pass<'_>, stmt: &Stmt) -> HashSet<ObjectId> {
    let mut out = HashSet::new();
    match stmt {
        Stmt::AssignStmt(AssignStmt { lhs, .. }) => {
            for e in lhs {
                if let Expr::Ident(id) = e {
                    if let Some(obj) = object_of(pass, id) {
                        out.insert(obj);
                    }
                }
            }
        }
        Stmt::IncDecStmt(inc) => {
            if let Expr::Ident(id) = unparen(&inc.x) {
                if let Some(obj) = object_of(pass, id) {
                    out.insert(obj);
                }
            }
        }
        Stmt::DeclStmt(d) => {
            if let guff::ast::Decl::GenDecl(gd) = &d.decl {
                for spec in &gd.specs {
                    if let guff::ast::Spec::ValueSpec(vs) = spec {
                        for name in &vs.names {
                            if let Some(obj) = object_of(pass, name) {
                                out.insert(obj);
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
    out
}

fn collect_used_objs(pass: &Pass<'_>, expr: &Expr, out: &mut HashSet<ObjectId>) {
    preorder(expr_ref(expr), |n| {
        if let NodeRef::Ident(id) = n {
            if let Some(obj) = object_of(pass, id) {
                out.insert(obj);
            }
        }
        true
    });
}

/// Locals **free** in a `FuncLit` / go-routine / defer body — stores look unused
/// in the enclosing function's NaiveForm SSA, but the closure reads them later
/// (traefik `bodySize = …; h.ServeHTTP` with `for range bodySize` in `next`).
///
/// "Free" is the whole point and it used to be missing: the walk collected
/// every `uses` entry in the literal's body, which includes the literal's *own*
/// locals — `x := 1; x = 2` inside a closure puts `x` in `uses` at the second
/// assignment. Since a wasted store always has a later mention of the variable,
/// that meant every local of every func literal was suppressed and
/// `wastedassign` reported nothing inside a closure at all. beats' four
/// remaining rows were all of them: a `client := <-await` and its `client = nil`
/// in a `testServer(func(…))`, a `p, err := …` in a `t.Run(func(…))`, and an
/// `offset += …` on a *parameter* of a returned literal.
///
/// A variable is free in literal `L` when its declaration is not inside `L` —
/// which covers the nesting case too: a local of an outer literal that only an
/// inner literal reads is free in the inner one, and the inner one's own pass
/// puts it in the set.
fn objs_captured_by_func_lits(pass: &Pass<'_>) -> HashSet<ObjectId> {
    let mut out = HashSet::new();
    let Some(info) = pass.types_info() else {
        return out;
    };
    let Some(inspect) = pass.result_of::<inspect::InspectResult>(inspect::analyzer()) else {
        return out;
    };
    inspect.preorder_typed(node_mask!(FuncLit), pass.files(), |n| {
        let NodeRef::FuncLit(fl) = n else {
            return;
        };
        // Everything the literal declares: its parameters and results, and
        // every `defs` entry anywhere in its body (nested literals included,
        // which is what keeps an inner literal's locals out of the outer
        // literal's free set).
        let mut declared_inside: HashSet<ObjectId> = HashSet::new();
        let mut note_def = |id: &guff::ast::Ident| {
            if let Some(Some(obj)) = info.defs.get(&id.id) {
                declared_inside.insert(*obj);
            }
        };
        for field in fl
            .ty
            .params
            .as_ref()
            .map(|f| f.list.as_slice())
            .unwrap_or_default()
            .iter()
            .chain(
                fl.ty
                    .results
                    .as_ref()
                    .map(|f| f.list.as_slice())
                    .unwrap_or_default()
                    .iter(),
            )
        {
            for name in &field.names {
                note_def(name);
            }
        }
        preorder(NodeRef::BlockStmt(&fl.body), |n| {
            if let NodeRef::Ident(id) = n {
                note_def(id);
            }
            true
        });

        preorder(NodeRef::BlockStmt(&fl.body), |n| {
            let NodeRef::Ident(id) = n else {
                return true;
            };
            if info.uses.contains_key(&id.id) {
                if let Some(obj) = object_of(pass, id) {
                    if !declared_inside.contains(&obj) {
                        out.insert(obj);
                    }
                }
            }
            true
        });
    });
    out
}

/// Whether `obj` is read after `after_pos` before being overwritten.
///
/// Assignment LHS of `=` is a use in `Info.Uses`, not a `Defs` entry — treat
/// those (and IncDec) as redefinitions so `b = 1; b = 2; use(b)` stays wasted.
///
/// Assignments that live in the sibling branch of an `if`/`else` that also
/// contains `after_pos` are not redefinitions (caddy `stor, err = …` / `else {
/// stor = … }` then shared use after the merge).
/// The position past which no read can be reached from a store at `after_pos`.
///
/// The AST fallback below is positional, and a position says nothing about
/// reachability. beats' `libbeat/reader/debug` makeNullCheck writes
///
/// ```go
/// if idx <= 0 {
///     offset += int64(len(buf))
///     return false
/// }
/// fmt.Println(offset + int64(idx))
/// ```
///
/// — the `offset` on the last line sits *after* the store, so the fallback
/// called the store live, but nothing can get there from inside a block that
/// ends in `return`. Any enclosing block whose last statement is a `return`
/// placed after the store cuts the function off at that block's end, and the
/// tightest such cut wins. Only `return` qualifies: `break` and `continue`
/// leave a loop but stay in the function, and `goto` jumps to a live label.
fn unreachable_use_cutoff(pass: &Pass<'_>, after_pos: u32) -> Option<u32> {
    let mut cutoff: Option<u32> = None;
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            let NodeRef::BlockStmt(b) = n else {
                return true;
            };
            let (start, end) = (b.pos().0 as u32, b.end().0 as u32);
            if after_pos < start || after_pos >= end {
                return true;
            }
            let Some(Stmt::ReturnStmt(r)) = b.list.last() else {
                return true;
            };
            if (r.return_.0 as u32) <= after_pos {
                return true;
            }
            cutoff = Some(cutoff.map_or(end, |c: u32| c.min(end)));
            true
        });
    }
    cutoff
}

fn ast_value_is_read_before_redef(pass: &Pass<'_>, obj: ObjectId, after_pos: u32) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let unreachable_after = unreachable_use_cutoff(pass, after_pos);
    let sibling_defs = sibling_branch_assign_positions(pass, obj, after_pos);
    let mut next_use: Option<u32> = None;
    let mut next_def: Option<u32> = None;
    // The right-hand side of the assignment being reported is evaluated
    // *before* its store, so a mention of the variable there is not a later
    // read. Positions alone cannot tell: in `s = strings.TrimSpace(s)` the
    // operand sits to the right of the store's own position, and counting it
    // silenced every self-referencing assignment — a parameter normalised on
    // entry and then unused (beats `libbeat/kibana/index_pattern_generator.go:39`)
    // among them. go/ssa orders the Load before the Store and upstream never
    // has to ask.
    let mut own_rhs: Option<(u32, u32)> = None;

    let note_def = |pos: u32, next_def: &mut Option<u32>| {
        if pos > after_pos && !sibling_defs.contains(&pos) {
            *next_def = Some(next_def.map_or(pos, |d| d.min(pos)));
        }
    };
    let note_use = |pos: u32, next_use: &mut Option<u32>| {
        if pos > after_pos && !unreachable_after.is_some_and(|c| pos >= c) {
            *next_use = Some(next_use.map_or(pos, |u| u.min(pos)));
        }
    };

    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::AssignStmt(a) => {
                    if own_rhs.is_none()
                        && a.lhs.iter().any(|e| {
                            matches!(e, Expr::Ident(id) if id.name_pos.0 as u32 == after_pos)
                        })
                    {
                        if let (Some(first), Some(last)) = (a.rhs.first(), a.rhs.last()) {
                            own_rhs = Some((first.pos().0 as u32, last.end().0 as u32));
                        }
                    }
                    for lhs in &a.lhs {
                        if let Expr::Ident(id) = lhs {
                            if object_of(pass, id) == Some(obj) {
                                note_def(id.name_pos.0 as u32, &mut next_def);
                            }
                        }
                    }
                }
                NodeRef::IncDecStmt(inc) => {
                    if let Expr::Ident(id) = unparen(&inc.x) {
                        if object_of(pass, id) == Some(obj) {
                            let pos = id.name_pos.0 as u32;
                            // IncDec both reads the old value and defines a new one.
                            note_use(pos, &mut next_use);
                            note_def(pos, &mut next_def);
                        }
                    }
                }
                NodeRef::Ident(id) => {
                    let pos = id.name_pos.0 as u32;
                    if pos <= after_pos || object_of(pass, id) != Some(obj) {
                        return true;
                    }
                    if own_rhs.is_some_and(|(start, end)| pos >= start && pos < end) {
                        return true;
                    }
                    if info.defs.get(&id.id).and_then(|d| *d) == Some(obj) {
                        note_def(pos, &mut next_def);
                    } else if info.uses.contains_key(&id.id) {
                        note_use(pos, &mut next_use);
                    }
                }
                _ => {}
            }
            true
        });
    }
    match (next_use, next_def) {
        (Some(u), Some(d)) => u < d,
        (Some(_), None) => true,
        _ => false,
    }
}

/// Positions of assigns to `obj` that sit in the sibling `if`/`else` branch of
/// the branch containing `after_pos`.
fn sibling_branch_assign_positions(
    pass: &Pass<'_>,
    obj: ObjectId,
    after_pos: u32,
) -> HashSet<u32> {
    let mut out = HashSet::new();
    let Some(inspect) = pass.result_of::<inspect::InspectResult>(inspect::analyzer()) else {
        return out;
    };
    inspect.preorder_typed(node_mask!(IfStmt), pass.files(), |n| {
        let NodeRef::IfStmt(ifs) = n else {
            return;
        };
        let Some(else_stmt) = ifs.else_.as_deref() else {
            return;
        };
        let in_then = pos_in_node(NodeRef::BlockStmt(&ifs.body), after_pos)
            || ifs
                .init
                .as_deref()
                .is_some_and(|s| pos_in_node(stmt_ref(s), after_pos));
        let in_else = pos_in_node(stmt_ref(else_stmt), after_pos);
        if in_then == in_else {
            return;
        }
        let sibling = if in_then {
            stmt_ref(else_stmt)
        } else {
            NodeRef::BlockStmt(&ifs.body)
        };
        collect_assign_positions(pass, sibling, obj, &mut out);
    });
    out
}

fn pos_in_node(root: NodeRef<'_>, pos: u32) -> bool {
    let mut lo = u32::MAX;
    let mut hi = 0u32;
    preorder(root, |n| {
        if let NodeRef::Ident(id) = n {
            let p = id.name_pos.0 as u32;
            lo = lo.min(p);
            hi = hi.max(p);
        }
        true
    });
    lo != u32::MAX && pos >= lo && pos <= hi
}

fn stmt_ref(stmt: &Stmt) -> NodeRef<'_> {
    match stmt {
        Stmt::AssignStmt(a) => NodeRef::AssignStmt(a),
        Stmt::BadStmt(b) => NodeRef::BadStmt(b),
        Stmt::BlockStmt(b) => NodeRef::BlockStmt(b),
        Stmt::BranchStmt(b) => NodeRef::BranchStmt(b),
        Stmt::CaseClause(c) => NodeRef::CaseClause(c),
        Stmt::CommClause(c) => NodeRef::CommClause(c),
        Stmt::DeclStmt(d) => NodeRef::DeclStmt(d),
        Stmt::DeferStmt(d) => NodeRef::DeferStmt(d),
        Stmt::EmptyStmt(e) => NodeRef::EmptyStmt(e),
        Stmt::ExprStmt(e) => NodeRef::ExprStmt(e),
        Stmt::ForStmt(f) => NodeRef::ForStmt(f),
        Stmt::GoStmt(g) => NodeRef::GoStmt(g),
        Stmt::IfStmt(i) => NodeRef::IfStmt(i),
        Stmt::IncDecStmt(i) => NodeRef::IncDecStmt(i),
        Stmt::LabeledStmt(l) => NodeRef::LabeledStmt(l),
        Stmt::RangeStmt(r) => NodeRef::RangeStmt(r),
        Stmt::ReturnStmt(r) => NodeRef::ReturnStmt(r),
        Stmt::SelectStmt(s) => NodeRef::SelectStmt(s),
        Stmt::SendStmt(s) => NodeRef::SendStmt(s),
        Stmt::SwitchStmt(s) => NodeRef::SwitchStmt(s),
        Stmt::TypeSwitchStmt(s) => NodeRef::TypeSwitchStmt(s),
    }
}

fn collect_assign_positions(
    pass: &Pass<'_>,
    root: NodeRef<'_>,
    obj: ObjectId,
    out: &mut HashSet<u32>,
) {
    preorder(root, |n| {
        if let NodeRef::AssignStmt(a) = n {
            for lhs in &a.lhs {
                if let Expr::Ident(id) = lhs {
                    if object_of(pass, id) == Some(obj) {
                        out.insert(id.name_pos.0 as u32);
                    }
                }
            }
        }
        true
    });
}

/// Resolve the `ObjectId` for a NaiveForm local Alloc named `comment` near `pos`.
fn local_obj_for_alloc(pass: &Pass<'_>, comment: &str, near_pos: u32) -> Option<ObjectId> {
    if comment.is_empty() || comment == "." {
        return None;
    }
    let info = pass.types_info()?;
    let mut best: Option<(u32, ObjectId)> = None;
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            let NodeRef::Ident(id) = n else {
                return true;
            };
            if id.name != comment {
                return true;
            }
            let pos = id.name_pos.0 as u32;
            let Some(Some(obj)) = info.defs.get(&id.id) else {
                return true;
            };
            // Prefer the def at/before the store; among those, the closest.
            if pos <= near_pos {
                best = Some(match best {
                    Some((bp, _)) if pos > bp => (pos, *obj),
                    Some(other) => other,
                    None => (pos, *obj),
                });
            }
            true
        });
    }
    best.map(|(_, o)| o)
}

fn op_in_locals(locals: &[InstrId], op: Value) -> bool {
    let Value::Instr(id) = op else {
        return false;
    };
    locals.contains(&id)
}


fn rm_same_block(succs: &[BlockId], current: BlockId) -> Vec<BlockId> {
    succs.iter().copied().filter(|&b| b != current).collect()
}

fn contain_reassigned_soon(ws: &[WastedReason]) -> bool {
    ws.iter().any(|&w| w == WastedReason::ReassignedSoon)
}

fn instr_uses_value(func: &Function, iid: InstrId, current: Value) -> bool {
    let mut found = false;
    func.instrs.get(iid).for_each_operand(|op| {
        if *op == current {
            found = true;
        }
    });
    found
}

fn is_next_operation_to_op_is_store(
    func: &Function,
    blocks: &[(BlockId, Option<&[InstrId]>)],
    current_op: Value,
    have_checked: &mut HashMap<i32, u8>,
) -> WastedReason {
    let mut wasted_reasons = Vec::new();
    let mut wasted_reasons_current = Vec::new();

    for &(bid, instr_override) in blocks {
        let block = func.blocks.get(bid);
        let idx = block.index;
        if have_checked.get(&idx) == Some(&2) {
            continue;
        }
        *have_checked.entry(idx).or_insert(0) += 1;

        let instrs = instr_override.unwrap_or(&block.instrs);
        let mut break_flag = false;
        for &iid in instrs {
            if break_flag {
                break;
            }
            match func.instrs.get(iid) {
                InstrData::Store(store) => {
                    if instr_uses_value(func, iid, current_op) {
                        if store.addr == current_op {
                            wasted_reasons_current.push(WastedReason::ReassignedSoon);
                            break_flag = true;
                            break;
                        }
                        return WastedReason::NotWasted;
                    }
                }
                _ => {
                    if instr_uses_value(func, iid, current_op) {
                        return WastedReason::NotWasted;
                    }
                }
            }
        }

        if !block.succs.is_empty() && !break_flag {
            // Upstream hands the first frame a *copy* of the block
            // (`blCopy := *bl`, then `[]*ssa.BasicBlock{&blCopy}`), and
            // `rmSameBlock` compares successors against it by pointer. A copy
            // is not any block in the CFG, so nothing is filtered on that
            // first hop — a block that is its own successor is walked again.
            //
            // That is load-bearing. `optimizeBlocks` fuses `rangeint.loop`
            // into `rangeint.body`, so a `for i := range n` loop is a single
            // block that jumps to itself, and the read at the top of the next
            // iteration lives in that same block. Filtering the self-edge here
            // — which guff did, by passing the real `BlockId` — hid it, and
            // the last assignment in such a loop looked dead. k6
            // `lib/execution_segment_test.go` and `lib/executor/ramping_vus_test.go`
            // are two of them; go/ssa and guff agree on the CFG, byte for byte.
            //
            // Deeper frames pass the real block and do filter, as upstream does.
            let succs: Vec<(BlockId, Option<&[InstrId]>)> = if instr_override.is_some() {
                block.succs.iter().copied().map(|b| (b, None)).collect()
            } else {
                rm_same_block(&block.succs, bid)
                    .into_iter()
                    .map(|b| (b, None))
                    .collect()
            };
            let reason =
                is_next_operation_to_op_is_store(func, &succs, current_op, have_checked);
            if reason == WastedReason::NotWasted {
                return WastedReason::NotWasted;
            }
            wasted_reasons.push(reason);
        }
    }

    wasted_reasons.extend(wasted_reasons_current);
    if !wasted_reasons.is_empty() && contain_reassigned_soon(&wasted_reasons) {
        return WastedReason::ReassignedSoon;
    }
    WastedReason::NoUseUntilReturn
}


/// What go/ssa does with `x = <composite literal>`, which decides whether there
/// is a store to report and where it sits.
///
/// `compLit` writes an array or struct literal *into the address* elementwise,
/// so no `Store` exists and upstream's `opInLocals` loop never sees one. A
/// slice or map literal is built as a value first and then stored, and that
/// `Store` carries the literal's `Lbrace` — not the assignment's `=`.
///
/// Measured on 2026-09-21: `x := []int{1}` reports at the `{` in column 12,
/// `x := map[string]int{"a": 1}` at the `{` in column 21, and `x := [3]int{…}`
/// and `x := E{}` report nothing at all.
enum CompositeRhs {
    /// Written into the address: there is no `Store`.
    NoStore,
    /// Stored, at the literal's `Lbrace`.
    StoredAt(u32),
}

/// The composite-literal right-hand side of the assignment whose store sits at
/// `after`, if that is what it is.
fn composite_lit_rhs(pass: &Pass<'_>, after: u32) -> Option<CompositeRhs> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let mut found = None;
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if found.is_some() {
                return false;
            }
            let NodeRef::AssignStmt(a) = n else {
                return true;
            };
            // Only a 1:1 assignment pairs an LHS name with an RHS expression; a
            // multi-value call has one RHS for several names.
            if a.lhs.len() != a.rhs.len() {
                return true;
            }
            let Some(i) = a.lhs.iter().position(|e| {
                matches!(e, Expr::Ident(id) if id.name_pos.0 as u32 == after)
            }) else {
                return true;
            };
            let Expr::CompositeLit(lit) = unparen(&a.rhs[i]) else {
                return false;
            };
            let Some(tav) = info.types.get(&lit.id) else {
                return false;
            };
            let under = tav.typ.underlying(&artifacts.types);
            found = Some(match artifacts.types.get(under) {
                TypeData::Struct(_) | TypeData::Array(_) => CompositeRhs::NoStore,
                _ => CompositeRhs::StoredAt(lit.lbrace.0 as u32),
            });
            false
        });
        if found.is_some() {
            break;
        }
    }
    found
}

fn check_func(
    func: &Function,
    type_switch_lines: &HashSet<i64>,
    if_init_cond_reads: &HashSet<u32>,
    captured: &HashSet<ObjectId>,
    loop_incdec_lines: &HashSet<i64>,
    pass: &Pass<'_>,
    out: &mut Vec<(u32, String)>,
) {
    let fset = pass.fset().as_ref();
    for (bid, block) in func.live_blocks() {
        for &iid in &block.instrs {
            let InstrData::Store(_) = func.instrs.get(iid) else {
                continue;
            };
            let pos_in_block = block
                .instrs
                .iter()
                .position(|&id| id == iid)
                .unwrap_or(block.instrs.len());
            let bl_copy = block.instrs[pos_in_block + 1..].to_vec();
            let start = [(bid, Some(bl_copy.as_slice()))];

            let InstrData::Store(store) = func.instrs.get(iid) else {
                continue;
            };
            if !op_in_locals(&func.locals, store.addr) {
                continue;
            }
            let op = store.addr;
            let reason =
                is_next_operation_to_op_is_store(func, &start, op, &mut HashMap::new());
            if reason == WastedReason::NotWasted {
                continue;
            }

            let pos = func.pos(iid);
            if !pos.is_valid() {
                continue;
            }
            let line = fset.position(pos).line;
            if type_switch_lines.contains(&line) {
                continue;
            }
            if loop_incdec_lines.contains(&line) {
                continue;
            }

            let Value::Instr(alloc_id) = op else {
                continue;
            };
            let InstrData::Alloc(Alloc { comment, .. }) = func.instrs.get(alloc_id) else {
                continue;
            };

            let after = pos.0 as u32;
            // Only report stores to real Go locals. Synthetic NaiveForm temps
            // (`rangeint.iter`, `complit`, …) have Alloc comments that do not
            // map to an ObjectId — upstream wastedassign never sees them.
            let Some(obj) = local_obj_for_alloc(pass, comment, after) else {
                continue;
            };
            if captured.contains(&obj) {
                continue;
            }
            // AST fallback: NaiveForm often never Loads locals used via register-
            // lifted Extracts (if-init cond, type-assert receivers, etc.).
            if if_init_cond_reads.contains(&after)
                || ast_value_is_read_before_redef(pass, obj, after)
            {
                continue;
            }

            let report_at = match composite_lit_rhs(pass, after) {
                Some(CompositeRhs::NoStore) => continue,
                Some(CompositeRhs::StoredAt(lbrace)) => lbrace,
                None => after,
            };
            if let Some(msg) = format_reason(reason, comment) {
                out.push((report_at, msg));
            }
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass.pkg().ill_typed {
        return Ok(None);
    }
    let artifacts = pass
        .pkg()
        .type_artifacts
        .as_ref()
        .ok_or_else(|| "wastedassign requires type artifacts (load with types mode)".to_string())?
        .snapshot();
    let built = build_package_for_analysis(
        artifacts,
        pass.files(),
        pass.fset().clone(),
        BuilderMode::NAIVE_FORM,
        Default::default(),
    )
    .map_err(|e| format!("wastedassign: {e}"))?;

    let type_switch_lines = collect_type_switch_lines(pass);
    let if_init_cond_reads = if_init_cond_read_stores(pass);
    let captured = objs_captured_by_func_lits(pass);
    let loop_incdec_lines = for_loop_var_body_incdec_lines(pass);
    let mut reports = Vec::new();
    // Upstream's `srcFuncs` is every named function in the package's AST —
    // package-level *and* methods. Members alone leave out every method in the
    // package, which is the blind spot `collect_src_funcs_with_methods` was
    // written for (SA4006 once reported `x := f(); x = g(); return x` in a
    // function and stayed silent on the identical body with a receiver on it).
    // wastedassign carried its own members-only copy and had the same hole:
    // beats' `(FileTree).getByComponents` is a method, so none of its stores
    // were ever looked at.
    let src_funcs = collect_src_funcs_with_methods(&built.prog, built.pkg);
    for fid in src_funcs {
        let func = built.prog.functions.get(fid);
        check_func(
            func,
            &type_switch_lines,
            &if_init_cond_reads,
            &captured,
            &loop_incdec_lines,
            pass,
            &mut reports,
        );
    }

    for (pos, message) in reports {
        if pos == 0 {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

/// The `wastedassign` analyzer.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "wastedassign",
        doc: "Finds wasted assignment statements.",
        url: "https://github.com/sanposhiho/wastedassign",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
