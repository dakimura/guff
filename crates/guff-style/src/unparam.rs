//! Port of [`mvdan.cc/unparam`](https://github.com/mvdan/unparam)
//! (golangci-lint wrapper in `pkg/golinters/unparam`).
//!
//! Reports unused function parameters. This is an AST-based approximation:
//! a parameter is unused when its name does not appear as an identifier in the
//! function body (excluding `_ = param` intentional keeps).
//!
//! Functions whose signature cannot be changed are skipped (AST approx of
//! upstream SSA `signRequiredBy`):
//! - package-level funcs referenced as values (not only called)
//! - methods used as values (assigned / passed / returned, not only called)
//! - func literals that are not immediately invoked (IIFE / `go`/`defer`)
//! - methods whose name and parameter/result types match a method declared by
//!   an interface in this package ([`collect_interface_methods`])
//!
//! Upstream also checks unused / constant results and uses SSA for interface
//! satisfaction, forwarded calls, and call-graph precision.
//!
//! DEFERRED: full SSA (`buildir`), unused/constant results, generated-file
//! skips, recursive-only uses, `paramsRequiredBy`.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{Decl, Expr, FuncDecl, FuncLit, Stmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::{buildir, inspect};
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use crate::options::UnparamOptions;

fn is_blank_param(name: &str) -> bool {
    name.is_empty() || name.starts_with('_')
}

fn recv_type_string(expr: &Expr) -> String {
    match expr {
        Expr::Ident(id) => id.name.clone(),
        Expr::StarExpr(s) => format!("*{}", recv_type_string(&s.x)),
        _ => "?".to_string(),
    }
}

/// The receiver's type name with any `*` stripped — the key
/// [`collect_types_implementing`] uses. (Go: `findNamed(recv.Type()).Obj().Name()`.)
fn recv_base_type_name(recv: &guff::ast::FieldList) -> Option<String> {
    let ty = recv.list.first()?.ty.as_ref()?;
    let name = recv_type_string(ty);
    let name = name.strip_prefix('*').unwrap_or(&name);
    // A generic receiver renders as `T[P]`; upstream keys on the origin's name.
    let name = name.split('[').next().unwrap_or(name);
    (name != "?" && !name.is_empty()).then(|| name.to_string())
}

fn func_display_name(fd: &FuncDecl) -> String {
    if let Some(recv) = &fd.recv {
        if let Some(field) = recv.list.first() {
            if let Some(ty) = &field.ty {
                return format!("({}).{}", recv_type_string(ty), fd.name.name);
            }
        }
    }
    fd.name.name.clone()
}

/// AST stand-in for upstream's `dummyImpl`: a function whose entry block
/// "almost immediately panics, throws or returns constants only".
///
/// Upstream walks the SSA entry block and stops at the first `Return`/`Panic`,
/// so `func f(p *T) error { return nil }` is a stub and its parameters are
/// never reported — consul's `validateURLRewrite` / `validateHeaderFilter`.
/// Anything that would appear as a `BinOp` operand (`return used + 1`) or as a
/// non-harmless `Call` instruction (`n := compute(s)`) disqualifies it, which is
/// why `example - unused is unused` is still reported.
fn is_stub_body(body: &guff::ast::BlockStmt) -> bool {
    if body.list.is_empty() {
        return true;
    }
    for stmt in &body.list {
        match stmt {
            Stmt::ReturnStmt(ret) => {
                return ret.results.iter().all(harmless_expr);
            }
            Stmt::ExprStmt(e) => {
                if matches!(&e.x, Expr::CallExpr(call) if is_panic_call(call)) {
                    return true;
                }
                if !harmless_expr(&e.x) {
                    return false;
                }
            }
            Stmt::AssignStmt(asgn) => {
                if !asgn.rhs.iter().all(harmless_expr) {
                    return false;
                }
            }
            Stmt::DeclStmt(_) | Stmt::EmptyStmt(_) => {}
            // Any control flow ends the entry block without reaching a
            // terminator we accept.
            _ => return false,
        }
    }
    // Falling off the end of a body with no results is an implicit `return`.
    true
}

