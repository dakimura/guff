//! Port of [`github.com/polyfloyd/go-errorlint`](https://github.com/polyfloyd/go-errorlint).
//!
//! Default flags match upstream analyzer defaults: comparison + asserts on,
//! errorf off. The allowed-errors table is upstream's, verbatim, and is keyed
//! the way upstream keys it: on the (sentinel, producing function) *pair*.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{
    BinaryExpr, CaseClause, Expr, FuncDecl, Ident, Stmt, SwitchStmt, TypeAssertExpr,
    TypeSwitchStmt,
};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_types::arena::ObjectId;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::util::{expr_string, is_pure_error, type_of, unparen, implements_error};

/// Upstream's `setDefaultAllowedErrors` table, verbatim: a sentinel is only
/// exempt from the comparison check when the error being compared came out of
/// one of *these* functions. Comparing `io.EOF` against something a random
/// function returned is still a finding.
static ALLOWED_ERRORS: &[(&str, &str)] = &[
    ("io.EOF", "(*archive/tar.Reader).Next"),
    ("io.EOF", "(*archive/tar.Reader).Read"),
    ("io.EOF", "(*bufio.Reader).Discard"),
    ("io.EOF", "(*bufio.Reader).Peek"),
    ("io.EOF", "(*bufio.Reader).Read"),
    ("io.EOF", "(*bufio.Reader).ReadByte"),
    ("io.EOF", "(*bufio.Reader).ReadBytes"),
    ("io.EOF", "(*bufio.Reader).ReadLine"),
    ("io.EOF", "(*bufio.Reader).ReadSlice"),
    ("io.EOF", "(*bufio.Reader).ReadString"),
    ("io.EOF", "(*bufio.Scanner).Scan"),
    ("io.EOF", "(*bytes.Buffer).Read"),
    ("io.EOF", "(*bytes.Buffer).ReadByte"),
    ("io.EOF", "(*bytes.Buffer).ReadBytes"),
    ("io.EOF", "(*bytes.Buffer).ReadRune"),
    ("io.EOF", "(*bytes.Buffer).ReadString"),
    ("io.EOF", "(*bytes.Reader).Read"),
    ("io.EOF", "(*bytes.Reader).ReadAt"),
    ("io.EOF", "(*bytes.Reader).ReadByte"),
    ("io.EOF", "(*bytes.Reader).ReadRune"),
    ("io.EOF", "(*bytes.Reader).ReadString"),
    ("database/sql.ErrNoRows", "(*database/sql.Row).Scan"),
    ("io.EOF", "debug/elf.Open"),
    ("io.EOF", "debug/elf.NewFile"),
    ("io.EOF", "(io.ReadCloser).Read"),
    ("io.EOF", "(io.Reader).Read"),
    ("io.EOF", "(io.ReaderAt).ReadAt"),
    ("io.EOF", "(*io.LimitedReader).Read"),
    ("io.EOF", "(*io.SectionReader).Read"),
    ("io.EOF", "(*io.SectionReader).ReadAt"),
    ("io.ErrClosedPipe", "(*io.PipeWriter).Write"),
    ("io.EOF", "io.ReadAtLeast"),
    ("io.ErrShortBuffer", "io.ReadAtLeast"),
    ("io.ErrUnexpectedEOF", "io.ReadAtLeast"),
    ("io.EOF", "io.ReadFull"),
    ("io.ErrUnexpectedEOF", "io.ReadFull"),
    ("mime.ErrInvalidMediaParameter", "mime.ParseMediaType"),
    ("net/http.ErrServerClosed", "(*net/http.Server).ListenAndServe"),
    ("net/http.ErrServerClosed", "(*net/http.Server).ListenAndServeTLS"),
    ("net/http.ErrServerClosed", "(*net/http.Server).Serve"),
    ("net/http.ErrServerClosed", "(*net/http.Server).ServeTLS"),
    ("net/http.ErrServerClosed", "net/http.ListenAndServe"),
    ("net/http.ErrServerClosed", "net/http.ListenAndServeTLS"),
    ("net/http.ErrServerClosed", "net/http.Serve"),
    ("net/http.ErrServerClosed", "net/http.ServeTLS"),
    ("io.EOF", "(*os.File).Read"),
    ("io.EOF", "(*os.File).ReadAt"),
    ("io.EOF", "(*os.File).ReadDir"),
    ("io.EOF", "(*os.File).Readdir"),
    ("io.EOF", "(*os.File).Readdirnames"),
    ("io.EOF", "(*strings.Reader).Read"),
    ("io.EOF", "(*strings.Reader).ReadAt"),
    ("io.EOF", "(*strings.Reader).ReadByte"),
    ("io.EOF", "(*strings.Reader).ReadRune"),
    ("context.DeadlineExceeded", "(context.Context).Err"),
    ("context.Canceled", "(context.Context).Err"),
    ("io.EOF", "(*encoding/json.Decoder).Decode"),
    ("io.EOF", "(*encoding/json.Decoder).Token"),
    ("io.EOF", "(*encoding/csv.Reader).Read"),
    ("io.EOF", "(*mime/multipart.Reader).NextPart"),
    ("io.EOF", "(*mime/multipart.Reader).NextRawPart"),
    ("mime/multipart.ErrMessageTooLarge", "(*mime/multipart.Reader).ReadForm"),
];

