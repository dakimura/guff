//! Pattern matching helpers (`code.Matches` / `code.Match`).

use std::collections::HashSet;

use guff::walk::NodeRef;
use guff_pattern::{match_node, IndexSymbol, MatchEnv, Matcher, Pattern};

use crate::pass::Pass;
use crate::passes::inspect::InspectResult;
use crate::passes::typeindex::{self, Index};

/// Build a [`MatchEnv`] from the current pass.
pub fn match_env<'a>(pass: &'a Pass<'_>) -> MatchEnv<'a> {
    let artifacts = pass.pkg().type_artifacts.as_ref();
    MatchEnv {
        types: pass.types_info(),
        type_arena: artifacts.map(|a| &a.types),
        objects: artifacts.map(|a| &a.objects),
        packages: artifacts.map(|a| &a.packages),
    }
}

/// Returns whether `pat` matches `node`.
pub fn match_pattern<'a>(
    pass: &'a Pass<'_>,
    pat: &Pattern,
    node: NodeRef<'a>,
) -> Option<Matcher<'a>> {
    match_node(match_env(pass), pat, node)
}

/// Visit AST nodes that may match `pat`, invoking `f` for each successful match.
///
/// When `pat.root_call_symbols` is non-empty and the pass has a [`typeindex`]
/// result, only call sites for those symbols are visited (port of Go
/// `code.Matches` typeindex fast path). Otherwise falls back to an
/// `entry_kinds`-filtered preorder walk.
pub fn matches<F>(pass: &Pass<'_>, inspect: &InspectResult, pat: &Pattern, mut f: F)
where
    F: FnMut(NodeRef<'_>, Matcher<'_>) -> bool,
{
    if !pat.root_call_symbols.is_empty() {
        if let Some(index) = pass.result_of::<Index>(typeindex::analyzer()) {
            if matches_via_typeindex(pass, inspect, index, pat, &mut f) {
                return;
            }
        }
    }

    let kinds: HashSet<_> = pat.entry_kinds.iter().copied().collect();
    let visit_all = kinds.is_empty();

    inspect.preorder(pass.files(), |node| {
        if !visit_all {
            let name = node.kind_name();
            if !kinds.contains(name) {
                return;
            }
        }
        if let Some(m) = match_pattern(pass, pat, node) {
            if !f(node, m) {
                return;
            }
        }
    });
}

/// Returns `true` if the typeindex path handled the pattern (even with zero hits).
fn matches_via_typeindex<F>(
    pass: &Pass<'_>,
    _inspect: &InspectResult,
    index: &Index,
    pat: &Pattern,
    f: &mut F,
) -> bool
where
    F: FnMut(NodeRef<'_>, Matcher<'_>) -> bool,
{
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };

    let mut call_ids: HashSet<u32> = HashSet::new();
    let mut any_resolved = false;
    for sym in &pat.root_call_symbols {
        let obj = resolve_index_symbol(index, artifacts, sym);
        if let Some(obj) = obj {
            any_resolved = true;
            for id in index.calls(obj) {
                call_ids.insert(*id);
            }
        }
    }
    // Mirror Go CouldMatchAny: if no symbol resolves in this package, skip.
    if !any_resolved {
        return true;
    }
    if call_ids.is_empty() {
        return true;
    }

    for file in pass.files() {
        guff::walk::preorder(NodeRef::File(file), |node| {
            let NodeRef::CallExpr(call) = node else {
                return true;
            };
            if !call_ids.contains(&call.id) {
                return true;
            }
            if let Some(m) = match_pattern(pass, pat, node) {
                if !f(node, m) {
                    return false;
                }
            }
            true
        });
    }
    true
}

fn resolve_index_symbol(
    index: &Index,
    artifacts: &guff_packages::TypecheckArtifacts,
    sym: &IndexSymbol,
) -> Option<guff_types::ObjectId> {
    if sym.typename.is_empty() {
        index.object(
            &artifacts.packages,
            &artifacts.scopes,
            &sym.path,
            &sym.ident,
        )
    } else {
        index.selection(
            &artifacts.types,
            &artifacts.objects,
            &artifacts.packages,
            &artifacts.scopes,
            &sym.path,
            &sym.typename,
            &sym.ident,
        )
    }
}

/// Returns the diagnostic position for a matched node.
pub fn match_pos(node: NodeRef<'_>) -> u32 {
    use guff::walk::NodeRef;
    match node {
        NodeRef::BinaryExpr(e) => e.op_pos.0 as u32,
        NodeRef::CallExpr(e) => e.lparen.0 as u32,
        NodeRef::AssignStmt(s) => s.tok_pos.0 as u32,
        NodeRef::RangeStmt(s) => s.for_.0 as u32,
        NodeRef::ForStmt(s) => s.for_.0 as u32,
        NodeRef::IfStmt(s) => s.if_.0 as u32,
        NodeRef::ReturnStmt(s) => s.return_.0 as u32,
        NodeRef::SliceExpr(e) => e.lbrack.0 as u32,
        NodeRef::UnaryExpr(e) => e.op_pos.0 as u32,
        NodeRef::TypeAssertExpr(e) => e.lparen.0 as u32,
        NodeRef::SelectStmt(s) => s.select_.0 as u32,
        NodeRef::BasicLit(l) => l.value_pos.0 as u32,
        NodeRef::Ident(i) => i.name_pos.0 as u32,
        NodeRef::CompositeLit(c) => c.lbrace.0 as u32,
        _ => 0,
    }
}
