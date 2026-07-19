//! `var-naming` — naming conventions for identifiers.
//!
//! Arguments (revive / golangci-lint):
//! 1. allowlist — initialisms that should *not* be forced to capitals (e.g. `ID`)
//! 2. blocklist — additional initialisms to enforce (e.g. `VM`)
//! 3. options slice of one map:
//!    - `skipInitialismNameChecks` / `skip-initialism-name-checks`
//!    - `upperCaseConst` / `upper-case-const`
//!    - `skipPackageNameChecks` / `extraBadPackageNames` /
//!      `skipPackageNameCollisionWithGoStd` — accepted but ignored
//!      (upstream revive moved package checks to `package-naming`)

use guff::ast::{AssignStmt, Expr, FuncDecl, GenDecl, RangeStmt, Spec, TypeSpec, ValueSpec};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;
use crate::names::{canonical_name_with_lists, is_upper_underscore};
use crate::settings::RuleArgument;
use crate::util::{is_blank, unparen};

const KNOWN_EXCEPTIONS: &[&str] = &["LastInsertId", "kWh"];

struct Options {
    allowlist: Vec<String>,
    blocklist: Vec<String>,
    skip_initialism_checks: bool,
    upper_case_const: bool,
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let opts = parse_options(pass);
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::AssignStmt(a) => check_assign(a, &opts, &mut failures),
                NodeRef::FuncDecl(f) => check_func(f, &opts, &mut failures),
                NodeRef::GenDecl(g) => check_gen(g, &opts, &mut failures),
                NodeRef::RangeStmt(r) => check_range(r, &opts, &mut failures),
                NodeRef::StructType(s) => {
                    for field in &s.fields.list {
                        for name in &field.names {
                            check(name, "struct field", &opts, &mut failures);
                        }
                    }
                }
                NodeRef::InterfaceType(iface) => {
                    // Do not check interface method names (often constrained by
                    // concrete types); only check their params/results.
                    for field in &iface.methods.list {
                        let Some(ty) = field.ty.as_ref() else {
                            continue;
                        };
                        let Expr::FuncType(ft) = unparen(ty) else {
                            continue;
                        };
                        if let Some(params) = &ft.params {
                            check_field_list(
                                params,
                                "interface method parameter",
                                &opts,
                                &mut failures,
                            );
                        }
                        if let Some(results) = &ft.results {
                            check_field_list(
                                results,
                                "interface method result",
                                &opts,
                                &mut failures,
                            );
                        }
                    }
                }
                _ => {}
            }
            true
        });
    }
    failures
}

fn parse_options(pass: &Pass<'_>) -> Options {
    let args = config::rule_arguments(pass, "var-naming");
    let allowlist = string_list_at(&args, 0);
    let blocklist = string_list_at(&args, 1);
    let mut skip_initialism_checks = false;
    let mut upper_case_const = false;

    if let Some(map) = options_map_at(&args, 2) {
        for (key, value) in map {
            if config::rule_option_matches(&key, "skipInitialismNameChecks") {
                skip_initialism_checks = arg_is_true(value);
            } else if config::rule_option_matches(&key, "upperCaseConst") {
                upper_case_const = arg_is_true(value);
            }
            // DEFERRED / upstream-ignored: skipPackageNameChecks,
            // extraBadPackageNames, skipPackageNameCollisionWithGoStd
            // (use package-naming instead).
        }
    }

    Options {
        allowlist,
        blocklist,
        skip_initialism_checks,
        upper_case_const,
    }
}

fn string_list_at(args: &[RuleArgument], index: usize) -> Vec<String> {
    let Some(RuleArgument::List(items)) = args.get(index) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match item {
            RuleArgument::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// Third argument is typically `[{option: true}]` (list of one map).
fn options_map_at<'a>(
    args: &'a [RuleArgument],
    index: usize,
) -> Option<&'a std::collections::HashMap<String, RuleArgument>> {
    match args.get(index)? {
        RuleArgument::List(items) => items.iter().find_map(|item| match item {
            RuleArgument::Map(m) => Some(m),
            _ => None,
        }),
        RuleArgument::Map(m) => Some(m),
        _ => None,
    }
}

fn arg_is_true(value: &RuleArgument) -> bool {
    match value {
        RuleArgument::String(s) => {
            let lower = s.to_ascii_lowercase();
            lower == "true" || lower == "1" || lower == "yes"
        }
        RuleArgument::Integer(n) => *n != 0,
        _ => false,
    }
}

