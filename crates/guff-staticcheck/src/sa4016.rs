//! SA4016 — certain bitwise operations with zero do nothing useful
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4016`.

use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};


use guff::ast::{Decl, Expr, GenDecl, Spec};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{is_integer_constant, is_integer_literal};
use guff_types::arena::ObjectId;

use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;
use crate::render::render_expr;

fn is_integer(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(tav) = info.types.get(&expr.id()) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let u = tav.typ.underlying(&artifacts.types);
    matches!(artifacts.types.get(u), TypeData::Basic(b) if matches!(b.kind(), BasicKind::Int | BasicKind::Int8 | BasicKind::Int16 | BasicKind::Int32 | BasicKind::Int64 | BasicKind::Uint | BasicKind::Uint8 | BasicKind::Uint16 | BasicKind::Uint32 | BasicKind::Uint64 | BasicKind::Uintptr))
}

/// Constants of this package declared as exactly `name = iota` — one name, one
/// value, and that value the identifier `iota`.
///
/// Upstream reaches the spec with `astutil.PathEnclosingInterval` from the
/// object's position (`sa4016.go:66-83`); collecting them once per package is
/// the same set. Membership also answers upstream's `obj.Pkg() != pass.Pkg`
/// guard, since only this package's files are walked.
fn iota_consts(pass: &Pass<'_>) -> std::collections::HashSet<ObjectId> {
    let mut out = std::collections::HashSet::new();
    let Some(info) = pass.types_info() else {
        return out;
    };
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::GenDecl(GenDecl { tok, specs, .. }) = decl else {
                continue;
            };
            if *tok != Some(Token::CONST) {
                continue;
            }
            for spec in specs {
                let Spec::ValueSpec(vs) = spec else { continue };
                // "TODO(dh): we could support this" — upstream declines a spec
                // that declares or assigns more than one thing.
                if vs.names.len() != 1 || vs.values.len() != 1 {
                    continue;
                }
                if !matches!(&vs.values[0], Expr::Ident(id) if id.name == "iota") {
                    continue;
                }
                if let Some(Some(obj)) = info.defs.get(&vs.names[0].id) {
                    out.insert(*obj);
                }
            }
        }
    }
    out
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4016 requires inspect analyzer".to_string())?
        .clone();
    let iota_zero = iota_consts(pass);
    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(BinaryExpr), pass.files(), |node| {
        let NodeRef::BinaryExpr(bin) = node else { return };
        if !matches!(bin.op, Token::AND | Token::OR | Token::XOR) { return; }
        if !is_integer(pass, &bin.x) { return; }
        // The right operand must evaluate to zero either way — the folded
        // constant, not a literal: upstream's two branches ask different
        // questions of it. The ident branch takes `constant.Int64Val(obj.Val())`
        // (so `flagA = iota` qualifies), and only the *else* branch asks
        // `code.IsIntegerLiteral`. Sharing the literal test across both would
        // drop the iota branch entirely.
        if !is_integer_constant(pass, &bin.y, 0) { return; }

        let rendered = render_expr(&Expr::BinaryExpr(bin.clone()));
        let y_rendered = render_expr(&bin.y);

        // Branch 1: `x | FLAG` where FLAG is one of *this* package's constants
        // written `FLAG = iota`. Upstream reads that as a likely mistake for
        // `1 << iota` and says so.
        let is_iota_const = matches!(&*bin.y, Expr::Ident(id) if pass
            .types_info()
            .and_then(|info| info.uses.get(&id.id).copied())
            .is_some_and(|obj| iota_zero.contains(&obj)));

        let msg = if is_iota_const {
            match bin.op {
                Token::AND => format!(
                    "{rendered} always equals 0; {y_rendered} is defined as iota and has value 0, \
                     maybe {y_rendered} is meant to be 1 << iota?"
                ),
                Token::OR | Token::XOR => format!(
                    "{rendered} always equals {}; {y_rendered} is defined as iota and has value 0, \
                     maybe {y_rendered} is meant to be 1 << iota?",
                    render_expr(&bin.x)
                ),
                _ => return,
            }
        } else if is_integer_literal(pass, &bin.y, 0) {
            match bin.op {
                Token::AND => format!("{rendered} always equals 0"),
                Token::OR | Token::XOR => {
                    format!("{rendered} always equals {}", render_expr(&bin.x))
                }
                _ => return,
            }
        } else {
            // A named constant that is zero for some other reason — syncthing's
            // `OptReadOnly = os.O_RDONLY`. Upstream's ident branch requires the
            // spec's value to be literally `iota`, and its literal branch
            // requires a literal, so neither fires and it stays silent.
            return;
        };
        // Upstream reports the BinaryExpr node; its position is the start of
        // the left operand, not the operator.
        pending.push((match_pos(node), msg));
    });
    for (pos, msg) in pending { pass.reportf(pos, msg); }
    Ok(None)
}


fn sa4016_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4016",
        doc: "certain bitwise operations with zero do nothing useful",
        url: "https://staticcheck.dev/docs/checks/#SA4016",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4016_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4016_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
