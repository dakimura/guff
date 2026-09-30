//! `dot-imports` — forbid dot imports.
//!
//! Port of revive v1.15.0 `rule/dot_imports.go`. The one argument is a map
//! whose `allowedPackages` key lists import paths that may be dot-imported;
//! lima allows its own `pkg/must` that way. The key is matched as revive's
//! `isRuleOption` does — lowercased with hyphens removed, so `allowedPackages`,
//! `allowed-packages` and `AllowedPackages` all count, but `allowed_packages`
//! does not (guff's shared `config::is_rule_option` also drops underscores,
//! which is why this rule does not use it).

use std::collections::HashSet;

use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;
use crate::settings::RuleArgument;
use crate::util::import_spec_pos;

pub struct Checker {
    failures: Vec<Failure>,
    /// Allowed import paths, as written in the source: quoted.
    allowed: HashSet<String>,
}

/// revive `normalizeRuleOption`: lowercase, hyphens removed.
fn normalize_rule_option(s: &str) -> String {
    s.replace('-', "").to_lowercase()
}

fn allowed_packages(pass: &Pass<'_>) -> HashSet<String> {
    let mut out = HashSet::new();
    let args = config::rule_arguments(pass, "dot-imports");
    let Some(RuleArgument::Map(map)) = args.first() else {
        return out;
    };
    for (k, v) in map {
        if normalize_rule_option(k) != normalize_rule_option("allowedPackages") {
            continue;
        }
        if let RuleArgument::List(items) = v {
            for item in items {
                if let RuleArgument::String(path) = item {
                    // `ap[strconv.Quote(pkg)]`: compared with the quoted literal.
                    out.insert(format!("{path:?}"));
                }
            }
        }
    }
    out
}

impl Checker {
    pub fn new(pass: &Pass<'_>) -> Self {
        Self {
            failures: Vec::new(),
            allowed: allowed_packages(pass),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::ImportSpec(imp) = n else {
            return;
        };
        let is_dot = imp.name.as_ref().is_some_and(|n| n.name == ".");
        if is_dot && !self.allowed.contains(&imp.path.value) {
            self.failures.push(Failure {
                rule: "dot-imports",
                pos: import_spec_pos(imp),
                message: "should not use dot imports".into(),
                ..Failure::default()
            });
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
