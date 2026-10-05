//! `tests` — check Go test, benchmark, fuzz, and example naming conventions.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/tests` (v0.50.0).

use std::cell::RefCell;
use std::sync::OnceLock;

use guff::ast::{CallExpr, CommentGroup, Expr, FuncDecl, StarExpr};
use guff::commentmap::{node_end, node_pos};
use guff::walk::{self, NodeRef};
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::{PackageId, TypeArena, TypeData};
use guff_types::basic::BasicKind;
use guff_types::lookup::{lookup_field_or_method, LookupResult};
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::typestring::type_string;
use guff_types::{ObjectId, TypeId};

use crate::govet_util::is_type_named;

/// upstream: `strings.HasSuffix(pass.Fset.File(f.FileStart).Name(), "_test.go")`
///
/// The question is about **this** file, not the package. Asking whether *any*
/// file in the package is a test file means every ordinary file in a package
/// that happens to have tests gets the `Example`/`Test` signature rules applied
/// to it — go-ethereum's `metrics/internal/sampledata.go` declares
/// `func ExampleMetrics() metrics.Registry` and is not a test file at all.
fn is_test_file(path: Option<&std::path::Path>) -> bool {
    path.is_some_and(|p| p.to_string_lossy().ends_with("_test.go"))
}

/// `acceptedFuzzTypes`, tested with `types.Identical`: the predeclared basic
/// types themselves (aliases such as `byte` included, named types not) and
/// `[]byte`.
const ACCEPTED_FUZZ_TYPES: &str = "string, bool, float32, float64, int, int8, int16, int32, \
     int64, uint, uint8, uint16, uint32, uint64, []byte";

fn is_accepted_fuzz_type(types: &TypeArena, t: TypeId) -> bool {
    let t = guff_types::alias::unalias_readonly(types, t);
    match types.get(t) {
        TypeData::Basic(b) => matches!(
            b.kind(),
            BasicKind::String
                | BasicKind::Bool
                | BasicKind::Float32
                | BasicKind::Float64
                | BasicKind::Int
                | BasicKind::Int8
                | BasicKind::Int16
                | BasicKind::Int32
                | BasicKind::Int64
                | BasicKind::Uint
                | BasicKind::Uint8
                | BasicKind::Uint16
                | BasicKind::Uint32
                | BasicKind::Uint64
        ),
        TypeData::Slice(s) => matches!(
            types.get(guff_types::alias::unalias_readonly(types, s.elem())),
            TypeData::Basic(b) if b.kind() == BasicKind::Uint8
        ),
        _ => false,
    }
}

struct Ctx<'p, 'a> {
    pass: &'p Pass<'a>,
    /// Scratch copy for the checks that need a mutable arena
    /// (`types.Identical`, `types.LookupFieldOrMethod`); cloned on first use.
    scratch: RefCell<Option<TypeArena>>,
    out: Vec<(u32, String)>,
}

impl Ctx<'_, '_> {
    fn types(&self) -> Option<&TypeArena> {
        self.pass.pkg().type_artifacts.as_ref().map(|a| &a.types)
    }

    fn type_of(&self, e: &Expr) -> Option<TypeId> {
        self.pass.types_info()?.types.get(&e.id()).map(|tv| tv.typ)
    }

    fn type_str(&self, t: TypeId) -> String {
        let Some(a) = self.pass.pkg().type_artifacts.as_ref() else {
            return String::new();
        };
        type_string(&a.types, &a.objects, &a.packages, t, None)
    }

    fn with_scratch<R>(&self, f: impl FnOnce(&mut TypeArena) -> R) -> Option<R> {
        let a = self.pass.pkg().type_artifacts.as_ref()?;
        let mut slot = self.scratch.borrow_mut();
        let arena = slot.get_or_insert_with(|| a.types.clone());
        Some(f(arena))
    }

    fn identical(&self, x: TypeId, y: TypeId) -> bool {
        let Some(a) = self.pass.pkg().type_artifacts.as_ref() else {
            return false;
        };
        self.with_scratch(|t| guff_types::predicates::identical(t, &a.objects, &a.packages, x, y))
            .unwrap_or(false)
    }

