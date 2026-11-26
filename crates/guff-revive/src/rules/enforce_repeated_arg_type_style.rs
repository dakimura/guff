//! `enforce-repeated-arg-type-style` — enforce short vs full repeated parameter types.

use guff::ast::FuncDecl;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;
use crate::settings::RuleArgument;
use crate::util::render_node;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RepeatedTypeStyle {
    Any,
    Short,
    Full,
}

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    arg_style: RepeatedTypeStyle,
    ret_style: RepeatedTypeStyle,
    failures: Vec<Failure>,
}

impl<'a> Checker<'a> {
    pub fn try_new(pass: &'a Pass<'a>) -> Option<Self> {
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
            pass,
            arg_style,
            ret_style,
            failures: Vec::new(),
        })
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::FuncDecl(f) = n else {
            return;
        };
        check_func(self.pass, f, self.arg_style, self.ret_style, &mut self.failures);
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
    pass: &Pass<'_>,
    f: &FuncDecl,
    arg_style: RepeatedTypeStyle,
    ret_style: RepeatedTypeStyle,
    failures: &mut Vec<Failure>,
) {
    if let Some(params) = &f.ty.params {
        check_fields(pass, &params.list, arg_style, "argument", false, failures);
    }
    if let Some(results) = &f.ty.results {
        // Upstream's results branch carries an extra `field.Names != nil`
        // guard that its params branch does not: `func f() (int, int)` cannot
        // drop a type, since there is no name to attach the shared one to.
        check_fields(pass, &results.list, ret_style, "return", true, failures);
    }
}

fn check_fields(
    pass: &Pass<'_>,
    fields: &[guff::ast::Field],
    style: RepeatedTypeStyle,
    kind: &str,
    require_names: bool,
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
                        ..Failure::default()
                    });
                }
            }
        }
        RepeatedTypeStyle::Short => {
            // Upstream's failure node is `prevType` — the type expression of
            // the *preceding* field, which is the one that would be dropped —
            // not the field that repeats it.
            let mut prev: Option<(String, u32)> = None;
            for field in fields {
                let current = field
                    .ty
                    .as_ref()
                    .map(|ty| (render_node(pass, ty), ty.pos().0 as u32));
                if let (Some((prev_ty, prev_pos)), Some((cur_ty, _))) =
                    (prev.as_ref(), current.as_ref())
                {
                    if prev_ty == cur_ty && !(require_names && field.names.is_empty()) {
                        failures.push(Failure {
                            rule: "enforce-repeated-arg-type-style",
                            pos: *prev_pos,
                            message: format!("repeated {kind} type \"{prev_ty}\" can be omitted"),
                            ..Failure::default()
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
