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
    pub dupl: DuplSettings,
    pub misspell: MisspellSettings,
    pub gocyclo: GocycloSettings,
    pub gocognit: GocognitSettings,
    pub nestif: NestifSettings,
    pub dogsled: DogsledSettings,
    pub funlen: FunlenSettings,
    pub cyclop: CyclopSettings,
    pub lll: LllSettings,
    pub nakedret: NakedretSettings,
    pub nlreturn: NlreturnSettings,
    pub predeclared: PredeclaredSettings,
    pub whitespace: WhitespaceSettings,
    pub mnd: MndSettings,
    pub prealloc: PreallocSettings,
    pub tagalign: TagalignSettings,
    pub wsl: WslSettings,
    pub perfsprint: PerfsprintSettings,
    pub goconst: GoconstSettings,
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
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReviveSettings {
    /// Default severity for revive failures (`warning`, `error`, …).
    #[serde(default)]
    pub severity: Option<String>,
    /// Per-rule enablement and arguments. `None` = golint-default rules only.
    #[serde(default)]
    pub rules: Option<Vec<ReviveRuleSetting>>,
    /// Minimum failure confidence to report (revive default: 0.8).
    #[serde(default)]
    pub confidence: Option<f64>,
    /// When true, skip diagnostics in generated files.
    #[serde(default, rename = "ignore-generated-header")]
    pub ignore_generated_header: bool,
}

/// One revive rule entry from golangci-lint YAML.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ReviveRuleSetting {
    pub name: String,
    #[serde(default)]
    pub arguments: Vec<serde_yaml::Value>,
    #[serde(default)]
    pub disabled: bool,
    /// Per-rule severity override (`warning`, `error`, …).
    #[serde(default)]
    pub severity: Option<String>,
    // DEFERRED: exclude.
}

/// `linters.settings.dupl` / `linters-settings.dupl`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct DuplSettings {
    /// Token-count threshold for clone detection (golangci default: 150).
    #[serde(default)]
    pub threshold: Option<i32>,
}

/// `linters.settings.misspell` / `linters-settings.misspell`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct MisspellSettings {
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default, rename = "ignore-words")]
    pub ignore_words: Vec<String>,
    #[serde(default, rename = "extra-words")]
    pub extra_words: Vec<MisspellExtraWordSetting>,
    #[serde(default)]
    pub mode: Option<String>,
}

/// One `extra-words` entry from golangci-lint YAML.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct MisspellExtraWordSetting {
    pub typo: String,
    pub correction: String,
}

/// `linters.settings.gocyclo` / `linters-settings.gocyclo`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GocycloSettings {
    #[serde(default, rename = "min-complexity")]
    pub min_complexity: Option<usize>,
}

/// `linters.settings.gocognit` / `linters-settings.gocognit`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GocognitSettings {
    #[serde(default, rename = "min-complexity")]
    pub min_complexity: Option<usize>,
}

/// `linters.settings.nestif` / `linters-settings.nestif`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct NestifSettings {
    #[serde(default, rename = "min-complexity")]
    pub min_complexity: Option<usize>,
}

/// `linters.settings.dogsled` / `linters-settings.dogsled`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct DogsledSettings {
    #[serde(default, rename = "max-blank-identifiers")]
    pub max_blank_identifiers: Option<usize>,
}

/// `linters.settings.funlen` / `linters-settings.funlen`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct FunlenSettings {
    #[serde(default)]
    pub lines: Option<usize>,
    #[serde(default)]
    pub statements: Option<usize>,
    #[serde(default, rename = "ignore-comments")]
    pub ignore_comments: Option<bool>,
}

/// `linters.settings.cyclop` / `linters-settings.cyclop`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct CyclopSettings {
    #[serde(default, rename = "max-complexity")]
    pub max_complexity: Option<usize>,
    #[serde(default, rename = "package-average")]
    pub package_average: Option<f64>,
    #[serde(default, rename = "skip-tests")]
    pub skip_tests: Option<bool>,
}

