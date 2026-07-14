//! guff-errcheck — unchecked error return detection.
//!
//! Port of [`github.com/kisielk/errcheck`](https://github.com/kisielk/errcheck).

mod excludes;

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, CallExpr, Expr, ExprStmt, GenDecl, GoStmt, Ident, IndexExpr, IndexListExpr,
    ParenExpr, Spec, TypeAssertExpr,
};
use guff::walk::NodeRef;
use guff_analysis::code::{self, type_with_name};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::ObjectData;
use guff_types::TypeId;

use expreq::unparen;

/// Options controlling errcheck behaviour (kisielk/errcheck flags).
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// When true, flag errors assigned to `_` (`r, _ := fn()`).
    pub check_blank: bool,
    /// When true, flag ignored type assertion results.
    pub check_asserts: bool,
}

fn make_analyzer(run: RunFn) -> Analyzer {
    Analyzer {
        name: "errcheck",
        doc: "check that errors returned by functions are handled",
        url: "https://github.com/kisielk/errcheck",
        run,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

fn run_default(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let opts = pass
        .settings::<Options>("errcheck")
        .copied()
        .unwrap_or_default();
    run(pass, opts)
}

fn run_blank(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    run(
        pass,
        Options {
            check_blank: true,
            ..Options::default()
        },
    )
}

fn run_asserts(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    run(
        pass,
        Options {
            check_asserts: true,
            ..Options::default()
        },
    )
}

fn run(pass: &mut Pass<'_>, opts: Options) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "errcheck requires inspect analyzer".to_string())?
        .clone();

    let exclude: HashSet<&str> = excludes::DEFAULT_EXCLUDED_SYMBOLS.iter().copied().collect();
    let mut pending: Vec<(u32, String)> = Vec::new();
    let mut visitor = Visitor {
        pass,
        exclude: &exclude,
        opts,
        pending: &mut pending,
        skip_assert_positions: HashSet::new(),
    };

    inspect.preorder(pass.files(), |n| {
        match n {
            NodeRef::ExprStmt(ExprStmt { x, .. }) => visitor.visit_expr_stmt(x),
            NodeRef::GoStmt(GoStmt { call, .. }) => visitor.visit_go_defer(call),
            NodeRef::DeferStmt(guff::ast::DeferStmt { call, .. }) => visitor.visit_go_defer(call),
            NodeRef::AssignStmt(s) => visitor.visit_assign(s),
            NodeRef::GenDecl(g) => visitor.visit_gendecl(g),
            NodeRef::TypeAssertExpr(t) => visitor.visit_type_assert(t),
            _ => {}
        }
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

struct Visitor<'a, 'b> {
    pass: &'a Pass<'b>,
    exclude: &'a HashSet<&'static str>,
    opts: Options,
    pending: &'a mut Vec<(u32, String)>,
    skip_assert_positions: HashSet<u32>,
}

impl Visitor<'_, '_> {
    fn report(&mut self, pos: u32) {
        self.pending.push((pos, "unchecked error".into()));
    }

    fn visit_expr_stmt(&mut self, x: &Expr) {
        if let Expr::CallExpr(call) = x {
            if !self.ignore_call(call) && self.call_returns_error(call) {
                self.report(call.lparen.0 as u32);
            }
        }
    }

    fn visit_go_defer(&mut self, call: &CallExpr) {
        if !self.ignore_call(call) && self.call_returns_error(call) {
            self.report(call.lparen.0 as u32);
        }
    }

    fn visit_assign(&mut self, s: &AssignStmt) {
        self.check_assignment(&s.lhs, &s.rhs);
        if self.opts.check_asserts && s.rhs.len() == 1 {
            if let Expr::TypeAssertExpr(t) = unparen(&s.rhs[0]) {
                if t.ty.is_some() {
                    self.skip_assert_positions.insert(t.lparen.0 as u32);
                }
            }
        }
    }

    fn visit_gendecl(&mut self, g: &GenDecl) {
        if g.tok != Some(guff::token::Token::VAR) {
            return;
        }
        for spec in &g.specs {
            let Spec::ValueSpec(vs) = spec else { continue };
            if vs.values.is_empty() {
                continue;
            }
            let lhs: Vec<Expr> = vs.names.iter().map(|n| Expr::Ident(n.clone())).collect();
            self.check_assignment(&lhs, &vs.values);
        }
    }

    fn visit_type_assert(&mut self, t: &TypeAssertExpr) {
        if !self.opts.check_asserts || t.ty.is_none() {
            return;
        }
        let pos = t.lparen.0 as u32;
        if self.skip_assert_positions.contains(&pos) {
            return;
        }
        self.report(pos);
    }

    fn check_assignment(&mut self, lhs: &[Expr], rhs: &[Expr]) -> bool {
        if rhs.len() == 1 {
            if let Expr::CallExpr(call) = unparen(&rhs[0]) {
                if !self.opts.check_blank {
                    return true;
                }
                if self.ignore_call(call) {
                    return true;
                }
                let is_error = self.errors_by_arg(call);
                for (i, l) in lhs.iter().enumerate() {
                    let Expr::Ident(id) = unparen(l) else {
                        continue;
                    };
                    if id.name != "_" {
                        continue;
                    }
                    if self.is_recover(call) || is_error.get(i).copied().unwrap_or(false) {
                        self.report(id.name_pos.0 as u32);
                    }
                }
            } else if let Expr::TypeAssertExpr(assert) = unparen(&rhs[0]) {
                if !self.opts.check_asserts {
                    return false;
                }
                if assert.ty.is_none() {
                    return false;
                }
                if lhs.len() < 2 {
                    self.report(rhs[0].pos().0 as u32);
                } else if let Expr::Ident(id) = unparen(&lhs[1]) {
                    if self.opts.check_blank && id.name == "_" {
                        self.report(id.name_pos.0 as u32);
                    }
                }
                return false;
            }
        } else {
            for (i, l) in lhs.iter().enumerate() {
                let Expr::Ident(id) = unparen(l) else {
                    continue;
                };
                if id.name != "_" {
                    continue;
                }
                if let Some(Expr::CallExpr(call)) = rhs.get(i) {
                    if !self.opts.check_blank {
                        continue;
                    }
                    if self.ignore_call(call) {
                        continue;
                    }
                    if self.call_returns_error(call) {
                        self.report(id.name_pos.0 as u32);
                    }
                } else if let Some(Expr::TypeAssertExpr(assert)) = rhs.get(i) {
                    if !self.opts.check_asserts || assert.ty.is_none() {
                        continue;
                    }
                    self.report(id.name_pos.0 as u32);
                }
            }
        }
        true
    }

    fn ignore_call(&self, call: &CallExpr) -> bool {
        self.exclude_call(call)
    }

    fn exclude_call(&self, call: &CallExpr) -> bool {
        let arg0 = call.args.first().map(|a| self.arg_name(a)).unwrap_or_default();
        for name in self.names_for_exclude(call) {
            if self.exclude.contains(name.as_str()) {
                return true;
            }
            if !arg0.is_empty() {
                let with_arg = format!("{name}({arg0})");
                if self.exclude.contains(with_arg.as_str()) {
                    return true;
                }
            }
        }
        false
    }

    fn names_for_exclude(&self, call: &CallExpr) -> Vec<String> {
        let Some(obj) = self.call_target_object(call) else {
            return Vec::new();
        };
        let artifacts = match self.pass.pkg().type_artifacts.as_ref() {
            Some(a) => a,
            None => return Vec::new(),
        };
        let mut names = Vec::new();
        if let Some(sel_name) = code::call_name(self.pass, &call.fun) {
            names.push(sel_name);
        }
        names.push(code::type_func_name(
            &artifacts.types,
            &artifacts.objects,
            &artifacts.packages,
            obj,
        ));
        names
    }

    fn arg_name(&self, expr: &Expr) -> String {
        if let Expr::SelectorExpr(sel) = expr {
            if let Some(obj) = self.pass.types_info().and_then(|i| i.uses.get(&sel.sel.id)) {
                let artifacts = self.pass.pkg().type_artifacts.as_ref().unwrap();
                if let ObjectData::Var(_) = artifacts.objects.get(*obj) {
                    if let Some(pkg) = obj.pkg(&artifacts.objects) {
                        if artifacts.packages.get(pkg).name() == "os"
                            && (obj.name(&artifacts.objects) == "Stderr"
                                || obj.name(&artifacts.objects) == "Stdout")
                        {
                            return format!("os.{}", obj.name(&artifacts.objects));
                        }
                    }
                }
            }
        }
        let info = match self.pass.types_info() {
            Some(i) => i,
            None => return String::new(),
        };
        let artifacts = match self.pass.pkg().type_artifacts.as_ref() {
            Some(a) => a,
            None => return String::new(),
        };
        let Some(tav) = info.types.get(&expr.id()) else {
            return String::new();
        };
        guff_types::typestring::type_string(
            &artifacts.types,
            &artifacts.objects,
            &artifacts.packages,
            tav.typ,
            None,
        )
    }

    fn call_target_object(&self, call: &CallExpr) -> Option<guff_types::arena::ObjectId> {
        let fun = base_call_expr(&call.fun);
        code::call_target_object(self.pass, fun)
    }

    fn errors_by_arg(&self, call: &CallExpr) -> Vec<bool> {
        let info = match self.pass.types_info() {
            Some(i) => i,
            None => return vec![false],
        };
        let Some(tav) = info.types.get(&call.id) else {
            return vec![false];
        };
        result_type_errors(self.pass, tav.typ)
    }

    fn call_returns_error(&self, call: &CallExpr) -> bool {
        if self.is_recover(call) {
            return true;
        }
        self.errors_by_arg(call).iter().any(|&e| e)
    }

    fn is_recover(&self, call: &CallExpr) -> bool {
        matches!(base_call_expr(&call.fun), Expr::Ident(Ident { name, .. }) if name == "recover")
    }
}

fn base_call_expr(fun: &Expr) -> &Expr {
    let mut cur = fun;
    loop {
        cur = match cur {
            Expr::IndexExpr(IndexExpr { x, .. }) => x,
            Expr::IndexListExpr(IndexListExpr { x, .. }) => x,
            Expr::ParenExpr(ParenExpr { x, .. }) => x,
            _ => return cur,
        };
    }
}

fn result_type_errors(pass: &Pass<'_>, typ: TypeId) -> Vec<bool> {
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return vec![false],
    };
    match artifacts.types.get(typ) {
        guff_types::arena::TypeData::Tuple(t) => (0..t.len())
            .map(|i| {
                t.at(i)
                    .typ(&artifacts.objects)
                    .is_some_and(|rt| is_error_type(pass, rt))
            })
            .collect(),
        _ => vec![is_error_type(pass, typ)],
    }
}

fn is_error_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    type_with_name(pass, typ, "error")
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| make_analyzer(run_default as RunFn))
}

/// Analyzer with blank-assignment checking enabled (for tests / strict mode).
pub fn analyzer_check_blank() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| make_analyzer(run_blank as RunFn))
}

/// Analyzer with type-assertion checking enabled (for tests / strict mode).
pub fn analyzer_check_asserts() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| make_analyzer(run_asserts as RunFn))
}

pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![analyzer()]
}

mod expreq {
    use guff::ast::{Expr, ParenExpr};
    pub fn unparen<'a>(e: &'a Expr) -> &'a Expr {
        let mut cur = e;
        while let Expr::ParenExpr(ParenExpr { x, .. }) = cur {
            cur = x;
        }
        cur
    }
}