//! Port of [`github.com/bombsimon/wsl/v5`](https://github.com/bombsimon/wsl)
//! (golangci-lint wrapper `wsl_v5`).
//!
//! Implements default cuddle checks with v5 message format and core settings
//! (`allow-first-in-block` / `allow-whole-block` / `branch-max-lines` /
//! `cuddle-max-statements` / `default`+`enable`/`disable`).
//!
//! DEFERRED: SuggestedFix; `case-max-lines` trailing newlines; `after-block` /
//! `after-decl` / `after-defer` / `after-expr` / `after-go`; `assign-exclusive` /
//! `assign-expr` / `cuddle-group`; full `err` type Implements; decl grouping
//! (`maybeGroupDecl`); Lock/Unlock heuristics; comment-map nuance.

use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;

use guff::ast::{BlockStmt, Decl, Expr, Spec, Stmt};
use guff::walk::{preorder, stmt_ref, NodeRef};
use guff::parser::{parse_file, COMMENTS_ONLY};
use guff::position::{FileSet, Pos};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use guff_types::arena::{ObjectData, ObjectId};

use crate::options::{WslV5Check, WslV5Options};

const MSG_ABOVE: &str = "missing whitespace above this line";
const MSG_REMOVE: &str = "unnecessary whitespace";
const MSG_BLOCK_START: &str = "unnecessary whitespace (leading-whitespace)";
const MSG_BLOCK_END: &str = "unnecessary whitespace (trailing-whitespace)";

fn line(fset: &FileSet, pos: Pos) -> i64 {
    fset.position(pos).line
}

fn stmt_start(fset: &FileSet, s: &Stmt) -> i64 {
    line(fset, s.pos())
}

fn stmt_end(fset: &FileSet, s: &Stmt) -> i64 {
    line(fset, s.end())
}

fn lists_overlap(a: &[String], b: &[String]) -> bool {
    let set: HashSet<&str> = a.iter().map(|s| s.as_str()).collect();
    b.iter().any(|x| set.contains(x.as_str()))
}

fn find_lhs(stmt: &Stmt) -> Vec<String> {
    match stmt {
        Stmt::AssignStmt(a) => a.lhs.iter().flat_map(expr_idents).collect(),
        Stmt::IncDecStmt(i) => expr_idents(&i.x),
        Stmt::DeclStmt(d) => match &d.decl {
            Decl::GenDecl(g) => g
                .specs
                .iter()
                .flat_map(|sp| match sp {
                    Spec::ValueSpec(vs) => {
                        vs.names.iter().map(|n| n.name.clone()).collect::<Vec<_>>()
                    }
                    _ => Vec::new(),
                })
                .collect(),
            _ => Vec::new(),
        },
        Stmt::IfStmt(i) => expr_idents(&i.cond),
        _ => Vec::new(),
    }
}

fn find_rhs(stmt: &Stmt) -> Vec<String> {
    match stmt {
        Stmt::AssignStmt(a) => a.rhs.iter().flat_map(expr_idents).collect(),
        Stmt::ExprStmt(e) => expr_idents(&e.x),
        Stmt::IfStmt(i) => {
            let mut v = expr_idents(&i.cond);
            if let Some(init) = &i.init {
                v.extend(find_rhs(init));
                v.extend(find_lhs(init));
            }
            v
        }
        Stmt::RangeStmt(r) => {
            let mut v = expr_idents(&r.x);
            if let Some(k) = &r.key {
                v.extend(expr_idents(k));
            }
            if let Some(val) = &r.value {
                v.extend(expr_idents(val));
            }
            v
        }
        Stmt::ForStmt(f) => f.cond.as_ref().map(expr_idents).unwrap_or_default(),
        Stmt::DeferStmt(d) => call_idents(&d.call),
        Stmt::GoStmt(g) => call_idents(&g.call),
        Stmt::ReturnStmt(r) => r.results.iter().flat_map(expr_idents).collect(),
        Stmt::SwitchStmt(s) => s.tag.as_ref().map(expr_idents).unwrap_or_default(),
        Stmt::TypeSwitchStmt(s) => {
            let mut v = find_lhs(&s.assign);
            v.extend(find_rhs(&s.assign));
            v
        }
        Stmt::SelectStmt(_) => Vec::new(),
        Stmt::SendStmt(s) => {
            let mut v = expr_idents(&s.chan_);
            v.extend(expr_idents(&s.value));
            v
        }
        Stmt::IncDecStmt(i) => expr_idents(&i.x),
        Stmt::LabeledStmt(l) => find_rhs(&l.stmt),
        _ => Vec::new(),
    }
}

