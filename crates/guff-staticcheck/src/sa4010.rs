//! SA4010 — result of append will never be observed.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4010`.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::Expr;
use guff::position::Pos;
use guff::walk::{expr_ref, preorder, NodeRef};
use guff_analysis::passes::buildir;
use guff_analysis::referrers;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_ssa::function::Function;
use guff_ssa::ids::InstrId;
use guff_ssa::instr::{Call, InstrData};
use guff_ssa::program::Program;
use guff_ssa::value::Value;

fn is_append(prog: &Program, func: &Function, iid: InstrId) -> bool {
    let InstrData::Call(Call { call, .. }) = func.instrs.get(iid) else {
        return false;
    };
    if call.method.is_some() {
        return false;
    }
    match call.value {
        Value::Builtin(b) => prog.builtins.get(b).name == "append",
        _ => false,
    }
}

/// True when the append result is only observed by Phi / further appends
/// (upstream `walkRefs`).
///
/// Upstream skips when `Referrers()` is nil. Empty referrers here are treated
/// the same (not proof of unused). Closed Phi/append-only graphs that miss a
/// real Return use are suppressed by [`ast_append_result_observed`].
fn append_result_unused(prog: &Program, func: &Function, append_iid: InstrId) -> bool {
    let refs = referrers(func, Value::Instr(append_iid));
    if refs.is_empty() {
        return false;
    }
    let mut is_used = false;
    let mut visited = HashSet::new();
    let mut stack: Vec<InstrId> = refs.to_vec();
    while let Some(rid) = stack.pop() {
        if !visited.insert(rid) {
            continue;
        }
        match func.instrs.get(rid) {
            InstrData::DebugRef(_) => {}
            InstrData::Phi(_) => {
                stack.extend(referrers(func, Value::Instr(rid)).iter().copied());
            }
            other if other.is_value() => {
                if is_append(prog, func, rid) {
                    stack.extend(referrers(func, Value::Instr(rid)).iter().copied());
                } else {
                    is_used = true;
                    break;
                }
            }
            _ => {
                is_used = true;
                break;
            }
        }
    }
    !is_used
}

fn validate_argument(
    prog: &Program,
    func: &Function,
    v: Value,
    seen: &mut HashSet<Value>,
) -> bool {
    if !seen.insert(v) {
        return true;
    }
    match v {
        Value::Const(_) => true,
        Value::Instr(iid) => match func.instrs.get(iid) {
            InstrData::Phi(p) => p
                .edges
                .iter()
                .flatten()
                .all(|&e| validate_argument(prog, func, e, seen)),
            InstrData::Slice(s) => validate_argument(prog, func, s.x, seen),
            InstrData::MakeSlice(_) | InstrData::Alloc(_) => true,
            InstrData::Call(_) if is_append(prog, func, iid) => {
                let InstrData::Call(Call { call, .. }) = func.instrs.get(iid) else {
                    return false;
                };
                call.args
                    .first()
                    .copied()
                    .is_some_and(|a| validate_argument(prog, func, a, seen))
            }
            _ => false,
        },
        _ => false,
    }
}

fn validate_referrers(func: &Function, v: Value, seen: &mut HashSet<InstrId>) -> bool {
    for &rid in referrers(func, v) {
        if !seen.insert(rid) {
            continue;
        }
        match func.instrs.get(rid) {
            InstrData::Phi(_)
            | InstrData::Slice(_)
            | InstrData::MakeSlice(_)
            | InstrData::Alloc(_)
            | InstrData::DebugRef(_) => {}
            _ => return false,
        }
        if func.instrs.get(rid).is_value()
            && !validate_referrers(func, Value::Instr(rid), seen)
        {
            return false;
        }
    }
    true
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let ir = pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .ok_or_else(|| "SA4010 requires buildir analyzer".to_string())?;
    let mut pending: Vec<(u32, String)> = Vec::new();
    for &fid in ir.src_funcs_with_methods() {
        let func = ir.prog.functions.get(fid);
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                if !is_append(&ir.prog, func, iid) {
                    continue;
                }
                if !append_result_unused(&ir.prog, func, iid) {
                    continue;
                }
                let InstrData::Call(Call { call, .. }) = func.instrs.get(iid) else {
                    continue;
                };
                let Some(&arg0) = call.args.first() else {
                    continue;
                };
                let mut seen_args = HashSet::new();
                if !validate_argument(&ir.prog, func, arg0, &mut seen_args) {
                    continue;
                }
                let mut seen_refs: HashSet<InstrId> = seen_args
                    .iter()
                    .filter_map(|v| match v {
                        Value::Instr(i) => Some(*i),
                        _ => None,
                    })
                    .collect();
                seen_refs.insert(iid);
                let mut ok = true;
                for v in &seen_args {
                    if !validate_referrers(func, *v, &mut seen_refs) {
                        ok = false;
                        break;
                    }
                }
                if !ok {
                    continue;
                }
                let pos = func.pos(iid).0 as u32;
                if ast_append_result_observed(pass, pos) {
                    continue;
                }
                pending.push((
                    pos,
                    "this result of append is never used, except maybe in other appends".into(),
                ));
            }
        }
    }
    // Remap only if we have findings: `call_node_starts` walks the AST.
    let call_starts = (!pending.is_empty())
        .then(|| guff_analysis::call_node_starts(pass))
        .unwrap_or_default();
    for (pos, msg) in pending {
        pass.reportf(call_starts.get(&pos).copied().unwrap_or(pos), msg);
    }
    Ok(None)
}

