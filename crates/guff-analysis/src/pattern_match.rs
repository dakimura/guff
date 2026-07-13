//! Pattern matching helpers (`code.Matches` / `code.Match`).

use guff::walk::NodeRef;
use guff_pattern::{match_node, MatchEnv, Matcher, Pattern};

use crate::pass::Pass;
use crate::passes::inspect::InspectResult;

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
pub fn matches<F>(pass: &Pass<'_>, inspect: &InspectResult, pat: &Pattern, mut f: F)
where
    F: FnMut(NodeRef<'_>, Matcher<'_>) -> bool,
{
    let kinds: std::collections::HashSet<_> = pat.entry_kinds.iter().copied().collect();
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