fn call_idents(c: &guff::ast::CallExpr) -> Vec<String> {
    let mut out = Vec::new();
    collect_idents(&c.fun, &mut out);
    for a in &c.args {
        collect_idents(a, &mut out);
    }
    out
}

fn expr_idents(e: &Expr) -> Vec<String> {
    let mut out = Vec::new();
    collect_idents(e, &mut out);
    out
}

fn collect_idents(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Ident(i) => out.push(i.name.clone()),
        Expr::SelectorExpr(s) => {
            collect_idents(&s.x, out);
            out.push(s.sel.name.clone());
        }
        Expr::CallExpr(c) => {
            collect_idents(&c.fun, out);
            for a in &c.args {
                collect_idents(a, out);
            }
        }
        Expr::ParenExpr(p) => collect_idents(&p.x, out),
        Expr::UnaryExpr(u) => collect_idents(&u.x, out),
        Expr::StarExpr(s) => collect_idents(&s.x, out),
        Expr::BinaryExpr(b) => {
            collect_idents(&b.x, out);
            collect_idents(&b.y, out);
        }
        Expr::IndexExpr(i) => {
            collect_idents(&i.x, out);
            collect_idents(&i.index, out);
        }
        Expr::SliceExpr(s) => {
            collect_idents(&s.x, out);
            if let Some(l) = &s.low {
                collect_idents(l, out);
            }
            if let Some(h) = &s.high {
                collect_idents(h, out);
            }
        }
        Expr::TypeAssertExpr(t) => collect_idents(&t.x, out),
        Expr::KeyValueExpr(kv) => {
            collect_idents(&kv.key, out);
            collect_idents(&kv.value, out);
        }
        Expr::CompositeLit(c) => {
            for elt in &c.elts {
                collect_idents(elt, out);
            }
        }
        Expr::FuncLit(f) => {
            for s in &f.body.list {
                out.extend(find_lhs(s));
                out.extend(find_rhs(s));
            }
        }
        _ => {}
    }
}

fn first_body_stmt(stmt: &Stmt) -> Option<&Stmt> {
    match stmt {
        Stmt::IfStmt(i) => i.body.list.first(),
        Stmt::RangeStmt(r) => r.body.list.first(),
        Stmt::ForStmt(f) => f.body.list.first(),
        Stmt::SwitchStmt(s) => {
            if let Some(Stmt::CaseClause(c)) = s.body.list.first() {
                c.body.first()
            } else {
                None
            }
        }
        Stmt::TypeSwitchStmt(s) => {
            if let Some(Stmt::CaseClause(c)) = s.body.list.first() {
                c.body.first()
            } else {
                None
            }
        }
        Stmt::SelectStmt(s) => {
            if let Some(Stmt::CommClause(c)) = s.body.list.first() {
                c.body.first()
            } else {
                None
            }
        }
        Stmt::DeferStmt(d) => {
            if let Expr::FuncLit(fl) = &*d.call.fun {
                fl.body.list.first()
            } else {
                None
            }
        }
        Stmt::GoStmt(g) => {
            if let Expr::FuncLit(fl) = &*g.call.fun {
                fl.body.list.first()
            } else {
                None
            }
        }
        _ => None,
    }
}

fn whole_block_idents(stmt: &Stmt) -> Vec<String> {
    let mut out = find_lhs(stmt);
    out.extend(find_rhs(stmt));
    match stmt {
        Stmt::IfStmt(i) => {
            for s in &i.body.list {
                out.extend(find_lhs(s));
                out.extend(find_rhs(s));
            }
        }
        Stmt::RangeStmt(r) => {
            for s in &r.body.list {
                out.extend(find_lhs(s));
                out.extend(find_rhs(s));
            }
        }
        Stmt::ForStmt(f) => {
            for s in &f.body.list {
                out.extend(find_lhs(s));
                out.extend(find_rhs(s));
            }
        }
        _ => {}
    }
    out
}

