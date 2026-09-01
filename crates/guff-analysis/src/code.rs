//! Structural and type queries on Go code for linters.
//!
//! Minimal port of `honnef.co/go/tools/analysis/code` helpers used by
//! Staticcheck checks.

use guff::ast::{is_generated, BasicLit, CallExpr, Expr, Ident, SelectorExpr};
use guff::position::{FileSet, Pos};
use guff_constant::{bool_val, int64_val, string_val, Kind};
use guff_types::arena::{ObjectArena, ObjectData, ObjectId, PackageArena, TypeArena, TypeData};
use guff_types::basic::BasicKind;
use guff_types::operand::OperandMode;
use guff_types::selection::SelectionKind;
use guff_types::signature::{signature_params, signature_recv};
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::TypeId;

use crate::pass::Pass;

/// The top-level declarations an SSA-based analyzer can actually reach.
///
/// `go/analysis/passes/buildssa` builds its `SrcFuncs` from a file's
/// `*ast.FuncDecl`s and the function literals nested inside them — and from
/// nothing else:
///
/// ```text
/// for _, f := range pass.Files {
///     for _, decl := range f.Decls {
///         if fdecl, ok := decl.(*ast.FuncDecl); ok { … addAnons(fn) }
/// ```
///
/// A literal in a package-level `var`/`const` initializer therefore belongs to
/// the *synthesized* package `init`, which has no `FuncDecl` and never enters
/// that list.
///
/// guff ports several `buildssa` linters as AST walks, and an AST walk rooted
/// at the file sees those initializers — which is how `bodyclose`, `noctx`,
/// `rowserrcheck` and `sqlclosecheck` each reported findings golangci-lint
/// cannot produce. Ginkgo makes that the common case rather than a curiosity:
/// a suite is written `var _ = Describe("…", func() { … })`, so an entire test
/// file's bodies sit where those analyzers cannot look.
///
/// Root such a walk at each of these instead of at the file.
///
/// **This is not a property of SSA analysis in general — check the upstream
/// before reaching for it.** A linter that builds its own `ssa.Program` and
/// enumerates `ssautil.AllFunctions` sees the package initializer and every
/// literal under it. `unparam` does exactly that
/// (`mvdan.cc/unparam/check.check.go`), and reports on `init$1$1`; restricting
/// its walk this way would delete true findings rather than false ones.
pub fn src_func_decls(file: &guff::ast::File) -> impl Iterator<Item = &guff::ast::FuncDecl> {
    file.decls.iter().filter_map(|d| match d {
        guff::ast::Decl::FuncDecl(fd) => Some(fd),
        _ => None,
    })
}

/// Returns the fully-qualified name of a function or builtin call target,
/// e.g. `"time.Sleep"` or `"len"`.
///
/// Port of `code.CallName` / `typeutil.FuncName`.
pub fn call_name(pass: &Pass<'_>, fun: &Expr) -> Option<String> {
    let info = pass.types_info()?;

    // Upstream opens with `astutil.Unparen(call.Fun)` and then peels one level
    // of instantiation — `f[int](x)` names `f`. Instantiating a function cannot
    // yield another generic function, so once is enough, and `(foo)[T]` is not a
    // valid instantiation, so there is no second unparen.
    let fun = match unparen(fun) {
        Expr::IndexExpr(ix) => unparen(&ix.x),
        Expr::IndexListExpr(ix) => unparen(&ix.x),
        other => other,
    };

    let obj_id = match fun {
        Expr::Ident(id) => info.uses.get(&id.id).copied(),
        Expr::SelectorExpr(sel) => info.uses.get(&sel.sel.id).copied(),
        _ => None,
    }?;
    object_call_name(pass, obj_id)
}

/// Returns the fully-qualified name of an object when used as a call target.
pub fn object_call_name(pass: &Pass<'_>, obj_id: ObjectId) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    match artifacts.objects.get(obj_id) {
        ObjectData::Func(_) => Some(func_name(
            &artifacts.objects,
            &artifacts.packages,
            obj_id,
        )),
        ObjectData::Builtin(b) => Some(b.name().to_string()),
        _ => None,
    }
}

/// Returns the call-check rule key for a function object, matching Go's
/// `typeutil.FuncName`: `"strings.Replace"` for package functions and
/// `"(*regexp.Regexp).FindAll"` for methods.
pub fn type_func_name(
    type_arena: &TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    obj_id: ObjectId,
) -> String {
    let name = obj_id.name(objects);
    let Some(sig_id) = obj_id.typ(objects) else {
        return func_name(objects, packages, obj_id);
    };
    let Some(recv) = signature_recv(type_arena, sig_id) else {
        return func_name(objects, packages, obj_id);
    };
    let recv_type = recv
        .typ(objects)
        .expect("method receiver must have type");
    let recv_str = {
        let resolved = guff_types::alias::unalias_readonly(type_arena, recv_type);
        match type_arena.get(resolved) {
            TypeData::Interface(_) => "interface".to_string(),
            _ => guff_types::typestring::type_string(type_arena, objects, packages, recv_type, None),
        }
    };
    format!("({recv_str}).{name}")
}

/// Returns `"import/path.FuncName"` for a function object.
pub fn func_name(
    objects: &ObjectArena,
    packages: &PackageArena,
    obj_id: ObjectId,
) -> String {
    let name = obj_id.name(objects);
    match obj_id.pkg(objects) {
        Some(pkg) => {
            let path = packages.get(pkg).path();
            if path.is_empty() {
                name.to_string()
            } else {
                format!("{path}.{name}")
            }
        }
        None => name.to_string(),
    }
}