/// `linters.settings.lll` / `linters-settings.lll`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct LllSettings {
    #[serde(default, rename = "line-length")]
    pub line_length: Option<usize>,
    #[serde(default, rename = "tab-width")]
    pub tab_width: Option<usize>,
}

/// `linters.settings.nakedret` / `linters-settings.nakedret`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct NakedretSettings {
    #[serde(default, rename = "max-func-lines")]
    pub max_func_lines: Option<usize>,
    #[serde(default, rename = "skip-test-files")]
    pub skip_test_files: Option<bool>,
}

/// `linters.settings.nlreturn` / `linters-settings.nlreturn`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct NlreturnSettings {
    #[serde(default, rename = "block-size")]
    pub block_size: Option<i64>,
}

/// `linters.settings.predeclared` / `linters-settings.predeclared`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct PredeclaredSettings {
    #[serde(default)]
    pub ignore: Vec<String>,
    #[serde(default, rename = "q", alias = "qualified-name")]
    pub qualified: Option<bool>,
}

/// `linters.settings.whitespace` / `linters-settings.whitespace`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct WhitespaceSettings {
    #[serde(default, rename = "multi-if")]
    pub multi_if: Option<bool>,
    #[serde(default, rename = "multi-func")]
    pub multi_func: Option<bool>,
}

/// `linters.settings.mnd` / `linters-settings.mnd`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct MndSettings {
    #[serde(default)]
    pub checks: Option<Vec<String>>,
    #[serde(default, rename = "ignored-numbers")]
    pub ignored_numbers: Vec<String>,
    #[serde(default, rename = "ignored-files")]
    pub ignored_files: Vec<String>,
    #[serde(default, rename = "ignored-functions")]
    pub ignored_functions: Vec<String>,
}

/// `linters.settings.prealloc` / `linters-settings.prealloc`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct PreallocSettings {
    #[serde(default)]
    pub simple: Option<bool>,
    #[serde(default, rename = "range-loops")]
    pub range_loops: Option<bool>,
    #[serde(default, rename = "for-loops")]
    pub for_loops: Option<bool>,
}

/// `linters.settings.tagalign` / `linters-settings.tagalign`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TagalignSettings {
    #[serde(default)]
    pub align: Option<bool>,
    #[serde(default)]
    pub sort: Option<bool>,
    #[serde(default)]
    pub order: Vec<String>,
    #[serde(default)]
    pub strict: Option<bool>,
}

/// `linters.settings.wsl` / `linters-settings.wsl`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct WslSettings {
    #[serde(default, rename = "strict-append")]
    pub strict_append: Option<bool>,
    #[serde(default, rename = "allow-assign-and-call")]
    pub allow_assign_and_call: Option<bool>,
    #[serde(default, rename = "allow-assign-and-anything")]
    pub allow_assign_and_anything: Option<bool>,
    #[serde(default, rename = "allow-multiline-assign")]
    pub allow_multiline_assign: Option<bool>,
    #[serde(default, rename = "allow-cuddle-with-calls")]
    pub allow_cuddle_with_calls: Vec<String>,
    #[serde(default, rename = "allow-cuddle-with-rhs")]
    pub allow_cuddle_with_rhs: Vec<String>,
}

/// `linters.settings.perfsprint` / `linters-settings.perfsprint`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct PerfsprintSettings {
    #[serde(default, rename = "integer-format")]
    pub integer_format: Option<bool>,
    #[serde(default, rename = "int-conversion")]
    pub int_conversion: Option<bool>,
    #[serde(default, rename = "error-format")]
    pub error_format: Option<bool>,
    #[serde(default, rename = "err-error")]
    pub err_error: Option<bool>,
    #[serde(default, rename = "errorf")]
    pub errorf: Option<bool>,
    #[serde(default, rename = "string-format")]
    pub string_format: Option<bool>,
    #[serde(default, rename = "sprintf1")]
    pub sprintf1: Option<bool>,
    #[serde(default, rename = "strconcat")]
    pub strconcat: Option<bool>,
    #[serde(default, rename = "bool-format")]
    pub bool_format: Option<bool>,
    #[serde(default, rename = "hex-format")]
    pub hex_format: Option<bool>,
}