/// wsl's `identsFromNode(node, skipBlock=true)`: every identifier a statement
/// mentions, minus the ones upstream drops — type names, universe constants,
/// `nil`, package qualifiers and `_`. `skip` holds the node ids of those, built
/// once per package from the type-checker's `Uses`/`Defs` (see `ident_skip_set`);
/// an identifier the checker never resolved is *kept*, as upstream keeps it.
///
/// Nested blocks are not descended into, which is what `skipBlock` means: a
/// func literal's body does not contribute the caller's identifiers.
/// `(start_line, end_line)` for every comment group in `path`.
///
/// Mirrors `funlen`'s on-demand re-parse: the production typecheck runs without
/// `PARSE_COMMENTS`, so `File::comments` is empty in a pass. The re-parse gets
/// its own `FileSet`, which is why callers compare lines rather than positions.
fn reparse_comment_lines(path: &Path, cached: Option<&[u8]>) -> Option<Vec<(i64, i64)>> {
    let owned;
    let src: &[u8] = match cached {
        Some(b) => b,
        None => {
            owned = std::fs::read(path).ok()?;
            &owned
        }
    };
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, src, COMMENTS_ONLY).ok()?;
    Some(
        file.comments
            .iter()
            .map(|cg| (line(&fset, cg.pos()), line(&fset, cg.end())))
            .collect(),
    )
}

fn stmt_all_idents(stmt: &Stmt, skip: &HashSet<u32>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    preorder(stmt_ref(stmt), |n| {
        match n {
            // `skipBlock`: do not descend into a nested block.
            NodeRef::BlockStmt(_) => return false,
            NodeRef::Ident(id) => {
                if id.name.is_empty() || id.name == "_" || skip.contains(&id.id) {
                    return true;
                }
                if seen.insert(id.name.clone()) {
                    out.push(id.name.clone());
                }
            }
            _ => {}
        }
        true
    });
    out
}

/// wsl's `hasIntersection`: do the two statements name a common identifier?
fn has_intersection(a: &Stmt, b: &Stmt, skip: &HashSet<u32>) -> bool {
    lists_overlap(&stmt_all_idents(a, skip), &stmt_all_idents(b, skip))
}

/// The node ids of identifiers `identsFromNode` filters out —
/// `isTypeOrPredeclConst` plus `*types.Nil` and `*types.PkgName`.
///
/// Without this an assignment cuddled with a call that merely shares a *type*
/// name would read as sharing a variable, and the `assign` check would go
/// quiet where upstream reports.
fn ident_skip_set(pass: &Pass<'_>) -> HashSet<u32> {
    let mut skip = HashSet::new();
    let (Some(info), Some(artifacts)) = (pass.types_info(), pass.pkg().type_artifacts.as_ref())
    else {
        return skip;
    };
    let objects = &artifacts.objects;
    let mut consider = |node: u32, obj: ObjectId| {
        let drop = match objects.get(obj) {
            ObjectData::TypeName(_) => true,
            ObjectData::Nil(_) => true,
            ObjectData::PkgName(_) => true,
            // Only the universe constants (`true`, `false`, `iota`), not every
            // named constant.
            ObjectData::Const(_) => obj.parent(objects).is_none(),
            _ => false,
        };
        if drop {
            skip.insert(node);
        }
    };
    for (node, obj) in &info.uses {
        consider(*node, *obj);
    }
    for (node, obj) in &info.defs {
        if let Some(obj) = obj {
            consider(*node, *obj);
        }
    }
    skip
}

fn is_assign_or_inc(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::AssignStmt(_) | Stmt::IncDecStmt(_))
}

fn is_assign_decl_or_inc(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::AssignStmt(_) | Stmt::DeclStmt(_) | Stmt::IncDecStmt(_)
    )
}

fn cuddle_target(
    stmt: &Stmt,
    options: &WslV5Options,
) -> Vec<String> {
    if options.allow_whole_block {
        return whole_block_idents(stmt);
    }
    let mut v = find_lhs(stmt);
    v.extend(find_rhs(stmt));
    if options.allow_first_in_block {
        if let Some(first) = first_body_stmt(stmt) {
            v.extend(find_lhs(first));
            v.extend(find_rhs(first));
        }
    }
    v
}

fn n_cuddled_before(fset: &FileSet, stmts: &[Stmt], i: usize) -> usize {
    let mut n = 0;
    let mut cur = i;
    while cur > 0 {
        let prev = &stmts[cur - 1];
        let curr = &stmts[cur];
        if stmt_end(fset, prev) + 1 != stmt_start(fset, curr) {
            break;
        }
        n += 1;
        cur -= 1;
    }
    n
}

fn check_enabled(options: &WslV5Options, check: WslV5Check) -> bool {
    options.checks.contains(&check)
}

