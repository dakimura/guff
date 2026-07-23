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
#[derive(Debug, Clone)]
pub struct Settings {
    /// Default severity for failures when a rule does not set one.
    pub severity: Option<String>,
    /// When `None`, only [`super::config::DEFAULT_RULES`] run (golint behaviour).
    /// When `Some`, listed rules (minus `disabled`) run; combined with
    /// [`Self::enable_default_rules`] / [`Self::enable_all_rules`] like golangci-lint.
    pub rules: Option<Vec<RuleSetting>>,
    /// Minimum failure confidence to report (revive default: 0.8).
    pub confidence: Option<f64>,
    /// When true, skip diagnostics in generated files.
    pub ignore_generated_header: bool,
    /// When true, also enable golint-default rules (golangci `enable-default-rules`).
    pub enable_default_rules: bool,
    /// When true, enable all known revive rules (golangci `enable-all-rules`).
    pub enable_all_rules: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            severity: None,
            rules: None,
            confidence: None,
            ignore_generated_header: false,
            enable_default_rules: false,
            enable_all_rules: false,
        }
    }
}

impl Settings {
    /// Effective confidence threshold (golangci-lint / revive default: 0.8).
    pub fn confidence_threshold(&self) -> f64 {
        self.confidence.unwrap_or(0.8)
    }
    pub fn rule(&self, name: &str) -> Option<&RuleSetting> {
        let rules = self.rules.as_ref()?;
        rules.iter().find(|r| r.name == name)
    }

    pub fn rule_enabled(&self, name: &str, default_rules: &[&str], all_rules: &[&str]) -> bool {
        if let Some(rule) = self.rule(name) {
            return !rule.disabled;
        }
        if self.enable_all_rules {
            return all_rules.contains(&name);
        }
        if self.enable_default_rules {
            return default_rules.contains(&name);
        }
        match &self.rules {
            None => default_rules.contains(&name),
            Some(_) => false,
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