/// Go's `types.Func.FullName()` for the callee of `call`: `encoding/json.Marshal`
/// for a package function, `(*encoding/json.Encoder).Encode` for a method.
///
/// [`call_name`] cannot answer this. It ends in [`func_name`], which is package
/// path plus object name, so a method comes back as `encoding/json.Encode` — a
/// name Go never produces and no upstream rule table ever spells. Every port
/// that needs to recognise a method call has therefore rebuilt this on the
/// side: `noctx::full_call_name` and errcheck both call `call_name` and then
/// redo the lookup for the method form, `musttag::callee_name` skips
/// `call_name` entirely, and `waitgroup` / SA2000 each carry the same
/// three-line "selector says Add and the receiver is a sync.WaitGroup"
/// fallback under a dead `is_call_to(…, "(*sync.WaitGroup).Add")`. errchkjson
/// is the one that did not, and lost every `(*encoding/json.Encoder).Encode`
/// in the corpus without a diff to show for it.
pub fn callee_full_name(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    let obj = call_target_object(pass, &call.fun)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if !matches!(artifacts.objects.get(obj), ObjectData::Func(_)) {
        return None;
    }
    Some(type_func_name(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        obj,
    ))
}

/// Reports whether `call` invokes the named function (`"time.Sleep"`, `"len"`, …).
pub fn is_call_to(pass: &Pass<'_>, call: &CallExpr, name: &str) -> bool {
    call_name(pass, &call.fun).as_deref() == Some(name)
}

/// Reports whether `call` invokes any of the named functions.
pub fn is_call_to_any(pass: &Pass<'_>, call: &CallExpr, names: &[&str]) -> bool {
    call_name(pass, &call.fun)
        .is_some_and(|n| names.iter().any(|want| *want == n))
}

/// If `expr` is an integer constant representable as `i64`, returns its value.
pub fn expr_to_int(pass: &Pass<'_>, expr: &Expr) -> Option<i64> {
    let info = pass.types_info()?;
    let tav = info.types.get(&expr.id())?;
    let val = tav.val.as_ref()?;
    if val.kind() != Kind::Int {
        return None;
    }
    let (n, exact) = int64_val(val);
    exact.then_some(n)
}

/// If `expr` is a string constant, returns its value as the **bytes** Go would
/// see. `"\xff"` is one byte here, and offsets into the result are the offsets
/// Go's own analyzers compute.
pub fn expr_to_bytes(pass: &Pass<'_>, expr: &Expr) -> Option<Vec<u8>> {
    if let Expr::BasicLit(BasicLit { value, .. }) = expr {
        if value.starts_with('"') || value.starts_with('`') {
            return Some(unquote_go_string_bytes(value));
        }
    }
    let info = pass.types_info()?;
    let tav = info.types.get(&expr.id())?;
    let val = tav.val.as_ref()?;
    if val.kind() != Kind::String {
        return None;
    }
    Some(string_val(val))
}

/// If `expr` is a string constant, returns its value as text, with ill-formed
/// bytes replaced by U+FFFD — the same substitution Go makes when a check
/// ranges over the string. A check that inspects the bytes themselves, or that
/// computes an offset into the string, wants [`expr_to_bytes`] instead.
pub fn expr_to_string(pass: &Pass<'_>, expr: &Expr) -> Option<String> {
    expr_to_bytes(pass, expr).map(|b| guff_constant::decode_lossy(&b))
}

/// Reports whether `expr` is the untyped `nil` constant.
///
/// Port of `code.IsNil`.
pub fn is_nil(pass: &Pass<'_>, expr: &Expr) -> bool {
    if let Expr::Ident(ident) = expr {
        if ident.name != "nil" {
            return false;
        }
        let Some(info) = pass.types_info() else {
            return true;
        };
        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
            return true;
        };
        if let Some(obj_id) = info.uses.get(&ident.id).copied() {
            return matches!(artifacts.objects.get(obj_id), ObjectData::Nil(_));
        }
        return true;
    }
    let Some(info) = pass.types_info() else {
        return false;
    };
    info.types
        .get(&expr.id())
        .is_some_and(|tav| tav.mode == OperandMode::NilValue)
}

/// Reports whether `expr` is an untyped or predeclared `true`/`false` identifier.
///
/// Port of `code.IsBoolConst`.
pub fn is_bool_const(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Expr::Ident(ident) = expr else {
        return false;
    };
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(obj_id) = info.uses.get(&ident.id).copied() else {
        return false;
    };
    let ObjectData::Const(c) = artifacts.objects.get(obj_id) else {
        return false;
    };
    match artifacts.types.get(c.typ()) {
        TypeData::Basic(b) => {
            matches!(b.kind(), BasicKind::UntypedBool | BasicKind::Bool)
        }
        _ => false,
    }
}

/// Returns the value of a `true`/`false` identifier constant.
///
/// Port of `code.BoolConst`. Panics if `expr` is not a bool constant ident.
pub fn bool_const(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Expr::Ident(ident) = expr else {
        return false;
    };
    let info = pass.types_info().expect("types info");
    let artifacts = pass.pkg().type_artifacts.as_ref().expect("artifacts");
    let obj_id = info.uses.get(&ident.id).copied().expect("use");
    let ObjectData::Const(c) = artifacts.objects.get(obj_id) else {
        return false;
    };
    let val = c.val();
    debug_assert_eq!(val.kind(), Kind::Bool);
    bool_val(val)
}

/// Reports whether `expr` is an integer constant equal to `value`.
///
/// Port of `code.IsIntegerLiteral` (simplified to `i64` values).
/// Reports whether `expr` is an integer **constant** equal to `value` — any
/// constant expression the type checker folded, `size` and `2*n+1` included.
pub fn is_integer_constant(pass: &Pass<'_>, expr: &Expr, value: i64) -> bool {
    expr_to_int(pass, expr) == Some(value)
}