fn msg_invalid(check: &str) -> String {
    format!("{MSG_ABOVE} (invalid statement above {check})")
}

fn msg_no_shared(check: &str) -> String {
    format!("{MSG_ABOVE} (no shared variables above {check})")
}

fn msg_too_many(check: &str) -> String {
    format!("{MSG_ABOVE} (too many statements above {check})")
}

fn msg_too_many_lines(check: &str) -> String {
    format!("{MSG_ABOVE} (too many lines above {check})")
}

fn msg_never(check: &str) -> String {
    format!("{MSG_ABOVE} (never cuddle {check})")
}

fn is_err_not_nil_check(stmt: &Stmt) -> Option<String> {
    let Stmt::IfStmt(ifs) = stmt else {
        return None;
    };
    if ifs.init.is_some() {
        return None;
    }
    let Expr::BinaryExpr(b) = &ifs.cond else {
        return None;
    };
    if b.op != guff::token::Token::NEQ {
        return None;
    }
    // err != nil  /  nil != err
    let (err_side, nil_side) = if matches!(&*b.y, Expr::Ident(id) if id.name == "nil") {
        (&*b.x, &*b.y)
    } else if matches!(&*b.x, Expr::Ident(id) if id.name == "nil") {
        (&*b.y, &*b.x)
    } else {
        return None;
    };
    let _ = nil_side;
    let Expr::Ident(err) = err_side else {
        return None;
    };
    // Upstream checks Implements(error); we approximate with name "err" / "*err*".
    if err.name == "err" || err.name.ends_with("Err") || err.name.starts_with("err") {
        Some(err.name.clone())
    } else {
        None
    }
}

fn assign_defines_err(stmt: &Stmt, err_name: &str) -> bool {
    find_lhs(stmt).iter().any(|n| n == err_name)
}

/// `checkError`'s own `previousIdents`, which is **not** `identsFromNode`:
/// upstream builds it from the left-hand side of an `*ast.AssignStmt` and the
/// names of a `*ast.DeclStmt`, and from nothing else (wsl v5.8.0 `wsl.go:777`).
/// So an `if` above whose *init* assigns `err` contributes no idents, the
/// intersection is empty, and upstream returns before reporting anything.
///
/// authelia's `parseAttributeURI` is exactly that shape:
///
/// ```go
/// if uri, err = url.ParseRequestURI(value); err == nil {
///     …
/// }
///
/// if err != nil {
/// ```
///
/// `find_lhs` answers the wider question (it reaches into an `if` cond), which
/// is right for `checkCuddlingMaxAllowed` and wrong here.
fn err_declared_directly_above(stmt: &Stmt, err_name: &str) -> bool {
    match stmt {
        Stmt::AssignStmt(_) | Stmt::DeclStmt(_) => {
            find_lhs(stmt).iter().any(|n| n == err_name)
        }
        _ => false,
    }
}

/// Upstream bails out of `checkError` when a comment sits between the error
/// assignment and the `if`, unless that comment ends on the assignment's own
/// last line — i.e. unless it is a trailing comment (wsl v5.8.0 `wsl.go:803`).
/// A comment on its own line is content the author put there deliberately, and
/// removing the blank line would move it.
///
/// Compared by line, not position: the production typecheck parses without
/// comments, so `comment_lines` comes from a re-parse with its own `FileSet`
/// (see `check_leading_trailing` for the same constraint).
fn comment_blocks_err_cuddle(
    comment_lines: &[(i64, i64)],
    prev_end_line: i64,
    if_line: i64,
) -> bool {
    comment_lines.iter().any(|&(cg_start, cg_end)| {
        cg_start >= prev_end_line && cg_start < if_line && cg_end != prev_end_line
    })
}

