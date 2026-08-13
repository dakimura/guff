//! guff-errcheck — unchecked error return detection.
//!
//! Port of [`github.com/kisielk/errcheck`](https://github.com/kisielk/errcheck).

mod excludes;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, CallExpr, Expr, ExprStmt, GenDecl, GoStmt, Ident, IndexExpr, IndexListExpr,
    ParenExpr, Spec, TypeAssertExpr,
};
use guff::node_mask;
use guff::position::Pos;
use guff::walk::{NodeMask, NodeRef};
use guff_analysis::code::{self, type_with_name};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::api_predicates::api_implements;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::BasicKind;
use guff_types::operand::OperandMode;
use guff_types::TypeId;

/// Options controlling errcheck behaviour (kisielk/errcheck / golangci flags).
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// When true, flag errors assigned to `_` (`r, _ := fn()`).
    pub check_blank: bool,
    /// When true, flag ignored type assertion results.
    pub check_asserts: bool,
    /// When true, do not apply [`excludes::DEFAULT_EXCLUDED_SYMBOLS`]
    /// (`disable-default-exclusions`).
    pub disable_default_exclusions: bool,
    /// Extra symbols to skip (`exclude-functions`), kisielk/errcheck format
    /// (e.g. `io.Copy`, `(*net/http.Server).Shutdown`).
    pub exclude_functions: Vec<String>,
    /// Name the callee by its qualified `types.Func.FullName()` rather than by
    /// the selector as written (golangci `errcheck.verbose`).
    pub verbose: bool,
}