    /// `isTestingType`: `*testing.<name>` — the pointer itself, no unalias
    /// ("I doubt go test recognizes `type A = *testing.T`").
    fn is_testing_type(&self, t: TypeId, name: &str) -> bool {
        let Some(types) = self.types() else {
            return false;
        };
        let TypeData::Pointer(p) = types.get(t) else {
            return false;
        };
        is_type_named(self.pass, p.elem(), "testing", name)
    }

    /// `isFuzzTargetDot`: `call.Fun` is a selector (no parens) on a
    /// `*testing.F`, naming `name` (any method when empty).
    fn is_fuzz_target_dot(&self, call: &CallExpr, name: &str) -> bool {
        let Expr::SelectorExpr(sel) = &*call.fun else {
            return false;
        };
        if !self.type_of(&sel.x).is_some_and(|t| self.is_testing_type(t, "F")) {
            return false;
        }
        name.is_empty() || sel.sel.name == name
    }

    /// The signature's `(params, results)` tuples when `t`'s underlying type
    /// is a signature.
    fn signature(&self, t: TypeId) -> Option<(Option<TypeId>, Option<TypeId>)> {
        let types = self.types()?;
        match types.get(t.underlying(types)) {
            TypeData::Signature(s) => Some((s.params(), s.results())),
            _ => None,
        }
    }

    fn tuple_types(&self, tuple: Option<TypeId>) -> Vec<TypeId> {
        let Some(a) = self.pass.pkg().type_artifacts.as_ref() else {
            return Vec::new();
        };
        let Some(tuple) = tuple else {
            return Vec::new();
        };
        (0..tuple_len(&a.types, Some(tuple)))
            .filter_map(|i| tuple_at(&a.types, tuple, i).typ(&a.objects))
            .collect()
    }

    fn report(&mut self, pos: u32, msg: impl Into<String>) {
        self.out.push((pos, msg.into()));
    }

    fn check_fuzz(&mut self, fn_: &FuncDecl) {
        if let Some(params) = self.check_fuzz_call(fn_) {
            self.check_add_calls(fn_, &params);
        }
    }

    /// `checkFuzzCall`: the argument of every `f.Fuzz(…)`, and the `*F`
    /// methods called inside it. Returns the parameters of the first valid
    /// fuzz target.
    fn check_fuzz_call(&mut self, fn_: &FuncDecl) -> Option<Vec<TypeId>> {
        let mut params: Option<Vec<TypeId>> = None;
        let mut calls: Vec<&CallExpr> = Vec::new();
        // `ast.Inspect` that does not descend into a Fuzz call it handled.
        walk::inspect(NodeRef::FuncDecl(fn_), |n| {
            let Some(NodeRef::CallExpr(call)) = n else {
                return true;
            };
            if !self.is_fuzz_target_dot(call, "Fuzz") || call.args.len() != 1 {
                return true;
            }
            calls.push(call);
            false
        });
        for call in calls {
            let expr = &call.args[0];
            let Some(t) = self.type_of(expr) else {
                continue;
            };
            let pos = expr.pos().0 as u32;
            let Some((sig_params, sig_results)) = self.signature(t) else {
                self.report(pos, "argument to Fuzz must be a function");
                continue;
            };
            if !self.tuple_types(sig_results).is_empty() {
                self.report(pos, "fuzz target must not return any value");
            }
            let ps = self.tuple_types(sig_params);
            if ps.is_empty() {
                self.report(pos, "fuzz target must have 1 or more argument");
                continue;
            }
            if self.validate_fuzz_args(&ps, expr) && params.is_none() {
                params = Some(ps);
            }
            // No `*F` method but Name and Failed inside the target.
            let mut bad: Vec<u32> = Vec::new();
            walk::inspect(walk::expr_ref(expr), |n| {
                if let Some(NodeRef::CallExpr(c)) = n {
                    if self.is_fuzz_target_dot(c, "")
                        && !self.is_fuzz_target_dot(c, "Name")
                        && !self.is_fuzz_target_dot(c, "Failed")
                    {
                        bad.push(node_pos(NodeRef::CallExpr(c)).0 as u32);
                    }
                }
                true
            });
            for p in bad {
                self.report(p, "fuzz target must not call any *F methods");
            }
        }
        params
    }

