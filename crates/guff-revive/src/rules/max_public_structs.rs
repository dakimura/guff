//! `max-public-structs` — limit the number of exported struct types per file.
//!
//! A port of revive v1.17.1 `rule/max_public_structs.go`. Every `TypeSpec` in
//! the file counts — function-local ones too, and aliases (`type A = struct{}`)
//! — when its name is exported (`ast.IsExported`: the first rune is an
//! upper-case letter, `Éclair` included) and its type is a struct literal.
//! v1.17.0 compared the first byte with `strings.ToUpper` and counted
//! interfaces, named types and `_foo` too.

use guff::ast::Expr;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::settings::RuleArgument;

const RULE: &str = "max-public-structs";
const DEFAULT_MAX: i64 = 5;

/// Upstream `Configure`: the first argument must be an integer; no arguments
/// means 5. A non-integer is a configuration error upstream, which guff does
/// not surface; it falls back to the default.
fn max(pass: &Pass<'_>) -> i64 {
    match crate::config::rule_arguments(pass, RULE).first() {
        Some(RuleArgument::Integer(n)) => *n,
        _ => DEFAULT_MAX,
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let max = max(pass);
    let mut failures = Vec::new();
    if max < 1 {
        return failures;
    }
    for file in pass.files() {
        let mut current: i64 = 0;
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(NodeRef::TypeSpec(ts)) = n {
                let exported = ts.name.name.chars().next().is_some_and(char::is_uppercase);
                if exported && matches!(ts.ty, Expr::StructType(_)) {
                    current += 1;
                }
            }
            true
        });
        if current > max {
            // `Node: fileAst`: File.Pos() is the `package` keyword.
            failures.push(Failure {
                rule: RULE,
                pos: file.package.0 as u32,
                message: format!(
                    "you have exceeded the maximum number ({max}) of public struct declarations"
                ),
                ..Failure::default()
            });
        }
    }
    failures
}
