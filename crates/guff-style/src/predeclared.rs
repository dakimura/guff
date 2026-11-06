//! Port of [`github.com/nishanths/predeclared`](https://github.com/nishanths/predeclared)
//! v0.3.0 (golangci-lint wrapper in `pkg/golinters/predeclared`).
//!
//! v0.3.0 works on type information rather than declaration syntax: every
//! object in `Defs`, plus the implicit ones of import specs, type-switch case
//! clauses and anonymous fields, is reported per mode — `d` when its name is
//! predeclared, `s` when it also shadows the universe object at its position.
//! golangci passes `pkglevel: false`, so that flag is not ported.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::walk::{preorder, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::{ObjectArena, ObjectData, ObjectId, ScopeArena, ScopeId};

use crate::options::PredeclaredOptions;

/// Go `doc.IsPredeclared` identifier set.
fn is_predeclared(name: &str) -> bool {
    matches!(
        name,
        // types
        "any"
            | "bool"
            | "byte"
            | "comparable"
            | "complex64"
            | "complex128"
            | "error"
            | "float32"
            | "float64"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "rune"
            | "string"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uintptr"
            // funcs
            | "append"
            | "cap"
            | "clear"
            | "close"
            | "complex"
            | "copy"
            | "delete"
            | "imag"
            | "len"
            | "make"
            | "max"
            | "min"
            | "new"
            | "panic"
            | "print"
            | "println"
            | "real"
            | "recover"
            // constants
            | "false"
            | "iota"
            | "nil"
            | "true"
    )
}

fn message_prefix(mode: char) -> &'static str {
    match mode {
        'd' => "same name as",
        _ => "shadows",
    }
}

/// `parseMode`: the distinct `d`/`s` letters, sorted. golangci only ever
/// passes those two letters and never an empty string.
fn parse_mode(s: &str) -> Vec<char> {
    let mut modes: Vec<char> = s.chars().filter(|c| matches!(c, 'd' | 's')).collect();
    modes.sort_unstable();
    modes.dedup();
    if modes.is_empty() {
        modes.push('s');
    }
    modes
}

/// `lookupWhile(obj.Parent(), name, obj.Pos(), obj2 == obj)`: the object the
/// name resolves to at its own declaration, skipping the object itself.
fn lookup_while(
    scopes: &ScopeArena,
    objects: &ObjectArena,
    start: Option<ScopeId>,
    name: &str,
    pos: u32,
    obj: ObjectId,
) -> Option<(ScopeId, ObjectId)> {
    let mut s = start;
    loop {
        let found = s.and_then(|cur| guff_types::scope::lookup_parent(scopes, objects, cur, name, pos));
        match (found, s) {
            (Some((_, c)), Some(cur)) if c == obj => s = scopes.get(cur).parent(),
            _ => return found,
        }
    }
}

/// `shadowsPredeclared`: the lookup lands in the universe (the only scope
/// without a parent). A field or method has no parent scope and never does.
fn shadows_predeclared(scopes: &ScopeArena, objects: &ObjectArena, obj: ObjectId) -> bool {
    let name = obj.name(objects);
    matches!(
        lookup_while(scopes, objects, obj.parent(objects), name, obj.pos(objects), obj),
        Some((scope, _)) if scopes.get(scope).parent().is_none()
    )
}

struct Reporter {
    modes: Vec<char>,
    check_fields: bool,
    ignore: HashSet<String>,
    /// `reportAtMostOnce`: the case clauses of `switch x := v.(type)` each
    /// declare their own `x`, all at the same position.
    reported: HashMap<char, HashSet<u32>>,
    pending: Vec<(u32, String)>,
}

impl Reporter {
    fn report(&mut self, mode: char, pos: u32, name: &str) {
        if self.reported.entry(mode).or_default().insert(pos) {
            self.pending
                .push((pos, format!("{name}: {} predeclared identifier", message_prefix(mode))));
        }
    }

    /// `handleObject`.
    fn handle_object(&mut self, scopes: &ScopeArena, objects: &ObjectArena, obj: ObjectId) {
        let name = obj.name(objects);
        if self.ignore.contains(name) || !is_predeclared(name) {
            return;
        }
        // Go: a struct field or method has a nil `Parent()`.
        let field_or_method = match objects.get(obj) {
            ObjectData::Var(v) => v.is_field(),
            ObjectData::Func(_) => obj.parent(objects).is_none(),
            _ => false,
        };
        for mode in self.modes.clone() {
            match mode {
                'd' if field_or_method && !self.check_fields => continue,
                's' if field_or_method || !shadows_predeclared(scopes, objects, obj) => continue,
                _ => {}
            }
            let name = name.to_string();
            self.report(mode, obj.pos(objects), &name);
        }
    }

    /// A label is a `*types.Label` upstream: never a shadow, but reported in
    /// declare mode. guff has no label object, so it is read off the syntax.
    fn handle_label(&mut self, label: &guff::ast::Ident) {
        if self.ignore.contains(&label.name) || !is_predeclared(&label.name) {
            return;
        }
        if self.modes.contains(&'d') {
            self.report('d', label.name_pos.0 as u32, &label.name);
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "predeclared requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<PredeclaredOptions>("predeclared")
        .cloned()
        .unwrap_or_default();
    // golangci joins the list with "," and predeclared splits and trims it.
    let ignore: HashSet<String> = options
        .ignore
        .join(",")
        .split(',')
        .filter(|_| !options.ignore.is_empty())
        .map(|n| n.trim().to_string())
        .collect();

    let (Some(info), Some(artifacts)) = (pass.types_info(), pass.pkg().type_artifacts.as_ref()) else {
        return Ok(None);
    };
    let (scopes, objects) = (&artifacts.scopes, &artifacts.objects);

    let mut pending = Vec::new();
    for file in pass.files() {
        let mut r = Reporter {
            modes: parse_mode(&options.mode),
            check_fields: options.qualified,
            ignore: ignore.clone(),
            reported: HashMap::new(),
            pending: Vec::new(),
        };
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::Ident(id) => {
                    if let Some(Some(obj)) = info.defs.get(&id.id).copied() {
                        r.handle_object(scopes, objects, obj);
                    }
                }
                NodeRef::ImportSpec(spec) => {
                    if let Some(obj) = info.implicits.get(&spec.id).copied() {
                        r.handle_object(scopes, objects, obj);
                    }
                }
                NodeRef::CaseClause(cc) => {
                    if let Some(obj) = info.implicits.get(&cc.id).copied() {
                        r.handle_object(scopes, objects, obj);
                    }
                }
                NodeRef::Field(f) => {
                    if let Some(obj) = info.implicits.get(&f.id).copied() {
                        r.handle_object(scopes, objects, obj);
                    }
                }
                NodeRef::LabeledStmt(s) => r.handle_label(&s.label),
                _ => {}
            }
            true
        });
        pending.append(&mut r.pending);
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "predeclared",
        doc: "find code that shadows one of Go's predeclared identifiers",
        url: "https://github.com/nishanths/predeclared",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
