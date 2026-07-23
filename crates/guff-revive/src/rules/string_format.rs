//! `string-format` — warn on string literals that fail configured regex checks.

use guff::ast::{BasicLit, CallExpr, CompositeLit, Expr, Ident, KeyValueExpr, SelectorExpr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;
use regex::Regex;

use crate::config;
use crate::failure::Failure;
use crate::util::{basic_lit_string_value, unparen};

struct Subrule {
    func_name: String,
    argument: usize,
    field: Option<String>,
    regex: Regex,
    negated: bool,
    message: String,
}

pub struct Checker {
    rules: Vec<Subrule>,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn try_new(pass: &Pass<'_>) -> Option<Self> {
        let raw = config::string_format_rules(pass);
        if raw.is_empty() {
            return None;
        }
        let rules: Vec<Subrule> = raw
            .iter()
            .filter_map(|(scope, pattern, msg)| parse_subrule(scope, pattern, msg))
            .collect();
        if rules.is_empty() {
            return None;
        }
        Some(Self {
            rules,
            failures: Vec::new(),
        })
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        let Some(call_name) = call_name(call) else {
            return;
        };
        for rule in &self.rules {
            if rule.func_name != call_name {
                continue;
            }
            if let Some(lit) = string_arg(call, rule.argument, rule.field.as_deref()) {
                let value = basic_lit_string_value(lit).unwrap_or_default();
                let ok = rule.regex.is_match(value) ^ rule.negated;
                if !ok {
                    self.failures.push(Failure {
                        rule: "string-format",
                        pos: lit.value_pos.0 as u32,
                        message: rule.message.clone(),
                        confidence: None,
                    });
                }
            }
        }
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

fn parse_subrule(scope: &str, pattern: &str, message: &str) -> Option<Subrule> {
    if pattern.len() < 3 || !pattern.starts_with('/') || !pattern.ends_with('/') {
        return None;
    }
    let (func_name, argument, field) = parse_scope(scope)?;
    let negated = pattern.starts_with("/!");
    let start = if negated { 2 } else { 1 };
    let regex = Regex::new(&pattern[start..pattern.len() - 1]).ok()?;
    Some(Subrule {
        func_name,
        argument,
        field,
        regex,
        negated,
        message: message.to_string(),
    })
}

fn parse_scope(scope: &str) -> Option<(String, usize, Option<String>)> {
    let mut func_name = scope.to_string();
    let mut argument = 0usize;
    let mut field = None;
    if let Some(bracket) = scope.find('[') {
        func_name = scope[..bracket].to_string();
        let rest = &scope[bracket + 1..];
        let end = rest.find(']')?;
        argument = rest[..end].parse().ok()?;
        let after = &rest[end + 1..];
        if let Some(stripped) = after.strip_prefix('.') {
            field = Some(stripped.to_string());
        }
    }
    Some((func_name, argument, field))
}

fn call_name(call: &CallExpr) -> Option<String> {
    match unparen(&call.fun) {
        Expr::Ident(Ident { name, .. }) => Some(name.clone()),
        Expr::SelectorExpr(SelectorExpr { x, sel, .. }) => {
            let pkg = match unparen(x) {
                Expr::Ident(Ident { name, .. }) => name.clone(),
                Expr::SelectorExpr(inner) => inner.sel.name.clone(),
                _ => return None,
            };
            Some(format!("{pkg}.{}", sel.name))
        }
        _ => None,
    }
}

fn string_arg<'a>(
    call: &'a CallExpr,
    index: usize,
    field: Option<&str>,
) -> Option<&'a BasicLit> {
    let arg = call.args.get(index)?;
    if let Some(field_name) = field {
        let Expr::CompositeLit(comp) = unparen(arg) else {
            return None;
        };
        for el in &comp.elts {
            let Expr::KeyValueExpr(KeyValueExpr { key, value, .. }) = el else {
                continue;
            };
            let Expr::Ident(Ident { name, .. }) = unparen(key) else {
                continue;
            };
            if name != field_name {
                continue;
            }
            let Expr::BasicLit(lit) = unparen(value) else {
                return None;
            };
            if lit.kind == Some(Token::STRING) {
                return Some(lit);
            }
            return None;
        }
        None
    } else {
        let Expr::BasicLit(lit) = unparen(arg) else {
            return None;
        };
        if lit.kind == Some(Token::STRING) {
            Some(lit)
        } else {
            None
        }
    }
}
