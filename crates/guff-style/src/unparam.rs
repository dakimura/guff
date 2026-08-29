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
    always_const: &[Option<String>],
    pending: &mut Vec<(u32, String)>,
) {
    if is_stub_body(body) {
        return;
    }
    let used = collect_used_idents(body);
    let mut index = 0usize;
    for field in params {
        for name in &field.names {
            let i = index;
            index += 1;
            let pname = &name.name;
            if is_blank_param(pname) {
                continue;
            }
            // `reason` is "is unused" unless every call site passes the same
            // constant, which upstream reports whether or not the body uses it.
            if let Some(cnst) = always_const.get(i).and_then(|c| c.as_deref()) {
                pending.push((
                    name.name_pos.0 as u32,
                    format!("{func_name} - {pname} always receives {cnst}"),
                ));
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


// ---------------------------------------------------------------- results

/// The flattened result list of a signature: one entry per result, with the
/// position upstream reports at (`types.Var.Pos()` — the name if there is one,
/// otherwise the type expression) and the name if it has one.
fn result_fields(ty: &guff::ast::FuncType) -> Vec<(u32, Option<String>)> {
    let mut out = Vec::new();
    let Some(results) = &ty.results else {
        return out;
    };
    for field in &results.list {
        if field.names.is_empty() {
            let pos = field
                .ty
                .as_ref()
                .map(|t| t.pos().0 as u32)
                .unwrap_or_default();
            out.push((pos, None));
        } else {
            for name in &field.names {
                out.push((name.pos().0 as u32, Some(name.name.clone())));
            }
        }
    }
    out
}

/// `paramDesc`: the name, or `index (type)` when there is none.
fn result_desc(
    prog: &guff_ssa::program::Program,
    index: usize,
    name: Option<&str>,
    typ: guff_types::TypeId,
) -> String {
    match name {
        Some(n) if n != "_" => n.to_string(),
        _ => format!(
            "{index} ({})",
            guff_types::typestring::type_string(
                &prog.type_arena,
                &prog.object_arena,
                &prog.package_arena,
                typ,
                None,
            )
        ),
    }
}

/// go/ssa normalizes a zero `Const` through `soleTypeKind`, so a zero of a
/// numeric, boolean or string type reads back as `0` / `false` / `""` and only
/// a nillable type reads back as nil. guff keeps the `None` (see
/// `guff_ssa::const_val::Const`), so normalize here — the difference decides
/// both the message and the `numRets == 1` exemption.
fn const_repr(prog: &guff_ssa::program::Program, c: &guff_ssa::const_val::Const) -> (bool, String) {
    use guff_types::arena::TypeData;
    if let Some(v) = &c.val {
        return (false, v.to_string());
    }
    let under = c.typ.underlying(&prog.type_arena);
    match prog.type_arena.get(under) {
        TypeData::Basic(b) => {
            use guff_types::basic::{IS_BOOLEAN, IS_NUMERIC, IS_STRING};
            let info = b.info();
            if info.contains(IS_NUMERIC) {
                (false, "0".to_string())
            } else if info.contains(IS_BOOLEAN) {
                (false, "false".to_string())
            } else if info.contains(IS_STRING) {
                (false, "\"\"".to_string())
            } else {
                (true, "nil".to_string())
            }
        }
        _ => (true, "nil".to_string()),
    }
}

/// `constValue`: the constant behind a value, peeling the interface boxing that
/// `return nil` through an `error` result produces.
fn const_of(
    prog: &guff_ssa::program::Program,
    func: &guff_ssa::function::Function,
    v: guff_ssa::value::Value,
) -> Option<guff_ssa::ids::ConstId> {
    use guff_ssa::instr::InstrData;
    use guff_ssa::value::Value;
    match v {
        Value::Const(id) => Some(id),
        Value::Instr(iid) => match func.instrs.get(iid) {
            InstrData::MakeInterface(mi) => const_of(prog, func, mi.x),
            _ => None,
        },
        _ => None,
    }
}

/// `result N is always X` — every `return` in the function gives result `N` the
/// same constant.
fn check_constant_results(
    prog: &guff_ssa::program::Program,
    func: &guff_ssa::function::Function,
    fname: &str,
    fields: &[(u32, Option<String>)],
    result_types: &[guff_types::TypeId],
    pending: &mut Vec<(u32, String)>,
) {
    use guff_ssa::instr::InstrData;

    let n = fields.len();
    if n == 0 {
        return;
    }
    // `sameConsts[i]`: Some(None) = not seen yet, Some(Some(c)) = agreed so far.
    let mut same: Vec<Option<guff_ssa::ids::ConstId>> = vec![None; n];
    let mut num_rets = 0usize;
    for (_, block) in func.live_blocks() {
        let Some(&last) = block.instrs.last() else {
            continue;
        };
        let InstrData::Return(ret) = func.instrs.get(last) else {
            continue;
        };
        if ret.results.len() != n {
            return;
        }
        for (i, &val) in ret.results.iter().enumerate() {
            let cnst = const_of(prog, func, val);
            if num_rets == 0 {
                same[i] = cnst;
            } else if !consts_equal(prog, same[i], cnst) {
                same[i] = None;
            }
        }
        num_rets += 1;
    }
    if num_rets == 0 {
        return;
    }
    for (i, slot) in same.iter().enumerate() {
        let Some(cid) = slot else {
            continue;
        };
        let (is_nil, repr) = const_repr(prog, prog.constants.get(*cid));
        if !is_nil && num_rets == 1 {
            // just one return and it's not an untyped nil (too many false
            // positives)
            continue;
        }
        let (pos, name) = &fields[i];
        let desc = result_desc(prog, i, name.as_deref(), result_types[i]);
        pending.push((*pos, format!("{fname} - result {desc} is always {repr}")));
    }
}

/// `eqlConsts`, over the ids guff hands out.
fn consts_equal(
    prog: &guff_ssa::program::Program,
    a: Option<guff_ssa::ids::ConstId>,
    b: Option<guff_ssa::ids::ConstId>,
) -> bool {
    let (Some(a), Some(b)) = (a, b) else {
        return a.is_none() && b.is_none();
    };
    let (ca, cb) = (prog.constants.get(a), prog.constants.get(b));
    if ca.typ != cb.typ {
        return false;
    }
    match (&ca.val, &cb.val) {
        (None, None) => true,
        (Some(x), Some(y)) => x.to_string() == y.to_string(),
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn check_func_decl(
    pass: &Pass<'_>,
    fd: &FuncDecl,
    check_exported: bool,
    sign_required: &HashSet<String>,
    sign_required_methods: &HashSet<String>,
    interface_methods: &HashSet<String>,
    types_implementing: &HashSet<String>,
    decl_counts: &std::collections::HashMap<String, usize>,
    ssa: Option<&SsaFuncs<'_>>,
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
    let func_name = func_display_name(fd);

    // Multiple implementations via build tags: a parameter unused in this one
    // may well be used in the other.
    if decl_counts
        .get(&format!("{}{}", recv_prefix(fd.recv.as_ref()), fd.name.name))
        .is_some_and(|&n| n > 1)
    {
        return;
    }

    // The result families need the SSA body: `result N is always X` reads every
    // `return`, and the parameter and unused-result families read the call
    // sites.
    let mut always_const: Vec<Option<String>> = Vec::new();
    if let Some(ssa) = ssa {
        if let Some((fid, func)) = ssa.func_for(pass, fd) {
            if dummy_impl(ssa.prog, func) {
                return;
            }
            let fields = result_fields(&fd.ty);
            let types = ssa.result_types(func);
            // `return f(...)` in another function fixes f's results.
            if fields.len() == types.len() && !ssa.results_required.contains(&fid) {
                check_constant_results(ssa.prog, func, &func_name, &fields, &types, pending);
                check_unused_results(
                    ssa.prog,
                    &ssa.sites,
                    fid,
                    func,
                    &func_name,
                    &fields,
                    &types,
                    pending,
                );
            }
            always_const = always_received_consts(ssa, fid, fd);
        }
    }

    let Some(params) = &fd.ty.params else {
        return;
    };
    check_params(&func_name, &params.list, body, &always_const, pending);
}


/// Every call instruction in the package that targets each function, and
/// whether the call's own value can be used (`site.Value()` is nil for a `go`
/// or `defer`). (Go: `localCallSites`.)
struct CallSites {
    /// callee -> (enclosing function, instruction, has a value)
    sites: std::collections::HashMap<
        guff_ssa::ids::FuncId,
        Vec<(guff_ssa::ids::FuncId, guff_ssa::ids::InstrId, bool)>,
    >,
}

impl CallSites {
    fn build(ir: &buildir::BuildIrResult) -> Self {
        use guff_ssa::instr::InstrData;
        use guff_ssa::value::Value;

        let mut sites: std::collections::HashMap<_, Vec<_>> = std::collections::HashMap::new();
        for &caller in ir.src_funcs_with_methods() {
            let func = ir.prog.functions.get(caller);
            for (_, block) in func.live_blocks() {
                for &iid in &block.instrs {
                    let (common, has_value) = match func.instrs.get(iid) {
                        InstrData::Call(c) => (&c.call, true),
                        InstrData::Go(g) => (&g.call, false),
                        InstrData::Defer(d) => (&d.call, false),
                        _ => continue,
                    };
                    if common.method.is_some() {
                        continue; // interface invoke: no static callee
                    }
                    let Value::Function(callee) = common.value else {
                        continue;
                    };
                    sites.entry(callee).or_default().push((caller, iid, has_value));
                }
            }
        }
        CallSites { sites }
    }

    fn for_func(
        &self,
        fid: guff_ssa::ids::FuncId,
    ) -> &[(guff_ssa::ids::FuncId, guff_ssa::ids::InstrId, bool)] {
        self.sites.get(&fid).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

/// `result N is never used` — no call site reads result `N`, and at least two
/// call sites ignore it.
#[allow(clippy::too_many_arguments)]
fn check_unused_results(
    prog: &guff_ssa::program::Program,
    sites: &CallSites,
    fid: guff_ssa::ids::FuncId,
    func: &guff_ssa::function::Function,
    fname: &str,
    fields: &[(u32, Option<String>)],
    result_types: &[guff_types::TypeId],
    pending: &mut Vec<(u32, String)>,
) {
    use guff_ssa::instr::InstrData;
    use guff_ssa::value::Value;

    let n = fields.len();
    if n == 0 {
        return;
    }
    // `allRetsExtracting`: every returned value comes straight out of another
    // call, so the results are not this function's to change.
    let mut all_rets_extracting = true;
    let mut any_return = false;
    for (_, block) in func.live_blocks() {
        let Some(&last) = block.instrs.last() else {
            continue;
        };
        let InstrData::Return(ret) = func.instrs.get(last) else {
            continue;
        };
        any_return = true;
        for &val in &ret.results {
            let is_extract = matches!(val, Value::Instr(iid) if matches!(func.instrs.get(iid), InstrData::Extract(_)));
            if !is_extract {
                all_rets_extracting = false;
            }
        }
    }
    if !any_return || all_rets_extracting {
        return;
    }

    'result: for i in 0..n {
        if is_error_type(prog, result_types[i]) {
            // "error is never used" is less useful, and it is errcheck's job.
            continue;
        }
        let mut count = 0usize;
        for &(caller, iid, has_value) in sites.for_func(fid) {
            if !has_value {
                count += 1;
                continue;
            }
            let caller_fn = prog.functions.get(caller);
            for rid in real_referrers(caller_fn, Value::Instr(iid)) {
                let InstrData::Extract(ex) = caller_fn.instrs.get(rid) else {
                    continue 'result; // direct, real use
                };
                if ex.index != i {
                    continue;
                }
                if real_referrers(caller_fn, Value::Instr(rid)).next().is_some() {
                    continue 'result; // real use after extraction
                }
            }
            count += 1;
        }
        if count < 2 {
            continue; // require ignoring at least twice
        }
        let (pos, name) = &fields[i];
        let desc = result_desc(prog, i, name.as_deref(), result_types[i]);
        pending.push((*pos, format!("{fname} - result {desc} is never used")));
    }
}

/// Referrers as upstream sees them. `buildssa` builds with `ssa.BuilderMode(0)`,
/// so there are no `DebugRef` instructions in the graph it walks; guff's SSA
/// keeps them, and counting one as a use makes every call site look like a real
/// use of the whole tuple.
fn real_referrers<'a>(
    func: &'a guff_ssa::function::Function,
    value: guff_ssa::value::Value,
) -> impl Iterator<Item = guff_ssa::ids::InstrId> + 'a {
    guff_analysis::referrers(func, value)
        .iter()
        .copied()
        .filter(|&rid| {
            !matches!(
                func.instrs.get(rid),
                guff_ssa::instr::InstrData::DebugRef(_)
            )
        })
}

/// `alwaysReceivedConst`, one entry per declared parameter (receiver excluded,
/// as in the AST): the constant every call site passes, described as upstream
/// describes it.
fn always_received_consts(
    ssa: &SsaFuncs<'_>,
    fid: guff_ssa::ids::FuncId,
    fd: &FuncDecl,
) -> Vec<Option<String>> {
    let params: Vec<&guff::ast::Field> = fd
        .ty
        .params
        .as_ref()
        .map(|p| p.list.iter().collect())
        .unwrap_or_default();
    let count: usize = params.iter().map(|f| f.names.len().max(1)).sum();
    let mut out = vec![None; count];

    let sites = ssa.sites.for_func(fid);
    if sites.len() < 4 {
        // Too few calls to be sure; upstream would rather miss than guess.
        return out;
    }
    if fd.name.is_exported() {
        // We might not have every call site of an exported func.
        return out;
    }
    // go/ast's `CallExpr.Args` does not include the receiver, go/ssa's does.
    let recv_offset = usize::from(fd.recv.is_some());
    // go/ssa packs variadic arguments into a slice, so the last parameter of a
    // variadic function never *is* a constant there. thanos calls
    // `zLabelSetFromStrings("a", "1")` fifteen times.
    let variadic = fd
        .ty
        .params
        .as_ref()
        .and_then(|p| p.list.last())
        .and_then(|f| f.ty.as_ref())
        .is_some_and(|t| matches!(t, Expr::Ellipsis(_)));

    for i in 0..count {
        if variadic && i + 1 == count {
            continue;
        }
        out[i] = ssa.const_received_at(sites, i + recv_offset, i);
    }
    out
}

/// `declCounts` + `multipleImpls`: how many times each function is declared in
/// the package *directory*, build-tag-excluded files included. A name declared
/// twice means a second implementation the analysis cannot see, where the
/// parameter or result may well be used — thanos builds two
/// `materializeForUnmarshal`s that way.
fn decl_counts(pass: &Pass<'_>) -> std::collections::HashMap<String, usize> {
    use guff::parser::{parse_file, Mode};
    use guff::position::FileSet;

    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let dir = &pass.pkg().dir;
    let want = pass.pkg().name.clone();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return counts;
    };
    let fset = FileSet::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("go") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(src) = std::fs::read(&path) else {
            continue;
        };
        let Ok(file) = parse_file(&fset, name, &src, Mode::NONE) else {
            continue;
        };
        // `parser.ParseDir` groups by package clause; only the one being
        // analysed counts (its `_test` variant declares its own names).
        if file.name.name != want {
            continue;
        }
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            let key = format!("{}{}", recv_prefix(fd.recv.as_ref()), fd.name.name);
            *counts.entry(key).or_default() += 1;
        }
    }
    counts
}

/// `recvPrefix`: `Foo.` for `func (*Foo) Bar()`, empty for a plain function.
fn recv_prefix(recv: Option<&guff::ast::FieldList>) -> String {
    let Some(recv) = recv else {
        return String::new();
    };
    let Some(ty) = recv.list.first().and_then(|f| f.ty.as_ref()) else {
        return String::new();
    };
    fn ident_name(e: &Expr) -> String {
        match e {
            Expr::Ident(id) => format!("{}.", id.name),
            Expr::StarExpr(s) => ident_name(&s.x),
            Expr::ParenExpr(p) => ident_name(&p.x),
            Expr::IndexExpr(i) => ident_name(&i.x),
            Expr::IndexListExpr(i) => ident_name(&i.x),
            _ => String::new(),
        }
    }
    ident_name(ty)
}

/// Functions whose *results* another function fixes by returning them
/// directly — `return f(...)` means f's results cannot change. (Go:
/// `resultsRequiredBy["return"]`, via `callExtract`.)
fn collect_results_required(ir: &buildir::BuildIrResult) -> HashSet<guff_ssa::ids::FuncId> {
    use guff_ssa::instr::InstrData;
    use guff_ssa::value::Value;

    let mut out = HashSet::new();
    for &fid in ir.src_funcs_with_methods() {
        let func = ir.prog.functions.get(fid);
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                let InstrData::Return(ret) = func.instrs.get(iid) else {
                    continue;
                };
                let Some(call_iid) = call_extract(func, iid, &ret.results) else {
                    continue;
                };
                let InstrData::Call(c) = func.instrs.get(call_iid) else {
                    continue;
                };
                if let Value::Function(callee) = c.call.value {
                    out.insert(callee);
                }
            }
        }
    }
    out
}