fn check_leading_trailing(
    fset: &FileSet,
    comment_lines: &[(i64, i64)],
    block: &BlockStmt,
    options: &WslV5Options,
    pending: &mut Vec<(u32, String)>,
) {
    if block.list.is_empty() {
        return;
    }
    let start = line(fset, Pos(block.lbrace.0 + 1));
    let end = line(fset, block.rbrace);
    if start == end {
        return;
    }
    let first = &block.list[0];
    if check_enabled(options, WslV5Check::LeadingWhitespace) {
        // A comment between `{` and the first statement is *content*, so the
        // gap is measured to the comment, not past it. Upstream then makes a
        // second check for a blank line between the comments and the first
        // statement. Without this, `func f() {` followed by `//nolint:…` and
        // then the first statement reads as a leading blank line.
        // (wsl v5.8.0 `wsl.go` checkLeadingNewline.)
        let first_stmt_line = stmt_start(fset, first);
        // Comments are compared by *line*, not position: the production
        // typecheck parses without comments, so these come from a re-parse
        // with its own `FileSet` (see `run`). Line numbers agree between two
        // parses of the same source; raw positions do not.
        let mut first_content_line = first_stmt_line;
        let mut last_comment_end_line = start;
        let mut saw_comment = false;
        for &(cg_start, cg_end) in comment_lines {
            if cg_start <= start || cg_start >= first_stmt_line {
                continue;
            }
            saw_comment = true;
            if cg_start < first_content_line {
                first_content_line = cg_start;
            }
            if cg_end > last_comment_end_line {
                last_comment_end_line = cg_end;
            }
        }
        if first_content_line > start + 1 {
            pending.push((block.lbrace.0 as u32 + 1, MSG_BLOCK_START.into()));
        }
        // A blank line between the leading comments and the first statement is
        // its own diagnostic upstream.
        if saw_comment
            && last_comment_end_line > start
            && first_stmt_line > last_comment_end_line + 1
        {
            pending.push((block.lbrace.0 as u32 + 1, MSG_BLOCK_START.into()));
        }
    }
    let last = block.list.last().unwrap();
    if check_enabled(options, WslV5Check::TrailingWhitespace)
        && end > stmt_end(fset, last) + 1
    {
        pending.push((block.rbrace.0 as u32, MSG_BLOCK_END.into()));
    }
}

fn check_err_cuddle(
    fset: &FileSet,
    comment_lines: &[(i64, i64)],
    stmts: &[Stmt],
    i: usize,
    options: &WslV5Options,
    pending: &mut Vec<(u32, String)>,
) {
    if !check_enabled(options, WslV5Check::Err) || i == 0 {
        return;
    }
    let stmt = &stmts[i];
    let Some(err_name) = is_err_not_nil_check(stmt) else {
        return;
    };
    let prev = &stmts[i - 1];
    if !err_declared_directly_above(prev, &err_name) {
        return;
    }
    let prev_end_line = stmt_end(fset, prev);
    let if_line = stmt_start(fset, stmt);
    if comment_blocks_err_cuddle(comment_lines, prev_end_line, if_line) {
        return;
    }
    // Gap between err assign and if → remove whitespace. Upstream reports the
    // *removal range*, whose start is `file.LineStart(previousEndLine + 1)` —
    // column 1 of the first blank line, not the assignment above it
    // (wsl v5.8.0 `wsl.go:827`).
    if prev_end_line + 1 < if_line {
        let pos = fset
            .file(prev.pos())
            .map(|f| f.line_start(prev_end_line as usize + 1))
            .unwrap_or_else(|| prev.pos());
        pending.push((pos.0 as u32, format!("{MSG_REMOVE} (err)")));
    }
}

/// `checkCuddlingMaxAllowed`.
///
/// `enforce_limit` is upstream's parameter of the same name, and only
/// `checkExprStmt` passes it `false` (wsl v5.8.0 `wsl.go:867`). It gates *both*
/// the `err`-precedence branch and the `cuddle-max-statements` limit, so an
/// expression statement is never reported for having too many statements above
/// it — only for cuddling an invalid type or sharing no identifier.
fn check_cuddle_blockish(
    fset: &FileSet,
    stmts: &[Stmt],
    i: usize,
    check_name: &str,
    enforce_limit: bool,
    options: &WslV5Options,
    pending: &mut Vec<(u32, String)>,
) {
    let stmt = &stmts[i];
    let prev = &stmts[i - 1];
    let n_above = n_cuddled_before(fset, stmts, i);
    if n_above == 0 {
        return;
    }

    if !is_assign_decl_or_inc(prev)
        && !matches!(stmt, Stmt::DeferStmt(_) | Stmt::GoStmt(_))
    {
        pending.push((stmt.pos().0 as u32, msg_invalid(check_name)));
        return;
    }

    let assigned_above = find_lhs(prev);
    let target = cuddle_target(stmt, options);
    if !lists_overlap(&assigned_above, &target)
        && !lists_overlap(&find_rhs(prev), &target)
    {
        // Decl above may also contribute names via find_lhs.
        pending.push((stmt.pos().0 as u32, msg_no_shared(check_name)));
        return;
    }

    if !enforce_limit {
        return;
    }

    // err check always wins over cuddle-max when enabled.
    if check_enabled(options, WslV5Check::Err) {
        if let Some(err_name) = is_err_not_nil_check(stmt) {
            if assign_defines_err(prev, &err_name) {
                if n_above > 1 {
                    if let Some(extra) = stmts.get(i.saturating_sub(2)) {
                        pending.push((extra.pos().0 as u32, msg_too_many(check_name)));
                    }
                }
                return;
            }
        }
    }

    let max = options.cuddle_max_statements;
    if max == 0 {
        pending.push((stmt.pos().0 as u32, msg_too_many(check_name)));
        return;
    }
    if n_above > max {
        let idx = i - max;
        pending.push((stmts[idx].pos().0 as u32, msg_too_many(check_name)));
    }
}

