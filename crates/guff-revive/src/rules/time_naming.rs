//! `time-naming` — `time.Duration` vars should not use unit-specific suffixes.

use guff::ast::{Decl, Spec, ValueSpec};
use guff::token::Token;
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_duration_type;

const TIME_SUFFIXES: &[&str] = &[
    "Hour", "Hours", "Min", "Mins", "Minutes", "Minute", "Sec", "Secs", "Seconds", "Second",
    "Msec", "Msecs", "Milli", "Millis", "Milliseconds", "Millisecond", "Usec", "Usecs",
    "Microseconds", "Microsecond", "MS", "Ms",
];

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::GenDecl(g) = decl else {
                continue;
            };
            if !matches!(g.tok, Some(Token::VAR) | Some(Token::CONST)) {
                continue;
            }
            for spec in &g.specs {
                let Spec::ValueSpec(ValueSpec { names, ty, .. }) = spec else {
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
                    });
                }
                let _ = ty;
            }
        }
    }
    failures
}