/// Reports whether `expr` is an integer **literal** equal to `value`.
///
/// This is honnef's `code.IsIntegerLiteral`, whose pattern is
/// `(Or (BasicLit "INT" _) (UnaryExpr (Or "+" "-") (IntegerLiteral _)))` — a
/// *syntactic* test, deliberately: "We only check for the math constants and
/// integer literals, not for all constant expressions. This is to avoid false
/// positives when constant values differ under different build tags."
///
/// Answering it with the folded constant instead made `l >= logrus.PanicLevel`
/// — a named constant that happens to be zero — read as `l >= 0` (velero,
/// SA4003; S1004, SA4024 and SA4028 had the same shape).
pub fn is_integer_literal(pass: &Pass<'_>, expr: &Expr, value: i64) -> bool {
    fn is_literal_shape(expr: &Expr) -> bool {
        match expr {
            Expr::BasicLit(lit) => lit.kind == Some(guff::token::Token::INT),
            Expr::UnaryExpr(u) => {
                matches!(u.op, guff::token::Token::ADD | guff::token::Token::SUB)
                    && is_literal_shape(&u.x)
            }
            // `pattern.match` strips parentheses before dispatching, so `(0)`
            // is an integer literal to upstream as well.
            Expr::ParenExpr(p) => is_literal_shape(&p.x),
            _ => false,
        }
    }
    is_literal_shape(expr) && expr_to_int(pass, expr) == Some(value)
}

/// Reports whether `pos` lies in a generated file (`// Code generated ... DO NOT EDIT.`).
pub fn is_generated_at(pass: &Pass<'_>, pos: u32) -> bool {
    let pos = Pos(pos as i64);
    for (i, file) in pass.files().iter().enumerate() {
        if file.file_start.0 > pos.0 || pos.0 > file.file_end.0 {
            continue;
        }
        if let Some(result) =
            pass.result_of::<crate::passes::facts::generated::GeneratedResult>(
                crate::passes::facts::generated::analyzer(),
            )
        {
            if let Some(path) = pass.pkg().compiled_go_files.get(i) {
                let key = path.to_string_lossy();
                if result.files.contains_key(key.as_ref()) {
                    return true;
                }
            }
        }
        return is_generated(file);
    }
    false
}

/// Strips redundant parentheses, as `astutil.Unparen` does.
///
/// Every check upstream writes as a `pattern.MustParse` gets this for free and
/// at *every* level: `pattern.match` unwraps `*ast.ParenExpr` on both sides
/// before it binds anything (`pattern/match.go`). A hand-rolled port that
/// destructures the AST directly does not, and the resulting check is silent on
/// exactly the inputs a parenthesis touches — the shape nobody writes a fixture
/// for. SA1006 (2026-08-13) and SA6006 (2026-08-13) were both found this way.
pub fn unparen(mut e: &Expr) -> &Expr {
    while let Expr::ParenExpr(p) = e {
        e = &p.x;
    }
    e
}

/// Source ranges of the package's runnable examples — the functions
/// `irutil.IsExample` answers `true` for.
///
/// Upstream's test is on the *IR function*: `strings.HasPrefix(fn.Name(),
/// "Example")` and the file it was declared in ends in `_test.go`. Nothing
/// about the receiver or the signature is consulted, and an anonymous function
/// inside `ExampleFoo` is named `ExampleFoo$1`, so closures are covered by the
/// prefix too. A range over the whole `FuncDecl` therefore matches: every node
/// upstream skips lies inside one of these, and no node it visits does.
///
/// Callers that walk the AST with `inspect` (rather than over `SrcFuncs`) need
/// this to reach the same set — see `SA9003` and `SA4006`, the two checks
/// upstream guards with it.
pub fn example_func_spans(pass: &Pass<'_>) -> Vec<(i64, i64)> {
    use guff::ast::Decl;

    let mut spans = Vec::new();
    for (i, file) in pass.files().iter().enumerate() {
        let is_test = pass
            .pkg()
            .compiled_go_files
            .get(i)
            .is_some_and(|p| p.to_string_lossy().ends_with("_test.go"));
        if !is_test {
            continue;
        }
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else { continue };
            if !f.name.name.starts_with("Example") {
                continue;
            }
            spans.push((decl.pos().0, decl.end().0));
        }
    }
    spans
}

/// Reports whether `pos` falls inside one of [`example_func_spans`].
pub fn in_example_func(spans: &[(i64, i64)], pos: u32) -> bool {
    let p = pos as i64;
    spans.iter().any(|&(start, end)| start <= p && p <= end)
}

/// Returns the type-checker object for an identifier, whether a definition or use.
pub fn object_of(pass: &Pass<'_>, ident: &Ident) -> Option<ObjectId> {
    let info = pass.types_info()?;
    if let Some(obj) = info.defs.get(&ident.id) {
        return obj.as_ref().copied();
    }
    info.uses.get(&ident.id).copied()
}

/// Reports whether `expr` refers to `obj` (honnef `code.RefersTo`).
///
/// Walks through parentheses, selectors, indexes, calls, unary/binary ops,
/// composites, and function literals so cases like `append(x, …)` or a
/// recursive `visit = func…{ visit(…) }` correctly suppress S1021.
pub fn refers_to(pass: &Pass<'_>, expr: &Expr, obj: ObjectId) -> bool {
    match expr {
        Expr::Ident(ident) => object_of(pass, ident) == Some(obj),
        Expr::ParenExpr(p) => refers_to(pass, &p.x, obj),
        Expr::SelectorExpr(s) => refers_to(pass, &s.x, obj),
        Expr::IndexExpr(i) => {
            refers_to(pass, &i.x, obj) || refers_to(pass, &i.index, obj)
        }
        Expr::IndexListExpr(i) => {
            refers_to(pass, &i.x, obj) || i.indices.iter().any(|e| refers_to(pass, e, obj))
        }
        Expr::SliceExpr(s) => {
            refers_to(pass, &s.x, obj)
                || s.low.as_ref().is_some_and(|e| refers_to(pass, e, obj))
                || s.high.as_ref().is_some_and(|e| refers_to(pass, e, obj))
                || s.max.as_ref().is_some_and(|e| refers_to(pass, e, obj))
        }
        Expr::StarExpr(s) => refers_to(pass, &s.x, obj),
        Expr::UnaryExpr(u) => refers_to(pass, &u.x, obj),
        Expr::BinaryExpr(b) => refers_to(pass, &b.x, obj) || refers_to(pass, &b.y, obj),
        Expr::CallExpr(c) => {
            refers_to(pass, &c.fun, obj) || c.args.iter().any(|a| refers_to(pass, a, obj))
        }
        Expr::TypeAssertExpr(t) => refers_to(pass, &t.x, obj),
        Expr::CompositeLit(cl) => {
            cl.ty.as_ref().is_some_and(|t| refers_to(pass, t, obj))
                || cl.elts.iter().any(|e| refers_to(pass, e, obj))
        }
        Expr::KeyValueExpr(kv) => {
            refers_to(pass, &kv.key, obj) || refers_to(pass, &kv.value, obj)
        }
        Expr::FuncLit(fl) => fl.body.list.iter().any(|stmt| stmt_refers_to(pass, stmt, obj)),
        _ => false,
    }
}

