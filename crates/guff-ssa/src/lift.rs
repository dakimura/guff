//! SSA lifting pass.
//!
//! Port of go/ssa's `lift.go`.

use crate::hash::{HashMap, HashSet};
use crate::program::Program;
use crate::function::Function;
use crate::ids::{BlockId, InstrId, FuncId};
use crate::value::Value;
use crate::instr::{InstrData, Phi};
use crate::dom::DomFrontier;
use crate::arena::ArenaId;
use guff::token::Token;

/// lift replaces local and new Allocs accessed only with load/store by SSA
/// registers, inserting φ-nodes where necessary.
///
/// Returns `true` if any Alloc was lifted (CFG/instrs changed). Callers can
/// skip a follow-up blockopt/dom pass when this returns `false`.
/// (Go: `lift`)
pub fn lift(prog: &mut Program, func_id: FuncId) -> bool {
    let f = prog.functions.get_mut(func_id);
    if f.blocks.is_empty() {
        return false;
    }

    f.compute_referrers();
    let df = DomFrontier::build(f);

    let mut instr_to_block = HashMap::default();
    for (id, block) in f.blocks.iter() {
        if block.deleted {
            continue;
        }
        for &instr_id in &block.instrs {
            instr_to_block.insert(instr_id, id);
        }
    }

    let mut new_phis = HashMap::default();
    let mut num_allocs = 0;

    // A deferred call can assign to a named result, so those cells stay
    // addressable in a function that defers — their stores must survive as
    // stores. (Go: `liftAlloc`'s `fn.Recover != nil` guard; honnef's IR spells
    // the same condition `fn.hasDefer`.) Without this, SA4006 reads the
    // overwritten value as dead and reports rclone's `startRc`.
    let has_defer = f.blocks.iter().any(|(_, b)| {
        !b.deleted
            && b.instrs
                .iter()
                .any(|&id| matches!(f.instrs.get(id), InstrData::Defer(_)))
    });

    // Determine which allocs we can lift and number them densely.
    let block_ids: Vec<_> = f
        .blocks
        .iter()
        .filter(|(_, b)| !b.deleted)
        .map(|(id, _)| id)
        .collect();
    for block_id in block_ids {
        let instrs = f.blocks.get(block_id).instrs.clone();
        for instr_id in instrs {
            if let InstrData::Alloc(_) = f.instrs.get(instr_id) {
                if has_defer && f.named_results.contains(&Value::Instr(instr_id)) {
                    continue;
                }
                if lift_alloc(f, &prog.type_arena, &df, instr_id, &instr_to_block, &mut new_phis) {
                    if let InstrData::Alloc(alloc) = f.instrs.get_mut(instr_id) {
                        alloc.index = num_allocs;
                        num_allocs += 1;
                    }
                }
            }
        }
    }

    if num_allocs == 0 {
        return false;
    }

    let mut renaming = vec![None; num_allocs as usize];
    let root_id = f.blocks.iter().next().map(|(id, _)| id).unwrap();
    rename(prog, func_id, root_id, &mut renaming, &new_phis);

    remove_dead_phis(prog, func_id, &mut new_phis);

    let f = prog.functions.get_mut(func_id);
    for (block_id, block) in f.blocks.iter_mut() {
        let nps = new_phis.get(&block_id);
        let mut new_instrs = Vec::new();
        if let Some(nps) = nps {
            for np in nps {
                new_instrs.push(np.phi_id);
            }
        }
        
        for &instr_id in &block.instrs {
            let instr = f.instrs.get(instr_id);
            match instr {
                InstrData::Alloc(a) if a.index >= 0 => continue,
                InstrData::Store(s) => {
                    if let Value::Instr(addr_id) = s.addr {
                        if let InstrData::Alloc(a) = f.instrs.get(addr_id) {
                            if a.index >= 0 { continue; }
                        }
                    }
                }
                InstrData::UnOp(u) if u.op == Token::MUL => {
                    if let Value::Instr(addr_id) = u.x {
                        if let InstrData::Alloc(a) = f.instrs.get(addr_id) {
                            if a.index >= 0 { continue; }
                        }
                    }
                }
                _ => {}
            }
            new_instrs.push(instr_id);
        }
        block.instrs = new_instrs;
    }

    f.locals.retain(|&id| {
        if let InstrData::Alloc(a) = f.instrs.get(id) {
            a.index < 0
        } else {
            true
        }
    });

    // Referrers still list lifted-away Stores/Loads; rebuild so analyzers
    // (e.g. SA4006 has_use) see post-lift use-def only.
    f.compute_referrers();
    true
}

struct NewPhi {
    phi_id: InstrId,
    alloc_id: InstrId,
}

