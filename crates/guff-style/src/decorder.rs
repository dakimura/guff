//! Port of [`gitlab.com/bosi/decorder`](https://gitlab.com/bosi/decorder)
//! (golangci-lint wrapper in `pkg/golinters/decorder`).
//!
//! Checks declaration order / count of `type`/`const`/`var`/`func`, and that
//! `init` is the first function in a file.
//!
//! Golangci-lint defaults disable all three check families; pass settings (or
//! use [`DecorderOptions::enabled`]) to turn them on. Upstream analyzer flags
//! enable everything by default.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{Decl, FuncDecl, GenDecl, Spec};
use guff::token::Token;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::DecorderOptions;

const KIND_TYPE: &str = "type";
const KIND_CONST: &str = "const";
const KIND_VAR: &str = "var";
const KIND_FUNC: &str = "func";

fn kind_of_token(tok: Token) -> Option<&'static str> {
    match tok {
        Token::TYPE => Some(KIND_TYPE),
        Token::CONST => Some(KIND_CONST),
        Token::VAR => Some(KIND_VAR),
        Token::FUNC => Some(KIND_FUNC),
        _ => None,
    }
}

fn decl_name(gd: &GenDecl) -> Option<&str> {
    for spec in &gd.specs {
        if let Spec::ValueSpec(vs) = spec {
            if let Some(name) = vs.names.first() {
                return Some(name.name.as_str());
            }
        }
    }
    None
}

fn wrong_order_msg(target: &str, not_after: &str, dec_order: &str) -> String {
    format!("{target} must not be placed after {not_after} (desired order: {dec_order})")
}

fn is_too_late(
    kind: &str,
    dec_order: &[String],
    counts: &HashMap<&'static str, usize>,
) -> Option<String> {
    let Some(i) = dec_order.iter().position(|k| k == kind) else {
        return None;
    };
    for later in &dec_order[i + 1..] {
        if counts.get(later.as_str()).copied().unwrap_or(0) > 0 {
            return Some(later.clone());
        }
    }
    None
}

fn num_check_disabled(kind: &str, opts: &DecorderOptions) -> bool {
    if opts.disable_dec_num_check {
        return true;
    }
    match kind {
        KIND_TYPE => opts.disable_type_dec_num_check,
        KIND_CONST => opts.disable_const_dec_num_check,
        KIND_VAR => opts.disable_var_dec_num_check,
        _ => true,
    }
}

fn handle_gen_decl(
    gd: &GenDecl,
    opts: &DecorderOptions,
    counts: &mut HashMap<&'static str, usize>,
    dec_order: &[String],
    desired_order: &str,
    pending: &mut Vec<(u32, String)>,
) {
    let Some(tok) = gd.tok else {
        return;
    };
    let Some(kind) = kind_of_token(tok) else {
        return;
    };
    if kind == KIND_FUNC {
        return;
    }

    if opts.ignore_underscore_vars && decl_name(gd) == Some("_") {
        return;
    }

    *counts.entry(kind).or_insert(0) += 1;
    let count = counts[kind];

    if !num_check_disabled(kind, opts) && count > 1 {
        pending.push((
            gd.tok_pos.0 as u32,
            format!("multiple \"{kind}\" declarations are not allowed; use parentheses instead"),
        ));
    }

    if !opts.disable_dec_order_check {
        if let Some(not_after) = is_too_late(kind, dec_order, counts) {
            pending.push((
                gd.tok_pos.0 as u32,
                wrong_order_msg(kind, &not_after, desired_order),
            ));
        }
    }
}

fn handle_func_decl(
    fd: &FuncDecl,
    opts: &DecorderOptions,
    counts: &mut HashMap<&'static str, usize>,
    dec_order: &[String],
    desired_order: &str,
    pending: &mut Vec<(u32, String)>,
) {
    *counts.entry(KIND_FUNC).or_insert(0) += 1;

    if !opts.disable_dec_order_check {
        if let Some(not_after) = is_too_late(KIND_FUNC, dec_order, counts) {
            pending.push((
                fd.ty.pos().0 as u32,
                wrong_order_msg(KIND_FUNC, &not_after, desired_order),
            ));
        }
    }
}

fn check_init_first(file_decls: &[Decl], pending: &mut Vec<(u32, String)>) {
    let mut non_init_found = false;
    for decl in file_decls {
        let Decl::FuncDecl(fd) = decl else {
            continue;
        };
        let is_init = fd.name.name == "init" && fd.recv.is_none();
        if is_init {
            if non_init_found {
                pending.push((
                    fd.ty.pos().0 as u32,
                    "init func must be the first function in file".to_string(),
                ));
            }
        } else {
            non_init_found = true;
        }
    }
}

fn check_file(decls: &[Decl], opts: &DecorderOptions, pending: &mut Vec<(u32, String)>) {
    if !opts.disable_dec_num_check || !opts.disable_dec_order_check {
        let mut counts: HashMap<&'static str, usize> = HashMap::new();
        let dec_order = opts.dec_order.clone();
        let desired_order = dec_order.join(",");

        for decl in decls {
            match decl {
                Decl::GenDecl(gd) => {
                    handle_gen_decl(gd, opts, &mut counts, &dec_order, &desired_order, pending);
                }
                Decl::FuncDecl(fd) => {
                    handle_func_decl(fd, opts, &mut counts, &dec_order, &desired_order, pending);
                }
                Decl::BadDecl(_) => {}
            }
        }
    }

    if !opts.disable_init_func_first_check {
        check_init_first(decls, pending);
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "decorder requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<DecorderOptions>("decorder")
        .cloned()
        .unwrap_or_else(DecorderOptions::enabled);

    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        check_file(&file.decls, &opts, &mut pending);
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "decorder",
        doc: "check declaration order and count of types, constants, variables and functions",
        url: "https://gitlab.com/bosi/decorder",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golangci_default_is_noop() {
        let opts = DecorderOptions::default();
        assert!(opts.disable_dec_num_check);
        assert!(opts.disable_dec_order_check);
        assert!(opts.disable_init_func_first_check);
    }

    #[test]
    fn enabled_turns_checks_on() {
        let opts = DecorderOptions::enabled();
        assert!(!opts.disable_dec_num_check);
        assert!(!opts.disable_dec_order_check);
        assert!(!opts.disable_init_func_first_check);
    }
}