/// AST fallback when SSA misses a use of `x = append(x, …)` (e.g. switch +
/// shadowed locals). Match the append site by **line number** (avoids FileSet
/// absolute/relative offset mismatches), then look for a later observing use
/// of the same name in the enclosing function.
fn ast_append_result_observed(pass: &Pass<'_>, pos: u32) -> bool {
    let fset = pass.fset();
    let report_line = fset.position(Pos(pos as i64)).line;
    if report_line == 0 {
        return false;
    }

    for file in pass.files() {
        let mut hit: Option<(String, u32, u32)> = None; // name, assign_pos, func_end
        preorder(NodeRef::File(file), |n| {
            if hit.is_some() {
                return false;
            }
            let NodeRef::FuncDecl(fd) = n else {
                return true;
            };
            let Some(body) = &fd.body else {
                return true;
            };
            let func_end = body.rbrace.0 as u32;
            // Walk without nested preorder (return false would abort the outer walk).
            let mut stack = vec![NodeRef::BlockStmt(body)];
            while let Some(node) = stack.pop() {
                if hit.is_some() {
                    break;
                }
                if let NodeRef::AssignStmt(a) = node {
                    if a.lhs.len() == 1 && a.rhs.len() == 1 {
                        if let Expr::CallExpr(call) = &a.rhs[0] {
                            if let Expr::Ident(fun) = call.fun.as_ref() {
                                if fun.name == "append" {
                                    let call_line = fset.position(fun.name_pos).line;
                                    if call_line == report_line {
                                        if let Expr::Ident(lhs) = &a.lhs[0] {
                                            if lhs.name != "_" {
                                                hit = Some((
                                                    lhs.name.clone(),
                                                    lhs.name_pos.0 as u32,
                                                    func_end,
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                guff::walk::for_each_child(node, |c| stack.push(c));
            }
            true
        });
        let Some((name, assign_pos, func_end)) = hit else {
            continue;
        };

        let mut later_defs = HashSet::new();
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::AssignStmt(a) = n {
                for lhs in &a.lhs {
                    if let Expr::Ident(id) = lhs {
                        if id.name == name {
                            let p = id.name_pos.0 as u32;
                            if p > assign_pos && p < func_end {
                                later_defs.insert(p);
                            }
                        }
                    }
                }
            }
            true
        });

        let mut ignore_pos = HashSet::new();
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::CallExpr(call) = n {
                if let Expr::Ident(fun) = call.fun.as_ref() {
                    if fun.name == "append" {
                        if let Some(arg0) = call.args.first() {
                            preorder(expr_ref(arg0), |n| {
                                if let NodeRef::Ident(id) = n {
                                    if id.name == name {
                                        ignore_pos.insert(id.name_pos.0 as u32);
                                    }
                                }
                                true
                            });
                        }
                    }
                }
            }
            true
        });

        let mut observed = false;
        preorder(NodeRef::File(file), |n| {
            if observed {
                return false;
            }
            if let NodeRef::Ident(id) = n {
                let p = id.name_pos.0 as u32;
                if id.name == name
                    && p > assign_pos
                    && p < func_end
                    && !later_defs.contains(&p)
                    && !ignore_pos.contains(&p)
                {
                    observed = true;
                }
            }
            true
        });
        if observed {
            return true;
        }
    }
    false
}

fn sa4010_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4010",
        doc: "the result of append will never be observed anywhere",
        url: "https://staticcheck.dev/docs/checks/#SA4010",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4010_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4010_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