fn lift_alloc(
    f: &mut Function,
    type_arena: &guff_types::TypeArena,
    df: &DomFrontier,
    alloc_id: InstrId,
    instr_to_block: &HashMap<InstrId, BlockId>,
    new_phis: &mut HashMap<BlockId, Vec<NewPhi>>,
) -> bool {
    let referrers = f.referrers.as_ref().unwrap().get(&Value::Instr(alloc_id));
    if referrers.is_none() {
        return true;
    }

    let mut def_blocks = HashSet::default();
    for &instr_id in referrers.unwrap() {
        let instr = f.instrs.get(instr_id);
        match instr {
            InstrData::Store(s) => {
                if s.val == Value::Instr(alloc_id) {
                    return false;
                }
                def_blocks.insert(*instr_to_block.get(&instr_id).expect("block not found"));
            }
            InstrData::UnOp(u) if u.op == Token::MUL => {}
            InstrData::DebugRef(_) => {}
            _ => return false,
        }
    }

    let alloc_block_id = *instr_to_block.get(&alloc_id).expect("alloc block not found");
    def_blocks.insert(alloc_block_id);

    let mut has_already = HashSet::default();
    let mut work = def_blocks.clone();
    let mut w: Vec<BlockId> = def_blocks.into_iter().collect();

    // The promoted register holds the *pointee* value, so the φ type is the
    // Alloc's pointee `T`, not its own `*T` value type. (Go: `deref(alloc.Type())`.)
    let alloc_typ = if let InstrData::Alloc(a) = f.instrs.get(alloc_id) {
        guff_types::pointer_elem(type_arena, a.typ)
    } else {
        panic!("not an alloc");
    };

    while let Some(u_id) = w.pop() {
        for &v_id in &df.frontier[u_id.index()] {
            if has_already.insert(v_id) {
                let v = f.blocks.get(v_id);
                let phi = Phi {
                    edges: vec![None; v.preds.len()],
                    comment: "".to_string(),
                    typ: alloc_typ,
                };
                let phi_id = f.instrs.alloc(InstrData::Phi(phi));
                new_phis.entry(v_id).or_default().push(NewPhi { phi_id, alloc_id });

                if work.insert(v_id) {
                    w.push(v_id);
                }
            }
        }
    }

    true
}

fn rename(
    prog: &mut Program,
    func_id: FuncId,
    u_id: BlockId,
    renaming: &mut Vec<Option<Value>>,
    new_phis: &HashMap<BlockId, Vec<NewPhi>>,
) {
    let f = prog.functions.get_mut(func_id);
    if let Some(nps) = new_phis.get(&u_id) {
        for np in nps {
            if let InstrData::Alloc(alloc) = f.instrs.get(np.alloc_id) {
                renaming[alloc.index as usize] = Some(Value::Instr(np.phi_id));
            }
        }
    }

    let instrs_to_process = f.blocks.get(u_id).instrs.clone();
    for instr_id in instrs_to_process {
        // We use a block here to drop the mutable borrow of f.instrs
        let action = {
            let f = prog.functions.get(func_id);
            match f.instrs.get(instr_id) {
                InstrData::Alloc(alloc) if alloc.index >= 0 => {
                    Some(RenameAction::KillAlloc(alloc.index))
                }
                InstrData::Store(store) => {
                    if let Value::Instr(addr_id) = store.addr {
                        if let InstrData::Alloc(alloc) = f.instrs.get(addr_id) {
                            if alloc.index >= 0 {
                                Some(RenameAction::Store(alloc.index, store.val))
                            } else { None }
                        } else { None }
                    } else { None }
                }
                InstrData::UnOp(unop) if unop.op == Token::MUL => {
                    if let Value::Instr(addr_id) = unop.x {
                        if let InstrData::Alloc(alloc) = f.instrs.get(addr_id) {
                            if alloc.index >= 0 {
                                Some(RenameAction::Load(alloc.index, unop.typ))
                            } else { None }
                        } else { None }
                    } else { None }
                }
                _ => None,
            }
        };

        match action {
            Some(RenameAction::KillAlloc(idx)) => {
                renaming[idx as usize] = None;
            }
            Some(RenameAction::Store(idx, val)) => {
                renaming[idx as usize] = Some(val);
            }
            Some(RenameAction::Load(idx, typ)) => {
                let new_val = renaming[idx as usize].unwrap_or_else(|| {
                    prog.emit_const(None, typ)
                });
                let f = prog.functions.get_mut(func_id);
                replace_all(f, Value::Instr(instr_id), new_val);
            }
            None => {}
        }
    }

    let succs = {
        let f = prog.functions.get(func_id);
        f.blocks.get(u_id).succs.clone()
    };
    for v_id in succs {
        if let Some(phis) = new_phis.get(&v_id) {
            let pred_idx = prog.functions.get(func_id).blocks.get(v_id).pred_index(u_id);
            for np in phis {
                let (idx, alloc_typ, phi_id) = {
                    let f = prog.functions.get(func_id);
                    if let InstrData::Alloc(alloc) = f.instrs.get(np.alloc_id) {
                        (alloc.index, alloc.typ, np.phi_id)
                    } else {
                        panic!("not an alloc");
                    }
                };
                // The zero value for a never-stored cell has the pointee type
                // `T`, not the Alloc's own `*T`. (Go: `zeroConst(deref(...))`.)
                let typ = guff_types::pointer_elem(&prog.type_arena, alloc_typ);
                let new_val = renaming[idx as usize].unwrap_or_else(|| {
                    prog.emit_const(None, typ)
                });
                let f = prog.functions.get_mut(func_id);
                if let InstrData::Phi(phi) = f.instrs.get_mut(phi_id) {
                    phi.edges[pred_idx] = Some(new_val);
                    f.referrers.as_mut().unwrap().entry(new_val).or_default().push(phi_id);
                }
            }
        }
    }

    let children = {
        let f = prog.functions.get(func_id);
        f.blocks.get(u_id).dom.children.clone()
    };
    for v_id in children {
        let mut r_copy = renaming.clone();
        rename(prog, func_id, v_id, &mut r_copy, new_phis);
    }
}