fn is_panic_call(call: &guff::ast::CallExpr) -> bool {
    matches!(&*call.fun, Expr::Ident(id) if id.name == "panic")
}

/// `rxHarmlessCall`: `(?i)\b(log(ger)?|errors)\b|\bf?print|errorf?$`.
fn is_harmless_call_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let word_at = |hay: &str, needle: &str| -> bool {
        hay.match_indices(needle).any(|(i, _)| {
            let before_ok = i == 0 || !is_word_byte(hay.as_bytes()[i - 1]);
            let j = i + needle.len();
            let after_ok = j == hay.len() || !is_word_byte(hay.as_bytes()[j]);
            before_ok && after_ok
        })
    };
    if word_at(&lower, "log") || word_at(&lower, "logger") || word_at(&lower, "errors") {
        return true;
    }
    // `\bf?print`
    if lower.match_indices("print").any(|(i, _)| {
        let start = if i > 0 && lower.as_bytes()[i - 1] == b'f' {
            i - 1
        } else {
            i
        };
        start == 0 || !is_word_byte(lower.as_bytes()[start - 1])
    }) {
        return true;
    }
    lower.ends_with("error") || lower.ends_with("errorf")
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn callee_name(call: &guff::ast::CallExpr) -> String {
    fn name(e: &Expr) -> String {
        match e {
            Expr::Ident(id) => id.name.clone(),
            Expr::SelectorExpr(s) => format!("{}.{}", name(&s.x), s.sel.name),
            Expr::ParenExpr(p) => name(&p.x),
            _ => String::new(),
        }
    }
    name(&call.fun)
}

/// True when the expression would not put a disqualifying instruction in the
/// entry block: no arithmetic (`BinOp`), and no call other than the
/// panic/log/print/error family upstream allows.
fn harmless_expr(e: &Expr) -> bool {
    match e {
        Expr::BinaryExpr(_) => false,
        Expr::CallExpr(call) => {
            if !is_panic_call(call) && !is_harmless_call_name(&callee_name(call)) {
                return false;
            }
            call.args.iter().all(harmless_expr)
        }
        Expr::ParenExpr(p) => harmless_expr(&p.x),
        Expr::UnaryExpr(u) => harmless_expr(&u.x),
        Expr::StarExpr(s) => harmless_expr(&s.x),
        Expr::SelectorExpr(s) => harmless_expr(&s.x),
        Expr::IndexExpr(i) => harmless_expr(&i.x) && harmless_expr(&i.index),
        Expr::SliceExpr(s) => {
            harmless_expr(&s.x)
                && [&s.low, &s.high, &s.max]
                    .into_iter()
                    .flatten()
                    .all(|e| harmless_expr(e))
        }
        Expr::CompositeLit(c) => c.elts.iter().all(harmless_expr),
        Expr::KeyValueExpr(kv) => harmless_expr(&kv.value),
        Expr::TypeAssertExpr(t) => harmless_expr(&t.x),
        _ => true,
    }
}

fn intentional_keep(body: &guff::ast::BlockStmt, param: &str) -> bool {
    let mut kept = false;
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(NodeRef::AssignStmt(asgn)) = n else {
            return true;
        };
        if asgn.tok != Some(Token::ASSIGN) || asgn.lhs.len() != 1 || asgn.rhs.len() != 1 {
            return true;
        }
        let (Expr::Ident(blank), Expr::Ident(rhs)) = (&asgn.lhs[0], &asgn.rhs[0]) else {
            return true;
        };
        if blank.name == "_" && rhs.name == param {
            kept = true;
        }
        true
    });
    kept
}

fn collect_used_idents(body: &guff::ast::BlockStmt) -> HashSet<String> {
    let mut used = HashSet::new();
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        if let Some(NodeRef::Ident(id)) = n {
            used.insert(id.name.clone());
        }
        true
    });
    used
}