    /// `validateFuzzArgs`. A function literal's report is at the offending
    /// parameter's type; any other expression's at the expression.
    fn validate_fuzz_args(&mut self, params: &[TypeId], expr: &Expr) -> bool {
        let flit = match expr {
            Expr::FuncLit(l) => Some(l),
            _ => None,
        };
        let expr_pos = expr.pos().0 as u32;
        let field_type_pos = |i: usize| -> Option<u32> {
            let list = &flit?.ty.params.as_ref()?.list;
            Some(list.get(i)?.ty.as_ref()?.pos().0 as u32)
        };
        let mut range = expr_pos;
        let mut ok = true;
        if !self.is_testing_type(params[0], "T") {
            if flit.is_some() {
                range = field_type_pos(0).unwrap_or(expr_pos);
            }
            self.report(range, "the first parameter of a fuzz target must be *testing.T");
            ok = false;
        }
        for (i, &p) in params.iter().enumerate().skip(1) {
            let accepted = self.types().is_some_and(|t| is_accepted_fuzz_type(t, p));
            if accepted {
                continue;
            }
            if let Some(l) = flit {
                // The field holding parameter i, counted by names; a field
                // with no names counts none, as upstream's loop does.
                let mut curr = 0;
                if let Some(fl) = l.ty.params.as_ref() {
                    for field in &fl.list {
                        curr += field.names.len();
                        if i < curr {
                            if let Some(t) = field.ty.as_ref() {
                                range = t.pos().0 as u32;
                            }
                            break;
                        }
                    }
                }
            }
            self.report(
                range,
                format!("fuzzing arguments can only have the following types: {ACCEPTED_FUZZ_TYPES}"),
            );
            ok = false;
        }
        ok
    }

    /// `checkAddCalls`: every `f.Add(…)` matches the fuzz target's parameters
    /// after the `*testing.T`.
    fn check_add_calls(&mut self, fn_: &FuncDecl, params: &[TypeId]) {
        let mut adds: Vec<&CallExpr> = Vec::new();
        walk::inspect(NodeRef::FuncDecl(fn_), |n| {
            if let Some(NodeRef::CallExpr(call)) = n {
                if self.is_fuzz_target_dot(call, "Add") {
                    adds.push(call);
                }
            }
            true
        });
        let want = &params[1..];
        for call in adds {
            let call_pos = node_pos(NodeRef::CallExpr(call)).0 as u32;
            if call.args.len() != want.len() {
                self.report(
                    call_pos,
                    format!(
                        "wrong number of values in call to (*testing.F).Add: {}, fuzz target expects {}",
                        call.args.len(),
                        want.len()
                    ),
                );
                continue;
            }
            let mut got = Vec::with_capacity(call.args.len());
            let mut mismatched = Vec::new();
            let mut untyped = false;
            for (i, e) in call.args.iter().enumerate() {
                let Some(t) = self.type_of(e) else {
                    untyped = true;
                    break;
                };
                got.push(t);
                if !self.identical(t, want[i]) {
                    mismatched.push(i);
                }
            }
            if untyped {
                continue;
            }
            match mismatched.as_slice() {
                [] => {}
                [i] => {
                    let pos = call.args[*i].pos().0 as u32;
                    let msg = format!(
                        "mismatched type in call to (*testing.F).Add: {}, fuzz target expects {}",
                        self.type_str(got[*i]),
                        self.type_str(want[*i])
                    );
                    self.report(pos, msg);
                }
                _ => {
                    let list = |ts: &[TypeId]| {
                        let s: Vec<String> = ts.iter().map(|&t| self.type_str(t)).collect();
                        format!("[{}]", s.join(" "))
                    };
                    let msg = format!(
                        "mismatched types in call to (*testing.F).Add: {}, fuzz target expects {}",
                        list(&got),
                        list(want)
                    );
                    self.report(call_pos, msg);
                }
            }
        }
    }