fn check_assign(a: &AssignStmt, opts: &Options, failures: &mut Vec<Failure>) {
    if a.tok == Some(Token::ASSIGN) {
        return;
    }
    for lhs in &a.lhs {
        if let Expr::Ident(id) = lhs {
            check(id, "var", opts, failures);
        }
    }
}

fn check_func(f: &FuncDecl, opts: &Options, failures: &mut Vec<Failure>) {
    let name = &f.name.name;
    if is_test_func(name) {
        return;
    }
    let thing = if f.recv.is_some() { "method" } else { "func" };
    check(&f.name, thing, opts, failures);
    if let Some(params) = &f.ty.params {
        check_field_list(params, &format!("{thing} parameter"), opts, failures);
    }
    if let Some(results) = &f.ty.results {
        check_field_list(results, &format!("{thing} result"), opts, failures);
    }
}

fn is_test_func(name: &str) -> bool {
    name.starts_with("Example")
        || name.starts_with("Test")
        || name.starts_with("Benchmark")
        || name.starts_with("Fuzz")
}

fn check_gen(g: &GenDecl, opts: &Options, failures: &mut Vec<Failure>) {
    if g.tok == Some(Token::IMPORT) {
        return;
    }
    let thing = match g.tok {
        Some(Token::CONST) => "const",
        Some(Token::TYPE) => "type",
        Some(Token::VAR) => "var",
        _ => return,
    };
    for spec in &g.specs {
        match spec {
            Spec::TypeSpec(TypeSpec { name, .. }) => check(name, thing, opts, failures),
            Spec::ValueSpec(ValueSpec { names, .. }) => {
                for id in names {
                    check(id, thing, opts, failures);
                }
            }
            _ => {}
        }
    }
}

fn check_range(r: &RangeStmt, opts: &Options, failures: &mut Vec<Failure>) {
    if r.tok == Some(Token::ASSIGN) {
        return;
    }
    if let Some(Expr::Ident(id)) = r.key.as_ref() {
        check(id, "range var", opts, failures);
    }
    if let Some(Expr::Ident(id)) = r.value.as_ref() {
        check(id, "range var", opts, failures);
    }
}

fn check_field_list(
    list: &guff::ast::FieldList,
    thing: &str,
    opts: &Options,
    failures: &mut Vec<Failure>,
) {
    for field in &list.list {
        for id in &field.names {
            check(id, thing, opts, failures);
        }
    }
}

fn check(id: &guff::ast::Ident, thing: &str, opts: &Options, failures: &mut Vec<Failure>) {
    if is_blank(id) || KNOWN_EXCEPTIONS.contains(&id.name.as_str()) {
        return;
    }
    // #851 upperCaseConst support
    if thing == "const" && opts.upper_case_const && is_upper_case_const(&id.name) {
        return;
    }
    if is_upper_underscore(&id.name) {
        failures.push(Failure {
            rule: "var-naming",
            pos: id.name_pos.0 as u32,
            message: "don't use ALL_CAPS in Go names; use CamelCase".into(),
            confidence: None,
        });
        return;
    }
    let allow: Vec<&str> = opts.allowlist.iter().map(String::as_str).collect();
    let block: Vec<&str> = opts.blocklist.iter().map(String::as_str).collect();
    let should =
        canonical_name_with_lists(&id.name, &allow, &block, opts.skip_initialism_checks);
    if id.name == should {
        return;
    }
    if id.name.len() > 2 && id.name[1..].contains('_') {
        failures.push(Failure {
            rule: "var-naming",
            pos: id.name_pos.0 as u32,
            message: format!(
                "don't use underscores in Go names; {thing} {} should be {should}",
                id.name
            ),
            confidence: None,
        });
        return;
    }
    failures.push(Failure {
        rule: "var-naming",
        pos: id.name_pos.0 as u32,
        message: format!("{thing} {} should be {should}", id.name),
            confidence: None,
        });
}

/// Constant-style names like `SOME_CONST`, `_SOME_PRIVATE_CONST`, `X123_3`.
fn is_upper_case_const(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let r: Vec<char> = s.chars().collect();
    let c = r[0];
    if r.len() == 1 {
        return is_ascii_upper(c);
    }
    if c != '_' && !is_ascii_upper(c) {
        return false;
    }
    for (i, ch) in r.iter().copied().enumerate() {
        if is_ascii_upper_or_digit(ch) {
            continue;
        }
        if ch == '_' {
            if i + 1 >= r.len() || !is_ascii_upper_or_digit(r[i + 1]) {
                return false;
            }
            continue;
        }
        return false;
    }
    true
}

fn is_ascii_upper(c: char) -> bool {
    c.is_ascii_uppercase()
}

fn is_ascii_upper_or_digit(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit()
}
