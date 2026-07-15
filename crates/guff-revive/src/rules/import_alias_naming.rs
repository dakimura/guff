//! `import-alias-naming` — enforce conventions for import alias names.

use guff::ast::{Decl, Spec};
use guff_analysis::Pass;
use regex::Regex;
use std::sync::OnceLock;

use crate::failure::Failure;

fn default_allow() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-z][a-z0-9]{0,}$").expect("valid regex"))
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let allow = default_allow();
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::GenDecl(g) = decl else {
                continue;
            };
            for spec in &g.specs {
                let Spec::ImportSpec(imp) = spec else {
                    continue;
                };
                let Some(alias) = &imp.name else {
                    continue;
                };
                if alias.name == "_" || alias.name == "." {
                    continue;
                }
                if !allow.is_match(&alias.name) {
                    failures.push(Failure {
                        rule: "import-alias-naming",
                        pos: alias.name_pos.0 as u32,
                        message: format!(
                            "import name ({}) must match the regular expression: {}",
                            alias.name,
                            allow.as_str()
                        ),
                    });
                }
            }
        }
    }
    failures
}
