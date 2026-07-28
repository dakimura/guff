//! `slog` — check log/slog key-value argument structure.

use std::sync::OnceLock;

use guff::ast::CallExpr;
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::ObjectData;
use guff_types::basic::BasicKind;

use crate::govet_util::{
    expr_type, is_empty_interface, is_type_named, method_expr_call, static_callee,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum PosKind {
    Key,
    Value,
    Unknown,
}

fn is_attr_type(pass: &Pass<'_>, typ: guff_types::TypeId) -> bool {
    is_type_named(pass, typ, "log/slog", "Attr")
}

fn kv_func_skip_args(pass: &Pass<'_>, obj: guff_types::ObjectId) -> Option<usize> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let ObjectData::Func(_) = artifacts.objects.get(obj) else {
        return None;
    };
    let pkg = obj.pkg(&artifacts.objects)?;
    if artifacts.packages.get(pkg).path() != "log/slog" {
        return None;
    }
    let name = obj.name(&artifacts.objects);
    let recv = obj
        .typ(&artifacts.objects)
        .and_then(|sig| guff_types::signature::signature_recv(&artifacts.types, sig))
        .and_then(|r| r.typ(&artifacts.objects))
        .and_then(|t| {
            if is_type_named(pass, t, "log/slog", "Logger") {
                Some("Logger")
            } else if is_type_named(pass, t, "log/slog", "Record") {
                Some("Record")
            } else {
                None
            }
        })
        .unwrap_or("");
    let table: &[(&str, &str, usize)] = &[
        ("", "Debug", 1),
        ("", "Info", 1),
        ("", "Warn", 1),
        ("", "Error", 1),
        ("", "DebugContext", 2),
        ("", "InfoContext", 2),
        ("", "WarnContext", 2),
        ("", "ErrorContext", 2),
        ("", "Log", 3),
        ("", "Group", 1),
        ("Logger", "Debug", 1),
        ("Logger", "Info", 1),
        ("Logger", "Warn", 1),
        ("Logger", "Error", 1),
        ("Logger", "DebugContext", 2),
        ("Logger", "InfoContext", 2),
        ("Logger", "WarnContext", 2),
        ("Logger", "ErrorContext", 2),
        ("Logger", "Log", 3),
        ("Logger", "With", 0),
        ("Record", "Add", 0),
    ];
    table
        .iter()
        .find(|(r, n, _)| *r == recv && *n == name)
        .map(|(_, _, skip)| *skip)
}

fn is_string_type(pass: &Pass<'_>, typ: guff_types::TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    matches!(
        artifacts.types.get(typ.underlying(&artifacts.types)),
        guff_types::arena::TypeData::Basic(b) if b.kind() == BasicKind::String
    )
}

fn check_call(pass: &Pass<'_>, call: &CallExpr) -> Option<(u32, String)> {
    if call.ellipsis.is_valid() {
        return None;
    }
    let obj = static_callee(pass, call)?;
    let mut skip = kv_func_skip_args(pass, obj)?;
    if method_expr_call(pass, call) {
        skip += 1;
    }
    if call.args.len() <= skip {
        return None;
    }
    let mut pos = PosKind::Key;
    for arg in call.args.iter().skip(skip) {
        let Some(typ) = expr_type(pass, arg) else {
            return None;
        };
        match pos {
            PosKind::Key => {
                if is_string_type(pass, typ) {
                    pos = PosKind::Value;
                } else if is_attr_type(pass, typ) {
                    pos = PosKind::Key;
                } else if is_empty_interface(pass, typ) {
                    pos = PosKind::Unknown;
                } else {
                    return Some((
                        arg.pos().0 as u32,
                        "slog argument should be a string or a slog.Attr (possible missing key or value)".into(),
                    ));
                }
            }
            PosKind::Value => pos = PosKind::Key,
            PosKind::Unknown => {
                if !is_string_type(pass, typ)
                    && !is_attr_type(pass, typ)
                    && !is_empty_interface(pass, typ)
                {
                    pos = PosKind::Key;
                }
            }
        }
    }
    if pos == PosKind::Value {
        return Some((
            call.pos().0 as u32,
            "call to slog missing a final value".into(),
        ));
    }
    None
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "slog requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if let Some((pos, msg)) = check_call(pass, call) {
            pending.push((pos, msg));
        }
    });
    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "slog",
        doc: "check log/slog key-value argument structure",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/slog",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
