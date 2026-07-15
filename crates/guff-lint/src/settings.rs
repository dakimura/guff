//! Typed `linters.settings` (golangci-lint `linters-settings` / v2 `linters.settings`).
//!
//! Parses YAML into per-linter structs, builds a [`SettingsBag`] for Pass-time
//! options (errcheck), and filters analyzer lists for selection-time options
//! (govet enable/disable, staticcheck checks).

use std::collections::HashSet;
use std::sync::Arc;

use guff_analysis::{Analyzer, SettingsBag};
use serde::Deserialize;

/// All per-linter settings currently understood by guff.
#[derive(Debug, Clone, Default)]
pub struct LinterSettings {
    pub errcheck: ErrcheckSettings,
    pub govet: GovetSettings,
    pub staticcheck: StaticcheckSettings,
    pub revive: ReviveSettings,
}

/// `linters.settings.errcheck` / `linters-settings.errcheck`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ErrcheckSettings {
    /// Report `_ = f()` / `x, _ := f()` when the ignored value is an error.
    #[serde(default, rename = "check-blank")]
    pub check_blank: bool,
    /// Report ignored type assertions `x := y.(T)` (no ok).
    #[serde(default, rename = "check-type-assertions")]
    pub check_type_assertions: bool,
    // DEFERRED (R4 follow-up): disable-default-exclusions, exclude-functions, verbose.
}

/// `linters.settings.govet` / `linters-settings.govet`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GovetSettings {
    #[serde(default, rename = "enable-all")]
    pub enable_all: bool,
    #[serde(default, rename = "disable-all")]
    pub disable_all: bool,
    #[serde(default)]
    pub enable: Vec<String>,
    #[serde(default)]
    pub disable: Vec<String>,
}

/// `linters.settings.staticcheck` / `linters-settings.staticcheck`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct StaticcheckSettings {
    /// Check selectors (`all`, `SA1000`, `-SA1000`, …). `None` = keep registry default.
    #[serde(default)]
    pub checks: Option<Vec<String>>,
    // DEFERRED: initialisms, dot-import-whitelist, http-status-code-whitelist.
}

/// `linters.settings.revive` / `linters-settings.revive`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ReviveSettings {
    /// Per-rule enablement and arguments. `None` = golint-default rules only.
    #[serde(default)]
    pub rules: Option<Vec<ReviveRuleSetting>>,
    // DEFERRED: severity, confidence, ignore-generated-header.
}

/// One revive rule entry from golangci-lint YAML.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ReviveRuleSetting {
    pub name: String,
    #[serde(default)]
    pub arguments: Vec<serde_yaml::Value>,
    #[serde(default)]
    pub disabled: bool,
    // DEFERRED: severity, exclude.
}

impl LinterSettings {
    /// Parse from v2 `linters.settings` or v1 `linters-settings` YAML mapping.
    pub fn from_yaml(value: &serde_yaml::Value) -> Self {
        let Some(map) = value.as_mapping() else {
            return Self::default();
        };
        let mut out = Self::default();
        if let Some(v) = map.get(serde_yaml::Value::String("errcheck".into())) {
            if let Ok(s) = serde_yaml::from_value::<ErrcheckSettings>(v.clone()) {
                out.errcheck = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("govet".into())) {
            if let Ok(s) = serde_yaml::from_value::<GovetSettings>(v.clone()) {
                out.govet = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("staticcheck".into())) {
            if let Ok(s) = serde_yaml::from_value::<StaticcheckSettings>(v.clone()) {
                out.staticcheck = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("revive".into())) {
            if let Ok(s) = serde_yaml::from_value::<ReviveSettings>(v.clone()) {
                out.revive = s;
            }
        }
        // Unknown linter keys are intentionally ignored (forward-compat with
        // golangci configs that mention linters guff does not have yet).
        out
    }

    /// Build a [`SettingsBag`] for Pass-time options.
    pub fn to_bag(&self) -> Arc<SettingsBag> {
        let mut bag = SettingsBag::new();
        bag.insert(
            "errcheck",
            guff_errcheck::Options {
                check_blank: self.errcheck.check_blank,
                check_asserts: self.errcheck.check_type_assertions,
            },
        );
        bag.insert("revive", self.revive.to_guff_revive());
        Arc::new(bag)
    }

    /// Filter / select analyzers for a single golangci linter name.
    pub fn apply_to_analyzers(
        &self,
        linter: &str,
        analyzers: Vec<&'static Analyzer>,
    ) -> Vec<&'static Analyzer> {
        match linter {
            "govet" => filter_govet(&self.govet, analyzers),
            "staticcheck" => filter_staticcheck(&self.staticcheck, analyzers),
            _ => analyzers,
        }
    }
}

fn filter_govet(
    settings: &GovetSettings,
    analyzers: Vec<&'static Analyzer>,
) -> Vec<&'static Analyzer> {
    if !settings.enable_all
        && !settings.disable_all
        && settings.enable.is_empty()
        && settings.disable.is_empty()
    {
        return analyzers;
    }

    let enable: HashSet<&str> = settings.enable.iter().map(String::as_str).collect();
    let disable: HashSet<&str> = settings.disable.iter().map(String::as_str).collect();

    // guff-govet::analyzers() is the full available set (= golangci default ∪ extras).
    if settings.disable_all {
        return analyzers
            .into_iter()
            .filter(|a| enable.contains(a.name))
            .collect();
    }

    // Default / enable-all: full registry minus `disable`.
    // `enable` only matters with disable-all (above); otherwise the registry
    // already includes every pass enable could add.
    let _ = (settings.enable_all, &enable);
    analyzers
        .into_iter()
        .filter(|a| !disable.contains(a.name))
        .collect()
}

fn filter_staticcheck(
    settings: &StaticcheckSettings,
    analyzers: Vec<&'static Analyzer>,
) -> Vec<&'static Analyzer> {
    let Some(checks) = settings.checks.as_ref() else {
        return analyzers;
    };
    if checks.is_empty() {
        return analyzers;
    }