/// `callExtract`: the single call these values all come out of, in order, and
/// only when the call is *part of* the parent instruction rather than something
/// assigned earlier —
///
/// ```ignore
/// return fn()          // yes
/// a, b := fn(); return a, b   // no: `prev.Pos() < parent.Pos()`
/// ```
fn call_extract(
    func: &guff_ssa::function::Function,
    parent: guff_ssa::ids::InstrId,
    values: &[guff_ssa::value::Value],
) -> Option<guff_ssa::ids::InstrId> {
    use guff_ssa::instr::InstrData;
    use guff_ssa::value::Value;

    if values.len() == 1 {
        if let Value::Instr(iid) = values[0] {
            if matches!(func.instrs.get(iid), InstrData::Call(_)) {
                return Some(iid);
            }
        }
    }
    let parent_pos = func.pos(parent);
    let mut prev: Option<guff_ssa::ids::InstrId> = None;
    for (i, val) in values.iter().enumerate() {
        let Value::Instr(iid) = val else {
            return None;
        };
        let InstrData::Extract(ex) = func.instrs.get(*iid) else {
            return None;
        };
        if ex.index != i {
            return None; // not extracted in the same order
        }
        let Value::Instr(tuple) = ex.tuple else {
            return None;
        };
        if !matches!(func.instrs.get(tuple), InstrData::Call(_)) {
            return None;
        }
        match prev {
            None => prev = Some(tuple),
            Some(p) if p != tuple => return None,
            _ => {}
        }
    }
    let call = prev?;
    if func.pos(call).0 < parent_pos.0 {
        // `a, b := fn()` then `return a, b`: the call is not part of the
        // return, so the callee's results are not fixed by it.
        return None;
    }
    Some(call)
}