fn check_statements(
    fset: &FileSet,
    skip: &HashSet<u32>,
    comment_lines: &[(i64, i64)],
    stmts: &[Stmt],
    options: &WslV5Options,
    pending: &mut Vec<(u32, String)>,
) {
    for (i, stmt) in stmts.iter().enumerate() {
        match stmt {
            Stmt::IfStmt(ifs) => {
                check_block(fset, skip, comment_lines, &ifs.body, options, pending);
                if let Some(else_) = &ifs.else_ {
                    walk_else(fset, skip, comment_lines, else_, options, pending);
                }
            }
            Stmt::RangeStmt(r) => check_block(fset, skip, comment_lines, &r.body, options, pending),
            Stmt::ForStmt(f) => check_block(fset, skip, comment_lines, &f.body, options, pending),
            Stmt::SwitchStmt(s) => {
                for c in &s.body.list {
                    if let Stmt::CaseClause(cc) = c {
                        check_statements(fset, skip, comment_lines, &cc.body, options, pending);
                    }
                }
            }
            Stmt::TypeSwitchStmt(s) => {
                for c in &s.body.list {
                    if let Stmt::CaseClause(cc) = c {
                        check_statements(fset, skip, comment_lines, &cc.body, options, pending);
                    }
                }
            }
            Stmt::SelectStmt(s) => {
                for c in &s.body.list {
                    if let Stmt::CommClause(cc) = c {
                        check_statements(fset, skip, comment_lines, &cc.body, options, pending);
                    }
                }
            }
            Stmt::AssignStmt(a) => {
                for r in &a.rhs {
                    if let Expr::FuncLit(fl) = r {
                        check_block(fset, skip, comment_lines, &fl.body, options, pending);
                    }
                }
            }
            Stmt::ExprStmt(e) => {
                if let Expr::CallExpr(c) = &e.x {
                    if let Expr::FuncLit(fl) = &*c.fun {
                        check_block(fset, skip, comment_lines, &fl.body, options, pending);
                    }
                }
            }
            Stmt::DeferStmt(d) => {
                if let Expr::FuncLit(fl) = &*d.call.fun {
                    check_block(fset, skip, comment_lines, &fl.body, options, pending);
                }
            }
            Stmt::GoStmt(g) => {
                if let Expr::FuncLit(fl) = &*g.call.fun {
                    check_block(fset, skip, comment_lines, &fl.body, options, pending);
                }
            }
            Stmt::LabeledStmt(l) => {
                // Labels recurse into labeled statement.
                check_statements(fset, skip, comment_lines, std::slice::from_ref(l.stmt.as_ref()), options, pending);
            }
            _ => {}
        }

        if i == 0 {
            continue;
        }
        check_err_cuddle(fset, comment_lines, stmts, i, options, pending);

        let prev = &stmts[i - 1];
        let cuddled = stmt_end(fset, prev) + 1 == stmt_start(fset, stmt);
        if !cuddled {
            continue;
        }

        match stmt {
            Stmt::IfStmt(_) if check_enabled(options, WslV5Check::If) => {
                check_cuddle_blockish(fset, stmts, i, "if", true, options, pending);
            }
            Stmt::ReturnStmt(_) if check_enabled(options, WslV5Check::Return) => {
                if stmts.is_empty() {
                    continue;
                }
                let first = &stmts[0];
                let last = stmts.last().unwrap();
                if stmt_end(fset, last) - stmt_start(fset, first) < options.branch_max_lines as i64
                {
                    continue;
                }
                pending.push((stmt.pos().0 as u32, msg_too_many_lines("return")));
            }
            Stmt::BranchStmt(_) if check_enabled(options, WslV5Check::Branch) => {
                let first = &stmts[0];
                let last = stmts.last().unwrap();
                if stmt_end(fset, last) - stmt_start(fset, first) < options.branch_max_lines as i64
                {
                    continue;
                }
                pending.push((stmt.pos().0 as u32, msg_too_many_lines("branch")));
            }
            Stmt::AssignStmt(a) => {
                let is_append = a.rhs.iter().any(|e| {
                    matches!(
                        e,
                        Expr::CallExpr(c)
                            if matches!(&*c.fun, Expr::Ident(id) if id.name == "append")
                    )
                });
                if is_append && check_enabled(options, WslV5Check::Append) {
                    let assigned_above = find_lhs(prev);
                    let called_above = find_rhs(prev);
                    let rhs = find_rhs(stmt);
                    let mut above = assigned_above;
                    above.extend(called_above);
                    if !lists_overlap(&above, &rhs) {
                        pending.push((stmt.pos().0 as u32, msg_no_shared("append")));
                    }
                    continue;
                }
                if !check_enabled(options, WslV5Check::Assign) {
                    continue;
                }
                if is_assign_or_inc(prev) {
                    continue;
                }
                if matches!(prev, Stmt::ExprStmt(_)) {
                    // Upstream only rejects expr→assign outright once the
                    // (non-default) `assign-expr` check is on. With the default
                    // set, an assignment may cuddle an expression statement
                    // *when the two share an identifier* — the common
                    // `assert.Equal(t, …, c.Now())` / `c.now = …` shape.
                    // (wsl v5.8.0 `wsl.go` checkAssign: the `CheckAssignExpr`
                    // guard around `hasIntersection`.)
                    if !check_enabled(options, WslV5Check::AssignExpr)
                        && has_intersection(stmt, prev, skip)
                    {
                        continue;
                    }
                    pending.push((stmt.pos().0 as u32, msg_invalid("assign")));
                    continue;
                }
                if is_assign_decl_or_inc(prev) {
                    continue;
                }
                // After blocks etc.
                if matches!(
                    prev,
                    Stmt::IfStmt(_)
                        | Stmt::ForStmt(_)
                        | Stmt::RangeStmt(_)
                        | Stmt::SwitchStmt(_)
                        | Stmt::TypeSwitchStmt(_)
                        | Stmt::SelectStmt(_)
                ) {
                    pending.push((stmt.pos().0 as u32, msg_invalid("assign")));
                }
            }
            Stmt::IncDecStmt(_) if check_enabled(options, WslV5Check::IncDec) => {
                if is_assign_or_inc(prev) {
                    continue;
                }
                pending.push((stmt.pos().0 as u32, msg_invalid("inc-dec")));
            }
            Stmt::DeclStmt(_) if check_enabled(options, WslV5Check::Decl) => {
                // Simplified: never cuddle decl (grouping DEFERRED).
                pending.push((stmt.pos().0 as u32, msg_never("decl")));
            }
            Stmt::ExprStmt(_) if check_enabled(options, WslV5Check::Expr) => {
                if matches!(prev, Stmt::ExprStmt(_)) {
                    continue;
                }
                check_cuddle_blockish(fset, stmts, i, "expr", false, options, pending);
            }
            Stmt::DeferStmt(_) if check_enabled(options, WslV5Check::Defer) => {
                if matches!(prev, Stmt::DeferStmt(_)) {
                    continue;
                }
                // Allow: assign / if-err / defer chain with shared var.
                if matches!(prev, Stmt::IfStmt(_)) {
                    let n = n_cuddled_before(fset, stmts, i);
                    if n >= 2 {
                        let before_if = &stmts[i - 2];
                        let target = find_rhs(stmt);
                        if lists_overlap(&find_lhs(before_if), &target)
                            || lists_overlap(&find_rhs(before_if), &target)
                        {
                            continue;
                        }
                    }
                }
                check_cuddle_blockish(fset, stmts, i, "defer", true, options, pending);
            }
            Stmt::GoStmt(_) if check_enabled(options, WslV5Check::Go) => {
                if matches!(prev, Stmt::GoStmt(_)) {
                    continue;
                }
                check_cuddle_blockish(fset, stmts, i, "go", true, options, pending);
            }
            Stmt::RangeStmt(_) if check_enabled(options, WslV5Check::Range) => {
                check_cuddle_blockish(fset, stmts, i, "range", true, options, pending);
            }
            Stmt::ForStmt(_) if check_enabled(options, WslV5Check::For) => {
                check_cuddle_blockish(fset, stmts, i, "for", true, options, pending);
            }
            Stmt::SwitchStmt(_) if check_enabled(options, WslV5Check::Switch) => {
                check_cuddle_blockish(fset, stmts, i, "switch", true, options, pending);
            }
            Stmt::TypeSwitchStmt(_) if check_enabled(options, WslV5Check::TypeSwitch) => {
                check_cuddle_blockish(fset, stmts, i, "type-switch", true, options, pending);
            }
            Stmt::SelectStmt(_) if check_enabled(options, WslV5Check::Select) => {
                check_cuddle_blockish(fset, stmts, i, "select", true, options, pending);
            }
            Stmt::SendStmt(_) if check_enabled(options, WslV5Check::Send) => {
                check_cuddle_blockish(fset, stmts, i, "send", true, options, pending);
            }
            Stmt::LabeledStmt(_) if check_enabled(options, WslV5Check::Label) => {
                pending.push((stmt.pos().0 as u32, msg_never("label")));
            }
            _ => {}
        }
    }
}

