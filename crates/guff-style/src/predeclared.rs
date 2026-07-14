//! Port of [`github.com/nishanths/predeclared`](https://github.com/nishanths/predeclared)
//! (golangci-lint wrapper in `pkg/golinters/predeclared`).
//!
//! Defaults match golangci-lint: `qualified=false` (methods/fields skipped),
//! empty ignore list.
//!
//! DEFERRED: `linters.settings.predeclared` wiring (`ignore`, `q`/`qualified`).

use std::sync::OnceLock;

use guff::ast::{Ident, Spec};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

/// Go `doc.IsPredeclared` identifier set.
fn is_predeclared(name: &str) -> bool {
    matches!(
        name,
        // types
        "any"
            | "bool"
            | "byte"
            | "comparable"
            | "complex64"
            | "complex128"
            | "error"
            | "float32"
            | "float64"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "rune"
            | "string"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uintptr"
            // funcs
            | "append"
            | "cap"
            | "clear"
            | "close"
            | "complex"
            | "copy"
            | "delete"
            | "imag"
            | "len"
            | "make"
            | "max"
            | "min"
            | "new"
            | "panic"
            | "print"
            | "println"
            | "real"
            | "recover"
            // constants
            | "false"
            | "iota"
            | "nil"
            | "true"
    )
}

fn maybe_report(ident: &Ident, kind: &str, pending: &mut Vec<(u32, String)>) {
    if !is_predeclared(&ident.name) {
        return;
    }
    pending.push((
        ident.name_pos.0 as u32,
        format!("{kind} {} has same name as predeclared identifier", ident.name),
    ));
}

fn check_field_names(fields: Option<&guff::ast::FieldList>, kind: &str, pending: &mut Vec<(u32, String)>) {
    let Some(fields) = fields else {
        return;
    };
    for field in &fields.list {
        for name in &field.names {
            maybe_report(name, kind, pending);
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "predeclared requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    for file in pass.files() {
        maybe_report(&file.name, "package name", &mut pending);

        for imp in &file.imports {
            if let Some(name) = &imp.name {
                maybe_report(name, "import name", &mut pending);
            }
        }

        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::GenDecl(d) => {
                    let kind = match d.tok {
                        Some(Token::CONST) => "const",
                        Some(Token::VAR) => "variable",
                        _ => return true,
                    };
                    for spec in &d.specs {
                        if let Spec::ValueSpec(vs) = spec {
                            for name in &vs.names {
                                maybe_report(name, kind, &mut pending);
                            }
                        }
                    }
                }
                NodeRef::TypeSpec(sp) => maybe_report(&sp.name, "type", &mut pending),
                NodeRef::FuncDecl(f) => {
                    if f.recv.is_none() {
                        maybe_report(&f.name, "function", &mut pending);
                    }
                    // DEFERRED (qualified=true): method names.
                    check_field_names(f.recv.as_ref(), "receiver", &mut pending);
                }
                NodeRef::FuncType(ty) => {
                    check_field_names(ty.params.as_ref(), "param", &mut pending);
                    check_field_names(ty.results.as_ref(), "named return", &mut pending);
                }
                NodeRef::LabeledStmt(s) => maybe_report(&s.label, "label", &mut pending),
                NodeRef::AssignStmt(a) if a.tok == Some(Token::DEFINE) => {
                    for expr in &a.lhs {
                        if let guff::ast::Expr::Ident(id) = expr {
                            maybe_report(id, "variable", &mut pending);
                        }
                    }
                }
                // DEFERRED (qualified=true): struct fields / interface methods.
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
        name: "predeclared",
        doc: "find code that shadows one of Go's predeclared identifiers",
        url: "https://github.com/nishanths/predeclared",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
