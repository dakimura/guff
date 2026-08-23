//! Port of [`github.com/breml/errchkjson`](https://github.com/breml/errchkjson).
//!
//! Defaults match golangci-lint: `omit-safe` (= `!check-error-free-encoding`) is
//! **true**, so unnecessary checks on safe encodings are not reported.
//! Settings: `linters.settings.errchkjson.check-error-free-encoding` /
//! `report-no-exported`.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, Expr, Ident};
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::arena::{ObjectData, TypeArena, TypeData, TypeId};
use guff_types::array::array_elem;
use guff_types::basic::{basic_info, basic_kind, BasicKind, IS_BOOLEAN, IS_COMPLEX, IS_INTEGER, IS_STRING};
use guff_types::lookup::{lookup_field_or_method, LookupResult};
use guff_types::map::{map_elem, map_key};
use guff_types::pointer::pointer_elem;
use guff_types::r#struct::{struct_field, struct_num_fields, struct_tag};
use guff_types::slice::slice_elem;
use guff_types::typestring::type_string;

use crate::util::{type_of, unparen};

/// Pass-time options from `linters.settings.errchkjson`.
///
/// golangci maps `check-error-free-encoding` → `omit-safe: !check-error-free-encoding`.
#[derive(Debug, Clone, Copy)]
pub struct ErrchkjsonOptions {
    /// When true, skip "checked but safe" reports (golangci default).
    pub omit_safe: bool,
    /// When true, report structs with no exported JSON fields.
    pub report_no_exported: bool,
}

impl Default for ErrchkjsonOptions {
    fn default() -> Self {
        Self {
            omit_safe: true,
            report_no_exported: false,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum ErrorTarget {
    BlankIdentifier,
    VariableAssignment,
    FunctionArgument,
}

enum JsonErr {
    Unsupported(String),
    NoExported,
    Unsafe(String),
}

impl std::fmt::Display for JsonErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JsonErr::Unsupported(s) | JsonErr::Unsafe(s) => write!(f, "{s}"),
            JsonErr::NoExported => write!(f, "struct does not export any field"),
        }
    }
}

fn json_tag_name(tag: &str) -> Option<&str> {
    // reflect.StructTag.Get("json") — tags are raw like `json:"name,omitempty" xml:"..."`.
    for part in tag.split_whitespace() {
        let Some((key, val)) = part.split_once(':') else {
            continue;
        };
        if key != "json" {
            continue;
        }
        let val = val.trim_matches('"');
        return Some(val.split(',').next().unwrap_or(val));
    }
    None
}

fn has_marshaler_method(pass: &Pass<'_>, typ: TypeId, name: &str) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    match lookup_field_or_method(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        true,
        None,
        name,
    ) {
        LookupResult::Found { obj, .. } => matches!(artifacts.objects.get(obj), ObjectData::Func(_)),
        _ => false,
    }
}

fn implements_text_or_json_marshaler(pass: &Pass<'_>, typ: TypeId) -> bool {
    has_marshaler_method(pass, typ, "MarshalJSON")
        || has_marshaler_method(pass, typ, "MarshalText")
}

fn type_name_string(pass: &Pass<'_>, typ: TypeId) -> String {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return "<unknown>".into();
    };
    type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    )
}

fn json_safe_map_key(pass: &Pass<'_>, typ: TypeId) -> Result<(), JsonErr> {
    if implements_text_or_json_marshaler(pass, typ) {
        return Err(JsonErr::Unsafe(format!(
            "unsafe type `{}` as map key found",
            type_name_string(pass, typ)
        )));
    }
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return Err(JsonErr::Unsafe("unsafe type as map key found".into()));
    };
    let ut = typ.underlying(&artifacts.types);
    match artifacts.types.get(ut) {
        TypeData::Basic(_) => {
            let info = basic_info(&artifacts.types, ut);
            if info.contains(IS_STRING)
                && type_name_string(pass, typ) == "encoding/json.Number"
            {
                return Err(JsonErr::Unsafe(format!(
                    "unsafe type `{}` as map key found",
                    type_name_string(pass, typ)
                )));
            }
            if info.contains(IS_INTEGER) || info.contains(IS_STRING) {
                return Ok(());
            }
            Err(JsonErr::Unsupported(format!(
                "unsupported type `{}` as map key found",
                type_name_string(pass, typ)
            )))
        }
        TypeData::Interface(_) => Err(JsonErr::Unsafe(format!(
            "unsafe type `{}` as map key found",
            type_name_string(pass, typ)
        ))),
        _ => Err(JsonErr::Unsupported(format!(
            "unsupported type `{}` as map key found",
            type_name_string(pass, typ)
        ))),
    }
}

