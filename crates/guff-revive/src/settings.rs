//! Revive linter configuration (`linters.settings.revive`).

use std::collections::HashMap;

/// Per-rule configuration entry (mirrors golangci-lint / revive `rules` list items).
#[derive(Debug, Clone)]
pub struct RuleSetting {
    pub name: String,
    pub arguments: Vec<RuleArgument>,
    pub disabled: bool,
    /// Per-rule severity override (`warning`, `error`, …).
    pub severity: Option<String>,
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
    /// Default severity for failures when a rule does not set one.
    pub severity: Option<String>,
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

    /// Effective severity for `name`: per-rule override, else global default.
    pub fn rule_severity(&self, name: &str) -> Option<&str> {
        if let Some(rule) = self.rule(name) {
            if let Some(sev) = rule.severity.as_deref() {
                if !sev.is_empty() {
                    return Some(sev);
                }
            }
        }
        self.severity.as_deref().filter(|s| !s.is_empty())
    }
}