/// `allowedErrorWildcards`: prefix match on both halves. `syscall.Errno` values
/// are never wrapped, so any `syscall.E*` compared against the result of any
/// `syscall.*` function is fine.
static ALLOWED_WILDCARDS: &[(&str, &str)] = &[
    ("syscall.E", "syscall."),
    ("golang.org/x/sys/unix.E", "golang.org/x/sys/unix."),
];

/// Upstream's `isNil` is `ex.(*ast.Ident) && ident.Name == "nil"` — a bare type
/// assertion, so a parenthesized `(nil)` is **not** a nil comparison to it and
/// `err != (nil)` is reported. Unparenthesizing here is more sensible and less
/// compatible; all three of upstream's call sites (binary comparison, value
/// switch, type switch) share this one function, so all three follow it.
///
/// The rule is per-matcher, not per-project: `switchComparesNonNil` two
/// functions down asks the same question and answers it the same way, while
/// honnef's `pattern.match` strips parens on both sides and its `astutil.Equal`
/// does not. Read the matcher being ported; there is no general policy.
fn is_nil_ident(e: &Expr) -> bool {
    matches!(e, Expr::Ident(Ident { name, .. }) if name == "nil")
}

fn is_error_type(pass: &Pass<'_>, e: &Expr) -> bool {
    is_pure_error(pass, e)
}

/// Package-wide index that `isAllowedErrorComparison` needs: upstream reads it
/// off `TypesInfoExt` (`IdentifiersForObject` + `NodeParent`), which is built
/// once per package.
struct AllowIndex<'a> {
    /// Ident node id -> the RHS expression assigned to it, when that ident is
    /// the LHS of an assignment (`pass.NodeParent[ident]` being an `AssignStmt`
    /// is the only parent kind upstream looks at).
    assigned_rhs: HashMap<u32, &'a Expr>,
    /// Object -> every ident node in the package that denotes it.
    obj_idents: HashMap<ObjectId, Vec<u32>>,
}

impl<'a> AllowIndex<'a> {
    fn build(pass: &Pass<'_>, files: &'a [guff::ast::File]) -> Self {
        let mut idx = AllowIndex {
            assigned_rhs: HashMap::new(),
            obj_idents: HashMap::new(),
        };
        for file in files {
            walk::inspect(NodeRef::File(file), |n| {
                match n {
                    Some(NodeRef::Ident(id)) => {
                        if let Some(obj) = object_of(pass, id) {
                            idx.obj_idents.entry(obj).or_default().push(id.id);
                        }
                    }
                    Some(NodeRef::AssignStmt(a)) => {
                        for (i, lhs) in a.lhs.iter().enumerate() {
                            let Expr::Ident(name) = lhs else {
                                continue;
                            };
                            // Upstream defaults to `Rhs[0]` and only pairs LHS
                            // with RHS position-wise when the two lists are the
                            // same length — which is what makes `a, b := f()`
                            // (2 vs 1) still trace back to the single call.
                            let rhs = if a.lhs.len() == a.rhs.len() {
                                let j = a
                                    .lhs
                                    .iter()
                                    .position(|l| {
                                        matches!(l, Expr::Ident(other) if other.name == name.name)
                                    })
                                    .unwrap_or(i);
                                a.rhs.get(j)
                            } else {
                                a.rhs.first()
                            };
                            if let Some(rhs) = rhs {
                                idx.assigned_rhs.insert(name.id, rhs);
                            }
                        }
                    }
                    _ => {}
                }
                true
            });
        }
        idx
    }
}