/// `dummyImpl`: a first block that will almost immediately panic, throw, or
/// return constants only. Upstream skips such a function entirely — which is
/// why `func f() (int, error) { return 0, nil }` is not "result 1 is always
/// nil", and why a body whose only call is to `errors.New` is not checked
/// either.
fn dummy_impl(prog: &guff_ssa::program::Program, func: &guff_ssa::function::Function) -> bool {
    use guff_ssa::instr::InstrData;
    use guff_ssa::value::Value;

    let Some((_, block)) = func.live_blocks().next() else {
        return false;
    };
    for &iid in &block.instrs {
        if inserted_store(func, iid) {
            continue; // inserted by go/ssa, not from the code
        }
        let data = func.instrs.get(iid);
        if matches!(data, InstrData::DebugRef(_)) {
            // `buildssa` builds without debug info, so upstream's block holds
            // no `DebugRef`s at all.
            continue;
        }
        let mut bad_operand = false;
        data.for_each_operand(|v| {
            if bad_operand {
                return;
            }
            let ok = match v {
                Value::Const(_) | Value::Function(_) | Value::Global(_) | Value::Param(_) => true,
                Value::Instr(op) => matches!(
                    func.instrs.get(*op),
                    InstrData::ChangeType(_)
                        | InstrData::Alloc(_)
                        | InstrData::MakeInterface(_)
                        | InstrData::MakeMap(_)
                        | InstrData::IndexAddr(_)
                        | InstrData::Slice(_)
                        | InstrData::UnOp(_)
                        // A call operand is neither accepted nor rejected
                        // upstream: the switch simply ends.
                        | InstrData::Call(_)
                ),
                _ => false,
            };
            if !ok {
                bad_operand = true;
            }
        });
        if bad_operand {
            return false;
        }
        match data {
            InstrData::Alloc(_)
            | InstrData::Store(_)
            | InstrData::UnOp(_)
            | InstrData::BinOp(_)
            | InstrData::MakeInterface(_)
            | InstrData::MakeMap(_)
            | InstrData::Extract(_)
            | InstrData::IndexAddr(_)
            | InstrData::FieldAddr(_)
            | InstrData::Slice(_)
            | InstrData::Lookup(_)
            | InstrData::ChangeType(_)
            | InstrData::TypeAssert(_)
            | InstrData::Convert(_)
            | InstrData::ChangeInterface(_) => {}
            InstrData::Return(_) | InstrData::Panic(_) => return true,
            InstrData::Call(c) => {
                let name = call_target_name(prog, func, &c.call);
                if is_harmless_call_name(&name) {
                    continue;
                }
                return name.rsplit('.').next() == Some("throw");
            }
            _ => return false,
        }
    }
    false
}

