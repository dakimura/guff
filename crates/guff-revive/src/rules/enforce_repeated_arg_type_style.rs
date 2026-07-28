//! `enforce-repeated-arg-type-style` — enforce short vs full repeated parameter types.

use guff::ast::FuncDecl;
use guff::walk::{self, NodeRef};
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

pub struct Checker {
    arg_style: RepeatedTypeStyle,
    ret_style: RepeatedTypeStyle,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn try_new(pass: &Pass<'_>) -> Option<Self> {
        let args = config::rule_arguments(pass, "enforce-repeated-arg-type-style");
        if args.is_empty() {
            return None;
        }
        let (arg_style, ret_style) = match args.first() {
            Some(RuleArgument::String(s)) => {
                let style = repeated_style_from_string(Some(s.as_str()));
                if style == RepeatedTypeStyle::Any {
                    return None;
                }
                (style, style)
            }
            Some(RuleArgument::Map(map)) => {
                let arg = map_value_style(map, "funcArgStyle");
                let ret = map_value_style(map, "funcRetValStyle");
                if arg == RepeatedTypeStyle::Any && ret == RepeatedTypeStyle::Any {
                    return None;
                }
                (arg, ret)
            }
            _ => return None,
        };
        Some(Self {
            arg_style,
            ret_style,
            failures: Vec::new(),
        })
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::FuncDecl(f) = n else {
            return;
        };
        check_func(f, self.arg_style, self.ret_style, &mut self.failures);
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let Some(mut c) = Checker::try_new(pass) else {
        return Vec::new();
    };
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
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
                        confidence: None,
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
                            confidence: None,
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