fn object_of(pass: &Pass<'_>, ident: &Ident) -> Option<ObjectId> {
    let info = pass.types_info()?;
    info.uses
        .get(&ident.id)
        .copied()
        .or_else(|| info.defs.get(&ident.id).copied().flatten())
}

/// `assigningCallExprs`: every call whose result was assigned to `subject`,
/// following assignments through intermediate identifiers.
fn assigning_call_exprs<'a>(
    pass: &Pass<'_>,
    idx: &AllowIndex<'a>,
    subject: &Ident,
    visited: &mut HashSet<ObjectId>,
    out: &mut Vec<&'a Expr>,
) {
    let Some(obj) = object_of(pass, subject) else {
        return;
    };
    if !visited.insert(obj) {
        return;
    }
    let Some(idents) = idx.obj_idents.get(&obj) else {
        return;
    };
    for &ident_id in idents {
        if ident_id == subject.id {
            continue;
        }
        let Some(rhs) = idx.assigned_rhs.get(&ident_id) else {
            continue;
        };
        match rhs {
            Expr::CallExpr(_) => out.push(rhs),
            Expr::Ident(next) => {
                if object_of(pass, next) != Some(obj) {
                    assigning_call_exprs(pass, idx, next, visited, out);
                }
            }
            _ => {}
        }
    }
}

/// `(recv).Method` for a method call, `pkg/path.Func` for a package function,
/// and `None` for anything whose callee is not a selector — upstream treats
/// that last case as "not a stdlib function", i.e. not allowed.
fn call_function_name(pass: &Pass<'_>, call: &Expr) -> Option<String> {
    let Expr::CallExpr(call) = call else {
        return None;
    };
    let Expr::SelectorExpr(sel) = unparen(&call.fun) else {
        return None;
    };
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if let Some(selection) = info.selections.get(&sel.id) {
        let recv = guff_types::typestring::type_string(
            &artifacts.types,
            &artifacts.objects,
            &artifacts.packages,
            selection.recv(),
            None,
        );
        let name = selection.obj().name(&artifacts.objects);
        return Some(format!("({recv}).{name}"));
    }
    code::selector_name(pass, sel)
}

fn is_allowed_err_and_func(err: &str, fun: &str) -> bool {
    if ALLOWED_ERRORS
        .iter()
        .any(|(e, f)| *e == err && *f == fun)
    {
        return true;
    }
    ALLOWED_WILDCARDS
        .iter()
        .any(|(e, f)| fun.starts_with(f) && err.starts_with(e))
}

/// Port of `isAllowedErrorComparison`. The exemption is a *pair*: the sentinel
/// on one side and the function that produced the error on the other. guff used
/// to allow four sentinels no matter where the error came from, which both let
/// `net/http.ErrServerClosed` through as a diff (it was not in the four) and
/// silently dropped findings upstream reports (`err == io.EOF` on an error from
/// a function that is not on the list).
fn is_allowed_error_comparison<'a>(
    pass: &Pass<'_>,
    idx: &AllowIndex<'a>,
    a: &'a Expr,
    b: &'a Expr,
) -> bool {
    let mut err_name = String::new();
    let mut calls: Vec<&Expr> = Vec::new();
    for expr in [a, b] {
        match expr {
            Expr::SelectorExpr(sel) => {
                err_name = code::selector_name(pass, sel).unwrap_or_default();
            }
            Expr::Ident(id) => {
                let mut visited = HashSet::new();
                assigning_call_exprs(pass, idx, id, &mut visited, &mut calls);
            }
            Expr::CallExpr(_) => calls.push(expr),
            _ => {}
        }
    }
    if err_name.is_empty() || calls.is_empty() {
        return false;
    }
    calls.iter().all(|call| {
        call_function_name(pass, call).is_some_and(|fun| is_allowed_err_and_func(&err_name, &fun))
    })
}

