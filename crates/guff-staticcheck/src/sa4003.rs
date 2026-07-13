//! SA4003 — comparing unsigned values against negative values is pointless
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4003`.

use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};


use guff::ast::{BinaryExpr, Expr};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::is_integer_literal;

use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;
use guff_types::typestring::type_string;

fn is_unsigned(pass: &Pass<'_>, expr: &Expr) -> Option<(String, bool)> {
    let info = pass.types_info()?;
    let tav = info.types.get(&expr.id())?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let u = tav.typ.underlying(&artifacts.types);
    let TypeData::Basic(b) = artifacts.types.get(u) else { return None };
    let unsigned = matches!(b.kind(), BasicKind::Uint | BasicKind::Uint8 | BasicKind::Uint16 | BasicKind::Uint32 | BasicKind::Uint64 | BasicKind::Uintptr);
    let name = type_string(&artifacts.types, &artifacts.objects, &artifacts.packages, tav.typ, None);
    Some((name, unsigned))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4003 requires inspect analyzer".to_string())?
        .clone();
    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::BinaryExpr(bin) = node else { return };
        let Some((tname, unsigned)) = is_unsigned(pass, &bin.x) else { return };
        if !unsigned { return; }
        let is_zero = |e: &Expr| is_integer_literal(pass, e, 0);
        let pos = bin.op_pos.0 as u32;
        match bin.op {
            Token::LSS if is_zero(&bin.y) => pending.push((pos, format!("no value of type {tname} is less than 0"))),
            Token::GTR if is_zero(&bin.x) => pending.push((pos, format!("no value of type {tname} is less than 0"))),
            Token::GEQ if is_zero(&bin.y) => pending.push((pos, format!("every value of type {tname} is >= 0"))),
            Token::LEQ if is_zero(&bin.x) => pending.push((pos, format!("every value of type {tname} is >= 0"))),
            _ => {}
        }
    });
    for (pos, msg) in pending { pass.report_unless_generated(pos, msg); }
    Ok(None)
}


fn sa4003_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4003",
        doc: "comparing unsigned values against negative values is pointless",
        url: "https://staticcheck.dev/docs/checks/#SA4003",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4003_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4003_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