fn walk_else(
    fset: &FileSet,
    skip: &HashSet<u32>,
    comment_lines: &[(i64, i64)],
    else_: &Stmt,
    options: &WslV5Options,
    pending: &mut Vec<(u32, String)>,
) {
    match else_ {
        Stmt::BlockStmt(b) => check_block(fset, skip, comment_lines, b, options, pending),
        Stmt::IfStmt(e) => {
            check_block(fset, skip, comment_lines, &e.body, options, pending);
            if let Some(next) = &e.else_ {
                walk_else(fset, skip, comment_lines, next, options, pending);
            }
        }
        _ => {}
    }
}

fn check_block(
    fset: &FileSet,
    skip: &HashSet<u32>,
    comment_lines: &[(i64, i64)],
    block: &BlockStmt,
    options: &WslV5Options,
    pending: &mut Vec<(u32, String)>,
) {
    check_leading_trailing(fset, comment_lines, block, options, pending);
    check_statements(fset, skip, comment_lines, &block.list, options, pending);
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "wsl_v5 requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<WslV5Options>("wsl_v5")
        .cloned()
        .unwrap_or_default();

    let mut pending = Vec::new();
    let fset = pass.fset().clone();
    let skip = ident_skip_set(pass);
    let want_comments = check_enabled(&options, WslV5Check::LeadingWhitespace);
    let paths = pass.pkg().compiled_go_files.clone();
    for (i, file) in pass.files().iter().enumerate() {
        // `leading-whitespace` has to know where the comments are, and the
        // production typecheck parses without them, so the file is re-parsed
        // comments-only — but only when that check is on.
        let comment_lines: Vec<(i64, i64)> = if want_comments {
            if file.comments.is_empty() {
                paths
                    .get(i)
                    .and_then(|p| reparse_comment_lines(p, pass.pkg().source_bytes(i)))
                    .unwrap_or_default()
            } else {
                file.comments
                    .iter()
                    .map(|cg| (line(&fset, cg.pos()), line(&fset, cg.end())))
                    .collect()
            }
        } else {
            Vec::new()
        };
        for decl in &file.decls {
            if let Decl::FuncDecl(f) = decl {
                if let Some(body) = &f.body {
                    check_block(&fset, &skip, &comment_lines, body, &options, &mut pending);
                }
            }
        }
        // Also walk package-level FuncLit assigned in vars? Upstream walks
        // FuncLit via Inspect; we cover nested FuncLits inside function bodies.
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "wsl_v5",
        doc: "add or remove empty lines",
        url: "https://github.com/bombsimon/wsl",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_checks_include_if_and_err() {
        let o = WslV5Options::default();
        assert!(o.checks.contains(&WslV5Check::If));
        assert!(o.checks.contains(&WslV5Check::Err));
        assert!(!o.checks.contains(&WslV5Check::AfterBlock));
        assert!(o.allow_first_in_block);
        assert!(!o.allow_whole_block);
        assert_eq!(o.branch_max_lines, 2);
        assert_eq!(o.cuddle_max_statements, 1);
    }

    #[test]
    fn analyzer_name() {
        assert_eq!(analyzer().name, "wsl_v5");
    }
}
