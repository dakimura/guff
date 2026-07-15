//! `enforce-repeated-arg-type-style` — enforce short vs full repeated parameter types.

use guff::ast::{Decl, FuncDecl};
use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;
use crate::settings::RuleArgument;
use crate::util::expr_string;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RepeatedTypeStyle {
    Any,
    Short,
    Full,
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let args = config::rule_arguments(pass, "enforce-repeated-arg-type-style");
    if args.is_empty() {
        return Vec::new();
    }
    let (arg_style, ret_style) = match args.first() {
        Some(RuleArgument::String(s)) => {
            let style = repeated_style_from_string(Some(s.as_str()));
            if style == RepeatedTypeStyle::Any {
                return Vec::new();
            }
            (style, style)
        }
        Some(RuleArgument::Map(map)) => {
            let arg = map_value_style(map, "funcArgStyle");
            let ret = map_value_style(map, "funcRetValStyle");
            if arg == RepeatedTypeStyle::Any && ret == RepeatedTypeStyle::Any {
                return Vec::new();
            }
            (arg, ret)
        }
        _ => return Vec::new(),
    };
    check_all(pass, arg_style, ret_style)
}

fn check_all(
    pass: &Pass<'_>,
    arg_style: RepeatedTypeStyle,
    ret_style: RepeatedTypeStyle,
) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            check_func(f, arg_style, ret_style, &mut failures);
        }
    }
    let _ = pass;
    failures
}

fn check_func(
    f: &FuncDecl,
    arg_style: RepeatedTypeStyle,
    ret_style: RepeatedTypeStyle,
    failures: &mut Vec<Failure>,
) {
    if let Some(params) = &f.ty.params {
        check_fields(&params.list, arg_style, "argument", failures);
    }
    if let Some(results) = &f.ty.results {
        check_fields(&results.list, ret_style, "return", failures);
    }
}

fn check_fields(
    fields: &[guff::ast::Field],
    style: RepeatedTypeStyle,
    kind: &str,
    failures: &mut Vec<Failure>,
) {
    match style {
        RepeatedTypeStyle::Any => {}
        RepeatedTypeStyle::Full => {
            for field in fields {
                if field.names.len() > 1 {
                    failures.push(Failure {
                        rule: "enforce-repeated-arg-type-style",
                        pos: field.pos().0 as u32,
                        message: format!("{kind} types should not be omitted"),
                    });
                }
            }
        }
        RepeatedTypeStyle::Short => {
            let mut prev = None;
            for field in fields {
                let current = field.ty.as_ref().map(expr_string);
                if let (Some(prev_ty), Some(cur_ty)) = (prev.as_ref(), current.as_ref()) {
                    if prev_ty == cur_ty {
                        failures.push(Failure {
                            rule: "enforce-repeated-arg-type-style",
                            pos: field.pos().0 as u32,
                            message: format!("repeated {kind} type \"{cur_ty}\" can be omitted"),
                        });
                    }
                }
                prev = current;
            }
        }
    }
}

fn map_value_style(map: &std::collections::HashMap<String, RuleArgument>, key: &str) -> RepeatedTypeStyle {
    for (k, v) in map {
        if !rule_option_key(key, k) {
            continue;
        }
        if let RuleArgument::String(s) = v {
            return repeated_style_from_string(Some(s.as_str()));
        }
    }
    RepeatedTypeStyle::Any
}

fn rule_option_key(expected: &str, actual: &str) -> bool {
    let norm = |s: &str| {
        s.chars()
            .filter(|c| *c != '-' && *c != '_')
            .collect::<String>()
            .to_ascii_lowercase()
    };
    norm(expected) == norm(actual)
}

fn repeated_style_from_string(s: Option<&str>) -> RepeatedTypeStyle {
    match s {
        Some("short") => RepeatedTypeStyle::Short,
        Some("full") => RepeatedTypeStyle::Full,
        _ => RepeatedTypeStyle::Any,
    }
}