/// `linters.settings.goconst` / `linters-settings.goconst`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GoconstSettings {
    #[serde(default, rename = "min-len")]
    pub min_len: Option<usize>,
    #[serde(default, rename = "min-occurrences")]
    pub min_occurrences: Option<usize>,
    #[serde(default, rename = "ignore-calls")]
    pub ignore_calls: Option<bool>,
    #[serde(default, rename = "ignore-tests")]
    pub ignore_tests: Option<bool>,
    #[serde(default, rename = "match-constant")]
    pub match_constant: Option<bool>,
    pub numbers: Option<bool>,
    pub min: Option<i64>,
    pub max: Option<i64>,
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
        if let Some(v) = map.get(serde_yaml::Value::String("dupl".into())) {
            if let Ok(s) = serde_yaml::from_value::<DuplSettings>(v.clone()) {
                out.dupl = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("misspell".into())) {
            if let Ok(s) = serde_yaml::from_value::<MisspellSettings>(v.clone()) {
                out.misspell = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("gocyclo".into())) {
            if let Ok(s) = serde_yaml::from_value::<GocycloSettings>(v.clone()) {
                out.gocyclo = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("gocognit".into())) {
            if let Ok(s) = serde_yaml::from_value::<GocognitSettings>(v.clone()) {
                out.gocognit = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("nestif".into())) {
            if let Ok(s) = serde_yaml::from_value::<NestifSettings>(v.clone()) {
                out.nestif = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("dogsled".into())) {
            if let Ok(s) = serde_yaml::from_value::<DogsledSettings>(v.clone()) {
                out.dogsled = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("funlen".into())) {
            if let Ok(s) = serde_yaml::from_value::<FunlenSettings>(v.clone()) {
                out.funlen = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("cyclop".into())) {
            if let Ok(s) = serde_yaml::from_value::<CyclopSettings>(v.clone()) {
                out.cyclop = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("lll".into())) {
            if let Ok(s) = serde_yaml::from_value::<LllSettings>(v.clone()) {
                out.lll = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("nakedret".into())) {
            if let Ok(s) = serde_yaml::from_value::<NakedretSettings>(v.clone()) {
                out.nakedret = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("nlreturn".into())) {
            if let Ok(s) = serde_yaml::from_value::<NlreturnSettings>(v.clone()) {
                out.nlreturn = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("predeclared".into())) {
            if let Ok(s) = serde_yaml::from_value::<PredeclaredSettings>(v.clone()) {
                out.predeclared = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("whitespace".into())) {
            if let Ok(s) = serde_yaml::from_value::<WhitespaceSettings>(v.clone()) {
                out.whitespace = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("mnd".into())) {
            if let Ok(s) = serde_yaml::from_value::<MndSettings>(v.clone()) {
                out.mnd = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("prealloc".into())) {
            if let Ok(s) = serde_yaml::from_value::<PreallocSettings>(v.clone()) {
                out.prealloc = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("tagalign".into())) {
            if let Ok(s) = serde_yaml::from_value::<TagalignSettings>(v.clone()) {
                out.tagalign = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("wsl".into())) {
            if let Ok(s) = serde_yaml::from_value::<WslSettings>(v.clone()) {
                out.wsl = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("perfsprint".into())) {
            if let Ok(s) = serde_yaml::from_value::<PerfsprintSettings>(v.clone()) {
                out.perfsprint = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("goconst".into())) {
            if let Ok(s) = serde_yaml::from_value::<GoconstSettings>(v.clone()) {
                out.goconst = s;
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
        bag.insert("dupl", self.dupl.to_guff_dupl());
        bag.insert("misspell", self.misspell.to_guff_misspell());
        bag.insert("gocyclo", self.gocyclo.to_guff_gocyclo());
        bag.insert("gocognit", self.gocognit.to_guff_gocognit());
        bag.insert("nestif", self.nestif.to_guff_nestif());
        bag.insert("dogsled", self.dogsled.to_guff_dogsled());
        bag.insert("funlen", self.funlen.to_guff_funlen());
        bag.insert("cyclop", self.cyclop.to_guff_cyclop());
        bag.insert("lll", self.lll.to_guff_lll());
        bag.insert("nakedret", self.nakedret.to_guff_nakedret());
        bag.insert("nlreturn", self.nlreturn.to_guff_nlreturn());
        bag.insert("predeclared", self.predeclared.to_guff_predeclared());
        bag.insert("whitespace", self.whitespace.to_guff_whitespace());
        bag.insert("mnd", self.mnd.to_guff_mnd());
        bag.insert("prealloc", self.prealloc.to_guff_prealloc());
        bag.insert("tagalign", self.tagalign.to_guff_tagalign());
        bag.insert("wsl", self.wsl.to_guff_wsl());
        bag.insert("perfsprint", self.perfsprint.to_guff_perfsprint());
        bag.insert("goconst", self.goconst.to_guff_goconst());
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
                    severity: rule.severity.clone(),
                })
                .collect()
        });
        guff_revive::Settings {
            severity: self.severity.clone(),
            rules,
            confidence: self.confidence,
            ignore_generated_header: self.ignore_generated_header,
        }
    }
}

impl DuplSettings {
    pub fn to_guff_dupl(&self) -> guff_dupl::Options {
        guff_dupl::Options {
            threshold: self.threshold.unwrap_or(guff_dupl::DEFAULT_THRESHOLD),
        }
    }
}

impl MisspellSettings {
    pub fn to_guff_misspell(&self) -> guff_misspell::Options {
        guff_misspell::Options {
            locale: self.locale.clone().unwrap_or_default(),
            ignore_words: self.ignore_words.clone(),
            extra_words: self
                .extra_words
                .iter()
                .map(|w| guff_misspell::ExtraWord {
                    typo: w.typo.clone(),
                    correction: w.correction.clone(),
                })
                .collect(),
            mode: self.mode.clone().unwrap_or_default(),
        }
    }
}

impl GocycloSettings {
    pub fn to_guff_gocyclo(&self) -> guff_style::GocycloOptions {
        let defaults = guff_style::GocycloOptions::default();
        guff_style::GocycloOptions {
            min_complexity: self.min_complexity.unwrap_or(defaults.min_complexity),
        }
    }
}

impl GocognitSettings {
    pub fn to_guff_gocognit(&self) -> guff_style::GocognitOptions {
        let defaults = guff_style::GocognitOptions::default();
        guff_style::GocognitOptions {
            min_complexity: self.min_complexity.unwrap_or(defaults.min_complexity),
        }
    }
}

impl NestifSettings {
    pub fn to_guff_nestif(&self) -> guff_style::NestifOptions {
        let defaults = guff_style::NestifOptions::default();
        guff_style::NestifOptions {
            min_complexity: self.min_complexity.unwrap_or(defaults.min_complexity),
        }
    }
}

impl DogsledSettings {
    pub fn to_guff_dogsled(&self) -> guff_style::DogsledOptions {
        let defaults = guff_style::DogsledOptions::default();
        guff_style::DogsledOptions {
            max_blank_identifiers: self
                .max_blank_identifiers
                .unwrap_or(defaults.max_blank_identifiers),
        }
    }
}

impl FunlenSettings {
    pub fn to_guff_funlen(&self) -> guff_style::FunlenOptions {
        let defaults = guff_style::FunlenOptions::default();
        guff_style::FunlenOptions {
            lines: self.lines.unwrap_or(defaults.lines),
            statements: self.statements.unwrap_or(defaults.statements),
            ignore_comments: self.ignore_comments.unwrap_or(defaults.ignore_comments),
        }
    }
}

impl CyclopSettings {
    pub fn to_guff_cyclop(&self) -> guff_style::CyclopOptions {
        let defaults = guff_style::CyclopOptions::default();
        guff_style::CyclopOptions {
            max_complexity: self.max_complexity.unwrap_or(defaults.max_complexity),
            package_average: self.package_average.unwrap_or(defaults.package_average),
            skip_tests: self.skip_tests.unwrap_or(defaults.skip_tests),
        }
    }
}

impl LllSettings {
    pub fn to_guff_lll(&self) -> guff_style::LllOptions {
        let defaults = guff_style::LllOptions::default();
        guff_style::LllOptions {
            line_length: self.line_length.unwrap_or(defaults.line_length),
            tab_width: self.tab_width.unwrap_or(defaults.tab_width),
        }
    }
}

impl NakedretSettings {
    pub fn to_guff_nakedret(&self) -> guff_style::NakedretOptions {
        let defaults = guff_style::NakedretOptions::default();
        guff_style::NakedretOptions {
            max_func_lines: self.max_func_lines.unwrap_or(defaults.max_func_lines),
            skip_test_files: self.skip_test_files.unwrap_or(defaults.skip_test_files),
        }
    }
}

impl NlreturnSettings {
    pub fn to_guff_nlreturn(&self) -> guff_style::NlreturnOptions {
        let defaults = guff_style::NlreturnOptions::default();
        guff_style::NlreturnOptions {
            block_size: self.block_size.unwrap_or(defaults.block_size),
        }
    }
}

impl PredeclaredSettings {
    pub fn to_guff_predeclared(&self) -> guff_style::PredeclaredOptions {
        let defaults = guff_style::PredeclaredOptions::default();
        guff_style::PredeclaredOptions {
            ignore: if self.ignore.is_empty() {
                defaults.ignore
            } else {
                self.ignore.clone()
            },
            qualified: self.qualified.unwrap_or(defaults.qualified),
        }
    }
}

impl WhitespaceSettings {
    pub fn to_guff_whitespace(&self) -> guff_style::WhitespaceOptions {
        let defaults = guff_style::WhitespaceOptions::default();
        guff_style::WhitespaceOptions {
            multi_if: self.multi_if.unwrap_or(defaults.multi_if),
            multi_func: self.multi_func.unwrap_or(defaults.multi_func),
        }
    }
}

impl MndSettings {
    pub fn to_guff_mnd(&self) -> guff_style::MndOptions {
        let defaults = guff_style::MndOptions::default();
        guff_style::MndOptions {
            checks: self.checks.clone().unwrap_or(defaults.checks),
            ignored_numbers: if self.ignored_numbers.is_empty() {
                defaults.ignored_numbers
            } else {
                self.ignored_numbers.clone()
            },
            ignored_files: self.ignored_files.clone(),
            ignored_functions: if self.ignored_functions.is_empty() {
                defaults.ignored_functions
            } else {
                self.ignored_functions.clone()
            },
        }
    }
}

impl PreallocSettings {
    pub fn to_guff_prealloc(&self) -> guff_style::PreallocOptions {
        let defaults = guff_style::PreallocOptions::default();
        guff_style::PreallocOptions {
            simple: self.simple.unwrap_or(defaults.simple),
            range_loops: self.range_loops.unwrap_or(defaults.range_loops),
            for_loops: self.for_loops.unwrap_or(defaults.for_loops),
        }
    }
}

impl TagalignSettings {
    pub fn to_guff_tagalign(&self) -> guff_style::TagalignOptions {
        let defaults = guff_style::TagalignOptions::default();
        guff_style::TagalignOptions {
            align: self.align.unwrap_or(defaults.align),
            sort: self.sort.unwrap_or(defaults.sort),
            order: if self.order.is_empty() {
                defaults.order
            } else {
                self.order.clone()
            },
            strict: self.strict.unwrap_or(defaults.strict),
        }
    }
}

impl WslSettings {
    pub fn to_guff_wsl(&self) -> guff_style::WslOptions {
        let defaults = guff_style::WslOptions::default();
        guff_style::WslOptions {
            strict_append: self.strict_append.unwrap_or(defaults.strict_append),
            allow_assign_and_call: self
                .allow_assign_and_call
                .unwrap_or(defaults.allow_assign_and_call),
            allow_assign_and_anything: self
                .allow_assign_and_anything
                .unwrap_or(defaults.allow_assign_and_anything),
            allow_multiline_assign: self
                .allow_multiline_assign
                .unwrap_or(defaults.allow_multiline_assign),
            allow_cuddle_with_calls: if self.allow_cuddle_with_calls.is_empty() {
                defaults.allow_cuddle_with_calls
            } else {
                self.allow_cuddle_with_calls.clone()
            },
            allow_cuddle_with_rhs: if self.allow_cuddle_with_rhs.is_empty() {
                defaults.allow_cuddle_with_rhs
            } else {
                self.allow_cuddle_with_rhs.clone()
            },
        }
    }
}

impl PerfsprintSettings {
    pub fn to_guff_perfsprint(&self) -> guff_style::PerfsprintOptions {
        let defaults = guff_style::PerfsprintOptions::default();
        guff_style::PerfsprintOptions {
            integer_format: self.integer_format.unwrap_or(defaults.integer_format),
            int_conversion: self.int_conversion.unwrap_or(defaults.int_conversion),
            error_format: self.error_format.unwrap_or(defaults.error_format),
            err_error: self.err_error.unwrap_or(defaults.err_error),
            errorf: self.errorf.unwrap_or(defaults.errorf),
            string_format: self.string_format.unwrap_or(defaults.string_format),
            sprintf1: self.sprintf1.unwrap_or(defaults.sprintf1),
            strconcat: self.strconcat.unwrap_or(defaults.strconcat),
            bool_format: self.bool_format.unwrap_or(defaults.bool_format),
            hex_format: self.hex_format.unwrap_or(defaults.hex_format),
        }
    }
}

impl GoconstSettings {
    pub fn to_guff_goconst(&self) -> guff_style::GoconstOptions {
        let defaults = guff_style::GoconstOptions::default();
        guff_style::GoconstOptions {
            min_len: self.min_len.unwrap_or(defaults.min_len),
            min_occurrences: self.min_occurrences.unwrap_or(defaults.min_occurrences),
            ignore_calls: self.ignore_calls.unwrap_or(defaults.ignore_calls),
            ignore_tests: self.ignore_tests.unwrap_or(defaults.ignore_tests),
            match_constant: self.match_constant.unwrap_or(defaults.match_constant),
            numbers: self.numbers.unwrap_or(defaults.numbers),
            number_min: self.min.unwrap_or(defaults.number_min),
            number_max: self.max.unwrap_or(defaults.number_max),
        }
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
  severity: warning
  rules:
    - name: enforce-map-style
      arguments: ["make"]
    - name: comments-density
      arguments: [15]
      severity: error
"#,
        )
        .unwrap();
        let s = LinterSettings::from_yaml(&yaml);
        assert_eq!(s.revive.severity.as_deref(), Some("warning"));
        assert_eq!(s.revive.rules.as_ref().map(|r| r.len()), Some(2));
        assert_eq!(s.revive.rules.as_ref().unwrap()[0].name, "enforce-map-style");
        assert_eq!(
            s.revive.rules.as_ref().unwrap()[1].severity.as_deref(),
            Some("error")
        );
        let bag = s.to_bag();
        let revive = bag
            .get::<guff_revive::Settings>("revive")
            .expect("revive settings");
        assert_eq!(revive.severity.as_deref(), Some("warning"));
        assert!(revive.rule("enforce-map-style").is_some());
        assert_eq!(revive.rule_severity("comments-density"), Some("error"));
        assert_eq!(revive.rule_severity("enforce-map-style"), Some("warning"));
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