fn json_safe(
    pass: &Pass<'_>,
    typ: TypeId,
    level: usize,
    seen: &mut HashSet<TypeId>,
    report_no_exported: bool,
) -> Result<(), JsonErr> {
    if !seen.insert(typ) {
        return Ok(());
    }
    if implements_text_or_json_marshaler(pass, typ) {
        return Err(JsonErr::Unsafe(format!(
            "unsafe type `{}` found",
            type_name_string(pass, typ)
        )));
    }
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return Err(JsonErr::Unsafe("unsafe type found".into()));
    };
    let ut = typ.underlying(&artifacts.types);
    match artifacts.types.get(ut) {
        TypeData::Basic(_) => {
            let info = basic_info(&artifacts.types, ut);
            if info.contains(IS_BOOLEAN) || info.contains(IS_INTEGER) || info.contains(IS_STRING) {
                if info.contains(IS_STRING)
                    && type_name_string(pass, typ) == "encoding/json.Number"
                {
                    return Err(JsonErr::Unsafe(format!(
                        "unsafe type `{}` found",
                        type_name_string(pass, typ)
                    )));
                }
                return Ok(());
            }
            if info.contains(IS_COMPLEX) {
                return Err(JsonErr::Unsupported(format!(
                    "unsupported type `{}` found",
                    basic_kind(&artifacts.types, ut).name_like()
                )));
            }
            match basic_kind(&artifacts.types, ut) {
                BasicKind::UntypedNil => Ok(()),
                BasicKind::UnsafePointer => Err(JsonErr::Unsupported(format!(
                    "unsupported type `{}` found",
                    type_name_string(pass, ut)
                ))),
                _ => Err(JsonErr::Unsafe(format!(
                    "unsafe type `{}` found",
                    type_name_string(pass, ut)
                ))),
            }
        }
        TypeData::Array(_) => json_safe(
            pass,
            array_elem(&artifacts.types, ut),
            level + 1,
            seen,
            report_no_exported,
        ),
        TypeData::Slice(_) => json_safe(
            pass,
            slice_elem(&artifacts.types, ut),
            level + 1,
            seen,
            report_no_exported,
        ),
        TypeData::Struct(_) => {
            let n = struct_num_fields(&artifacts.types, ut);
            let mut exported = 0usize;
            for i in 0..n {
                let field = struct_field(&artifacts.types, ut, i);
                if !field.exported(&artifacts.objects) {
                    continue;
                }
                let tag = struct_tag(&artifacts.types, ut, i);
                if json_tag_name(tag) == Some("-") {
                    continue;
                }
                let Some(ft) = field.typ(&artifacts.objects) else {
                    continue;
                };
                json_safe(pass, ft, level + 1, seen, report_no_exported)?;
                exported += 1;
            }
            if report_no_exported && level == 0 && exported == 0 {
                return Err(JsonErr::NoExported);
            }
            Ok(())
        }
        TypeData::Pointer(_) => json_safe(
            pass,
            pointer_elem(&artifacts.types, ut),
            level + 1,
            seen,
            report_no_exported,
        ),
        TypeData::Map(_) => {
            json_safe_map_key(pass, map_key(&artifacts.types, ut))?;
            json_safe(
                pass,
                map_elem(&artifacts.types, ut),
                level + 1,
                seen,
                report_no_exported,
            )
        }
        TypeData::Chan(_) | TypeData::Signature(_) => Err(JsonErr::Unsupported(format!(
            "unsupported type `{}` found",
            type_name_string(pass, ut)
        ))),
        _ => Err(JsonErr::Unsafe(format!(
            "unsafe type `{}` found",
            type_name_string(pass, typ)
        ))),
    }
}

trait BasicKindName {
    fn name_like(self) -> &'static str;
}

impl BasicKindName for BasicKind {
    fn name_like(self) -> &'static str {
        match self {
            BasicKind::Complex64 => "complex64",
            BasicKind::Complex128 => "complex128",
            BasicKind::UnsafePointer => "unsafe.Pointer",
            _ => "basic",
        }
    }
}

fn evaluate_error_target(n: &Expr) -> ErrorTarget {
    match unparen(n) {
        Expr::Ident(Ident { name, .. }) if name == "_" => ErrorTarget::BlankIdentifier,
        _ => ErrorTarget::VariableAssignment,
    }
}

fn peel_pointer(types: &TypeArena, typ: TypeId) -> TypeId {
    let typ = unalias_readonly(types, typ);
    match types.get(typ) {
        TypeData::Pointer(_) => pointer_elem(types, typ),
        _ => typ,
    }
}

