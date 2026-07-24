//! Port of [`github.com/sanposhiho/wastedassign`](https://github.com/sanposhiho/wastedassign).
//!
//! Finds local variable assignments whose value is never read before the next
//! assignment or function exit. Builds NaiveForm SSA internally (upstream
//! requires it; shared `buildir` uses GlobalDebug instead).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Diagnostic, RunError, RunFn, Pass};
use guff_ssa::function::Function;
use guff_ssa::ids::{BlockId, InstrId};
use guff_ssa::instr::{Alloc, InstrData};
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_ssa::ssautil::build_package_for_analysis;
use guff_ssa::value::Value;
use guff_ssa::ids::{FuncId, PackageId};

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
    for member in ssa_pkg.members.values() {
        if let MemberData::Function(fid) = member {
            funcs.push(*fid);
            collect_anon_funcs(prog, *fid, &mut funcs);
        }
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
    inspect.preorder(pass.files(), |n| {
        if let NodeRef::TypeSwitchStmt(stmt) = n {
            lines.insert(fset.as_ref().position(stmt.switch).line);
        }
    });
    lines
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
    fset: &guff::position::FileSet,
    out: &mut Vec<(u32, String)>,
) {
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

            let Value::Instr(alloc_id) = op else {
                continue;
            };
            let InstrData::Alloc(Alloc { comment, .. }) = func.instrs.get(alloc_id) else {
                continue;
            };
            if let Some(msg) = format_reason(reason, comment) {
                out.push((pos.0 as u32, msg));
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
    let mut reports = Vec::new();
    let src_funcs = collect_src_funcs(&built.prog, built.pkg);
    for fid in src_funcs {
        let func = built.prog.functions.get(fid);
        check_func(func, &type_switch_lines, pass.fset().as_ref(), &mut reports);
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
