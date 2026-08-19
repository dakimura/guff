//! SA6001 — map indexing with `string(key)` should inline conversion.
//!
//! AST simplification of `honnef.co/go/tools/staticcheck/sa6001`.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, Expr, Ident, IndexExpr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::{ObjectId, TypeData};
use guff_types::basic::BasicKind;

fn is_byte_slice_type(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(tav) = info.types.get(&expr.id()) else {
        return false;
    };
    let TypeData::Slice(s) = artifacts.types.get(tav.typ.underlying(&artifacts.types)) else {
        return false;
    };
    let elem = s.elem().underlying(&artifacts.types);
    matches!(
        artifacts.types.get(elem),
        TypeData::Basic(b) if b.kind() == BasicKind::Uint8
    )
}

fn is_string_convert(call: &CallExpr) -> bool {
    matches!(&*call.fun, Expr::Ident(Ident { name, .. }) if name == "string")
}

/// Everything the per-candidate test needs, collected in one pass.
///
/// Two things keep this off the hot path. Nothing here runs unless the package
/// actually contains a `k := string(b)` — most do not — and the walks only
/// account for the handful of objects those assignments bind, rather than
/// every identifier in the package. Walking all of them cost prometheus
/// `./...` about 0.7s of a 2.0s run.
struct Uses {
    /// Node ids of identifiers that index a map for *reading*.
    map_read_index_idents: HashSet<u32>,
    /// How many times each candidate key is mentioned at all.
    mentions: HashMap<ObjectId, usize>,
    /// How many of those mentions are map reads.
    map_reads: HashMap<ObjectId, usize>,
}

fn collect_uses(
    pass: &Pass<'_>,
    inspect: &inspect::InspectResult,
    keys: &HashSet<ObjectId>,
) -> Uses {
    // An index expression on the left of an assignment is a map *write*, and
    // `m[k]++` writes too. In upstream's IR those are `MapUpdate`, which lands
    // on the `default:` arm that abandons the conversion.
    let mut written: HashSet<u32> = HashSet::new();
    inspect.preorder_typed(
        node_mask!(AssignStmt, IncDecStmt),
        pass.files(),
        |n| match n {
            NodeRef::AssignStmt(a) => {
                for lhs in &a.lhs {
                    if let Expr::IndexExpr(ix) = lhs {
                        written.insert(ix.id);
                    }
                }
            }
            NodeRef::IncDecStmt(st) => {
                if let Expr::IndexExpr(ix) = &st.x {
                    written.insert(ix.id);
                }
            }
            _ => {}
        },
    );

    let mut map_read_index_idents: HashSet<u32> = HashSet::new();
    let mut map_reads: HashMap<ObjectId, usize> = HashMap::new();
    inspect.preorder_typed(node_mask!(IndexExpr), pass.files(), |n| {
        let NodeRef::IndexExpr(ix) = n else {
            return;
        };
        if written.contains(&ix.id) {
            return;
        }
        let Expr::Ident(id) = ix.index.as_ref() else {
            return;
        };
        let Some(obj) = object_of(pass, id) else {
            return;
        };
        if !keys.contains(&obj) {
            return;
        }
        let (Some(info), Some(artifacts)) = (pass.types_info(), pass.pkg().type_artifacts.as_ref())
        else {
            return;
        };
        let is_map = info.types.get(&ix.x.id()).is_some_and(|t| {
            matches!(
                artifacts.types.get(t.typ.underlying(&artifacts.types)),
                TypeData::Map(_)
            )
        });
        if is_map {
            map_read_index_idents.insert(id.id);
            *map_reads.entry(obj).or_default() += 1;
        }
    });

    let mut mentions: HashMap<ObjectId, usize> = HashMap::new();
    inspect.preorder_typed(node_mask!(Ident), pass.files(), |n| {
        let NodeRef::Ident(id) = n else {
            return;
        };
        // `object_of` is a map lookup per identifier, so ask only about the
        // ones that can matter: an identifier that is not one of the keys has
        // a different name, and names are cheap to reject.
        if let Some(obj) = object_of(pass, id) {
            if keys.contains(&obj) {
                *mentions.entry(obj).or_default() += 1;
            }
        }
    });

    Uses {
        map_read_index_idents,
        mentions,
        map_reads,
    }
}

/// `k := string(b)` where `b` is a byte slice — the shape upstream's IR sees as
/// a `Convert` to string.
fn candidate<'a>(pass: &Pass<'_>, assign: &'a AssignStmt) -> Option<&'a Ident> {
    if assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return None;
    }
    let Expr::Ident(key) = &assign.lhs[0] else {
        return None;
    };
    let Expr::CallExpr(call) = &assign.rhs[0] else {
        return None;
    };
    if !is_string_convert(call) || call.args.len() != 1 || !is_byte_slice_type(pass, &call.args[0]) {
        return None;
    }
    Some(key)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA6001 requires inspect analyzer".to_string())?
        .clone();

    let mut candidates: Vec<(ObjectId, u32, u32)> = Vec::new();
    let mut keys: HashSet<ObjectId> = HashSet::new();
    inspect.preorder_typed(node_mask!(AssignStmt), pass.files(), |n| {
        let NodeRef::AssignStmt(assign) = n else {
            return;
        };
        let Some(key) = candidate(pass, assign) else {
            return;
        };
        let Some(obj) = object_of(pass, key) else {
            return;
        };
        keys.insert(obj);
        // Upstream reports the conversion instruction, whose source node is the
        // `string(key)` call — not the `:=`.
        candidates.push((obj, key.id, assign.rhs[0].pos().0 as u32));
    });
    if candidates.is_empty() {
        return Ok(None);
    }

    let uses = collect_uses(pass, &inspect, &keys);
    let mut pending = Vec::new();
    for (obj, key_id, pos) in candidates {
        // Every use of the converted string has to be a map *read* index.
        //
        // Upstream walks the IR referrers of the `string(b)` conversion and
        // accepts exactly two kinds: a `DebugRef` naming the identifier it was
        // bound to, and a `MapLookup`. Everything else — the `MapUpdate` of
        // `m[k] = v`, a call argument, a return — hits the `default:` arm and
        // abandons the conversion entirely. One lookup is enough; there is no
        // count threshold.
        //
        // This port only looked at `IndexExpr` nodes whose index is the key, so
        // a map *write* counted as a lookup and every other use of the variable
        // was invisible. gitea writes `attributesMap[filename] = attribute2info`
        // next to its read, and argo-cd both writes `paramSetsByMergeKey[k]` and
        // interpolates `k` into an error message; neither is a finding upstream.
        // The old `>= 2` threshold was a recall gap in the other direction.
        let reads = uses.map_reads.get(&obj).copied().unwrap_or(0);
        if reads == 0 {
            continue;
        }
        // The declaration itself plus the reads is the whole of it; anything
        // else is a use upstream refuses to see past. The bound identifier is a
        // mention too, and it is never a map index, so it is the `+ 1`.
        let mentions = uses.mentions.get(&obj).copied().unwrap_or(0);
        if mentions != reads + 1 || uses.map_read_index_idents.contains(&key_id) {
            continue;
        }
        pending.push(pos);
    }
    for pos in pending {
        pass.report_unless_generated(
            pos,
            "m[string(key)] would be more efficient than k := string(key); m[k]",
        );
    }
    Ok(None)
}

fn sa6001_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA6001",
        doc: "missing an optimization opportunity when indexing maps by byte slices",
        url: "https://staticcheck.dev/docs/checks/#SA6001",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa6001_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa6001_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