fn stmt_refers_to(pass: &Pass<'_>, stmt: &guff::ast::Stmt, obj: ObjectId) -> bool {
    use guff::ast::Stmt;
    match stmt {
        Stmt::ExprStmt(e) => refers_to(pass, &e.x, obj),
        Stmt::AssignStmt(a) => {
            a.lhs.iter().any(|e| refers_to(pass, e, obj))
                || a.rhs.iter().any(|e| refers_to(pass, e, obj))
        }
        Stmt::ReturnStmt(r) => r.results.iter().any(|e| refers_to(pass, e, obj)),
        Stmt::IfStmt(i) => {
            i.init
                .as_ref()
                .is_some_and(|s| stmt_refers_to(pass, s, obj))
                || refers_to(pass, &i.cond, obj)
                || i.body.list.iter().any(|s| stmt_refers_to(pass, s, obj))
                || i.else_
                    .as_ref()
                    .is_some_and(|s| stmt_refers_to(pass, s, obj))
        }
        Stmt::ForStmt(f) => {
            f.init
                .as_ref()
                .is_some_and(|s| stmt_refers_to(pass, s, obj))
                || f.cond.as_ref().is_some_and(|e| refers_to(pass, e, obj))
                || f.post
                    .as_ref()
                    .is_some_and(|s| stmt_refers_to(pass, s, obj))
                || f.body.list.iter().any(|s| stmt_refers_to(pass, s, obj))
        }
        Stmt::RangeStmt(r) => {
            r.key.as_ref().is_some_and(|e| refers_to(pass, e, obj))
                || r.value.as_ref().is_some_and(|e| refers_to(pass, e, obj))
                || refers_to(pass, &r.x, obj)
                || r.body.list.iter().any(|s| stmt_refers_to(pass, s, obj))
        }
        Stmt::BlockStmt(b) => b.list.iter().any(|s| stmt_refers_to(pass, s, obj)),
        Stmt::GoStmt(g) => call_refers_to(pass, &g.call, obj),
        Stmt::DeferStmt(d) => call_refers_to(pass, &d.call, obj),
        Stmt::SendStmt(s) => {
            refers_to(pass, &s.chan_, obj) || refers_to(pass, &s.value, obj)
        }
        Stmt::IncDecStmt(i) => refers_to(pass, &i.x, obj),
        Stmt::SwitchStmt(s) => {
            s.init
                .as_ref()
                .is_some_and(|st| stmt_refers_to(pass, st, obj))
                || s.tag.as_ref().is_some_and(|e| refers_to(pass, e, obj))
                || s.body.list.iter().any(|st| stmt_refers_to(pass, st, obj))
        }
        Stmt::TypeSwitchStmt(s) => {
            s.init
                .as_ref()
                .is_some_and(|st| stmt_refers_to(pass, st, obj))
                || stmt_refers_to(pass, &s.assign, obj)
                || s.body.list.iter().any(|st| stmt_refers_to(pass, st, obj))
        }
        Stmt::CaseClause(c) => {
            c.list.iter().any(|e| refers_to(pass, e, obj))
                || c.body.iter().any(|s| stmt_refers_to(pass, s, obj))
        }
        Stmt::CommClause(c) => {
            c.comm
                .as_ref()
                .is_some_and(|s| stmt_refers_to(pass, s, obj))
                || c.body.iter().any(|s| stmt_refers_to(pass, s, obj))
        }
        Stmt::SelectStmt(s) => s.body.list.iter().any(|st| stmt_refers_to(pass, st, obj)),
        Stmt::LabeledStmt(l) => stmt_refers_to(pass, &l.stmt, obj),
        _ => false,
    }
}

fn call_refers_to(pass: &Pass<'_>, call: &guff::ast::CallExpr, obj: ObjectId) -> bool {
    refers_to(pass, &call.fun, obj) || call.args.iter().any(|a| refers_to(pass, a, obj))
}