/// `insertedStore`: a position-less store into an alloc that nothing else
/// refers to — go/ssa's own spill, not the author's code.
fn inserted_store(func: &guff_ssa::function::Function, iid: guff_ssa::ids::InstrId) -> bool {
    use guff_ssa::instr::InstrData;
    use guff_ssa::value::Value;

    if func.pos(iid).is_valid() {
        return false;
    }
    let InstrData::Store(store) = func.instrs.get(iid) else {
        return false;
    };
    let Value::Instr(addr) = store.addr else {
        return false;
    };
    if !matches!(func.instrs.get(addr), InstrData::Alloc(_)) {
        return false;
    }
    guff_analysis::referrers(func, Value::Instr(addr)).len() == 1
}

/// The printed callee of a call, for `rxHarmlessCall`.
fn call_target_name(
    prog: &guff_ssa::program::Program,
    func: &guff_ssa::function::Function,
    common: &guff_ssa::instr::CallCommon,
) -> String {
    use guff_ssa::value::Value;
    if let Some(obj) = common.method {
        return obj.name(&prog.object_arena).to_string();
    }
    match common.value {
        Value::Function(fid) => {
            let callee = prog.functions.get(fid);
            match callee.object.and_then(|o| o.pkg(&prog.object_arena)) {
                Some(pkg) => format!(
                    "{}.{}",
                    prog.package_arena.get(pkg).path(),
                    callee.name
                ),
                None => callee.name.clone(),
            }
        }
        Value::Builtin(b) => prog.builtins.get(b).name.clone(),
        _ => {
            let _ = func;
            String::new()
        }
    }
}

