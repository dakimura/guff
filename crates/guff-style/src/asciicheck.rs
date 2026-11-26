//! Port of [`github.com/golangci/asciicheck`](https://github.com/golangci/asciicheck)
//! (formerly `github.com/tdakkota/asciicheck`).

use std::sync::OnceLock;

use guff::ast::{FieldList, Ident};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn first_non_ascii(s: &str) -> Option<char> {
    s.chars().find(|c| *c > '\u{007F}')
}

fn format_rune(ch: char) -> String {
    format!("U+{:04X} '{}'", u32::from(ch), ch)
}

fn check_ident(ident: Option<&Ident>, pending: &mut Vec<(u32, String)>) {
    let Some(ident) = ident else {
        return;
    };
    let Some(ch) = first_non_ascii(&ident.name) else {
        return;
    };
    pending.push((
        ident.name_pos.0 as u32,
        format!(
            "identifier {:?} contain non-ASCII character: {}",
            ident.name,
            format_rune(ch)
        ),
    ));
}

fn check_field_list(fields: Option<&FieldList>, pending: &mut Vec<(u32, String)>) {
    let Some(fields) = fields else {
        return;
    };
    for field in &fields.list {
        for name in &field.names {
            check_ident(Some(name), pending);
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "asciicheck requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::File(f) => check_ident(Some(&f.name), &mut pending),
                NodeRef::ImportSpec(sp) => check_ident(sp.name.as_ref(), &mut pending),
                NodeRef::TypeSpec(sp) => {
                    check_ident(Some(&sp.name), &mut pending);
                    check_field_list(sp.type_params.as_ref(), &mut pending);
                }
                NodeRef::ValueSpec(sp) => {
                    for name in &sp.names {
                        check_ident(Some(name), &mut pending);
                    }
                }
                NodeRef::FuncDecl(f) => {
                    check_ident(Some(&f.name), &mut pending);
                    check_field_list(f.recv.as_ref(), &mut pending);
                }
                NodeRef::StructType(s) => check_field_list(Some(&s.fields), &mut pending),
                NodeRef::FuncType(ty) => {
                    check_field_list(ty.type_params.as_ref(), &mut pending);
                    check_field_list(ty.params.as_ref(), &mut pending);
                    check_field_list(ty.results.as_ref(), &mut pending);
                }
                NodeRef::InterfaceType(i) => check_field_list(Some(&i.methods), &mut pending),
                NodeRef::LabeledStmt(s) => check_ident(Some(&s.label), &mut pending),
                NodeRef::AssignStmt(a) if a.tok == Some(Token::DEFINE) => {
                    for expr in &a.lhs {
                        if let guff::ast::Expr::Ident(id) = expr {
                            check_ident(Some(id), &mut pending);
                        }
                    }
                }
                _ => {}
            }
            true
        });
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "asciicheck",
        doc: "checks that all code identifiers do not have non-ASCII symbols in the name",
        url: "https://github.com/golangci/asciicheck",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