fn check_params(
    func_name: &str,
    params: &[guff::ast::Field],
    body: &guff::ast::BlockStmt,
    pending: &mut Vec<(u32, String)>,
) {
    if is_stub_body(body) {
        return;
    }
    let used = collect_used_idents(body);
    for field in params {
        for name in &field.names {
            let pname = &name.name;
            if is_blank_param(pname) {
                continue;
            }
            if used.contains(pname) || intentional_keep(body, pname) {
                continue;
            }
            pending.push((
                name.name_pos.0 as u32,
                format!("{func_name} - {pname} is unused"),
            ));
        }
    }
}

fn should_check_exported(pass: &Pass<'_>, fd: &FuncDecl, check_exported: bool) -> bool {
    if check_exported || pass.pkg().name == "main" {
        return true;
    }
    if fd.name.name.contains('$') {
        return true;
    }
    !fd.name.is_exported()
}

/// Record identity of a `FuncLit` (node id, falling back to body `{` pos).
fn func_lit_key(lit: &FuncLit) -> u32 {
    if lit.id != 0 {
        lit.id
    } else {
        lit.body.lbrace.0 as u32
    }
}

fn unwrap_func_lit(expr: &Expr) -> Option<&FuncLit> {
    match expr {
        Expr::FuncLit(lit) => Some(lit),
        Expr::ParenExpr(p) => unwrap_func_lit(&p.x),
        _ => None,
    }
}

/// Collect call-fun Ident ids, and FuncLit keys used in value positions
/// (call args / composite elts / assign|return values) — signature required.
fn collect_call_sites(files: &[guff::ast::File]) -> (HashSet<u32>, HashSet<u32>) {
    let mut call_fun_ids = HashSet::new();
    let mut value_lits = HashSet::new();

    let mark_value_expr = |expr: &Expr, value_lits: &mut HashSet<u32>| {
        if let Some(lit) = unwrap_func_lit(expr) {
            value_lits.insert(func_lit_key(lit));
        }
    };

    for file in files {
        walk::inspect(NodeRef::File(file), |n| {
            match n {
                Some(NodeRef::CallExpr(call)) => {
                    match &*call.fun {
                        Expr::Ident(id) => {
                            call_fun_ids.insert(id.id);
                        }
                        Expr::SelectorExpr(sel) => {
                            call_fun_ids.insert(sel.sel.id);
                        }
                        _ => {}
                    }
                    for arg in &call.args {
                        mark_value_expr(arg, &mut value_lits);
                    }
                }
                Some(NodeRef::CompositeLit(lit)) => {
                    for elt in &lit.elts {
                        match elt {
                            Expr::KeyValueExpr(kv) => {
                                mark_value_expr(&kv.value, &mut value_lits);
                            }
                            other => mark_value_expr(other, &mut value_lits),
                        }
                    }
                }
                Some(NodeRef::AssignStmt(asgn)) => {
                    for rhs in &asgn.rhs {
                        mark_value_expr(rhs, &mut value_lits);
                    }
                }
                Some(NodeRef::ReturnStmt(ret)) => {
                    for r in &ret.results {
                        mark_value_expr(r, &mut value_lits);
                    }
                }
                Some(NodeRef::ValueSpec(spec)) => {
                    for v in &spec.values {
                        mark_value_expr(v, &mut value_lits);
                    }
                }
                _ => {}
            }
            true
        });
    }
    (call_fun_ids, value_lits)
}

/// Package-level (non-method) funcs referenced as values — signature required.
fn collect_sign_required_funcs(
    files: &[guff::ast::File],
    call_fun_ids: &HashSet<u32>,
) -> HashSet<String> {
    let mut pkg_funcs = HashSet::new();
    let mut decl_name_ids = HashSet::new();
    for file in files {
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            if fd.recv.is_some() {
                continue;
            }
            pkg_funcs.insert(fd.name.name.clone());
            decl_name_ids.insert(fd.name.id);
        }
    }

    let mut required = HashSet::new();
    for file in files {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::Ident(id)) = n else {
                return true;
            };
            if !pkg_funcs.contains(&id.name) {
                return true;
            }
            if decl_name_ids.contains(&id.id) {
                return true;
            }
            if call_fun_ids.contains(&id.id) {
                return true;
            }
            required.insert(id.name.clone());
            true
        });
    }
    required
}