/// `nodeStr` for the expressions that appear as constant arguments.
fn arg_text(expr: &Expr) -> String {
    match expr {
        Expr::Ident(id) => id.name.clone(),
        Expr::BasicLit(lit) => lit.value.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", arg_text(&sel.x), sel.sel.name),
        Expr::ParenExpr(p) => format!("({})", arg_text(&p.x)),
        Expr::UnaryExpr(u) => format!("{}{}", u.op, arg_text(&u.x)),
        Expr::CallExpr(c) if c.args.len() == 1 => {
            format!("{}({})", arg_text(&c.fun), arg_text(&c.args[0]))
        }
        _ => String::new(),
    }
}

fn is_error_type(prog: &guff_ssa::program::Program, typ: guff_types::TypeId) -> bool {
    guff_types::typestring::type_string(
        &prog.type_arena,
        &prog.object_arena,
        &prog.package_arena,
        typ,
        None,
    ) == "error"
}

/// The package's SSA functions, keyed by the object they were declared as.
struct SsaFuncs<'a> {
    prog: &'a guff_ssa::program::Program,
    by_object: std::collections::HashMap<guff_types::ObjectId, guff_ssa::ids::FuncId>,
    sites: CallSites,
    results_required: HashSet<guff_ssa::ids::FuncId>,
    /// Rendered arguments of every call in the package, keyed by the position
    /// go/ssa gives the call — its `(`. (Go: `callByPos`.)
    call_by_pos: std::collections::HashMap<u32, Vec<String>>,
    /// go/ssa's name for each function literal, keyed by the `func` keyword's
    /// position. Upstream reports a literal by that name — `l1$1`, `l4$1$1`,
    /// `init$1$1` for one in a package-level `var` initializer — because it
    /// walks `ssa.Function`s and prints `fn.Name()`. See [`Self::lit_name`].
    lit_names: std::collections::HashMap<u32, String>,
}

