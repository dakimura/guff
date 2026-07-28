//! `time-naming` — `time.Duration` vars should not use unit-specific suffixes.

use guff::ast::{Decl, File, Spec, ValueSpec};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_duration_type;

const TIME_SUFFIXES: &[&str] = &[
    "Hour", "Hours", "Min", "Mins", "Minutes", "Minute", "Sec", "Secs", "Seconds", "Second",
    "Msec", "Msecs", "Milli", "Millis", "Milliseconds", "Millisecond", "Usec", "Usecs",
    "Microseconds", "Microsecond", "MS", "Ms",
];

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
}

impl<'a> Checker<'a> {
    pub fn new(pass: &'a Pass<'a>) -> Self {
        Self {
            pass,
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        // Package-level const/var only (mirrors the previous file.decls walk).
        let NodeRef::File(file) = n else {
            return;
        };
        check_file(self.pass, file, &mut self.failures);
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

fn check_file(pass: &Pass<'_>, file: &File, failures: &mut Vec<Failure>) {
    for decl in &file.decls {
        let Decl::GenDecl(g) = decl else {
            continue;
        };
        if !matches!(g.tok, Some(Token::VAR) | Some(Token::CONST)) {
            continue;
        }
        for spec in &g.specs {
            let Spec::ValueSpec(ValueSpec { names, .. }) = spec else {
                continue;
            };
            for name in names {
                let Some(info) = pass.types_info() else {
                    continue;
                };
                let Some(typ) = info.types.get(&name.id).map(|tv| tv.typ) else {
                    continue;
                };
                if !is_duration_type(pass, typ) {
                    continue;
                }
                let suffix = TIME_SUFFIXES
                    .iter()
                    .find(|s| name.name.ends_with(*s))
                    .copied();
                let Some(suffix) = suffix else {
                    continue;
                };
                let type_str = pass
                    .pkg()
                    .type_artifacts
                    .as_ref()
                    .map(|a| {
                        guff_types::typestring::type_string(
                            &a.types,
                            &a.objects,
                            &a.packages,
                            typ,
                            None,
                        )
                    })
                    .unwrap_or_else(|| "time.Duration".into());
                failures.push(Failure {
                    rule: "time-naming",
                    pos: name.name_pos.0 as u32,
                    message: format!(
                        "var {} is of type {}; don't use unit-specific suffix {:?}",
                        name.name, type_str, suffix
                    ),
                    confidence: None,
                });
            }
        }
    }
}