/// Renders a type expression as a comparison key. Parameter names are left out
/// and `any` is spelled `interface{}`, so an interface method and the method
/// implementing it compare equal however either was written.
fn type_key(e: &Expr) -> String {
    match e {
        Expr::Ident(id) => {
            if id.name == "any" {
                "interface{}".to_string()
            } else {
                id.name.clone()
            }
        }
        Expr::SelectorExpr(s) => format!("{}.{}", type_key(&s.x), s.sel.name),
        Expr::StarExpr(s) => format!("*{}", type_key(&s.x)),
        Expr::ParenExpr(p) => type_key(&p.x),
        Expr::Ellipsis(e) => match &e.elt {
            Some(elt) => format!("...{}", type_key(elt)),
            None => "...".to_string(),
        },
        Expr::ArrayType(a) => match &a.len {
            Some(len) => format!("[{}]{}", type_key(len), type_key(&a.elt)),
            None => format!("[]{}", type_key(&a.elt)),
        },
        Expr::MapType(m) => format!("map[{}]{}", type_key(&m.key), type_key(&m.value)),
        Expr::ChanType(c) => {
            let arrow = if c.dir == guff::ast::ChanDir::SEND {
                "chan<-"
            } else if c.dir == guff::ast::ChanDir::RECV {
                "<-chan"
            } else {
                "chan"
            };
            format!("{arrow} {}", type_key(&c.value))
        }
        Expr::FuncType(f) => format!("func{}", signature_key(f)),
        Expr::InterfaceType(i) => format!("interface{{{}}}", field_keys(&i.methods.list).join(";")),
        Expr::StructType(s) => format!("struct{{{}}}", field_keys(&s.fields.list).join(";")),
        Expr::IndexExpr(i) => format!("{}[{}]", type_key(&i.x), type_key(&i.index)),
        Expr::IndexListExpr(i) => format!(
            "{}[{}]",
            type_key(&i.x),
            i.indices.iter().map(type_key).collect::<Vec<_>>().join(",")
        ),
        Expr::BasicLit(b) => b.value.clone(),
        // Anything else cannot appear in a signature that guff can compare;
        // an opaque key keeps it from matching another unrenderable type.
        other => format!("?{}", other.pos().0),
    }
}

/// Field types, one entry per declared name (`a, b string` counts twice) so a
/// grouped parameter list compares equal to a spelled-out one.
fn field_keys(fields: &[guff::ast::Field]) -> Vec<String> {
    let mut out = Vec::new();
    for field in fields {
        let key = match &field.ty {
            Some(ty) => type_key(ty),
            None => "?".to_string(),
        };
        for _ in 0..field.names.len().max(1) {
            out.push(key.clone());
        }
    }
    out
}

fn signature_key(ty: &guff::ast::FuncType) -> String {
    let params = ty
        .params
        .as_ref()
        .map(|p| field_keys(&p.list))
        .unwrap_or_default();
    let results = ty
        .results
        .as_ref()
        .map(|r| field_keys(&r.list))
        .unwrap_or_default();
    format!("({}) ({})", params.join(","), results.join(","))
}

fn method_key(name: &str, ty: &guff::ast::FuncType) -> String {
    format!("{name}{}", signature_key(ty))
}

