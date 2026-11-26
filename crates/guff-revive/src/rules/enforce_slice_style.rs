//! `enforce-slice-style` — enforce `make([]type, 0)`, `[]type{}`, or `var []type`.

use std::collections::{HashMap, HashSet};

use guff::ast::{ArrayType, BasicLit, CallExpr, CompositeLit, Expr, File};
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;
use crate::util::{is_ident, unparen};

#[derive(Clone, Copy, PartialEq, Eq)]
enum SliceStyle {
    Any,
    Make,
    Literal,
    Nil,
}

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    style: SliceStyle,
    failures: Vec<Failure>,
    /// Name positions of this file's TypeSpecs whose type is a slice, through
    /// any chain of same-file named types: what `isSliceType` reaches by
    /// following `Ident.Obj.Decl`, which the parser only sets within a file.
    slice_specs: HashSet<u32>,
    /// Ids of the expressions that are directly a ValueSpec's values — the
    /// `parent.(*ast.ValueSpec)` of revive v1.17.0's `nilSliceFailureMessage`.
    value_spec_values: HashSet<u32>,
}

impl<'a> Checker<'a> {
    pub fn try_new(pass: &'a Pass<'a>) -> Option<Self> {
        let style = slice_style(pass);
        if style == SliceStyle::Any {
            return None;
        }
        Some(Self {
            pass,
            style,
            failures: Vec::new(),
            slice_specs: HashSet::new(),
            value_spec_values: HashSet::new(),
        })
    }

    pub fn on_file(&mut self, file: &File) {
        let mut specs: HashMap<u32, &Expr> = HashMap::new();
        self.value_spec_values.clear();
        walk::preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::TypeSpec(ts) => {
                    specs.insert(ts.name.pos().0 as u32, &ts.ty);
                }
                NodeRef::ValueSpec(vs) => {
                    self.value_spec_values.extend(vs.values.iter().map(|v| v.id()));
                }
                _ => {}
            }
            true
        });
        self.slice_specs.clear();
        for (&pos, ty) in &specs {
            if spec_is_slice(self.pass, &specs, ty, 0) {
                self.slice_specs.insert(pos);
            }
        }
    }

    /// `isSliceType`.
    fn is_slice_type(&self, expr: &Expr) -> bool {
        match unparen(expr) {
            Expr::ArrayType(ArrayType { len, .. }) => len.is_none(),
            Expr::Ident(id) => decl_pos(self.pass, id).is_some_and(|p| self.slice_specs.contains(&p)),
            _ => false,
        }
    }

    /// `nilSliceFailureMessage`.
    fn nil_message(&self, expr_id: u32, instead: &str) -> String {
        if self.value_spec_values.contains(&expr_id) {
            format!("use nil slice declaration (e.g. var args []type) instead of {instead}")
        } else {
            format!("use nil slice (e.g. []type(nil)) instead of {instead}")
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        match n {
            NodeRef::CompositeLit(lit)
                if matches!(self.style, SliceStyle::Make | SliceStyle::Nil) =>
            {
                if lit.ty.as_deref().is_some_and(|t| self.is_slice_type(t)) && lit.elts.is_empty() {
                    let message = if self.style == SliceStyle::Nil {
                        self.nil_message(lit.id, "[]type{}")
                    } else {
                        "use make([]type) instead of []type{} (or declare nil slice)".into()
                    };
                    self.failures.push(Failure {
                        rule: "enforce-slice-style",
                        // A composite literal's Pos() is its *type*, which is where
                        // upstream points: `map[string]int{}` reports at `map`,
                        // not at the brace.
                        pos: lit
                            .ty
                            .as_ref()
                            .map(|t| t.pos().0)
                            .unwrap_or(lit.lbrace.0) as u32,
                        message,
                        ..Failure::default()
                    });
                }
            }
            NodeRef::CallExpr(call)
                if matches!(self.style, SliceStyle::Literal | SliceStyle::Nil) =>
            {
                if !is_ident(&call.fun, "make") || call.args.len() < 2 {
                    return;
                }
                if !self.is_slice_type(&call.args[0]) {
                    return;
                }
                let Expr::BasicLit(BasicLit { value, .. }) = unparen(&call.args[1]) else {
                    return;
                };
                if value != "0" {
                    return;
                }
                if call.args.len() > 2 {
                    let Expr::BasicLit(BasicLit { value: cap, .. }) = unparen(&call.args[2])
                    else {
                        return;
                    };
                    if cap != "0" {
                        return;
                    }
                }
                let message = if self.style == SliceStyle::Nil {
                    self.nil_message(call.id, "make([]type, 0)")
                } else {
                    "use []type{} instead of make([]type, 0) (or declare nil slice)".into()
                };
                self.failures.push(Failure {
                    rule: "enforce-slice-style",
                    pos: call.args[0].pos().0 as u32,
                    message,
                    ..Failure::default()
                });
            }
            _ => {}
        }
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let Some(mut c) = Checker::try_new(pass) else {
        return Vec::new();
    };
    for file in pass.files() {
        c.on_file(file);
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}

fn slice_style(pass: &Pass<'_>) -> SliceStyle {
    match config::rule_arg_string(pass, "enforce-slice-style", 0).as_deref() {
        Some("make") => SliceStyle::Make,
        Some("literal") => SliceStyle::Literal,
        Some("nil") => SliceStyle::Nil,
        _ => SliceStyle::Any,
    }
}

/// Where `Ident.Obj.Decl` would point: the defining identifier's position.
fn decl_pos(pass: &Pass<'_>, id: &guff::ast::Ident) -> Option<u32> {
    let obj = code::object_of(pass, id)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    Some(obj.pos(&artifacts.objects))
}

fn spec_is_slice(pass: &Pass<'_>, specs: &HashMap<u32, &Expr>, ty: &Expr, depth: u32) -> bool {
    if depth > 16 {
        return false;
    }
    match unparen(ty) {
        Expr::ArrayType(ArrayType { len, .. }) => len.is_none(),
        Expr::Ident(id) => decl_pos(pass, id)
            .and_then(|p| specs.get(&p))
            .is_some_and(|t| spec_is_slice(pass, specs, t, depth + 1)),
        _ => false,
    }
}