    let mut allow_all = false;
    let mut enabled: HashSet<String> = HashSet::new();
    let mut disabled: HashSet<String> = HashSet::new();

    for c in checks {
        if c == "all" {
            allow_all = true;
            continue;
        }
        if let Some(rest) = c.strip_prefix('-') {
            disabled.insert(rest.to_string());
        } else {
            enabled.insert(c.clone());
        }
    }

    analyzers
        .into_iter()
        .filter(|a| {
            if disabled.contains(a.name) {
                return false;
            }
            if allow_all {
                return true;
            }
            if enabled.is_empty() {
                return true;
            }
            enabled.contains(a.name)
        })
        .collect()
}

impl ReviveSettings {
    pub fn to_guff_revive(&self) -> guff_revive::Settings {
        let rules = self.rules.as_ref().map(|rules| {
            rules
                .iter()
                .map(|rule| guff_revive::RuleSetting {
                    name: rule.name.clone(),
                    arguments: rule
                        .arguments
                        .iter()
                        .map(convert_revive_argument)
                        .collect(),
                    disabled: rule.disabled,
                })
                .collect()
        });
        guff_revive::Settings { rules }
    }
}

fn convert_revive_argument(value: &serde_yaml::Value) -> guff_revive::RuleArgument {
    match value {
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                guff_revive::RuleArgument::Integer(i)
            } else {
                guff_revive::RuleArgument::String(n.to_string())
            }
        }
        serde_yaml::Value::String(s) => guff_revive::RuleArgument::String(s.clone()),
        serde_yaml::Value::Sequence(seq) => {
            guff_revive::RuleArgument::List(seq.iter().map(convert_revive_argument).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let mut out = std::collections::HashMap::new();
            for (k, v) in map {
                let key = match k {
                    serde_yaml::Value::String(s) => s.clone(),
                    serde_yaml::Value::Number(n) => n.to_string(),
                    other => format!("{other:?}"),
                };
                out.insert(key, convert_revive_argument(v));
            }
            guff_revive::RuleArgument::Map(out)
        }
        serde_yaml::Value::Bool(b) => guff_revive::RuleArgument::String(b.to_string()),
        serde_yaml::Value::Null => guff_revive::RuleArgument::String(String::new()),
        serde_yaml::Value::Tagged(tagged) => convert_revive_argument(&tagged.value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_errcheck_check_blank() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            r#"
errcheck:
  check-blank: true
  check-type-assertions: true
"#,
        )
        .unwrap();
        let s = LinterSettings::from_yaml(&yaml);
        assert!(s.errcheck.check_blank);
        assert!(s.errcheck.check_type_assertions);
    }

    #[test]
    fn staticcheck_checks_disable_one() {
        let settings = StaticcheckSettings {
            checks: Some(vec!["all".into(), "-SA1004".into()]),
        };
        let names = ["SA1004", "SA1000", "S1000"];
        let analyzers: Vec<&'static Analyzer> = names
            .iter()
            .map(|n| leak_name(n))
            .collect();
        let filtered = filter_staticcheck(&settings, analyzers);
        let kept: Vec<&str> = filtered.iter().map(|a| a.name).collect();
        assert!(!kept.contains(&"SA1004"));
        assert!(kept.contains(&"SA1000"));
        assert!(kept.contains(&"S1000"));
    }

    #[test]
    fn parse_revive_rules_settings() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            r#"
revive:
  rules:
    - name: enforce-map-style
      arguments: ["make"]
    - name: comments-density
      arguments: [15]
"#,
        )
        .unwrap();
        let s = LinterSettings::from_yaml(&yaml);
        assert_eq!(s.revive.rules.as_ref().map(|r| r.len()), Some(2));
        assert_eq!(s.revive.rules.as_ref().unwrap()[0].name, "enforce-map-style");
        let bag = s.to_bag();
        let revive = bag
            .get::<guff_revive::Settings>("revive")
            .expect("revive settings");
        assert!(revive.rule("enforce-map-style").is_some());
    }

    #[test]
    fn govet_disable_all_plus_enable() {
        let settings = GovetSettings {
            disable_all: true,
            enable: vec!["printf".into()],
            ..GovetSettings::default()
        };
        let analyzers: Vec<&'static Analyzer> =
            ["printf", "assign", "shadow"].iter().map(|n| leak_name(n)).collect();
        let filtered = filter_govet(&settings, analyzers);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "printf");
    }

    fn leak_name(name: &'static str) -> &'static Analyzer {
        Box::leak(Box::new(Analyzer {
            name,
            doc: "",
            url: "",
            run: noop,
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        }))
    }

    fn noop(_: &mut guff_analysis::Pass<'_>) -> Result<Option<guff_analysis::AnalysisResult>, guff_analysis::RunError> {
        Ok(None)
    }
}