    /// `lookup`: `name` in the package scope, or failing that in the scope of
    /// every package it imports.
    fn lookup(&self, name: &str) -> Vec<ObjectId> {
        let Some(a) = self.pass.pkg().type_artifacts.as_ref() else {
            return Vec::new();
        };
        let Some(pkg) = self.pass.type_pkg() else {
            return Vec::new();
        };
        let in_pkg = |p: PackageId| {
            guff_types::scope::lookup(&a.scopes, a.packages.get(p).scope(), name)
        };
        if let Some(o) = in_pkg(pkg) {
            return vec![o];
        }
        a.packages
            .get(pkg)
            .imports()
            .iter()
            .filter_map(|&imp| in_pkg(imp))
            .collect()
    }

    /// `checkExampleName`; every report is at `fn.Pos()`, the `func` keyword.
    fn check_example_name(&mut self, fn_: &FuncDecl) {
        let name = fn_.name.name.as_str();
        let pos = fn_.ty.pos().0 as u32;
        if fn_.ty.params.as_ref().is_some_and(|p| !p.list.is_empty()) {
            self.report(pos, format!("{name} should be niladic"));
        }
        if fn_.ty.results.as_ref().is_some_and(|r| !r.list.is_empty()) {
            self.report(pos, format!("{name} should return nothing"));
        }
        if fn_.ty.type_params.as_ref().is_some_and(|t| !t.list.is_empty()) {
            self.report(pos, format!("{name} should not have type params"));
        }
        if name == "Example" {
            return;
        }
        let ex_name = name.strip_prefix("Example").unwrap_or(name);
        let elems: Vec<&str> = ex_name.splitn(3, '_').collect();
        let ident = elems[0];
        let objs = self.lookup(ident);
        if !ident.is_empty() && objs.is_empty() {
            self.report(pos, format!("{name} refers to unknown identifier: {ident}"));
            return;
        }
        if elems.len() < 2 {
            return;
        }
        if ident.is_empty() {
            let residual = ex_name.strip_prefix('_').unwrap_or(ex_name);
            if !is_example_suffix(residual) {
                self.report(pos, format!("{name} has malformed example suffix: {residual}"));
            }
            return;
        }
        let mmbr = elems[1];
        if !is_example_suffix(mmbr) {
            let found = objs.iter().any(|&obj| self.has_field_or_method(obj, mmbr));
            if !found {
                self.report(
                    pos,
                    format!("{name} refers to unknown field or method: {ident}.{mmbr}"),
                );
            }
        }
        if elems.len() == 3 && !is_example_suffix(elems[2]) {
            self.report(pos, format!("{name} has malformed example suffix: {}", elems[2]));
        }
    }

    /// `types.LookupFieldOrMethod(obj.Type(), true, obj.Pkg(), name) != nil`.
    fn has_field_or_method(&self, obj: ObjectId, name: &str) -> bool {
        let Some(a) = self.pass.pkg().type_artifacts.as_ref() else {
            return false;
        };
        let Some(t) = obj.typ(&a.objects) else {
            return false;
        };
        let pkg = obj.pkg(&a.objects);
        self.with_scratch(|types| {
            matches!(
                lookup_field_or_method(types, &a.objects, &a.packages, t, true, pkg, name),
                LookupResult::Found { .. }
            )
        })
        .unwrap_or(false)
    }

    /// `checkExampleOutput`: an output comment block that is not the last
    /// comment block inside the example.
    fn check_example_output(&mut self, fn_: &FuncDecl, comments: &[CommentGroup]) {
        let start = fn_.ty.pos().0;
        let end = node_end(NodeRef::FuncDecl(fn_)).0;
        let mut in_example: Vec<(bool, u32)> = Vec::new();
        let mut num_outputs = 0;
        for cg in comments {
            if cg.pos().0 < start {
                continue;
            } else if cg.end().0 > end {
                break;
            }
            let is_output = is_output_comment(&cg.text());
            if is_output {
                num_outputs += 1;
            }
            in_example.push((is_output, cg.pos().0 as u32));
        }
        let msg = if num_outputs > 1 {
            "there can only be one output comment block per example"
        } else {
            "output comment block must be the last comment block"
        };
        let last = in_example.len().saturating_sub(1);
        for (i, &(is_output, pos)) in in_example.iter().enumerate() {
            if is_output && i != last {
                self.report(pos, msg);
            }
        }
    }