fn in_error_is_method(stack: &[NodeRef<'_>], pass: &Pass<'_>) -> bool {
    for n in stack.iter().rev() {
        let NodeRef::FuncDecl(FuncDecl { name, recv, ty, .. }) = n else {
            continue;
        };
        if name.name != "Is" || recv.is_none() {
            return false;
        }
        let Some(params) = ty.params.as_ref() else {
            return false;
        };
        if params.list.len() != 1 {
            return false;
        }
        let Some(pt) = params.list[0].ty.as_ref() else {
            return false;
        };
        let param_ok = matches!(unparen(pt), Expr::Ident(Ident { name, .. }) if name == "error")
            || is_pure_error(pass, pt);
        if !param_ok {
            return false;
        }
        let Some(results) = ty.results.as_ref() else {
            return false;
        };
        if results.list.len() != 1 {
            return false;
        }
        return matches!(
            results.list[0].ty.as_ref().map(unparen),
            Some(Expr::Ident(Ident { name, .. })) if name == "bool"
        );
    }
    false
}

fn check_comparison<'a>(
    pass: &Pass<'_>,
    idx: &AllowIndex<'a>,
    be: &'a BinaryExpr,
    stack: &[NodeRef<'_>],
    pending: &mut Vec<Diagnostic>,
) {
    if be.op != Token::EQL && be.op != Token::NEQ {
        return;
    }
    if is_nil_ident(&be.x) || is_nil_ident(&be.y) {
        return;
    }
    if !is_error_type(pass, &be.x) && !is_error_type(pass, &be.y) {
        return;
    }
    if is_allowed_error_comparison(pass, idx, &be.x, &be.y) {
        return;
    }
    if in_error_is_method(stack, pass) {
        return;
    }

    let (err_var, target) = if is_error_type(pass, &be.y) && !is_error_type(pass, &be.x) {
        (&be.y, &be.x)
    } else {
        (&be.x, &be.y)
    };
    let mut replacement = format!(
        "errors.Is({}, {})",
        expr_string(err_var),
        expr_string(target)
    );
    if be.op == Token::NEQ {
        replacement = format!("!{replacement}");
    }
    let start = be.x.pos().0 as u32;
    let end = be.y.end().0 as u32;
    pending.push(Diagnostic {
        pos: start,
        end,
        message: format!(
            "comparing with {} will fail on wrapped errors. Use errors.Is to check for a specific error",
            be.op
        ),
        suggested_fixes: vec![SuggestedFix {
            message: "Use errors.Is() to compare errors".into(),
            text_edits: vec![TextEdit {
                pos: start,
                end,
                new_text: replacement,
            }],
        }],
        ..Diagnostic::default()
    });
}

fn check_type_assert(
    pass: &Pass<'_>,
    ta: &TypeAssertExpr,
    stack: &[NodeRef<'_>],
    pending: &mut Vec<Diagnostic>,
) {
    let Some(ty) = ta.ty.as_deref() else {
        return;
    };
    if !is_error_type(pass, &ta.x) {
        return;
    }
    if in_error_is_method(stack, pass) {
        return;
    }
    let Some(target_typ) = type_of(pass, ty) else {
        return;
    };
    if !implements_error(pass, target_typ) {
        return;
    }
    let message =
        "type assertion on error will fail on wrapped errors. Use errors.As to check for specific errors";
    let fix = type_assert_fix(ta, ty, stack);
    pending.push(Diagnostic {
        pos: ta.lparen.0 as u32,
        message: message.into(),
        suggested_fixes: fix.into_iter().collect(),
        ..Diagnostic::default()
    });
}

/// `generateErrorVarName`: keep the name the code already used, and invent one
/// from the type only when it was `_`.
fn generate_error_var_name(original: &str, type_name: &str) -> String {
    if original != "_" {
        return original.to_string();
    }
    let bare = match type_name.rfind('.') {
        Some(i) => &type_name[i + 1..],
        None => type_name,
    };
    let mut chars = bare.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
        None => "anErr".into(),
    }
}

/// errorlint's `errors.As` rewrite for a type assertion on an error.
///
/// Four shapes, in upstream's order (`lint.go` ~470-608). All of them render
/// the type and the asserted expression with [`expr_string`] — errorlint's own
/// walker, *not* `go/printer` — so the spelling of the replacement follows
/// that walker's arms.
fn type_assert_fix(
    ta: &TypeAssertExpr,
    ty: &Expr,
    stack: &[NodeRef<'_>],
) -> Option<SuggestedFix> {
    const FIX_MESSAGE: &str = "Use errors.As() for type assertions on errors";

    let target_type = expr_string(ty);
    let err_expr = expr_string(&ta.x);
    let (base_type, is_pointer) = match target_type.strip_prefix('*') {
        Some(rest) => (rest.to_string(), true),
        None => (target_type.clone(), false),
    };
    let decl = |var: &str| {
        if is_pointer {
            format!("{var} := &{base_type}{{}}")
        } else {
            format!("var {var} {base_type}")
        }
    };

    let parent = stack.last();

    // `targetErr, ok := err.(*SomeError)` — with or without an enclosing `if`.
    if let Some(NodeRef::AssignStmt(assign)) = parent {
        if assign.lhs.len() == 2 {
            if let Expr::Ident(id) = &assign.lhs[0] {
                let var = generate_error_var_name(&id.name, &base_type);

                // `if targetErr, ok := err.(*SomeError); ok {` replaces the
                // whole head of the if statement, up to the opening brace.
                if let Some(NodeRef::IfStmt(if_stmt)) = stack.get(stack.len().wrapping_sub(2)) {
                    // Upstream's condition is `ifParent.Init == assign`, i.e.
                    // this very statement is the if's initializer — not merely
                    // some assignment inside it.
                    let is_init = matches!(
                        if_stmt.init.as_deref(),
                        Some(Stmt::AssignStmt(init)) if std::ptr::eq(init, *assign)
                    );
                    if is_init {
                        return Some(SuggestedFix {
                            message: FIX_MESSAGE.into(),
                            text_edits: vec![TextEdit {
                                pos: if_stmt.if_.0 as u32,
                                end: if_stmt.body.lbrace.0 as u32,
                                new_text: format!(
                                    "{}\nif errors.As({err_expr}, &{var})",
                                    decl(&var)
                                ),
                            }],
                        });
                    }
                }

                // Upstream keeps whatever the second variable was called.
                let ok_name = match assign.lhs.get(1) {
                    Some(Expr::Ident(ok)) if ok.name != "_" => ok.name.clone(),
                    _ => "ok".to_string(),
                };
                return Some(SuggestedFix {
                    message: FIX_MESSAGE.into(),
                    text_edits: vec![TextEdit {
                        // `AssignStmt.Pos()` is `Lhs[0].Pos()` and `End()` is
                        // `Rhs[len-1].End()`.
                        pos: assign.lhs[0].pos().0 as u32,
                        end: assign
                            .rhs
                            .last()
                            .map(|e| e.end().0 as u32)
                            .unwrap_or(assign.tok_pos.0 as u32),
                        new_text: format!(
                            "{}\n{ok_name} := errors.As({err_expr}, &{var})",
                            decl(&var)
                        ),
                    }],
                });
            }
        }
    }

    // A type assertion sitting directly in an `if` condition. Ported for
    // fidelity; it needs the asserted type to be `bool`, which cannot also
    // implement `error`, so upstream's branch looks unreachable too.
    if let Some(NodeRef::IfStmt(_)) = parent {
        let var = generate_error_var_name("target", &base_type);
        return Some(SuggestedFix {
            message: FIX_MESSAGE.into(),
            text_edits: vec![TextEdit {
                pos: ta.x.pos().0 as u32,
                end: (ta.rparen.0 + 1) as u32,
                new_text: format!("{}\nif errors.As({err_expr}, &{var})", decl(&var)),
            }],
        });
    }

    // Standalone: wrap the assertion in an immediately-called function.
    let var = generate_error_var_name("target", &base_type);
    Some(SuggestedFix {
        message: FIX_MESSAGE.into(),
        text_edits: vec![TextEdit {
            // `TypeAssertExpr.Pos()` is `X.Pos()`, `End()` is `Rparen + 1`.
            pos: ta.x.pos().0 as u32,
            end: (ta.rparen.0 + 1) as u32,
            new_text: format!(
                "func() {target_type} {{\n\t{}\n\t_ = errors.As({err_expr}, &{var})\n\treturn {var}\n}}()",
                decl(&var)
            ),
        }],
    })
}

fn check_type_switch(
    pass: &Pass<'_>,
    ts: &TypeSwitchStmt,
    stack: &[NodeRef<'_>],
    pending: &mut Vec<(u32, String)>,
) {
    if in_error_is_method(stack, pass) {
        return;
    }
    // extract assert from assign
    let assert_x = match &*ts.assign {
        Stmt::ExprStmt(es) => match unparen(&es.x) {
            Expr::TypeAssertExpr(ta) => &ta.x,
            _ => return,
        },
        Stmt::AssignStmt(asgn) => match asgn.rhs.first().map(unparen) {
            Some(Expr::TypeAssertExpr(ta)) => &ta.x,
            _ => return,
        },
        _ => return,
    };
    if !is_error_type(pass, assert_x) {
        return;
    }
    // Only report if some case asserts to an error-implementing type
    let mut has_error_case = false;
    for stmt in &ts.body.list {
        let Stmt::CaseClause(CaseClause { list, .. }) = stmt else {
            continue;
        };
        for e in list {
            if is_nil_ident(e) {
                continue;
            }
            if let Some(t) = type_of(pass, e) {
                if implements_error(pass, t) {
                    has_error_case = true;
                    break;
                }
            }
        }
    }
    if !has_error_case {
        return;
    }
    pending.push((
        ts.switch.0 as u32,
        "type switch on error will fail on wrapped errors. Use errors.As to check for specific errors"
            .into(),
    ));
}

/// `switchComparesNonNil`: a `default` clause (empty list) and `case nil` are
/// safe; anything else — including an identifier that is not `nil` — is not.
fn switch_compares_non_nil(sw: &SwitchStmt) -> bool {
    for stmt in &sw.body.list {
        let Stmt::CaseClause(CaseClause { list, .. }) = stmt else {
            continue;
        };
        for clause in list {
            if let Expr::Ident(id) = clause {
                if id.name == "nil" {
                    continue;
                }
            }
            return true;
        }
    }
    false
}

fn check_value_switch<'a>(
    pass: &Pass<'_>,
    idx: &AllowIndex<'a>,
    sw: &'a SwitchStmt,
    stack: &[NodeRef<'_>],
    pending: &mut Vec<(u32, String)>,
) {
    let Some(tag) = sw.tag.as_ref() else {
        return;
    };
    if !is_error_type(pass, tag) {
        return;
    }
    if in_error_is_method(stack, pass) {
        return;
    }
    // Upstream keeps two questions apart, and they answer differently: *which*
    // clause is problematic (the first non-nil case the allowlist does not
    // exempt) and *whether* the switch compares against anything but `nil` at
    // all (purely syntactic, allowlist not consulted). The finding is reported
    // at the problematic clause's `case` keyword, not at the `switch`.
    let mut problematic: Option<u32> = None;
    'outer: for stmt in &sw.body.list {
        let Stmt::CaseClause(CaseClause { case, list, .. }) = stmt else {
            continue;
        };
        for e in list {
            if is_nil_ident(e) {
                continue;
            }
            if !is_allowed_error_comparison(pass, idx, tag, e) {
                problematic = Some(case.0 as u32);
                break 'outer;
            }
        }
    }
    let Some(case_pos) = problematic else {
        return;
    };
    if !switch_compares_non_nil(sw) {
        return;
    }
    pending.push((
        case_pos,
        "switch on an error will fail on wrapped errors. Use errors.Is to check for specific errors"
            .into(),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "errorlint requires inspect analyzer".to_string())?;

    let mut diags = Vec::new();
    let mut msgs = Vec::new();
    {
        let files = pass.files();
        let idx = AllowIndex::build(pass, files);
        for file in files {
            let mut stack = Vec::new();
            walk::preorder_stack(NodeRef::File(file), &mut stack, |n, stack| {
                match n {
                    NodeRef::BinaryExpr(be) => check_comparison(pass, &idx, be, stack, &mut diags),
                    NodeRef::TypeAssertExpr(ta) => check_type_assert(pass, ta, stack, &mut diags),
                    NodeRef::TypeSwitchStmt(ts) => check_type_switch(pass, ts, stack, &mut msgs),
                    NodeRef::SwitchStmt(sw) => check_value_switch(pass, &idx, sw, stack, &mut msgs),
                    _ => {}
                }
                true
            });
        }
    }
    for d in diags {
        pass.report(d);
    }
    for (pos, message) in msgs {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "errorlint",
        doc: "Linter for error wrapping issues (comparisons and type assertions)",
        url: "https://github.com/polyfloyd/go-errorlint",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
