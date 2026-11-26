//! `time-naming` — `time.Duration` vars should not use unit-specific suffixes.

use guff::ast::ValueSpec;
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
        // Upstream's visitor matches `*ast.ValueSpec` anywhere in the file, so
        // a `var` inside a function counts too. Walking `file.decls` instead
        // saw only package-level declarations.
        let NodeRef::ValueSpec(spec) = n else {
            return;
        };
        check_spec(self.pass, spec, &mut self.failures);
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

fn check_spec(pass: &Pass<'_>, spec: &ValueSpec, failures: &mut Vec<Failure>) {
    let Some(info) = pass.types_info() else {
        return;
    };
    for name in &spec.names {
        // Upstream resolves the type first and matches the suffix second; the
        // reported set is the same either way, and this order keeps
        // `is_duration_type` — which renders the type to a string — off every
        // variable in the package.
        let Some(suffix) = TIME_SUFFIXES
            .iter()
            .find(|s| name.name.ends_with(*s))
            .copied()
        else {
            continue;
        };
        // The names of a ValueSpec are *definitions*: `Info.Types` holds
        // expression types and has no entry for them. Upstream reads
        // `Pkg.TypeOf(name)`, which falls back to `Defs[name].Type()` — reading
        // `Info.Types` here meant the rule never fired at all.
        let Some(obj) = info.defs.get(&name.id).copied().flatten() else {
            continue;
        };
        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
            continue;
        };
        let Some(typ) = obj.typ(&artifacts.objects) else {
            continue;
        };
        if !is_duration_type(pass, typ) {
            continue;
        }
        let type_str = guff_types::typestring::type_string(
            &artifacts.types,
            &artifacts.objects,
            &artifacts.packages,
            typ,
            None,
        );
        failures.push(Failure {
            rule: "time-naming",
            pos: name.name_pos.0 as u32,
            message: format!(
                "var {} is of type {}; don't use unit-specific suffix {:?}",
                name.name, type_str, suffix
            ),
            ..Failure::default()
        });
    }
}