/// `"<named type>.<method>"` for every method an interface requires of a type
/// that is actually converted to it somewhere in this package.
///
/// This is upstream's `typesImplementing`, built the way upstream builds it:
/// walk the IR for `MakeInterface` and record every method of the destination
/// interface against `findNamed(instr.X.Type())` (`check/check.go`'s "skip -
/// method required to implement an interface").
///
/// Being driven by the conversions rather than the declarations makes it
/// *narrower* than [`collect_interface_methods`] where that matters — an
/// interface nothing is ever converted to does not silence a report — and
/// *wider* elsewhere: the interface may be declared in another package, which
/// the AST scan cannot see. `WithValidator(&podValidator{})` in
/// controller-runtime is exactly that shape.
fn collect_types_implementing(pass: &Pass<'_>) -> HashSet<String> {
    use guff_ssa::instr::InstrData;

    let mut out = HashSet::new();
    let Some(ir) = pass.result_of::<buildir::BuildIrResult>(buildir::analyzer()) else {
        return out;
    };
    let arena = &ir.prog.type_arena;
    for &fid in ir.src_funcs_with_methods() {
        let func = ir.prog.functions.get(fid);
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                let InstrData::MakeInterface(mi) = func.instrs.get(iid) else {
                    continue;
                };
                let boxed = guff_ssa::program::value_type_of(&ir.prog, func, mi.x);
                let Some(named) = find_named_name(&ir.prog, boxed) else {
                    continue;
                };
                let iface = mi.typ.underlying(arena);
                if !matches!(arena.get(iface), guff_types::arena::TypeData::Interface(_)) {
                    continue;
                }
                let mut names = Vec::new();
                collect_iface_method_names(&ir.prog, iface, &mut names, 0);
                for m in names {
                    out.insert(format!("{named}.{m}"));
                }
            }
        }
    }
    out
}

/// The name of the named type `t` denotes, following one level of pointer.
/// (Go: `findNamed`, plus `Named.Obj().Name()`.)
fn find_named_name(prog: &guff_ssa::program::Program, t: guff_types::TypeId) -> Option<String> {
    use guff_types::arena::TypeData;
    let arena = &prog.type_arena;
    let t = match arena.get(t) {
        TypeData::Pointer(p) => p.elem(),
        _ => t,
    };
    match arena.get(t) {
        TypeData::Named(_) => {
            let obj = guff_types::named::named_obj(arena, t);
            Some(obj.name(&prog.object_arena).to_string())
        }
        _ => None,
    }
}

/// Method names of an interface, explicit plus those promoted from embedded
/// interfaces. Walks the embeddings by hand because the type-set accessors that
/// would answer this directly need `&mut TypeArena`, and the IR hands out a
/// shared program.
fn collect_iface_method_names(
    prog: &guff_ssa::program::Program,
    iface: guff_types::TypeId,
    out: &mut Vec<String>,
    depth: u32,
) {
    use guff_types::arena::TypeData;
    if depth > 16 {
        return; // defensive: embedding cycles are ill-typed, but never hang
    }
    let arena = &prog.type_arena;
    let (methods, embeddeds) = match arena.get(iface) {
        TypeData::Interface(i) => (
            (0..i.num_explicit_methods())
                .map(|k| i.explicit_method(k))
                .collect::<Vec<_>>(),
            (0..i.num_embeddeds())
                .map(|k| i.embedded_type(k))
                .collect::<Vec<_>>(),
        ),
        _ => return,
    };
    for m in methods {
        out.push(m.name(&prog.object_arena).to_string());
    }
    for e in embeddeds {
        let u = e.underlying(arena);
        if matches!(arena.get(u), TypeData::Interface(_)) {
            collect_iface_method_names(prog, u, out, depth + 1);
        }
    }
}

/// Methods declared by an interface type in this package.
///
/// A method that satisfies an interface cannot have its signature changed, so
/// upstream never reports its parameters. This is the AST half of that: it
/// matches an interface method by name *and signature*, so it still covers
/// interfaces declared here that nothing in this package ever converts to —
/// which the IR-driven [`collect_types_implementing`] deliberately does not.
fn collect_interface_methods(files: &[guff::ast::File]) -> HashSet<String> {
    let mut out = HashSet::new();
    for file in files {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::InterfaceType(it)) = n else {
                return true;
            };
            for field in &it.methods.list {
                let Some(Expr::FuncType(ft)) = field.ty.as_ref() else {
                    continue; // embedded interface, not a method
                };
                for name in &field.names {
                    out.insert(method_key(&name.name, ft));
                }
            }
            true
        });
    }
    out
}