impl<'a> SsaFuncs<'a> {
    fn build(ir: &'a buildir::BuildIrResult, files: &[guff::ast::File]) -> Self {
        let mut by_object = std::collections::HashMap::new();
        for &fid in ir.src_funcs_with_methods() {
            if let Some(obj) = ir.prog.functions.get(fid).object {
                by_object.entry(obj).or_insert(fid);
            }
        }
        let mut call_by_pos = std::collections::HashMap::new();
        for file in files {
            walk::inspect(NodeRef::File(file), |n| {
                if let Some(NodeRef::CallExpr(call)) = n {
                    call_by_pos.insert(
                        call.lparen.0 as u32,
                        call.args.iter().map(arg_text).collect::<Vec<_>>(),
                    );
                }
                true
            });
        }
        // Every anonymous function of this package, by the position of its
        // `func` keyword — which is what the builder records as `decl_pos` and
        // what the AST side has in `lit.ty.func`.
        //
        // Walking `prog.functions` rather than `src_funcs_with_methods()` is
        // deliberate: that list starts from named functions, so it omits the
        // synthesized package `init` and everything under it, and a literal in
        // a package-level `var` initializer lives exactly there. Upstream
        // reaches it (`ssautil.AllFunctions`) and names it `init$1`.
        let mut lit_names = std::collections::HashMap::new();
        for (_, f) in ir.prog.functions.iter() {
            if f.pkg != Some(ir.pkg) || f.parent.is_none() {
                continue;
            }
            if f.decl_pos != guff::NO_POS {
                lit_names.insert(f.decl_pos.0 as u32, f.name.clone());
            }
        }
        SsaFuncs {
            prog: &ir.prog,
            by_object,
            sites: CallSites::build(ir),
            results_required: collect_results_required(ir),
            call_by_pos,
            lit_names,
        }
    }

