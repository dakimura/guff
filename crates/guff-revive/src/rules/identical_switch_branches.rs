//! `identical-switch-branches` — warn on tagged switch statements with identical case bodies.

use guff::ast::{BranchStmt, CaseClause, Stmt, SwitchStmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::astfmt::stmts_fmt;
use crate::config;
use crate::settings::RuleArgument;
use crate::failure::Failure;
use crate::util::line_of;

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
    allow_identical_default: bool,
}

/// revive v1.17.0 `Configure`: `allow-identical-default` (any spelling
/// `isRuleOption` accepts) in a map as the first argument.
fn allow_identical_default(pass: &Pass<'_>) -> bool {
    let args = config::rule_arguments(pass, "identical-switch-branches");
    let Some(RuleArgument::Map(map)) = args.first() else {
        return false;
    };
    map.iter().any(|(k, v)| {
        config::is_rule_option(k, "allowIdenticalDefault")
            && matches!(v, RuleArgument::String(s) if s == "true")
    })
}

impl<'a> Checker<'a> {
    pub fn new(pass: &'a Pass<'a>) -> Self {
        Self {
            pass,
            failures: Vec::new(),
            allow_identical_default: allow_identical_default(pass),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
                    let NodeRef::SwitchStmt(sw) = n else { return; };
                    check_switch(self.pass, sw, self.allow_identical_default, &mut self.failures);
        
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new(pass);
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


fn check_switch(
    pass: &Pass<'_>,
    sw: &SwitchStmt,
    allow_identical_default: bool,
    failures: &mut Vec<Failure>,
) {
    if sw.tag.is_none() {
        return;
    }
    let mut hashes = std::collections::HashMap::new();
    for stmt in &sw.body.list {
        let Stmt::CaseClause(case) = stmt else {
            continue;
        };
        if ends_with_fallthrough(case) {
            continue;
        }
        // The default clause is neither reported nor recorded, so no later
        // clause is reported against it either. (Nested switches inside it
        // are still visited: the walk reaches every SwitchStmt.)
        if allow_identical_default && case.list.is_empty() {
            continue;
        }
        let hash = stmts_fmt(&case.body);
        let line = line_of(pass, case.case.0);
        if let Some(prev) = hashes.get(&hash) {
            failures.push(Failure {
                rule: "identical-switch-branches",
                pos: sw.switch.0 as u32,
                message: format!(
                    "\"switch\" with identical branches (lines {prev} and {line})"
                ),
                ..Failure::default()
            });
        } else {
            hashes.insert(hash, line);
        }
    }
}

fn ends_with_fallthrough(case: &CaseClause) -> bool {
    let Some(Stmt::BranchStmt(BranchStmt { tok, .. })) = case.body.last() else {
        return false;
    };
    *tok == Token::FALLTHROUGH
}