    /// `checkTest`.
    fn check_test(&mut self, fn_: &FuncDecl, prefix: &str) {
        let ft = &fn_.ty;
        if ft.results.as_ref().is_some_and(|r| !r.list.is_empty()) {
            return;
        }
        let Some(params) = ft.params.as_ref() else {
            return;
        };
        if params.list.len() != 1 || params.list[0].names.len() > 1 {
            return;
        }
        let Some(ty) = params.list[0].ty.as_ref() else {
            return;
        };
        if !is_test_param(ty, &prefix[..1]) {
            return;
        }
        let name = &fn_.name.name;
        if let Some(tparams) = ft.type_params.as_ref().filter(|t| !t.list.is_empty()) {
            // `RangeOf(tparams.Opening, tparams.Closing)`.
            self.report(
                tparams.opening.0 as u32,
                format!(
                    "{name} has type parameters: it will not be run by go test as a {prefix}XXX function"
                ),
            );
        }
        let suffix = name.strip_prefix(prefix).unwrap_or(name);
        if !is_test_suffix(suffix) {
            self.report(
                fn_.name.pos().0 as u32,
                format!("{name} has malformed name: first letter after '{prefix}' must not be lowercase"),
            );
        }
    }
}

/// `isTestParam`: written as `*T` or `*pkg.T` — by name only.
fn is_test_param(ty: &Expr, want: &str) -> bool {
    let Expr::StarExpr(StarExpr { x, .. }) = ty else {
        return false;
    };
    match x.as_ref() {
        Expr::Ident(id) => id.name == want,
        Expr::SelectorExpr(sel) => sel.sel.name == want,
        _ => false,
    }
}

fn is_test_suffix(name: &str) -> bool {
    name.chars().next().is_none_or(|c| !c.is_lowercase())
}

fn is_example_suffix(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_lowercase())
}

/// `(?i)^[[:space:]]*(unordered )?output:` over `CommentGroup.Text()`.
fn is_output_comment(text: &str) -> bool {
    let t = text.trim_start_matches([' ', '\t', '\n', '\x0B', '\x0C', '\r']);
    let starts = |p: &str| t.len() >= p.len() && t.as_bytes()[..p.len()].eq_ignore_ascii_case(p.as_bytes());
    starts("output:") || starts("unordered output:")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut ctx = Ctx {
        pass: &*pass,
        scratch: RefCell::new(None),
        out: Vec::new(),
    };
    for (i, file) in pass.files().iter().enumerate() {
        let path = pass
            .pkg()
            .compiled_go_files
            .get(i)
            .or_else(|| pass.pkg().go_files.get(i))
            .map(|p| p.as_path());
        if !is_test_file(path) {
            continue;
        }
        let mut file_comments: Option<Vec<CommentGroup>> = None;
        for decl in &file.decls {
            let guff::ast::Decl::FuncDecl(fn_) = decl else {
                continue;
            };
            if fn_.recv.is_some() {
                continue;
            }
            let name = &fn_.name.name;
            if name.starts_with("Example") {
                ctx.check_example_name(fn_);
                // The analysis AST carries no comments: reparse once per file.
                let comments = file_comments.get_or_insert_with(|| {
                    guff_analysis::comments::file_comments(ctx.pass, file)
                });
                ctx.check_example_output(fn_, comments);
            } else if name.starts_with("Test") {
                ctx.check_test(fn_, "Test");
            } else if name.starts_with("Benchmark") {
                ctx.check_test(fn_, "Benchmark");
            } else if name.starts_with("Fuzz") {
                ctx.check_test(fn_, "Fuzz");
                ctx.check_fuzz(fn_);
            }
        }
    }
    let pending = std::mem::take(&mut ctx.out);
    drop(ctx);
    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "tests",
        doc: "check naming conventions for tests, benchmarks, fuzz targets, and examples",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/tests",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![],
        fact_types: vec![],
    })
}