    /// go/ssa's name for the literal whose `func` keyword is at `pos`.
    ///
    /// Upstream prints `fn.Name()`, so a literal is reported as
    /// `<enclosing>$<n>` — never as a placeholder. guff used the string
    /// "<func literal>", which no golangci-lint output can contain, so every
    /// such finding was a guaranteed mismatch. Nothing caught it because the
    /// fixture had no literal in it.
    fn lit_name(&self, pos: guff::Pos) -> Option<&str> {
        self.lit_names.get(&(pos.0 as u32)).map(String::as_str)
    }

    fn func_for(
        &self,
        pass: &Pass<'_>,
        fd: &FuncDecl,
    ) -> Option<(guff_ssa::ids::FuncId, &guff_ssa::function::Function)> {
        let info = pass.types_info()?;
        let obj = (*info.defs.get(&fd.name.id)?)?;
        let fid = *self.by_object.get(&obj)?;
        Some((fid, self.prog.functions.get(fid)))
    }

    /// The constant argument every call site passes at `ssa_pos`, described as
    /// upstream describes it: the source spelling when every site writes it the
    /// same way, and the value in parentheses when the two differ.
    fn const_received_at(
        &self,
        sites: &[(guff_ssa::ids::FuncId, guff_ssa::ids::InstrId, bool)],
        ssa_pos: usize,
        ast_pos: usize,
    ) -> Option<String> {
        use guff_ssa::instr::InstrData;

        let mut seen: Option<guff_ssa::ids::ConstId> = None;
        let mut seen_orig: Option<String> = None;
        let mut first = true;
        for &(caller, iid, _) in sites {
            let caller_fn = self.prog.functions.get(caller);
            let common = match caller_fn.instrs.get(iid) {
                InstrData::Call(c) => &c.call,
                InstrData::Go(g) => &g.call,
                InstrData::Defer(d) => &d.call,
                _ => return None,
            };
            if ssa_pos >= common.args.len() {
                return None;
            }
            let cnst = const_of(self.prog, caller_fn, common.args[ssa_pos])?;
            let orig = self
                .call_by_pos
                .get(&(caller_fn.pos(iid).0 as u32))
                .and_then(|args| args.get(ast_pos))
                .cloned()
                .unwrap_or_default();
            if first {
                seen = Some(cnst);
                seen_orig = Some(orig);
                first = false;
            } else {
                if !consts_equal(self.prog, seen, Some(cnst)) {
                    return None;
                }
                if seen_orig.as_deref() != Some(orig.as_str()) {
                    seen_orig = Some(String::new());
                }
            }
        }
        let cid = seen?;
        let (_, repr) = const_repr(self.prog, self.prog.constants.get(cid));
        match seen_orig {
            Some(orig) if !orig.is_empty() && orig != repr => Some(format!("{orig} ({repr})")),
            _ => Some(repr),
        }
    }