/// `astutil.EqualSyntax` (x/tools `internal/astutil/equal.go:32`): the two
/// expressions have the same shape, with identifiers compared **by name** and
/// positions ignored.
///
/// This is not [`same_non_dynamic`], which asks whether two expressions denote
/// the same *value* and therefore refuses anything it cannot prove pure. Several
/// x/tools analyzers ask the syntactic question instead, and get a different
/// answer: `count := len(a); if len(b) < len(a) { count = len(b) }` is a minmax
/// candidate upstream because `len(a)` is written twice, and was not for guff
/// because a call is dynamic. Two syncthing files and three others in the corpus
/// hang on exactly that.
///
/// Composite and type-literal operands (`func(){}`, `struct{…}`, an interface
/// literal) answer `false`: upstream compares them structurally, but no caller
/// here needs it and a wrong `true` is the dangerous direction.
pub fn equal_syntax(a: &Expr, b: &Expr) -> bool {
    use guff::ast::Expr as E;
    match (a, b) {
        (E::Ident(x), E::Ident(y)) => x.name == y.name,
        (E::BasicLit(x), E::BasicLit(y)) => x.kind == y.kind && x.value == y.value,
        (E::ParenExpr(x), E::ParenExpr(y)) => equal_syntax(&x.x, &y.x),
        (E::SelectorExpr(x), E::SelectorExpr(y)) => {
            x.sel.name == y.sel.name && equal_syntax(&x.x, &y.x)
        }
        (E::IndexExpr(x), E::IndexExpr(y)) => {
            equal_syntax(&x.x, &y.x) && equal_syntax(&x.index, &y.index)
        }
        (E::IndexListExpr(x), E::IndexListExpr(y)) => {
            equal_syntax(&x.x, &y.x) && equal_syntax_list(&x.indices, &y.indices)
        }
        (E::SliceExpr(x), E::SliceExpr(y)) => {
            x.slice3 == y.slice3
                && equal_syntax(&x.x, &y.x)
                && equal_syntax_opt(x.low.as_deref(), y.low.as_deref())
                && equal_syntax_opt(x.high.as_deref(), y.high.as_deref())
                && equal_syntax_opt(x.max.as_deref(), y.max.as_deref())
        }
        (E::TypeAssertExpr(x), E::TypeAssertExpr(y)) => {
            equal_syntax(&x.x, &y.x)
                && equal_syntax_opt(x.ty.as_deref(), y.ty.as_deref())
        }
        (E::CallExpr(x), E::CallExpr(y)) => {
            x.ellipsis.is_valid() == y.ellipsis.is_valid()
                && equal_syntax(&x.fun, &y.fun)
                && equal_syntax_list(&x.args, &y.args)
        }
        (E::StarExpr(x), E::StarExpr(y)) => equal_syntax(&x.x, &y.x),
        (E::UnaryExpr(x), E::UnaryExpr(y)) => x.op == y.op && equal_syntax(&x.x, &y.x),
        (E::BinaryExpr(x), E::BinaryExpr(y)) => {
            x.op == y.op && equal_syntax(&x.x, &y.x) && equal_syntax(&x.y, &y.y)
        }
        (E::KeyValueExpr(x), E::KeyValueExpr(y)) => {
            equal_syntax(&x.key, &y.key) && equal_syntax(&x.value, &y.value)
        }
        (E::Ellipsis(x), E::Ellipsis(y)) => {
            equal_syntax_opt(x.elt.as_deref(), y.elt.as_deref())
        }
        (E::ArrayType(x), E::ArrayType(y)) => {
            equal_syntax_opt(x.len.as_deref(), y.len.as_deref()) && equal_syntax(&x.elt, &y.elt)
        }
        (E::MapType(x), E::MapType(y)) => {
            equal_syntax(&x.key, &y.key) && equal_syntax(&x.value, &y.value)
        }
        (E::ChanType(x), E::ChanType(y)) => x.dir == y.dir && equal_syntax(&x.value, &y.value),
        _ => false,
    }
}

fn equal_syntax_opt(a: Option<&Expr>, b: Option<&Expr>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => equal_syntax(x, y),
        _ => false,
    }
}

fn equal_syntax_list(a: &[Expr], b: &[Expr]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| equal_syntax(x, y))
}

/// Reports whether two expressions denote the same non-dynamic value.
///
/// Port of the `sameNonDynamic` helper in `simple/s1017`.
pub fn same_non_dynamic(pass: &Pass<'_>, a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(x), Expr::Ident(y)) => {
            let Some(info) = pass.types_info() else {
                return x.name == y.name;
            };
            let ox = info
                .defs
                .get(&x.id)
                .and_then(|o| o.as_ref())
                .or_else(|| info.uses.get(&x.id));
            let oy = info
                .defs
                .get(&y.id)
                .and_then(|o| o.as_ref())
                .or_else(|| info.uses.get(&y.id));
            ox == oy
        }
        (Expr::SelectorExpr(x), Expr::SelectorExpr(y)) => {
            x.sel.name == y.sel.name && same_non_dynamic(pass, &x.x, &y.x)
        }
        (Expr::IndexExpr(x), Expr::IndexExpr(y)) => {
            same_non_dynamic(pass, &x.x, &y.x) && same_non_dynamic(pass, &x.index, &y.index)
        }
        (Expr::BasicLit(x), Expr::BasicLit(y)) => x.value == y.value,
        _ => false,
    }
}

/// Returns the fully-qualified name of a selector expression target,
/// e.g. `"sort.IntSlice"`.
pub fn selector_name(pass: &Pass<'_>, sel: &guff::ast::SelectorExpr) -> Option<String> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;

    if let Expr::Ident(pkg_ident) = &*sel.x {
        let obj_id = info
            .defs
            .get(&pkg_ident.id)
            .and_then(|o| *o)
            .or_else(|| info.uses.get(&pkg_ident.id).copied())?;
        if let ObjectData::PkgName(pkg) = artifacts.objects.get(obj_id) {
            let path = artifacts.packages.get(pkg.imported()).path();
            return Some(format!("{path}.{}", sel.sel.name));
        }
    }

    let pkg = call_name(pass, &sel.x)?;
    Some(format!("{pkg}.{}", sel.sel.name))
}

/// Reports whether `ident` refers to the predeclared `true` or `false` constant.
pub fn predeclared_bool_ident(pass: &Pass<'_>, ident: &Ident) -> Option<bool> {
    if ident.name != "true" && ident.name != "false" {
        return None;
    }
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let obj_id = info.uses.get(&ident.id).copied()?;
    if obj_id.pkg(&artifacts.objects).is_some() {
        return None;
    }
    match artifacts.objects.get(obj_id) {
        ObjectData::Const(c) => {
            let val = c.val();
            if val.kind() != Kind::Bool {
                return None;
            }
            Some(guff_constant::bool_val(val))
        }
        _ => None,
    }
}

