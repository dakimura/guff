//! `import-alias-naming` — enforce conventions for import alias names.

use guff::walk::{self, NodeRef};
use guff_analysis::Pass;
use regex::Regex;
use std::sync::OnceLock;

use crate::failure::Failure;
use crate::settings::RuleArgument;

const DEFAULT_ALLOW: &str = "^[a-z][a-z0-9]{0,}$";

fn default_allow() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(DEFAULT_ALLOW).expect("valid regex"))
}

/// `Configure`: `arguments[0]` is either a string — the allow expression — or a
/// map with `allowRegex` / `denyRegex`.
///
/// ```go
/// if len(arguments) == 0 { r.allowRegexp = defaultImportAliasNamingAllowRegexp; return nil }
/// switch namingRule := arguments[0].(type) {
/// case string:          r.setAllowRule(namingRule)
/// case map[string]any:  … isRuleOption(k, "allowRegex") / "denyRegex" …
/// }
/// if r.allowRegexp == nil && r.denyRegexp == nil {
///     r.allowRegexp = defaultImportAliasNamingAllowRegexp
/// }
/// ```
///
/// guff had the default baked in and no deny side at all, so telegraf's
/// `^[a-z][a-z0-9_]*[a-z0-9]+$` — which allows the underscore its aliases use —
/// was 128 findings golangci-lint does not make.
///
/// An expression that does not compile is a configuration *error* upstream;
/// here it leaves that side unset, so a typo cannot silently reject every
/// alias.
fn configure(pass: &Pass<'_>) -> (Option<Regex>, Option<Regex>) {
    let args = crate::config::rule_arguments(pass, "import-alias-naming");
    let mut allow: Option<Regex> = None;
    let mut deny: Option<Regex> = None;
    match args.first() {
        Some(RuleArgument::String(s)) => allow = Regex::new(s).ok(),
        Some(RuleArgument::Map(m)) => {
            for (k, v) in m {
                let RuleArgument::String(s) = v else {
                    continue;
                };
                if crate::config::rule_option_matches(k, "allowRegex") {
                    allow = Regex::new(s).ok();
                } else if crate::config::rule_option_matches(k, "denyRegex") {
                    deny = Regex::new(s).ok();
                }
            }
        }
        _ => {}
    }
    (allow, deny)
}

pub struct Checker {
    allow: Option<Regex>,
    deny: Option<Regex>,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new(pass: &Pass<'_>) -> Self {
        let (mut allow, deny) = configure(pass);
        if allow.is_none() && deny.is_none() {
            allow = Some(default_allow().clone());
        }
        Self {
            allow,
            deny,
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::ImportSpec(imp) = n else {
            return;
        };
        let Some(alias) = &imp.name else {
            return;
        };
        // `_` and `.` are other rules' business.
        if alias.name == "_" || alias.name == "." {
            return;
        }
        if let Some(allow) = &self.allow {
            if !allow.is_match(&alias.name) {
                self.failures.push(Failure {
                    rule: "import-alias-naming",
                    pos: alias.name_pos.0 as u32,
                    message: format!(
                        "import name ({}) must match the regular expression: {}",
                        alias.name,
                        allow.as_str()
                    ),
                    ..Failure::default()
                });
            }
        }
        // Both sides can fire on one alias: upstream appends, it does not
        // `else`.
        if let Some(deny) = &self.deny {
            if deny.is_match(&alias.name) {
                self.failures.push(Failure {
                    rule: "import-alias-naming",
                    pos: alias.name_pos.0 as u32,
                    message: format!(
                        "import name ({}) must NOT match the regular expression: {}",
                        alias.name,
                        deny.as_str()
                    ),
                    ..Failure::default()
                });
            }
        }
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