enum RenameAction {
    KillAlloc(i32),
    Store(i32, Value),
    Load(i32, guff_types::TypeId),
}

fn replace_all(f: &mut Function, x: Value, y: Value) {
    if let Some(mut referrers) = f.referrers.take() {
        if let Some(instr_ids) = referrers.remove(&x) {
            for instr_id in instr_ids {
                let instr = f.instrs.get_mut(instr_id);
                instr.for_each_operand_mut(|val| {
                    if *val == x {
                        *val = y;
                        referrers.entry(y).or_default().push(instr_id);
                    }
                });
            }
        }
        f.referrers = Some(referrers);
    }
}

fn remove_dead_phis(prog: &mut Program, func_id: FuncId, new_phis: &mut HashMap<BlockId, Vec<NewPhi>>) {
    let f = prog.functions.get_mut(func_id);
    let mut live_phis = HashSet::default();
    
    for nps in new_phis.values() {
        for np in nps {
            if !live_phis.contains(&np.phi_id) && phi_has_direct_referrer(f, np.phi_id) {
                mark_live_phi(f, &mut live_phis, np.phi_id);
            }
        }
    }
    
    for (_, block) in f.blocks.iter() {
        for &instr_id in &block.instrs {
            if let InstrData::Phi(_) = f.instrs.get(instr_id) {
                if !live_phis.contains(&instr_id) && phi_has_direct_referrer(f, instr_id) {
                    mark_live_phi(f, &mut live_phis, instr_id);
                }
            } else {
                break;
            }
        }
    }
    
    for nps in new_phis.values_mut() {
        nps.retain(|np| {
            if live_phis.contains(&np.phi_id) {
                true
            } else {
                let edges = if let InstrData::Phi(phi) = f.instrs.get(np.phi_id) {
                    phi.edges.clone()
                } else { vec![] };
                
                for edge in edges {
                    if let Some(val) = edge {
                        remove_referrer(f, val, np.phi_id);
                    }
                }
                false
            }
        });
    }
}

fn phi_has_direct_referrer(f: &Function, phi_id: InstrId) -> bool {
    if let Some(refs) = f.referrers.as_ref().unwrap().get(&Value::Instr(phi_id)) {
        for &instr_id in refs {
            if let InstrData::Phi(_) = f.instrs.get(instr_id) {
                continue;
            }
            return true;
        }
    }
    false
}

fn mark_live_phi(f: &Function, live_phis: &mut HashSet<InstrId>, phi_id: InstrId) {
    if !live_phis.insert(phi_id) {
        return;
    }
    if let InstrData::Phi(phi) = f.instrs.get(phi_id) {
        for edge in &phi.edges {
            if let Some(Value::Instr(v_instr_id)) = edge {
                if let InstrData::Phi(_) = f.instrs.get(*v_instr_id) {
                    mark_live_phi(f, live_phis, *v_instr_id);
                }
            }
        }
    }
}

fn remove_referrer(f: &mut Function, val: Value, instr_id: InstrId) {
    if let Some(referrers) = f.referrers.as_mut() {
        if let Some(instr_ids) = referrers.get_mut(&val) {
            instr_ids.retain(|&id| id != instr_id);
        }
    }
}