fn make_analyzer(run: RunFn) -> Analyzer {
    Analyzer {
        name: "errcheck",
        doc: "check that errors returned by functions are handled",
        url: "https://github.com/kisielk/errcheck",
        run,
        // Partial type info is still enough to see error results; skipping on
        // ill-typed packages drops real findings (k9s view OSS hunt).
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

fn run_default(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let opts = pass
        .settings::<Options>("errcheck")
        .cloned()
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

/// Build the symbol exclusion set from defaults + `exclude-functions`.
fn build_exclude_set(opts: &Options) -> HashSet<String> {
    let mut set = HashSet::new();
    if !opts.disable_default_exclusions {
        for sym in excludes::DEFAULT_EXCLUDED_SYMBOLS {
            set.insert((*sym).to_string());
        }
    }
    for raw in &opts.exclude_functions {
        let sym = raw.trim();
        if sym.is_empty() || sym.starts_with("//") {
            continue;
        }
        set.insert(sym.to_string());
        // A call through an interface reaches `types.Func.FullName()` as
        // `(interface).M` for an imported interface (gcimporter gives the
        // method the interface itself as its receiver) — upstream errcheck
        // sidesteps that by building the name from the *selection's* receiver
        // type instead (`namesForExcludeCheck` /
        // `walkThroughEmbeddedInterfaces`). Until that walk is ported, accept
        // `(interface).M` as an alias of any `(pkg.T).M` the config lists.
        if let Some(rest) = sym.strip_prefix('(') {
            if let Some((_, method)) = rest.split_once(").") {
                if !method.is_empty() {
                    set.insert(format!("(interface).{method}"));
                }
            }
        }
    }
    set
}

fn run(pass: &mut Pass<'_>, opts: Options) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "errcheck requires inspect analyzer".to_string())?
        .clone();

    let exclude = build_exclude_set(&opts);
    let mut pending: Vec<(u32, String)> = Vec::new();
    let mut visitor = Visitor {
        pass,
        exclude: &exclude,
        opts: &opts,
        pending: &mut pending,
        skip: None,
        error_types: RefCell::default(),
    };

    const WANTED: NodeMask = node_mask!(
        AssignStmt,
        DeferStmt,
        ExprStmt,
        GenDecl,
        GoStmt,
        TypeAssertExpr,
    );
    inspect.preorder_typed(WANTED, pass.files(), |n| {
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
    exclude: &'a HashSet<String>,
    opts: &'a Options,
    pending: &'a mut Vec<(u32, String)>,
    /// Span of the subtree upstream's `Visit` declined to descend into.
    ///
    /// kisielk's visitor prunes by returning `nil` — from `case
    /// *ast.TypeAssertExpr` always, and from an assignment whose single RHS is
    /// an assertion — and the pruned subtree is then *not* examined at all, so
    /// a call inside a function literal in there is never reported either.
    /// [`inspect::InspectResult::preorder_typed`] has no pruning, but it visits
    /// parents before children, and AST spans nest: a node whose position falls
    /// inside this range is inside that subtree, and one outside it is not.
    ///
    /// A single "everything below this offset" watermark would be one byte
    /// smaller and wrong across files — positions are per-`FileSet` and a later
    /// file need not start above an earlier one's spans.
    skip: Option<(u32, u32)>,
    /// Answers to "does this type implement `error`?", which every unchecked
    /// call asks about its result type.
    ///
    /// Both halves of that answer used to be recomputed per call: finding the
    /// predeclared `error` scans the whole object arena, and the mutable
    /// interface lookup needs a `TypeArena` clone. On Prometheus's `./tsdb/...`
    /// the pair was ~4.6% of guff's CPU samples — errcheck's single hottest
    /// frame. Result types repeat heavily (`error`, `int`, `[]byte`, one
    /// package's own types), so a `TypeId` memo answers almost every query.
    error_types: RefCell<ErrorTypes>,
}

/// Lazily built cache behind [`Visitor::error_types`].
#[derive(Default)]
struct ErrorTypes {
    /// `None` until looked up; `Some(None)` when the package has no universe
    /// `error` (nothing then implements it).
    universe_error: Option<Option<TypeId>>,
    /// One arena copy for the whole run, for the queries the read-only probe
    /// cannot answer. Appends land in its overlay and never touch the package's.
    scratch: Option<guff_types::arena::TypeArena>,
    memo: HashMap<TypeId, bool>,
}

impl Visitor<'_, '_> {
    fn report_unchecked(&mut self, pos: u32, call: Option<&CallExpr>) {
        let message = match call.and_then(|c| self.call_display_name(c)) {
            // Backticks match kisielk/golangci + exclusion presets like
            // `Error return value of .((...).|.*Flush|...). is not checked`.
            Some(name) => format!("Error return value of `{name}` is not checked"),
            None => "Error return value is not checked".into(),
        };
        self.pending.push((pos, message));
    }

    /// The code golangci prints between the backticks, or `None` for the
    /// short "Error return value is not checked" form.
    ///
    /// The wrapper picks `cmp.Or(err.SelectorName, err.FuncName)`, or
    /// `err.FuncName` alone under `errcheck.verbose`
    /// (pkg/golinters/errcheck/errcheck.go). Both fields come from
    /// `selectorAndFunc`, which requires the callee to be a **selector** whose
    /// `Sel` resolves to a `*types.Func`, so a plain `f()` — a call to a
    /// function in this package, or to a local variable of func type — has no
    /// name at all and takes the short form.
    fn call_display_name(&self, call: &CallExpr) -> Option<String> {
        let Expr::SelectorExpr(sel) = base_call_expr(&call.fun) else {
            return None;
        };
        // `fullName`: the receiver-qualified, import-path-qualified name
        // (`(*os.File).Close`, `fmt.Printf`) — `types.Func.FullName()`.
        let full = {
            let obj = self.call_target_object(call)?;
            let artifacts = self.pass.pkg().type_artifacts.as_ref()?;
            let name = code::type_func_name(
                &artifacts.types,
                &artifacts.objects,
                &artifacts.packages,
                obj,
            );
            if name.is_empty() {
                return None;
            }
            name
        };
        if self.opts.verbose {
            return Some(full);
        }
        // `selectorName`: the selector as *written*, but only while it is a
        // chain of plain identifiers. `foo().Close()` has no such spelling and
        // falls back to the qualified name.
        Some(selector_name(sel).unwrap_or(full))
    }

    /// Is this node inside a subtree upstream's `Visit` pruned? See
    /// [`Visitor::skip`].
    fn pruned(&self, pos: Pos) -> bool {
        let pos = pos.0 as u32;
        self.skip.is_some_and(|(start, end)| pos >= start && pos < end)
    }

    fn prune(&mut self, start: Pos, end: Pos) {
        self.skip = Some((start.0 as u32, end.0 as u32));
    }

    fn visit_expr_stmt(&mut self, x: &Expr) {
        if self.pruned(x.pos()) {
            return;
        }
        if let Expr::CallExpr(call) = x {
            if !self.ignore_call(call) && self.call_returns_error(call) {
                self.report_unchecked(call.lparen.0 as u32, Some(call));
            }
        }
    }

    fn visit_go_defer(&mut self, call: &CallExpr) {
        if self.pruned(call.pos()) {
            return;
        }
        if !self.ignore_call(call) && self.call_returns_error(call) {
            self.report_unchecked(call.lparen.0 as u32, Some(call));
        }
    }

    fn visit_assign(&mut self, s: &AssignStmt) {
        let start = s.lhs.first().map_or(s.tok_pos, Expr::pos);
        if self.pruned(start) {
            return;
        }
        if !self.check_assignment(&s.lhs, &s.rhs) {
            // `Visit` returned nil: nothing under this statement is examined —
            // including the left-hand side, which is why the span starts there
            // and not at the assertion.
            if let Some(last) = s.rhs.last() {
                self.prune(start, last.end());
            }
        }
    }

    fn visit_gendecl(&mut self, g: &GenDecl) {
        if g.tok != Some(guff::token::Token::VAR) || self.pruned(g.tok_pos) {
            return;
        }
        for spec in &g.specs {
            let Spec::ValueSpec(vs) = spec else { continue };
            if vs.values.is_empty() {
                continue;
            }
            let lhs: Vec<Expr> = vs.names.iter().map(|n| Expr::Ident(n.clone())).collect();
            if !self.check_assignment(&lhs, &vs.values) {
                // Upstream returns nil from inside the spec loop, so the specs
                // *after* this one are not checked either.
                // `Decl::end` for a `GenDecl`, without cloning the decl: the
                // `)` when the declaration is parenthesized, and otherwise its
                // one and only spec.
                let end = if g.rparen.is_valid() {
                    Pos(g.rparen.0 + 1)
                } else {
                    g.specs.last().map(|s| s.end()).unwrap_or_default()
                };
                self.prune(g.tok_pos, end);
                return;
            }
        }
    }

    fn visit_type_assert(&mut self, t: &TypeAssertExpr) {
        if self.pruned(t.x.pos()) {
            return;
        }
        // `case *ast.TypeAssertExpr` prunes unconditionally — before the two
        // early returns below, not after them.
        self.prune(t.x.pos(), Pos(t.rparen.0 + 1));
        if !self.opts.check_asserts || t.ty.is_none() {
            return;
        }
        // `checkAssertExpr` reports at `expr.Pos()`, and an
        // `ast.TypeAssertExpr` begins at its operand, not at `.(`. The two
        // coincide only when the operand is one character long — which is why
        // `_ = i.(string)` matched upstream and `return i.(string)` did not.
        self.report_unchecked(t.x.pos().0 as u32, None);
    }

    /// Port of `checkAssignment`. The return value is upstream's `followed`:
    /// `false` means `Visit` returns nil and the statement's whole subtree goes
    /// unexamined (see [`Visitor::skip_until`]).
    ///
    /// Nothing here unwraps parentheses, because upstream's type switches do
    /// not: `_ = (f())` has a `*ast.ParenExpr` on the right, matches neither
    /// arm, and is therefore not a blank assignment at all. `_ = (i.(string))`
    /// falls through the same way and is reported later, by the assertion's own
    /// visit — one column to the right of where the unwrapped form reports.
    fn check_assignment(&mut self, lhs: &[Expr], rhs: &[Expr]) -> bool {
        if rhs.len() == 1 {
            if let Expr::CallExpr(call) = &rhs[0] {
                if !self.opts.check_blank {
                    return true;
                }
                if self.ignore_call(call) {
                    return true;
                }
                let is_error = self.errors_by_arg(call);
                for (i, l) in lhs.iter().enumerate() {
                    let Expr::Ident(id) = l else {
                        continue;
                    };
                    if id.name != "_" {
                        continue;
                    }
                    if self.is_recover(call) || is_error.get(i).copied().unwrap_or(false) {
                        self.report_unchecked(id.name_pos.0 as u32, Some(call));
                    }
                }
            } else if let Expr::TypeAssertExpr(assert) = &rhs[0] {
                if !self.opts.check_asserts {
                    return false;
                }
                if assert.ty.is_none() {
                    // type switch
                    return false;
                }
                if lhs.len() < 2 {
                    self.report_unchecked(rhs[0].pos().0 as u32, None);
                } else if let Expr::Ident(id) = &lhs[1] {
                    if self.opts.check_blank && id.name == "_" {
                        self.report_unchecked(id.name_pos.0 as u32, None);
                    }
                }
                return false;
            }
        } else {
            for (i, l) in lhs.iter().enumerate() {
                let Expr::Ident(id) = l else {
                    continue;
                };
                if let Some(Expr::CallExpr(call)) = rhs.get(i) {
                    if !self.opts.check_blank {
                        continue;
                    }
                    if self.ignore_call(call) {
                        continue;
                    }
                    if id.name == "_" && self.call_returns_error(call) {
                        self.report_unchecked(id.name_pos.0 as u32, Some(call));
                    }
                } else if let Some(Expr::TypeAssertExpr(assert)) = rhs.get(i) {
                    // Note the asymmetry with the call arm above: an assertion
                    // in a multi-value assignment is reported for *any* name on
                    // the left, not only for `_`. `a, b = i.(int), j.(string)`
                    // is two findings under `check-type-assertions`.
                    if !self.opts.check_asserts || assert.ty.is_none() {
                        continue;
                    }
                    self.report_unchecked(id.name_pos.0 as u32, None);
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
        // Receiver expression type + embedded-interface walk (kisielk
        // `walkThroughEmbeddedInterfaces`): `h.Write` with `h hash.Hash64`
        // must also match default exclude `(hash.Hash).Write`.
        if let Expr::SelectorExpr(sel) = base_call_expr(&call.fun) {
            if let Some(info) = self.pass.types_info() {
                if let Some(tav) = info.types.get(&sel.x.id()) {
                    let mut ty = guff_types::alias::unalias_readonly(&artifacts.types, tav.typ);
                    if let guff_types::arena::TypeData::Pointer(p) = artifacts.types.get(ty) {
                        ty = guff_types::alias::unalias_readonly(&artifacts.types, p.elem());
                    }
                    push_iface_method_names(
                        &artifacts.types,
                        &artifacts.objects,
                        &artifacts.packages,
                        ty,
                        &sel.sel.name,
                        &mut names,
                    );
                }
                // Selection-based walk when the method was reached via struct
                // embedding of an interface (kisielk Index path).
                if let Some(selection) = info.selections.get(&sel.id) {
                    if let Some(more) = walk_embedded_iface_names(
                        &artifacts.types,
                        &artifacts.objects,
                        &artifacts.packages,
                        selection,
                        &sel.sel.name,
                    ) {
                        for n in more {
                            if !names.contains(&n) {
                                names.push(n);
                            }
                        }
                    }
                }
            }
        }
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
        // Void calls are recorded as NoValue + Typ[Invalid] (guff-types
        // recording). Invalid must not be treated as implementing `error`.
        if matches!(
            tav.mode,
            OperandMode::NoValue | OperandMode::Invalid | OperandMode::Builtin | OperandMode::TypeExpr
        ) {
            return vec![false];
        }
        self.result_type_errors(tav.typ)
    }

    fn result_type_errors(&self, typ: TypeId) -> Vec<bool> {
        let artifacts = match self.pass.pkg().type_artifacts.as_ref() {
            Some(a) => a,
            None => return vec![false],
        };
        match artifacts.types.get(typ) {
            guff_types::arena::TypeData::Tuple(t) => (0..t.len())
                .map(|i| {
                    t.at(i)
                        .typ(&artifacts.objects)
                        .is_some_and(|rt| self.is_error_type(rt))
                })
                .collect(),
            _ => vec![self.is_error_type(typ)],
        }
    }

    /// True when `typ` is (or implements) the predeclared `error` interface.
    ///
    /// Matches kisielk/errcheck: `types.Implements(t, errorType)` only — no `*T`
    /// fallback. Pointer-typed returns like `*gin.Error` still match because the
    /// call's result type is already a pointer. Value types that only have
    /// pointer-receiver `Error()` do not implement `error` in go/types either.
    fn is_error_type(&self, typ: TypeId) -> bool {
        if let Some(hit) = self.error_types.borrow().memo.get(&typ) {
            return *hit;
        }
        let answer = self.compute_is_error_type(typ);
        self.error_types.borrow_mut().memo.insert(typ, answer);
        answer
    }

    /// The uncached body of [`Self::is_error_type`]. Every step here is priced
    /// per distinct type, not per call: `type_with_name` renders the whole type
    /// to a `String`, `universe_error` scans the object arena, and
    /// `api_implements` needs a mutable arena.
    fn compute_is_error_type(&self, typ: TypeId) -> bool {
        if type_with_name(self.pass, typ, "error") {
            return true;
        }
        let Some(artifacts) = self.pass.pkg().type_artifacts.as_ref() else {
            return false;
        };
        // Typ[Invalid] is used for NoValue / broken types — never an error value.
        if matches!(
            artifacts.types.get(typ),
            TypeData::Basic(b) if b.kind() == BasicKind::Invalid
        ) {
            return false;
        }

        let cache = &mut *self.error_types.borrow_mut();
        let err = *cache
            .universe_error
            .get_or_insert_with(|| universe_error(self.pass));
        let Some(err) = err else {
            return false;
        };
        let scratch = cache
            .scratch
            .get_or_insert_with(|| artifacts.types.clone());
        api_implements(
            scratch,
            &artifacts.objects,
            &artifacts.packages,
            typ,
            err,
        )
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

/// kisielk `getSelectorName`: the selector spelled as it is in the source,
/// defined only when every step of the chain is a plain identifier.
///
/// `f.Close` → `f.Close`, `os.Stdout.Write` → `os.Stdout.Write`,
/// `newFile().Close` → `None`.
fn selector_name(sel: &guff::ast::SelectorExpr) -> Option<String> {
    match &*sel.x {
        Expr::Ident(ident) => Some(format!("{}.{}", ident.name, sel.sel.name)),
        Expr::SelectorExpr(inner) => {
            Some(format!("{}.{}", selector_name(inner)?, sel.sel.name))
        }
        _ => None,
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

/// Collect `(Iface).Method` names for `ty` and every embedded interface
/// (kisielk-style). Covers `hash.Hash64` → `(hash.Hash).Write`.
fn push_iface_method_names(
    types: &guff_types::TypeArena,
    objects: &guff_types::ObjectArena,
    packages: &guff_types::PackageArena,
    ty: TypeId,
    method: &str,
    names: &mut Vec<String>,
) {
    let ty = guff_types::alias::unalias_readonly(types, ty);
    let underlying = ty.underlying(types);
    let embeddeds: Vec<TypeId> = match types.get(underlying) {
        TypeData::Interface(iface) => (0..iface.num_embeddeds())
            .map(|i| iface.embedded_type(i))
            .collect(),
        _ => return,
    };
    let recv_str = guff_types::typestring::type_string(types, objects, packages, ty, None);
    if !recv_str.is_empty() {
        let n = format!("({recv_str}).{method}");
        if !names.contains(&n) {
            names.push(n);
        }
    }
    for emb in embeddeds {
        push_iface_method_names(types, objects, packages, emb, method, names);
    }
}

/// Selection Index walk + embedded-interface descent (kisielk
/// `walkThroughEmbeddedInterfaces`).
fn walk_embedded_iface_names(
    types: &guff_types::TypeArena,
    objects: &guff_types::ObjectArena,
    packages: &guff_types::PackageArena,
    selection: &guff_types::selection::Selection,
    method: &str,
) -> Option<Vec<String>> {
    if !matches!(objects.get(selection.obj()), ObjectData::Func(_)) {
        return None;
    }

    let mut current = selection.recv();
    let index = selection.index();
    if index.len() > 1 {
        for &field_index in &index[..index.len() - 1] {
            current = guff_types::alias::unalias_readonly(types, current);
            if let TypeData::Pointer(p) = types.get(current) {
                current = guff_types::alias::unalias_readonly(types, p.elem());
            }
            current = current.underlying(types);
            let TypeData::Struct(s) = types.get(current) else {
                return None;
            };
            let field = s.field(field_index as usize);
            current = field.typ(objects)?;
        }
    }

    current = guff_types::alias::unalias_readonly(types, current);
    let underlying = current.underlying(types);
    if !matches!(types.get(underlying), TypeData::Interface(_)) {
        return None;
    }

    let mut out = Vec::new();
    push_iface_method_names(types, objects, packages, current, method, &mut out);
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// The predeclared `error` interface.
///
/// A full scan of the object arena, so callers must cache the result — see
/// [`ErrorTypes`].
fn universe_error(pass: &Pass<'_>) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    for oid in artifacts.objects.ids() {
        let ObjectData::TypeName(tn) = artifacts.objects.get(oid) else {
            continue;
        };
        if tn.name() != "error" {
            continue;
        }
        if oid.pkg(&artifacts.objects).is_some() {
            continue;
        }
        return tn.typ();
    }
    None
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
