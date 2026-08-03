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
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Diagnostic, RunError, RunFn, Pass};
use guff_ssa::function::Function;
use guff_ssa::ids::{BlockId, FuncId, InstrId, PackageId};
use guff_ssa::instr::{Alloc, InstrData};
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_ssa::ssautil::build_package_for_analysis;
use guff_ssa::value::Value;
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

fn collect_src_funcs(prog: &Program, pkg: PackageId) -> Vec<FuncId> {
    let mut funcs = Vec::new();
    let ssa_pkg = prog.packages.get(pkg);
    // Sort by member name so FxHash map order cannot reorder analyzer walks
    // (PERF_TASKS_V2 §0-12 / §A-1).
    let mut top: Vec<(&str, FuncId)> = ssa_pkg
        .members
        .iter()
        .filter_map(|(name, m)| match m {
            MemberData::Function(fid) => Some((name.as_str(), *fid)),
            _ => None,
        })
        .collect();
    top.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (_, fid) in top {
        funcs.push(fid);
        collect_anon_funcs(prog, fid, &mut funcs);
    }
    funcs
}

fn collect_anon_funcs(prog: &Program, fid: FuncId, out: &mut Vec<FuncId>) {
    let anon = prog.functions.get(fid).anon_funcs.clone();
    for child in anon {
        out.push(child);
        collect_anon_funcs(prog, child, out);
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
fn if_init_objs_used_in_cond(pass: &Pass<'_>) -> HashSet<ObjectId> {
    let mut out = HashSet::new();
    let Some(inspect) = pass.result_of::<inspect::InspectResult>(inspect::analyzer()) else {
        return out;
    };
    inspect.preorder_typed(node_mask!(IfStmt), pass.files(), |n| {
        let NodeRef::IfStmt(ifs) = n else {
            return;
        };
        let Some(init) = ifs.init.as_deref() else {
            return;
        };
        let assigned = objs_assigned_in_stmt(pass, init);
        if assigned.is_empty() {
            return;
        }
        let mut used = HashSet::new();
        collect_used_objs(pass, &ifs.cond, &mut used);
        for obj in assigned.intersection(&used) {
            out.insert(*obj);
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

/// Whether `obj` is read after `after_pos` before being overwritten.
///
/// Assignment LHS of `=` is a use in `Info.Uses`, not a `Defs` entry — treat
/// those (and IncDec) as redefinitions so `b = 1; b = 2; use(b)` stays wasted.
///
/// Assignments that live in the sibling branch of an `if`/`else` that also
/// contains `after_pos` are not redefinitions (caddy `stor, err = …` / `else {
/// stor = … }` then shared use after the merge).
fn ast_value_is_read_before_redef(pass: &Pass<'_>, obj: ObjectId, after_pos: u32) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let sibling_defs = sibling_branch_assign_positions(pass, obj, after_pos);
    let mut next_use: Option<u32> = None;
    let mut next_def: Option<u32> = None;

    let note_def = |pos: u32, next_def: &mut Option<u32>| {
        if pos > after_pos && !sibling_defs.contains(&pos) {
            *next_def = Some(next_def.map_or(pos, |d| d.min(pos)));
        }
    };
    let note_use = |pos: u32, next_use: &mut Option<u32>| {
        if pos > after_pos {
            *next_use = Some(next_use.map_or(pos, |u| u.min(pos)));
        }
    };

    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::AssignStmt(a) => {
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
            let succs: Vec<(BlockId, Option<&[InstrId]>)> = rm_same_block(&block.succs, bid)
                .into_iter()
                .map(|b| (b, None))
                .collect();
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

fn check_func(
    func: &Function,
    type_switch_lines: &HashSet<i64>,
    if_init_used: &HashSet<ObjectId>,
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
            // AST fallback: NaiveForm often never Loads locals used via register-
            // lifted Extracts (if-init cond, type-assert receivers, etc.).
            if let Some(obj) = local_obj_for_alloc(pass, comment, after) {
                if if_init_used.contains(&obj) || ast_value_is_read_before_redef(pass, obj, after)
                {
                    continue;
                }
            }

            if let Some(msg) = format_reason(reason, comment) {
                out.push((after, msg));
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
    )
    .map_err(|e| format!("wastedassign: {e}"))?;

    let type_switch_lines = collect_type_switch_lines(pass);
    let if_init_used = if_init_objs_used_in_cond(pass);
    let loop_incdec_lines = for_loop_var_body_incdec_lines(pass);
    let mut reports = Vec::new();
    let src_funcs = collect_src_funcs(&built.prog, built.pkg);
    for fid in src_funcs {
        let func = built.prog.functions.get(fid);
        check_func(
            func,
            &type_switch_lines,
            &if_init_used,
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