/// Returns the type-checker object for a call expression's function target.
///
/// Peels type instantiation (`f[T]`, `f[T1, T2]`) like `typeutil.usedIdent`.
pub fn call_target_object(pass: &Pass<'_>, fun: &Expr) -> Option<ObjectId> {
    let info = pass.types_info()?;
    let mut e = fun;
    while let Expr::ParenExpr(p) = e {
        e = &p.x;
    }
    match e {
        Expr::IndexExpr(ix)
            if info
                .types
                .get(&ix.index.id())
                .is_some_and(|tv| tv.mode == OperandMode::TypeExpr) =>
        {
            call_target_object(pass, &ix.x)
        }
        Expr::IndexListExpr(ix) => call_target_object(pass, &ix.x),
        Expr::Ident(id) => info.uses.get(&id.id).copied(),
        Expr::SelectorExpr(sel) => info.uses.get(&sel.sel.id).copied(),
        _ => None,
    }
}

/// Returns the type of the first parameter of function `obj`, if any.
pub fn first_param_type(pass: &Pass<'_>, obj: ObjectId) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let ObjectData::Func(_) = artifacts.objects.get(obj) else {
        return None;
    };
    let sig = obj.typ(&artifacts.objects)?;
    let params = signature_params(&artifacts.types, sig)?;
    if tuple_len(&artifacts.types, Some(params)) == 0 {
        return None;
    }
    let param = tuple_at(&artifacts.types, params, 0);
    param.typ(&artifacts.objects)
}

/// Reports whether `expr`'s type renders as `want` (e.g. `"net/http.Header"`).
///
/// Port of `code.IsOfTypeWithName`.
pub fn is_of_type_with_name(pass: &Pass<'_>, expr: &Expr, want: &str) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(typ) = info.types.get(&expr.id()).map(|tv| tv.typ) else {
        return false;
    };
    type_with_name(pass, typ, want)
}

/// Reports whether `expr`'s type is a pointer to a named type `want`
/// (e.g. `want == "net/url.URL"` for `*url.URL`).
///
/// Port of `code.IsOfPointerToTypeWithName`.
pub fn is_of_pointer_to_type_with_name(pass: &Pass<'_>, expr: &Expr, want: &str) -> bool {
    is_of_pointer_to_type_with_name_id(pass, expr.id(), want)
}

/// Like [`is_of_pointer_to_type_with_name`] but keyed by AST node id (works for
/// pattern bindings that yield `Ident`/`NodeRef` rather than `&Expr`).
pub fn is_of_pointer_to_type_with_name_id(pass: &Pass<'_>, expr_id: u32, want: &str) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(typ) = info.types.get(&expr_id).map(|tv| tv.typ) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = guff_types::alias::unalias_readonly(&artifacts.types, typ);
    let guff_types::arena::TypeData::Pointer(p) = artifacts.types.get(typ) else {
        return false;
    };
    type_with_name(pass, p.elem(), want)
}

/// Reports whether `typ` renders as `want` (e.g. `"context.Context"`).
pub fn type_with_name(pass: &Pass<'_>, typ: TypeId, want: &str) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    ) == want
}

/// Reports whether `sel` selects method `name` on a value (not a method expression).
pub fn is_method_val(pass: &Pass<'_>, sel: &SelectorExpr, name: &str) -> bool {
    if sel.sel.name != name {
        return false;
    }
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(selection) = info.selections.get(&sel.id) else {
        return false;
    };
    if selection.kind() != SelectionKind::MethodVal {
        return false;
    }
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    matches!(artifacts.objects.get(selection.obj()), ObjectData::Func(_))
}

/// Reports whether `expr` refers to `io.SeekStart`, `io.SeekCurrent`, or `io.SeekEnd`.
pub fn is_io_seek_whence(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Expr::SelectorExpr(sel) = expr else {
        return false;
    };
    selector_name(pass, sel).is_some_and(|n| {
        matches!(
            n.as_str(),
            "io.SeekStart" | "io.SeekCurrent" | "io.SeekEnd"
        )
    })
}

/// Reports whether the package under analysis is `package main`.
pub fn is_main(pass: &Pass<'_>) -> bool {
    pass.pkg().name == "main"
}

/// Reports whether the package is main-like (main or imports cobra).
pub fn is_main_like(pass: &Pass<'_>) -> bool {
    if is_main(pass) {
        return true;
    }
    pass.pkg().imports.contains_key("github.com/spf13/cobra")
}

/// Reports whether `pos` lies in a `*_test.go` file.
pub fn is_in_test_at(pass: &Pass<'_>, pos: u32) -> bool {
    let p = Pos(pos as i64);
    for (i, file) in pass.files().iter().enumerate() {
        if file.file_start.0 <= p.0 && p.0 <= file.file_end.0 {
            if let Some(path) = pass.pkg().compiled_go_files.get(i) {
                return path.to_string_lossy().ends_with("_test.go");
            }
        }
    }
    false
}

/// Returns the module/toolchain Go version string (e.g. `"go1.22"`).
pub fn module_go_version(pass: &Pass<'_>) -> String {
    pass.pkg()
        .module
        .as_ref()
        .and_then(|m| {
            let v = m.go_version.trim();
            if v.is_empty() {
                None
            } else if v.starts_with("go") {
                Some(v.to_string())
            } else {
                Some(format!("go{v}"))
            }
        })
        .or_else(|| {
            pass.type_pkg().and_then(|pkg| {
                let artifacts = pass.pkg().type_artifacts.as_ref()?;
                let v = artifacts.packages.get(pkg).go_version();
                if v.is_empty() {
                    None
                } else {
                    Some(v.to_string())
                }
            })
        })
        .unwrap_or_else(|| "go1.22".to_string())
}