fn handle_json_marshal(
    pass: &Pass<'_>,
    call: &CallExpr,
    fn_name: &str,
    error_target: ErrorTarget,
    omit_safe: bool,
    report_no_exported: bool,
    pending: &mut Vec<(u32, String)>,
) {
    let Some(arg0) = call.args.first() else {
        return;
    };
    let pos = call.pos().0 as u32;
    let Some(mut typ) = type_of(pass, arg0) else {
        if error_target == ErrorTarget::BlankIdentifier {
            pending.push((
                pos,
                format!(
                    "Type of argument to `{fn_name}` could not be evaluated and error return value is not checked"
                ),
            ));
        }
        return;
    };
    if let Some(artifacts) = pass.pkg().type_artifacts.as_ref() {
        typ = peel_pointer(&artifacts.types, typ);
    }

    let mut seen = HashSet::new();
    let err = json_safe(pass, typ, 0, &mut seen, report_no_exported);
    match &err {
        Err(JsonErr::Unsupported(msg)) => {
            pending.push((pos, format!("`{fn_name}` for {msg}")));
            return;
        }
        Err(JsonErr::NoExported) => {
            pending.push((
                pos,
                format!("Error argument passed to `{fn_name}` does not contain any exported field"),
            ));
        }
        Err(JsonErr::Unsafe(msg)) => {
            if error_target == ErrorTarget::BlankIdentifier {
                pending.push((
                    pos,
                    format!("Error return value of `{fn_name}` is not checked: {msg}"),
                ));
            }
        }
        Ok(()) => {}
    }
    if err.is_ok() && error_target == ErrorTarget::VariableAssignment && !omit_safe {
        pending.push((
            pos,
            format!("Error return value of `{fn_name}` is checked but passed argument is safe"),
        ));
    }
    if err.is_ok() && error_target == ErrorTarget::BlankIdentifier && omit_safe {
        pending.push((
            pos,
            format!("Error return value of `{fn_name}` is not checked"),
        ));
    }
}

/// Returns `(fn_name, force_omit_safe)`. Encode always forces omit-safe (upstream).
///
/// Upstream keys off `types.Func.FullName()`, so the Encoder arm below is
/// spelled with a receiver — which is why this must not use `code::call_name`:
/// that returns `encoding/json.Encode` for the method and the arm never
/// matched. Every `(*encoding/json.Encoder).Encode` in the corpus went
/// unreported (7 on syncthing alone) while the `Marshal` arms, being package
/// functions, looked perfectly healthy.
fn marshal_fn_name(pass: &Pass<'_>, call: &CallExpr) -> Option<(String, bool)> {
    let name = code::callee_full_name(pass, call)?;
    match name.as_str() {
        "encoding/json.Marshal" | "encoding/json.MarshalIndent" => Some((name, false)),
        "(*encoding/json.Encoder).Encode" => Some((name, true)),
        _ => None,
    }
}

fn inspect_args(
    pass: &Pass<'_>,
    args: &[Expr],
    options: ErrchkjsonOptions,
    pending: &mut Vec<(u32, String)>,
) {
    for a in args {
        // Use Inspect (not Preorder): false only prunes this subtree.
        walk::inspect(walk::expr_ref(a), |n| {
            let Some(NodeRef::CallExpr(call)) = n else {
                return true;
            };
            if let Some((fn_name, force_omit)) = marshal_fn_name(pass, call) {
                handle_json_marshal(
                    pass,
                    call,
                    &fn_name,
                    ErrorTarget::FunctionArgument,
                    force_omit || options.omit_safe,
                    options.report_no_exported,
                    pending,
                );
            } else {
                inspect_args(pass, &call.args, options, pending);
            }
            false
        });
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "errchkjson requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<ErrchkjsonOptions>("errchkjson")
        .copied()
        .unwrap_or_default();

    let mut pending = Vec::new();
    for file in pass.files() {
        // Upstream uses ast.Inspect: returning false skips children of the
        // current node only. walk::preorder would abort the whole file.
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::ReturnStmt(_) => false,
                NodeRef::CallExpr(call) => {
                    if let Some((fn_name, force_omit)) = marshal_fn_name(pass, call) {
                        handle_json_marshal(
                            pass,
                            call,
                            &fn_name,
                            ErrorTarget::BlankIdentifier,
                            force_omit || options.omit_safe,
                            options.report_no_exported,
                            &mut pending,
                        );
                    } else {
                        inspect_args(pass, &call.args, options, &mut pending);
                    }
                    false
                }
                NodeRef::AssignStmt(AssignStmt { lhs, rhs, .. }) => {
                    let Some(Expr::CallExpr(call)) = rhs.first().map(unparen) else {
                        return true;
                    };
                    let Some((fn_name, force_omit)) = marshal_fn_name(pass, call) else {
                        return true;
                    };
                    let target = if fn_name.ends_with(".Encode") {
                        lhs.first()
                            .map(evaluate_error_target)
                            .unwrap_or(ErrorTarget::VariableAssignment)
                    } else if lhs.len() >= 2 {
                        evaluate_error_target(&lhs[1])
                    } else {
                        ErrorTarget::BlankIdentifier
                    };
                    handle_json_marshal(
                        pass,
                        call,
                        &fn_name,
                        target,
                        force_omit || options.omit_safe,
                        options.report_no_exported,
                        &mut pending,
                    );
                    false
                }
                _ => true,
            }
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
        name: "errchkjson",
        doc: "Checks types passed to json encoding functions for unsupported types and unchecked errors",
        url: "https://github.com/breml/errchkjson",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