    fn result_types(&self, func: &guff_ssa::function::Function) -> Vec<guff_types::TypeId> {
        let Some(sig) = func.signature else {
            return Vec::new();
        };
        let arena = &self.prog.type_arena;
        let results = guff_types::signature::signature_results(arena, sig);
        let n = guff_types::tuple::tuple_len(arena, results);
        let Some(results) = results else {
            return Vec::new();
        };
        (0..n)
            .filter_map(|i| {
                guff_types::tuple::tuple_at(arena, results, i).typ(&self.prog.object_arena)
            })
            .collect()
    }
}

fn check_func_lit(
    lit: &FuncLit,
    value_lits: &HashSet<u32>,
    ssa: Option<&SsaFuncs<'_>>,
    pending: &mut Vec<(u32, String)>,
) {
    // Literals stored / passed / returned have a fixed signature.
    if value_lits.contains(&func_lit_key(lit)) {
        return;
    }
    let Some(params) = &lit.ty.params else {
        return;
    };
    // Upstream names the literal after its enclosing function (`l1$1`). Without
    // the SSA name there is nothing truthful to print, and a placeholder can
    // only produce a finding golangci-lint never emits — so stay silent.
    let Some(name) = ssa.and_then(|s| s.lit_name(lit.ty.func)) else {
        return;
    };
    let name = name.to_string();
    check_params(&name, &params.list, &lit.body, &[], pending);
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
    let decl_counts = decl_counts(pass);
    let ir = pass.result_of::<buildir::BuildIrResult>(buildir::analyzer());
    let ssa = ir.as_ref().map(|ir| SsaFuncs::build(ir, files));

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
                &decl_counts,
                ssa.as_ref(),
                &mut pending,
            );
        }
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::FuncLit(lit)) = n else {
                return true;
            };
            check_func_lit(lit, &value_lits, ssa.as_ref(), &mut pending);
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