/// Methods used as values (assigned / passed / returned), not only called.
/// Upstream unparam skips these via SSA `signRequiredBy`.
fn collect_sign_required_methods(
    files: &[guff::ast::File],
    call_fun_ids: &HashSet<u32>,
) -> HashSet<String> {
    let mut method_names = HashSet::new();
    let mut decl_name_ids = HashSet::new();
    for file in files {
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            if fd.recv.is_none() {
                continue;
            }
            method_names.insert(fd.name.name.clone());
            decl_name_ids.insert(fd.name.id);
        }
    }

    let mut required = HashSet::new();
    for file in files {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::SelectorExpr(sel)) = n else {
                return true;
            };
            if !method_names.contains(&sel.sel.name) {
                return true;
            }
            if decl_name_ids.contains(&sel.sel.id) {
                return true;
            }
            if call_fun_ids.contains(&sel.sel.id) {
                return true;
            }
            required.insert(sel.sel.name.clone());
            true
        });
    }
    required
}

fn check_func_decl(
    pass: &Pass<'_>,
    fd: &FuncDecl,
    check_exported: bool,
    sign_required: &HashSet<String>,
    sign_required_methods: &HashSet<String>,
    interface_methods: &HashSet<String>,
    types_implementing: &HashSet<String>,
    pending: &mut Vec<(u32, String)>,
) {
    if fd.name.name == "init" {
        return;
    }
    let Some(body) = &fd.body else {
        return;
    };
    if !should_check_exported(pass, fd, check_exported) {
        return;
    }
    if fd.recv.is_none() && sign_required.contains(&fd.name.name) {
        return;
    }
    if fd.recv.is_some() && sign_required_methods.contains(&fd.name.name) {
        return;
    }
    if fd.recv.is_some() && interface_methods.contains(&method_key(&fd.name.name, &fd.ty)) {
        return;
    }
    if let Some(recv) = &fd.recv {
        if let Some(base) = recv_base_type_name(recv) {
            if types_implementing.contains(&format!("{base}.{}", fd.name.name)) {
                return;
            }
        }
    }
    let Some(params) = &fd.ty.params else {
        return;
    };
    let func_name = func_display_name(fd);
    check_params(&func_name, &params.list, body, pending);
}

fn check_func_lit(lit: &FuncLit, value_lits: &HashSet<u32>, pending: &mut Vec<(u32, String)>) {
    // Literals stored / passed / returned have a fixed signature.
    if value_lits.contains(&func_lit_key(lit)) {
        return;
    }
    let Some(params) = &lit.ty.params else {
        return;
    };
    check_params("<func literal>", &params.list, &lit.body, pending);
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "unparam requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<UnparamOptions>("unparam")
        .copied()
        .unwrap_or_default();

    let files = pass.files();
    let (call_fun_ids, value_lits) = collect_call_sites(files);
    let sign_required = collect_sign_required_funcs(files, &call_fun_ids);
    let sign_required_methods = collect_sign_required_methods(files, &call_fun_ids);
    let interface_methods = collect_interface_methods(files);
    let types_implementing = collect_types_implementing(pass);

    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in files {
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            check_func_decl(
                pass,
                fd,
                opts.check_exported,
                &sign_required,
                &sign_required_methods,
                &interface_methods,
                &types_implementing,
                &mut pending,
            );
        }
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::FuncLit(lit)) = n else {
                return true;
            };
            check_func_lit(lit, &value_lits, &mut pending);
            true
        });
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "unparam",
        doc: "Reports unused function parameters",
        url: "https://github.com/mvdan/unparam",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer(), buildir::analyzer()],
        fact_types: vec![],
    })
}