/// Returns the Go version applicable to source at `pos`.
///
/// Simplified port of `code.StdlibVersion`.
pub fn stdlib_version(pass: &Pass<'_>, pos: u32) -> String {
    let module = module_go_version(pass);
    let file_ver = file_go_version(pass, pos);
    if file_ver.is_empty() {
        return module;
    }
    if version_compare(&module, "go1.21") < 0 {
        return file_ver;
    }
    if version_compare(&file_ver, &module) > 0 {
        file_ver
    } else {
        module
    }
}

/// File-level Go version from `//go:build` (empty when unset).
pub fn file_go_version(pass: &Pass<'_>, pos: u32) -> String {
    let p = Pos(pos as i64);
    for file in pass.files() {
        if file.file_start.0 <= p.0 && p.0 <= file.file_end.0 {
            let v = file.go_version.trim();
            if v.is_empty() {
                return String::new();
            }
            if v.starts_with("go") {
                return v.to_string();
            }
            return format!("go{v}");
        }
    }
    String::new()
}

/// Effective language version for source at `pos`: file build-tag version, else
/// the module `go` line (matches go/types `FileVersions` defaulting).
pub fn effective_file_go_version(pass: &Pass<'_>, pos: u32) -> String {
    let file = file_go_version(pass, pos);
    if file.is_empty() {
        module_go_version(pass)
    } else {
        file
    }
}

/// Host toolchain version string (`go1.26.4`), memoized via GOROOT/`GOVERSION`.
pub fn toolchain_go_version() -> String {
    guff_packages::detect_go_version_string()
}

/// Compares two Go versions (`-1`, `0`, `1`). Invalid versions compare as equal.
pub fn version_compare(a: &str, b: &str) -> i32 {
    let a = parse_go_version(a);
    let b = parse_go_version(b);
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn parse_go_version(v: &str) -> u32 {
    let v = v.strip_prefix("go").unwrap_or(v);
    let major_minor = v.split('.').take(2).collect::<Vec<_>>();
    let major: u32 = major_minor.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor: u32 = major_minor.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    major * 1000 + minor
}

/// Returns the import path for an object's package, if any.
pub fn object_pkg_path(pass: &Pass<'_>, obj: ObjectId) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let pkg = obj.pkg(&artifacts.objects)?;
    Some(artifacts.packages.get(pkg).path().to_string())
}

/// Returns the selector name for a selector expression (e.g. `"io.SeekStart"`).
pub fn selector_name_for(pass: &Pass<'_>, sel: &SelectorExpr) -> String {
    selector_name(pass, sel).unwrap_or_else(|| format!("({}).{}", "?", sel.sel.name))
}

/// Returns the fully-qualified API name used by `knowledge.StdlibDeprecations`.
///
/// Port of `code.SelectorName`.
pub fn knowledge_selector_name(pass: &Pass<'_>, sel: &SelectorExpr) -> String {
    let (Some(info), Some(artifacts)) = (pass.types_info(), pass.pkg().type_artifacts.as_ref()) else {
        return selector_name_for(pass, sel);
    };
    if let Some(selection) = info.selections.get(&sel.id) {
        let obj = selection.obj();
        let name = obj.name(&artifacts.objects);
        let mut recv_type = selection.recv();
        if let ObjectData::Var(v) = artifacts.objects.get(obj) {
            if v.is_field() {
                let resolved =
                    guff_types::alias::unalias_readonly(&artifacts.types, recv_type);
                if let TypeData::Pointer(p) = artifacts.types.get(resolved) {
                    recv_type = p.elem();
                }
            }
        }
        let recv_str = {
            let resolved = guff_types::alias::unalias_readonly(&artifacts.types, recv_type);
            match artifacts.types.get(resolved) {
                TypeData::Interface(_) => "interface".to_string(),
                _ => guff_types::typestring::type_string(
                    &artifacts.types,
                    &artifacts.objects,
                    &artifacts.packages,
                    recv_type,
                    None,
                ),
            }
        };
        return format!("({recv_str}).{name}");
    }
    if let Expr::Ident(pkg_ident) = &*sel.x {
        let obj_id = info
            .defs
            .get(&pkg_ident.id)
            .and_then(|o| *o)
            .or_else(|| info.uses.get(&pkg_ident.id).copied());
        if let Some(obj_id) = obj_id {
            if let ObjectData::PkgName(pn) = artifacts.objects.get(obj_id) {
                let path = artifacts.packages.get(pn.imported()).path();
                return format!("{path}.{}", sel.sel.name);
            }
        }
    }
    selector_name_for(pass, sel)
}

fn unquote_go_string_bytes(lit: &str) -> Vec<u8> {
    if let Some(inner) = lit.strip_prefix('`').and_then(|s| s.strip_suffix('`')) {
        return inner.as_bytes().to_vec();
    }
    let bytes = lit.as_bytes();
    if bytes.first() != Some(&b'"') {
        return lit.as_bytes().to_vec();
    }
    // Go's escape set, byte for byte. The previous version handled only
    // `\n`, `\t`, `\"` and `\\` and dropped the backslash from everything
    // else, so `"\x41"` decoded to `x41` and `"\101"` to `101` — a silent
    // corruption of every string constant that uses a byte or unicode escape,
    // shared by ~40 call sites through `expr_to_string`.
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 1;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' {
            break;
        }
        if c != b'\\' {
            out.push(c);
            i += 1;
            continue;
        }
        let Some(&esc) = bytes.get(i + 1) else { break };
        i += 2;
        let simple = match esc {
            b'a' => Some(0x07),
            b'b' => Some(0x08),
            b'f' => Some(0x0c),
            b'n' => Some(b'\n'),
            b'r' => Some(b'\r'),
            b't' => Some(b'\t'),
            b'v' => Some(0x0b),
            b'\\' => Some(b'\\'),
            b'\'' => Some(b'\''),
            b'"' => Some(b'"'),
            _ => None,
        };
        if let Some(b) = simple {
            out.push(b);
            continue;
        }
        // `\xHH` and `\OOO` denote a single *byte*, which is why the result is
        // assembled as bytes: a Go string may hold invalid UTF-8.
        let hex = |n: usize, out: &mut Vec<u8>, i: &mut usize| -> bool {
            let Some(digits) = bytes.get(*i..*i + n).and_then(|d| std::str::from_utf8(d).ok())
            else {
                return false;
            };
            let Ok(v) = u32::from_str_radix(digits, 16) else {
                return false;
            };
            *i += n;
            match n {
                2 => out.push(v as u8),
                _ => match char::from_u32(v) {
                    Some(ch) => out.extend_from_slice(ch.encode_utf8(&mut [0u8; 4]).as_bytes()),
                    None => return false,
                },
            }
            true
        };
        let ok = match esc {
            b'x' => hex(2, &mut out, &mut i),
            b'u' => hex(4, &mut out, &mut i),
            b'U' => hex(8, &mut out, &mut i),
            // `\OOO` is exactly three octal digits, the first of which was
            // already consumed as `esc`.
            b'0'..=b'7' => match bytes
                .get(i - 1..i + 2)
                .and_then(|d| std::str::from_utf8(d).ok())
                .and_then(|d| u8::from_str_radix(d, 8).ok())
            {
                Some(v) => {
                    out.push(v);
                    i += 2;
                    true
                }
                None => false,
            },
            _ => false,
        };
        if !ok {
            // Not a valid escape: the literal did not come from the Go parser.
            // Keep the character so the result stays close to the source.
            out.push(esc);
        }
    }
    out
}

