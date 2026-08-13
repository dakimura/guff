//! `redefines-builtin-id` — warn when a builtin identifier is shadowed.

use guff::ast::{AssignStmt, Expr, FuncDecl, FuncType, GenDecl, Ident, Spec, TypeSpec, ValueSpec};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

pub struct Checker {
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        match n {
            NodeRef::GenDecl(g) => check_gen_decl(g, &mut self.failures),
            NodeRef::FuncDecl(f) => check_func_decl(f, &mut self.failures),
            NodeRef::FuncType(ft) => check_func_type(ft, &mut self.failures),
            NodeRef::AssignStmt(a) => check_assign(a, &mut self.failures),
            _ => {}
        }
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new();
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

fn builtin_kind(name: &str) -> Option<&'static str> {
    if BUILTIN_FUNCS.contains(&name) {
        return Some("function");
    }
    if BUILTIN_CONST_VARS.contains(&name) {
        return Some("constant or variable");
    }
    if BUILTIN_TYPES.contains(&name) {
        return Some("type");
    }
    None
}

fn check_gen_decl(g: &GenDecl, failures: &mut Vec<Failure>) {
    let Some(tok) = g.tok else {
        return;
    };
    // Upstream hands `addFailure` the **GenDecl**, not the name — so the
    // failure lands on the `var` / `const` / `type` keyword. The two spellings
    // only differ once the declaration is not a short one:
    //
    //     len := 1        // reported at `len`, which is also the statement
    //     var len int = 1 // reported at `var`, not at `len`
    //
    // `compat/fuzz.py`'s `littype` mutation writes the second form from the
    // first, which is how this surfaced (COMPAT-HARDENING §4, 2026-08-13).
    let pos = g.tok_pos.0 as u32;
    match tok {
        Token::TYPE => {
            // Upstream looks at `n.Specs[0]` only and stops descending.
            let Some(Spec::TypeSpec(TypeSpec { name, .. })) = g.specs.first() else {
                return;
            };
            if let Some(kind) = builtin_kind(&name.name) {
                add_failure(
                    pos,
                    failures,
                    format!("redefinition of the built-in {} {}", kind, name.name),
                );
            }
        }
        Token::VAR | Token::CONST => {
            for spec in &g.specs {
                let Spec::ValueSpec(ValueSpec { names, .. }) = spec else {
                    continue;
                };
                for name in names {
                    if let Some(kind) = builtin_kind(&name.name) {
                        add_failure(
                            pos,
                            failures,
                            format!("redefinition of the built-in {} {}", kind, name.name),
                        );
                    }
                }
            }
        }
        _ => {}
    }
}

fn check_func_decl(f: &FuncDecl, failures: &mut Vec<Failure>) {
    if f.recv.is_some() {
        return;
    }
    if let Some(kind) = builtin_kind(&f.name.name) {
        add_failure(
            f.name.name_pos.0 as u32,
            failures,
            format!(
                "redefinition of the built-in {} {}",
                kind, f.name.name
            ),
        );
    }
}

fn check_func_type(ft: &FuncType, failures: &mut Vec<Failure>) {
    let mut fields = Vec::new();
    if let Some(tp) = &ft.type_params {
        fields.extend(&tp.list);
    }
    if let Some(params) = &ft.params {
        fields.extend(&params.list);
    }
    if let Some(results) = &ft.results {
        fields.extend(&results.list);
    }
    for field in fields {
        for name in &field.names {
            if let Some(kind) = builtin_kind(&name.name) {
                add_failure(
                    name.name_pos.0 as u32,
                    failures,
                    format!(
                        "redefinition of the built-in {} {}",
                        kind, name.name
                    ),
                );
            }
        }
    }
}

fn check_assign(assign: &AssignStmt, failures: &mut Vec<Failure>) {
    for lhs in &assign.lhs {
        let Expr::Ident(Ident { name, name_pos, .. }) = lhs else {
            continue;
        };
        let Some(kind) = builtin_kind(name) else {
            continue;
        };
        let msg = match kind {
            "constant or variable" => {
                if assign.tok == Some(Token::DEFINE) {
                    format!("assignment creates a shadow of built-in identifier {name}")
                } else {
                    format!("assignment modifies built-in identifier {name}")
                }
            }
            _ => format!("redefinition of the built-in {kind} {name}"),
        };
        add_failure(name_pos.0 as u32, failures, msg);
    }
}

fn add_failure(pos: u32, failures: &mut Vec<Failure>, message: String) {
    failures.push(Failure {
        rule: "redefines-builtin-id",
        pos,
        message,
        ..Failure::default()
    });
}

const BUILTIN_CONST_VARS: &[&str] = &["true", "false", "iota", "nil"];

const BUILTIN_FUNCS: &[&str] = &[
    "append", "cap", "clear", "close", "complex", "copy", "delete", "imag", "len", "make", "max",
    "min", "new", "panic", "print", "println", "real", "recover",
];

const BUILTIN_TYPES: &[&str] = &[
    "any", "bool", "byte", "complex128", "complex64", "error", "float32", "float64", "int",
    "int16", "int32", "int64", "int8", "rune", "string", "uint", "uint16", "uint32", "uint64",
    "uint8", "uintptr",
];
