//! S1025 — don't use `fmt.Sprintf("%s", x)` unnecessarily.
//!
//! Port of `honnef.co/go/tools/simple/s1025`.

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::walk::NodeRef;
use guff_analysis::code::{expr_to_string, is_call_to, type_with_name};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::basic::BasicKind;
use guff_types::{Basic, TypeData, TypeId};

fn expr_type(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    pass.types_info()?.types.get(&expr.id()).map(|tv| tv.typ)
}

fn is_string_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let types = &artifacts.types;
    matches!(
        types.get(typ),
        TypeData::Basic(b) if b.kind() == BasicKind::String
    )
}

fn underlying_is_string(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    is_string_type(pass, typ.underlying(&artifacts.types))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1025 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if !is_call_to(pass, call, "fmt.Sprintf") || call.args.len() != 2 {
            return;
        };
        let Some(s) = expr_to_string(pass, &call.args[0]) else {
            return;
        };
        if s != "%s" {
            return;
        }
        let Some(typ) = expr_type(pass, &call.args[1]) else {
            return;
        };
        if type_with_name(pass, typ, "reflect.Value") {
            return;
        }

        let msg = if type_with_name(pass, typ, "fmt.Stringer") {
            "should use String() instead of fmt.Sprintf"
        } else if is_string_type(pass, typ) {
            "the argument is already a string, there's no need to use fmt.Sprintf"
        } else if underlying_is_string(pass, typ) {
            "the argument's underlying type is a string, should use a simple conversion instead of fmt.Sprintf"
        } else {
            return;
        };
        pending.push((match_pos(node), msg.into()));
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1025_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1025",
        doc: "don't use fmt.Sprintf(\"%s\", x) unnecessarily",
        url: "https://staticcheck.dev/docs/checks/#S1025",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1025_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1025_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
