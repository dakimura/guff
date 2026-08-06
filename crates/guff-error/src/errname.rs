//! Port of [`github.com/Antonboom/errname`](https://github.com/Antonboom/errname).

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{ArrayType, Decl, Expr, GenDecl, Spec, TypeSpec, ValueSpec};
use guff::token::Token;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::util::{implements_error, type_of};

fn starts_with_lower(n: &str) -> bool {
    n.chars().next().is_some_and(|c| c.is_lowercase())
}

fn is_initialism(s: &str) -> bool {
    let lower = s.to_lowercase();
    let upper = s.to_uppercase();
    s == lower || s == upper
}

fn split_words(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut words = Vec::new();
    let mut cur = String::new();
    cur.push(chars[0]);
    for &r in &chars[1..] {
        if r.is_uppercase() {
            words.push(cur.to_lowercase());
            cur.clear();
        }
        cur.push(r);
    }
    words.push(cur.to_lowercase());
    words
}

fn words_count(words: &[String]) -> HashMap<&str, usize> {
    let mut m = HashMap::new();
    for w in words {
        *m.entry(w.as_str()).or_default() += 1;
    }
    m
}

fn is_valid_error_type_name(s: &str) -> bool {
    if is_initialism(s) {
        return true;
    }
    let words = split_words(s);
    let cnt = words_count(&words);
    cnt.get("error").copied().unwrap_or(0) == 1 && words.last().is_some_and(|w| w == "error")
}

fn is_valid_error_array_type_name(s: &str) -> bool {
    if is_initialism(s) {
        return true;
    }
    let words = split_words(s);
    let cnt = words_count(&words);
    let has_errors = cnt.get("errors").copied().unwrap_or(0) == 1;
    let has_error = cnt.get("error").copied().unwrap_or(0) == 1;
    if !has_errors && !has_error {
        return false;
    }
    matches!(words.last().map(|s| s.as_str()), Some("errors" | "error"))
}

fn is_valid_error_var_name(s: &str) -> bool {
    if is_initialism(s) {
        return true;
    }
    let words = split_words(s);
    let cnt = words_count(&words);
    cnt.get("err").copied().unwrap_or(0) == 1 && words.first().is_some_and(|w| w == "err")
}

fn object_type(pass: &Pass<'_>, ident: &guff::ast::Ident) -> Option<guff_types::TypeId> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if let Some(Some(obj)) = info.defs.get(&ident.id) {
        return obj.typ(&artifacts.objects);
    }
    if let Some(obj) = info.uses.get(&ident.id) {
        return obj.typ(&artifacts.objects);
    }
    type_of(pass, &Expr::Ident(ident.clone()))
}

fn check_value_spec(pass: &Pass<'_>, vs: &ValueSpec, pending: &mut Vec<(u32, String)>) {
    if vs.names.len() != 1 {
        return;
    }
    let ident = &vs.names[0];
    let Some(typ) = object_type(pass, ident) else {
        return;
    };
    if !implements_error(pass, typ) {
        return;
    }
    if is_valid_error_var_name(&ident.name) {
        return;
    }
    let form = if starts_with_lower(&ident.name) {
        "errXxx"
    } else {
        "ErrXxx"
    };
    pending.push((
        ident.name_pos.0 as u32,
        format!(
            "the sentinel error name `{}` should conform to the `{form}` format",
            ident.name
        ),
    ));
}

fn check_type_spec(pass: &Pass<'_>, ts: &TypeSpec, pending: &mut Vec<(u32, String)>) {
    // Upstream only inspects concrete type declarations. An interface that
    // happens to include `Error() string` (`type AttrsGetter interface {
    // Error() string; Attrs() []slog.Attr }`) names a contract, not an error
    // type, and is never reported.
    if matches!(&ts.ty, Expr::InterfaceType(_)) {
        return;
    }
    let Some(typ) = object_type(pass, &ts.name) else {
        return;
    };
    if !implements_error(pass, typ) {
        return;
    }
    let name = &ts.name.name;
    // Upstream treats any `*ast.ArrayType` (`[n]T` or `[]T`) as array form.
    if matches!(&ts.ty, Expr::ArrayType(ArrayType { .. })) {
        if is_valid_error_array_type_name(name) {
            return;
        }
        let forms = if starts_with_lower(name) {
            "`xxxErrors` or `xxxError`"
        } else {
            "`XxxErrors` or `XxxError`"
        };
        pending.push((
            ts.name.name_pos.0 as u32,
            format!("the error type name `{name}` should conform to the {forms} format"),
        ));
        return;
    }
    if is_valid_error_type_name(name) {
        return;
    }
    let form = if starts_with_lower(name) {
        "xxxError"
    } else {
        "XxxError"
    };
    pending.push((
        ts.name.name_pos.0 as u32,
        format!("the error type name `{name}` should conform to the `{form}` format"),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "errname requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            match decl {
                Decl::GenDecl(GenDecl {
                    tok: Some(Token::VAR),
                    specs,
                    ..
                }) => {
                    for spec in specs {
                        if let Spec::ValueSpec(vs) = spec {
                            check_value_spec(pass, vs, &mut pending);
                        }
                    }
                }
                Decl::GenDecl(GenDecl {
                    tok: Some(Token::TYPE),
                    specs,
                    ..
                }) => {
                    for spec in specs {
                        if let Spec::TypeSpec(ts) = spec {
                            check_type_spec(pass, ts, &mut pending);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "errname",
        doc: "Checks that sentinel errors are prefixed with Err and error types are suffixed with Error",
        url: "https://github.com/Antonboom/errname",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