/// Translate a position from a re-parsed file's [`FileSet`] into the analysis
/// one.
///
/// Checks that need comments have to re-parse, because the analysis AST drops
/// them. A re-parse gets a fresh `FileSet`, so its positions are offsets into a
/// *different* position space — reporting one directly lands wherever that
/// offset happens to fall in the shared space, typically some unrelated file in
/// GOROOT. Mapping by line alone is the other half of the trap: it snaps every
/// finding to column 1.
///
/// `file_pos` is any position inside the analysis file being reported on
/// (`pass.files()[i].pos()`). Returns `None` when the line does not exist in the
/// analysis file, which means the two parses disagree and the caller should
/// drop the finding rather than guess.
pub fn remap_reparsed_pos(
    fset: &FileSet,
    file_pos: Pos,
    reparsed_fset: &FileSet,
    pos: Pos,
) -> Option<Pos> {
    let p = reparsed_fset.position(pos);
    let ft = fset.file(file_pos)?;
    if p.line < 1 || p.line as usize > ft.line_count() {
        return None;
    }
    Some(Pos(ft.line_start(p.line as usize).0 + (p.column - 1).max(0)))
}

/// Render `expr` exactly as `go/printer` would.
///
/// Upstream linters reach for `printer.Fprint` / `format.Node` when they need
/// an expression's *source text* in a message; several guff ports had grown a
/// hand-rolled walker instead. Those walkers put blanks around every binary
/// operator, while go/printer drops them around an operator nested under a
/// lower-precedence one (`len(a)/2 + len(b)`), and they answered a placeholder
/// string for every node kind they did not enumerate.
///
/// Not to be confused with [`guff_types::exprstring::expr_string`], the port of
/// `types.ExprString` — that one shortens deliberately (`…` for composite
/// literals) and is a different renderer with different callers.
pub fn node_text(pass: &Pass<'_>, expr: &Expr) -> Option<String> {
    let mut buf: Vec<u8> = Vec::new();
    guff::printer::fprint(&mut buf, pass.fset(), guff::printer::PrintNode::Expr(expr)).ok()?;
    String::from_utf8(buf).ok()
}

/// [`node_text`] for a statement.
pub fn stmt_text(pass: &Pass<'_>, stmt: &guff::ast::Stmt) -> Option<String> {
    let mut buf: Vec<u8> = Vec::new();
    guff::printer::fprint(&mut buf, pass.fset(), guff::printer::PrintNode::Stmt(stmt)).ok()?;
    String::from_utf8(buf).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unquote_handles_double_and_backtick_strings() {
        assert_eq!(unquote_go_string_bytes("\"hello\""), b"hello");
        assert_eq!(unquote_go_string_bytes("`raw`"), b"raw");
    }

    /// Every escape Go's scanner accepts. The old decoder only knew four of
    /// them and dropped the backslash from the rest, so `"\\x41"` came back as
    /// `x41` — wrong value *and* wrong length, which is how printf's column
    /// mapping caught it.
    #[test]
    fn unquote_decodes_every_go_escape() {
        assert_eq!(
            unquote_go_string_bytes(r#""\a\b\f\n\r\t\v""#),
            b"\x07\x08\x0c\n\r\t\x0b"
        );
        assert_eq!(unquote_go_string_bytes(r#""\\ \" \'""#), b"\\ \" '");
        assert_eq!(unquote_go_string_bytes(r#""\x41\x42""#), b"AB");
        assert_eq!(unquote_go_string_bytes(r#""\101\102""#), b"AB");
        assert_eq!(unquote_go_string_bytes(r#""\u00e9""#), "\u{e9}".as_bytes());
        assert_eq!(
            unquote_go_string_bytes(r#""\U0001F600""#),
            "\u{1f600}".as_bytes()
        );
        // Raw strings keep their backslashes verbatim.
        assert_eq!(unquote_go_string_bytes("`\\x41`"), b"\\x41");
    }

    /// A byte escape names one byte; `\u` for the same number names a code
    /// point, which is two bytes here. Conflating them is what let
    /// `regexp.MustCompile("\xff")` compile.
    #[test]
    fn unquote_keeps_byte_escapes_as_single_bytes() {
        assert_eq!(unquote_go_string_bytes(r#""\xff""#), b"\xff");
        assert_eq!(unquote_go_string_bytes(r#""\377""#), b"\xff");
        assert_eq!(unquote_go_string_bytes(r#""\u00ff""#), b"\xc3\xbf");
    }
}
