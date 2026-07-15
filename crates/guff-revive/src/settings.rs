//! Revive linter configuration (`linters.settings.revive`).

use std::collections::HashMap;

/// Per-rule configuration entry (mirrors golangci-lint / revive `rules` list items).
#[derive(Debug, Clone)]
pub struct RuleSetting {
    pub name: String,
    pub arguments: Vec<RuleArgument>,
    pub disabled: bool,
}

/// A single rule argument (string, int, list, or map).
#[derive(Debug, Clone)]
pub enum RuleArgument {
    Integer(i64),
    String(String),
    List(Vec<RuleArgument>),
    Map(HashMap<String, RuleArgument>),
}

/// Revive settings passed through [`guff_analysis::Pass`] or test hooks.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    /// When `None`, only [`super::config::DEFAULT_RULES`] run (golint behaviour).
    /// When `Some`, only listed rules (minus `disabled`) run.
    pub rules: Option<Vec<RuleSetting>>,
}

impl Settings {
    pub fn rule(&self, name: &str) -> Option<&RuleSetting> {
        let rules = self.rules.as_ref()?;
        rules.iter().find(|r| r.name == name)
    }

    pub fn rule_enabled(&self, name: &str, default_rules: &[&str]) -> bool {
        match &self.rules {
            None => default_rules.contains(&name),
            Some(rules) => rules
                .iter()
                .any(|r| r.name == name && !r.disabled),
        }
    }

    pub fn rule_arguments<'a>(&'a self, name: &str) -> &'a [RuleArgument] {
        self.rule(name)
            .map(|r| r.arguments.as_slice())
            .unwrap_or(&[])
    }
}
