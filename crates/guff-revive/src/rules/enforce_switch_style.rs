//! `enforce-switch-style` — enforce default clause presence and position.

use guff::ast::{BlockStmt, BranchStmt, CaseClause, ReturnStmt, Stmt, SwitchStmt, TypeSwitchStmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let allow_no_default = rule_option(pass, "allowNoDefault");
    let allow_default_not_last = rule_option(pass, "allowDefaultNotLast");

    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let (body, pos) = match n {
                Some(NodeRef::SwitchStmt(s)) => (&s.body, s.switch.0 as u32),
                Some(NodeRef::TypeSwitchStmt(s)) => (&s.body, s.switch.0 as u32),
                _ => return true,
            };
            let (default_clause, is_last) = seek_default_case(body);
            let has_default = default_clause.is_some();

            if !has_default && allow_no_default {
                return true;
            }
            if !has_default && !allow_no_default && !all_branches_end_with_jump(body) {
                failures.push(Failure {
                    rule: "enforce-switch-style",
                    pos,
                    message: "switch must have a default case clause".into(),
                });
                return true;
            }
            if has_default && !allow_default_not_last && !is_last {
                failures.push(Failure {
                    rule: "enforce-switch-style",
                    pos: default_clause.unwrap().case.0 as u32,
                    message: "default case clause must be the last one".into(),
                });
            }
            true
        });
    }
    failures
}

fn rule_option(pass: &Pass<'_>, name: &str) -> bool {
    let args = crate::config::rule_arguments(pass, "enforce-switch-style");
    args.iter().any(|arg| match arg {
        crate::settings::RuleArgument::String(s) => {
            s.eq_ignore_ascii_case(name)
                || s.eq_ignore_ascii_case(&name.replace("Default", "-default").to_lowercase())
                || matches!(
                    (name, s.as_str()),
                    ("allowNoDefault", "allownodefault")
                        | ("allowDefaultNotLast", "allowdefaultnotlast")
                )
        }
        _ => false,
    })
}

fn seek_default_case(body: &BlockStmt) -> (Option<&CaseClause>, bool) {
    let mut last: Option<&CaseClause> = None;
    let mut default_clause: Option<&CaseClause> = None;
    for stmt in &body.list {
        let Stmt::CaseClause(cc) = stmt else {
            continue;
        };
        last = Some(cc);
        if cc.list.is_empty() {
            default_clause = Some(cc);
        }
    }
    let is_last = matches!((default_clause, last), (Some(d), Some(l)) if std::ptr::eq(d, l));
    (default_clause, is_last)
}

fn all_branches_end_with_jump(body: &BlockStmt) -> bool {
    for stmt in &body.list {
        let Stmt::CaseClause(case) = stmt else {
            return false;
        };
        let Some(last) = case.body.last() else {
            return false;
        };
        match last {
            Stmt::ReturnStmt(_) => {}
            Stmt::BranchStmt(BranchStmt { tok: Token::BREAK, .. }) => {}
            _ => return false,
        }
    }
    true
}
