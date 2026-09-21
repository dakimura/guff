//! ST1016 — use consistent method receiver names.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1016`.
//! AST-based (upstream uses buildir + IntuitiveMethodSet); methods declared
//! in this package are grouped by receiver type name, which naturally skips
//! embedded methods from other types.
//!
//! The one thing the AST does not hand over is the *order*. Upstream reports at
//! `firstFn`, the first entry of `typeutil.IntuitiveMethodSet(T)`, and that is
//! not source order: "the order of the result is as for `types.MethodSet(T)`",
//! which `NewMethodSet` sorts by `obj.Id()` — the method name for an exported
//! method, `pkgpath + "." + name` for an unexported one. So a type whose
//! methods read `Zeta`, `Apex`, `Middle` is reported at `Apex`, wherever it
//! sits, and in whichever file of the package.
//!
//! `firstFn` is also set *before* upstream looks at the receiver's name, so a
//! method written `func (T) Apex()` or `func (_ T) Apex()` can carry the
//! diagnostic even though it contributes nothing to the `seen` counts.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use guff::ast::Expr;
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::is_generated_at;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

/// The method that upstream's `firstFn` lands on: the smallest `Id` wins.
#[derive(Clone)]
struct MethodId {
    id: String,
    pos: u32,
}

/// `types.Id`: an exported name stands alone, an unexported one is qualified
/// with the package path so that two packages' `foo` methods never collide.
/// (A package with no path spells that prefix `_`, which is what the type
/// checker does for the Universe scope.)
fn type_id(pkg_path: &str, name: &str) -> String {
    if is_exported(name) {
        return name.to_string();
    }
    let path = if pkg_path.is_empty() { "_" } else { pkg_path };
    format!("{path}.{name}")
}

/// `token.IsExported`: the first rune is an upper-case letter.
fn is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

fn recv_type_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StarExpr(s) => recv_type_name(&s.x),
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::IndexExpr(i) => recv_type_name(&i.x), // generic `T[K]`
        Expr::IndexListExpr(i) => recv_type_name(&i.x),
        Expr::ParenExpr(p) => recv_type_name(&p.x),
        _ => None,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ST1016 requires inspect analyzer".to_string())?
        .clone();

    // type name → (the method that sorts first by Id, counts of receiver names)
    let mut by_type: BTreeMap<String, (MethodId, BTreeMap<String, usize>)> = BTreeMap::new();
    let pkg_path = pass.pkg().pkg_path.clone();

    inspect.preorder_typed(node_mask!(FuncDecl), pass.files(), |node| {
        let NodeRef::FuncDecl(fd) = node else {
            return;
        };
        let Some(recv) = &fd.recv else {
            return;
        };
        let Some(field) = recv.list.first() else {
            return;
        };
        let Some(ty) = &field.ty else {
            return;
        };
        let Some(type_name) = recv_type_name(ty) else {
            return;
        };
        let recv_name = field.names.first().map(|n| n.name.as_str()).unwrap_or("");
        // `code.IsGenerated(pass, recv.Pos())`. An unnamed receiver's `Var` has
        // the type's position, which is what the fallback here spells.
        let recv_pos = field
            .names
            .first()
            .map(|n| n.pos().0 as u32)
            .unwrap_or_else(|| ty.pos().0 as u32);
        if is_generated_at(pass, recv_pos) {
            return;
        }

        // Upstream reports at the first method's *name*, not its receiver.
        let candidate = MethodId {
            id: type_id(&pkg_path, &fd.name.name),
            pos: fd.name.pos().0 as u32,
        };
        let entry = by_type
            .entry(type_name)
            .or_insert_with(|| (candidate.clone(), BTreeMap::new()));
        if candidate.id < entry.0.id {
            entry.0 = candidate;
        }
        // A receiver that is unnamed or `_` still competes for the position
        // above, but upstream does not count it among the names seen.
        if recv_name.is_empty() || recv_name == "_" {
            return;
        }
        *entry.1.entry(recv_name.to_string()).or_insert(0) += 1;
    });

    let mut pending = Vec::new();
    for (_ty, (first, names)) in by_type {
        if names.len() <= 1 {
            continue;
        }
        let mut seen: Vec<String> = names
            .iter()
            .map(|(name, count)| format!("{count}x {name:?}"))
            .collect();
        seen.sort();
        pending.push((
            first.pos,
            format!(
                "methods on the same type should have the same receiver name (seen {})",
                seen.join(", ")
            ),
        ));
    }

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn st1016_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1016",
        doc: "use consistent method receiver names",
        url: "https://staticcheck.dev/docs/checks/#ST1016",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1016_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1016_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
