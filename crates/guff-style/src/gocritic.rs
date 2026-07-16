//! Port of [`github.com/go-critic/go-critic`](https://github.com/go-critic/go-critic)
//! (golangci-lint wrapper: `linters.settings.gocritic`).
//!
//! Implemented default/stable checkers:
//! - `appendAssign`, `assignOp`, `badCall`, `captLocal`, `defaultCaseOrder`,
//!   `dupArg`, `dupCase`, `elseif`, `exitAfterDefer`, `flagDeref`,
//!   `ifElseChain`, `newDeref`, `singleCaseSwitch`, `sloppyLen`, `switchTrue`,
//!   `underef`, `unslice`, `valSwap`
//!
//! Settings: `enable-all` / `disable-all` / `enabled-checks` / `disabled-checks`
//! (prometheus-style `enable-all` + `disabled-checks` works).
//!
//! DEFERRED: remaining default checks (argOrder, badCond, caseOrder,
//! codegenComment, commentFormatting, deprecatedComment, dupBranchBody,
//! dupSubExpr, flagName, mapKey, offBy1, regexpMust, sloppyTypeAssert,
//! typeSwitchVar, unlambda, wrapperFunc, …), enable-all extras, per-check
//! `settings` params, SuggestedFix.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, BinaryExpr, BlockStmt, CallExpr, Expr, FieldList, FuncDecl, IfStmt, SliceExpr,
    StarExpr, Stmt, SwitchStmt, TypeSwitchStmt,
};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::GocriticOptions;

/// Checks enabled by default when neither `enable-all` nor `disable-all` is set
/// (golangci-lint stable list ∩ implemented).
const DEFAULT_CHECKS: &[&str] = &[
    "appendAssign",
    "assignOp",
    "badCall",
    "captLocal",
    "defaultCaseOrder",
    "dupArg",
    "dupCase",
    "elseif",
    "exitAfterDefer",
    "flagDeref",
    "ifElseChain",
    "newDeref",
    "singleCaseSwitch",
    "sloppyLen",
    "switchTrue",
    "underef",
    "unslice",
    "valSwap",
];

/// All checkers this port implements (used for `enable-all`).
const IMPLEMENTED_CHECKS: &[&str] = DEFAULT_CHECKS;

fn enabled_set(opts: &GocriticOptions) -> HashSet<String> {
    let mut set: HashSet<String> = if opts.enable_all {
        IMPLEMENTED_CHECKS.iter().map(|s| (*s).to_string()).collect()
    } else if opts.disable_all {
        HashSet::new()
    } else {
        DEFAULT_CHECKS.iter().map(|s| (*s).to_string()).collect()
    };
    for name in &opts.enabled_checks {
        set.insert(name.clone());
    }
    for name in &opts.disabled_checks {
        set.remove(name);
    }
    // Only keep implemented names (unknown / deferred names are ignored).
    set.retain(|n| IMPLEMENTED_CHECKS.contains(&n.as_str()));
    set
}

fn enabled(set: &HashSet<String>, name: &str) -> bool {
    set.contains(name)
}

fn expr_text(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::BasicLit(lit) => Some(lit.value.clone()),
        Expr::SelectorExpr(sel) => {
            let x = expr_text(&sel.x)?;
            Some(format!("{x}.{}", sel.sel.name))
        }
        Expr::StarExpr(s) => {
            let x = expr_text(&s.x)?;
            Some(format!("*{x}"))
        }
        Expr::ParenExpr(p) => expr_text(&p.x).map(|inner| format!("({inner})")),
        Expr::IndexExpr(ix) => {
            let x = expr_text(&ix.x)?;
            let index = expr_text(&ix.index)?;
            Some(format!("{x}[{index}]"))
        }
        Expr::CallExpr(call) => {
            let fun = expr_text(&call.fun)?;
            let args: Option<Vec<_>> = call.args.iter().map(expr_text).collect();
            let args = args?;
            Some(format!("{fun}({})", args.join(", ")))
        }
        Expr::UnaryExpr(u) if u.op == Token::NOT => {
            let x = expr_text(&u.x)?;
            Some(format!("!{x}"))
        }
        _ => None,
    }
}

fn exprs_equal(a: &Expr, b: &Expr) -> bool {
    match (expr_text(a), expr_text(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

fn is_true_lit(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(id) => id.name == "true",
        Expr::BasicLit(lit) => lit.value == "true",
        _ => false,
    }
}

fn is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

fn report(pending: &mut Vec<(u32, String)>, pos: u32, msg: impl Into<String>) {
    pending.push((pos, msg.into()));
}

fn check_elseif(stmt: &IfStmt, pending: &mut Vec<(u32, String)>) {
    let Some(Stmt::BlockStmt(else_body)) = stmt.else_.as_deref() else {
        return;
    };
    if else_body.list.len() != 1 {
        return;
    }
    let Stmt::IfStmt(inner) = &else_body.list[0] else {
        return;
    };
    // skipBalanced=true (golangci default): skip if then-body is a single if.
    if stmt.body.list.len() == 1 && matches!(stmt.body.list[0], Stmt::IfStmt(_)) {
        return;
    }
    if inner.else_.is_some() || inner.init.is_some() {
        return;
    }
    report(
        pending,
        else_body.lbrace.0 as u32,
        "can replace 'else {if cond {}}' with 'else if cond {}'",
    );
}

fn case_has_break(body: &[Stmt]) -> bool {
    fn walk(stmts: &[Stmt], nested: bool) -> bool {
        for s in stmts {
            match s {
                Stmt::BranchStmt(b) if b.tok == Token::BREAK && !nested => return true,
                Stmt::BlockStmt(b) => {
                    if walk(&b.list, nested) {
                        return true;
                    }
                }
                Stmt::IfStmt(i) => {
                    if walk(&i.body.list, nested) {
                        return true;
                    }
                    if let Some(e) = &i.else_ {
                        if walk(std::slice::from_ref(e.as_ref()), nested) {
                            return true;
                        }
                    }
                }
                Stmt::ForStmt(_) | Stmt::RangeStmt(_) | Stmt::SelectStmt(_) | Stmt::SwitchStmt(_)
                | Stmt::TypeSwitchStmt(_) => {
                    // Nested loops/switches own their breaks.
                }
                Stmt::CaseClause(cc) => {
                    if walk(&cc.body, nested) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }
    walk(body, false)
}

fn check_single_case_switch_body(pos: u32, body: &BlockStmt, pending: &mut Vec<(u32, String)>) {
    if body.list.len() != 1 {
        return;
    }
    let Stmt::CaseClause(cc) = &body.list[0] else {
        return;
    };
    if case_has_break(&cc.body) {
        return;
    }
    if cc.list.is_empty() {
        report(pending, pos, "found switch with default case only");
    } else if cc.list.len() == 1 {
        report(
            pending,
            pos,
            "should rewrite switch statement to if statement",
        );
    }
}

fn check_single_case_switch(stmt: &SwitchStmt, pending: &mut Vec<(u32, String)>) {
    check_single_case_switch_body(stmt.switch.0 as u32, &stmt.body, pending);
}

fn check_single_case_type_switch(stmt: &TypeSwitchStmt, pending: &mut Vec<(u32, String)>) {
    check_single_case_switch_body(stmt.switch.0 as u32, &stmt.body, pending);
}

fn check_default_case_order(stmt: &SwitchStmt, pending: &mut Vec<(u32, String)>) {
    let n = stmt.body.list.len();
    for (i, s) in stmt.body.list.iter().enumerate() {
        let Stmt::CaseClause(cc) = s else {
            continue;
        };
        if cc.list.is_empty() && i != 0 && i + 1 != n {
            report(
                pending,
                cc.case.0 as u32,
                "consider to make `default` case as first or as last case",
            );
        }
    }
}

fn check_switch_true(stmt: &SwitchStmt, pending: &mut Vec<(u32, String)>) {
    let Some(tag) = &stmt.tag else {
        return;
    };
    if !is_true_lit(tag) {
        return;
    }
    if stmt.init.is_some() {
        report(
            pending,
            stmt.switch.0 as u32,
            "replace 'switch $x; true {}' with 'switch $x; {}'",
        );
    } else {
        report(
            pending,
            stmt.switch.0 as u32,
            "replace 'switch true {}' with 'switch {}'",
        );
    }
}

fn check_sloppy_len(bin: &BinaryExpr, pending: &mut Vec<(u32, String)>) {
    let Expr::CallExpr(call) = bin.x.as_ref() else {
        return;
    };
    let fun_name = match call.fun.as_ref() {
        Expr::Ident(id) => id.name.as_str(),
        _ => return,
    };
    if fun_name != "len" || call.args.len() != 1 {
        return;
    }
    let pos = bin.op_pos.0 as u32;
    match bin.op {
        Token::GEQ if is_int_lit(&bin.y, 0) => {
            report(pending, pos, "len(_) >= 0 is always true");
        }
        Token::LSS if is_int_lit(&bin.y, 0) => {
            report(pending, pos, "len(_) < 0 is always false");
        }
        Token::LEQ if is_int_lit(&bin.y, 0) => {
            if let Some(arg) = expr_text(&call.args[0]) {
                report(
                    pending,
                    pos,
                    format!("len({arg}) <= 0 can be len({arg}) == 0"),
                );
            } else {
                report(pending, pos, "len(_) <= 0 can be len(_) == 0");
            }
        }
        _ => {}
    }
}

fn is_int_lit(expr: &Expr, want: i64) -> bool {
    match expr {
        Expr::BasicLit(lit) => lit.value.parse::<i64>().ok() == Some(want),
        Expr::UnaryExpr(u) if u.op == Token::SUB => {
            if let Expr::BasicLit(lit) = u.x.as_ref() {
                lit.value.parse::<i64>().ok().map(|v| -v) == Some(want)
            } else {
                false
            }
        }
        _ => false,
    }
}

fn check_unslice(slice: &SliceExpr, pending: &mut Vec<(u32, String)>) {
    if slice.low.is_some() || slice.high.is_some() || slice.max.is_some() || slice.slice3 {
        return;
    }
    let Some(x) = expr_text(&slice.x) else {
        return;
    };
    report(
        pending,
        slice.lbrack.0 as u32,
        format!("could simplify {x}[:] to {x}"),
    );
}

fn check_new_deref(star: &StarExpr, pending: &mut Vec<(u32, String)>) {
    let Expr::CallExpr(call) = star.x.as_ref() else {
        return;
    };
    let Expr::Ident(fun) = call.fun.as_ref() else {
        return;
    };
    if fun.name != "new" || call.args.len() != 1 {
        return;
    }
    let Some(arg) = expr_text(&call.args[0]) else {
        return;
    };
    let suggestion = match arg.as_str() {
        "bool" => "false".to_string(),
        "string" => "\"\"".to_string(),
        "int" | "int8" | "int16" | "int32" | "int64" | "uint" | "uint8" | "uint16" | "uint32"
        | "uint64" | "uintptr" | "byte" | "rune" | "float32" | "float64" | "complex64"
        | "complex128" => "0".to_string(),
        other => format!("{other}{{}}"),
    };
    report(
        pending,
        star.star.0 as u32,
        format!("replace `*new({arg})` with `{suggestion}`"),
    );
}

fn check_append_assign(assign: &AssignStmt, pending: &mut Vec<(u32, String)>) {
    if assign.tok != Some(Token::ASSIGN) && assign.tok != Some(Token::DEFINE) {
        return;
    }
    if assign.lhs.len() != assign.rhs.len() {
        return;
    }
    for (lhs, rhs) in assign.lhs.iter().zip(assign.rhs.iter()) {
        let Expr::CallExpr(call) = rhs else {
            continue;
        };
        let is_append = match call.fun.as_ref() {
            Expr::Ident(id) => id.name == "append",
            _ => false,
        };
        if !is_append || call.args.is_empty() {
            continue;
        }
        if let Expr::Ident(id) = lhs {
            if id.name == "_" {
                continue;
            }
        }
        // xs = append(ys, xs...) idiom
        if call.ellipsis.is_valid() {
            let ok = call.args[1..].iter().any(|arg| {
                let y = match arg {
                    Expr::SliceExpr(s) => s.x.as_ref(),
                    other => other,
                };
                exprs_equal(lhs, y)
            });
            if ok {
                continue;
            }
        }
        if matches!(lhs, Expr::IndexExpr(_)) && !matches!(&call.args[0], Expr::IndexExpr(_)) {
            continue;
        }
        let first = match &call.args[0] {
            Expr::SliceExpr(s) => s.x.as_ref(),
            other => other,
        };
        if !exprs_equal(lhs, first) {
            report(
                pending,
                call.fun.pos().0 as u32,
                "append result not assigned to the same slice",
            );
        }
    }
}

fn check_dup_case_switch(stmt: &SwitchStmt, pending: &mut Vec<(u32, String)>) {
    let mut seen = HashSet::new();
    for s in &stmt.body.list {
        let Stmt::CaseClause(cc) = s else {
            continue;
        };
        for x in &cc.list {
            let Some(text) = expr_text(x) else {
                continue;
            };
            if !seen.insert(text.clone()) {
                report(pending, x.pos().0 as u32, format!("'case {text}' is duplicated"));
            }
        }
    }
}

fn check_capt_local_fields(fields: &Option<FieldList>, pending: &mut Vec<(u32, String)>) {
    let Some(fl) = fields else {
        return;
    };
    for field in &fl.list {
        for name in &field.names {
            if is_exported(&name.name) {
                report(
                    pending,
                    name.pos().0 as u32,
                    format!("`{}' should not be capitalized", name.name),
                );
            }
        }
    }
}

fn check_capt_local(func: &FuncDecl, pending: &mut Vec<(u32, String)>) {
    // paramsOnly=true (golangci default)
    check_capt_local_fields(&func.ty.params, pending);
    check_capt_local_fields(&func.ty.results, pending);
}

fn call_qualified_name(call: &CallExpr) -> Option<String> {
    expr_text(&call.fun)
}

fn check_exit_after_defer(func: &FuncDecl, pending: &mut Vec<(u32, String)>) {
    let Some(body) = &func.body else {
        return;
    };
    let mut defer_pos: Option<(u32, String)> = None;
    let mut found = false;

    fn walk(
        stmts: &[Stmt],
        defer_pos: &mut Option<(u32, String)>,
        found: &mut bool,
        pending: &mut Vec<(u32, String)>,
        in_else: bool,
    ) {
        if *found {
            return;
        }
        for s in stmts {
            match s {
                Stmt::DeferStmt(d) => {
                    let label = call_qualified_name(&d.call)
                        .map(|n| format!("defer {n}(...)"))
                        .unwrap_or_else(|| "defer ...".into());
                    *defer_pos = Some((d.defer_.0 as u32, label));
                }
                Stmt::ExprStmt(e) => {
                    if let Expr::CallExpr(call) = &e.x {
                        check_exit_call(call, defer_pos, found, pending);
                    }
                }
                Stmt::IfStmt(i) => {
                    walk(&i.body.list, defer_pos, found, pending, false);
                    if !*found {
                        if let Some(e) = &i.else_ {
                            // Don't treat else-branch exits as after defer when
                            // defer was only seen on the if path (upstream skips Else).
                            if defer_pos.is_some() && !in_else {
                                // Still check else if defer already recorded before if.
                            }
                            match e.as_ref() {
                                Stmt::BlockStmt(b) => {
                                    walk(&b.list, defer_pos, found, pending, true)
                                }
                                Stmt::IfStmt(_) => {
                                    walk(std::slice::from_ref(e.as_ref()), defer_pos, found, pending, true)
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Stmt::BlockStmt(b) => walk(&b.list, defer_pos, found, pending, in_else),
                Stmt::ForStmt(f) => walk(&f.body.list, defer_pos, found, pending, false),
                Stmt::RangeStmt(r) => walk(&r.body.list, defer_pos, found, pending, false),
                Stmt::SwitchStmt(sw) => {
                    for c in &sw.body.list {
                        if let Stmt::CaseClause(cc) = c {
                            walk(&cc.body, defer_pos, found, pending, false);
                        }
                    }
                }
                Stmt::AssignStmt(a) => {
                    for rhs in &a.rhs {
                        if let Expr::CallExpr(call) = rhs {
                            check_exit_call(call, defer_pos, found, pending);
                        }
                    }
                }
                Stmt::GoStmt(_) => {
                    // Don't recurse into goroutines.
                }
                _ => {}
            }
            if *found {
                return;
            }
        }
    }

    fn check_exit_call(
        call: &CallExpr,
        defer_pos: &mut Option<(u32, String)>,
        found: &mut bool,
        pending: &mut Vec<(u32, String)>,
    ) {
        let Some(name) = call_qualified_name(call) else {
            return;
        };
        let is_exit = matches!(
            name.as_str(),
            "os.Exit" | "log.Fatal" | "log.Fatalf" | "log.Fatalln"
        );
        if !is_exit {
            return;
        }
        if let Some((_, defer_label)) = defer_pos {
            report(
                pending,
                call.fun.pos().0 as u32,
                format!("{name} will exit, and `{defer_label}` will not run"),
            );
            *found = true;
        }
    }

    walk(&body.list, &mut defer_pos, &mut found, pending, false);
}

fn count_if_else_len(stmt: &IfStmt) -> i32 {
    if stmt.init.is_some() {
        return 0;
    }
    let mut count = 0;
    let mut cur = stmt;
    loop {
        match cur.else_.as_deref() {
            Some(Stmt::IfStmt(next)) => {
                if next.init.is_some() {
                    return 0;
                }
                count += 1;
                cur = next;
            }
            Some(Stmt::BlockStmt(_)) => return count + 1,
            None => return count,
            _ => return 0,
        }
    }
}

fn check_if_else_chain(
    stmt: &IfStmt,
    visited: &mut HashSet<u32>,
    pending: &mut Vec<(u32, String)>,
) {
    if !visited.insert(stmt.id) && stmt.id != 0 {
        return;
    }
    // Mark nested else-ifs visited.
    let mut cur = stmt;
    while let Some(Stmt::IfStmt(next)) = cur.else_.as_deref() {
        if next.id != 0 {
            visited.insert(next.id);
        }
        cur = next;
    }
    // minThreshold default = 2
    if count_if_else_len(stmt) >= 2 {
        report(
            pending,
            stmt.if_.0 as u32,
            "rewrite if-else to switch statement",
        );
    }
}

fn check_val_swap(stmts: &[Stmt], pending: &mut Vec<(u32, String)>) {
    // tmp := y; y = x; x = tmp
    for window in stmts.windows(3) {
        let (Stmt::AssignStmt(a), Stmt::AssignStmt(b), Stmt::AssignStmt(c)) =
            (&window[0], &window[1], &window[2])
        else {
            continue;
        };
        if a.tok != Some(Token::DEFINE)
            || b.tok != Some(Token::ASSIGN)
            || c.tok != Some(Token::ASSIGN)
        {
            continue;
        }
        if a.lhs.len() != 1 || a.rhs.len() != 1 || b.lhs.len() != 1 || b.rhs.len() != 1
            || c.lhs.len() != 1 || c.rhs.len() != 1
        {
            continue;
        }
        let tmp = &a.lhs[0];
        let y = &a.rhs[0];
        if !exprs_equal(&b.lhs[0], y) {
            continue;
        }
        let x = &b.rhs[0];
        if !exprs_equal(&c.lhs[0], x) || !exprs_equal(&c.rhs[0], tmp) {
            continue;
        }
        let Some(x_t) = expr_text(x) else {
            continue;
        };
        let Some(y_t) = expr_text(y) else {
            continue;
        };
        report(
            pending,
            a.tok_pos.0 as u32,
            format!("can re-write as `{y_t}, {x_t} = {x_t}, {y_t}`"),
        );
    }
}

fn check_flag_deref(star: &StarExpr, pending: &mut Vec<(u32, String)>) {
    let Expr::CallExpr(call) = star.x.as_ref() else {
        return;
    };
    let Some(name) = call_qualified_name(call) else {
        return;
    };
    let suggest = match name.as_str() {
        "flag.Bool" => "flag.BoolVar",
        "flag.Duration" => "flag.DurationVar",
        "flag.Float64" => "flag.Float64Var",
        "flag.Int" => "flag.IntVar",
        "flag.Int64" => "flag.Int64Var",
        "flag.String" => "flag.StringVar",
        "flag.Uint" => "flag.UintVar",
        "flag.Uint64" => "flag.Uint64Var",
        _ => return,
    };
    report(
        pending,
        star.star.0 as u32,
        format!("immediate deref in *{name}(...) is most likely an error; consider using {suggest}"),
    );
}

fn check_bad_call(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    match name.as_str() {
        "append" if call.args.len() == 1 && !call.ellipsis.is_valid() => {
            report(
                pending,
                call.fun.pos().0 as u32,
                "no-op append call, probably missing arguments",
            );
        }
        n if (n == "filepath.Join" || n == "path/filepath.Join" || n.ends_with("/filepath.Join"))
            && call.args.len() == 1 =>
        {
            report(
                pending,
                call.fun.pos().0 as u32,
                "suspicious Join on 1 argument",
            );
        }
        "strings.Replace" | "bytes.Replace" | "strings.SplitN" | "bytes.SplitN"
            if call.args.len() >= 4 || (name.ends_with("SplitN") && call.args.len() >= 3) =>
        {
            let idx = if name.ends_with("SplitN") { 2 } else { 3 };
            if let Some(arg) = call.args.get(idx) {
                if code::is_integer_literal(pass, arg, 0) || is_int_lit(arg, 0) {
                    report(
                        pending,
                        arg.pos().0 as u32,
                        "suspicious arg 0, probably meant -1",
                    );
                }
            }
        }
        _ => {}
    }
}

fn check_assign_op(assign: &AssignStmt, pending: &mut Vec<(u32, String)>) {
    if assign.tok != Some(Token::ASSIGN) || assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return;
    }
    let lhs = &assign.lhs[0];
    let Expr::BinaryExpr(bin) = &assign.rhs[0] else {
        return;
    };
    if !exprs_equal(lhs, &bin.x) {
        return;
    }
    // Only simple lhs (ident / selector / index) — treat as "pure" enough.
    if !matches!(
        lhs,
        Expr::Ident(_) | Expr::SelectorExpr(_) | Expr::IndexExpr(_) | Expr::StarExpr(_)
    ) {
        return;
    }
    let Some(x_t) = expr_text(lhs) else {
        return;
    };
    let Some(y_t) = expr_text(&bin.y) else {
        return;
    };
    let msg = match bin.op {
        Token::ADD if y_t == "1" => format!("replace `{x_t} = {x_t} + 1` with `{x_t}++`"),
        Token::SUB if y_t == "1" => format!("replace `{x_t} = {x_t} - 1` with `{x_t}--`"),
        Token::ADD => format!("replace `{x_t} = {x_t} + {y_t}` with `{x_t} += {y_t}`"),
        Token::SUB => format!("replace `{x_t} = {x_t} - {y_t}` with `{x_t} -= {y_t}`"),
        Token::MUL => format!("replace `{x_t} = {x_t} * {y_t}` with `{x_t} *= {y_t}`"),
        Token::QUO => format!("replace `{x_t} = {x_t} / {y_t}` with `{x_t} /= {y_t}`"),
        Token::REM => format!("replace `{x_t} = {x_t} % {y_t}` with `{x_t} %= {y_t}`"),
        Token::AND => format!("replace `{x_t} = {x_t} & {y_t}` with `{x_t} &= {y_t}`"),
        Token::OR => format!("replace `{x_t} = {x_t} | {y_t}` with `{x_t} |= {y_t}`"),
        Token::XOR => format!("replace `{x_t} = {x_t} ^ {y_t}` with `{x_t} ^= {y_t}`"),
        Token::SHL => format!("replace `{x_t} = {x_t} << {y_t}` with `{x_t} <<= {y_t}`"),
        Token::SHR => format!("replace `{x_t} = {x_t} >> {y_t}` with `{x_t} >>= {y_t}`"),
        Token::AndNot => format!("replace `{x_t} = {x_t} &^ {y_t}` with `{x_t} &^= {y_t}`"),
        _ => return,
    };
    report(pending, assign.tok_pos.0 as u32, msg);
}

fn check_dup_arg(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    if call.args.len() < 2 {
        return;
    }
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    let watch = matches!(
        name.as_str(),
        "copy"
            | "cmp.Compare"
            | "maps.Equal"
            | "math.Dim"
            | "math.Max"
            | "math.Min"
            | "reflect.Copy"
            | "reflect.DeepEqual"
            | "slices.Compare"
            | "slices.Equal"
            | "strings.Contains"
            | "strings.Compare"
            | "strings.EqualFold"
            | "strings.HasPrefix"
            | "strings.HasSuffix"
            | "strings.Index"
            | "bytes.Contains"
            | "bytes.Compare"
            | "bytes.Equal"
            | "bytes.EqualFold"
            | "bytes.HasPrefix"
            | "bytes.HasSuffix"
            | "bytes.Index"
    );
    if !watch {
        return;
    }
    // Most of these take (a, b) as first two args.
    if exprs_equal(&call.args[0], &call.args[1]) {
        report(
            pending,
            call.args[1].pos().0 as u32,
            "suspicious duplicated args in call",
        );
    }
}

fn walk_block_for_val_swap(body: &BlockStmt, pending: &mut Vec<(u32, String)>) {
    check_val_swap(&body.list, pending);
    for s in &body.list {
        match s {
            Stmt::BlockStmt(b) => walk_block_for_val_swap(b, pending),
            Stmt::IfStmt(i) => {
                walk_block_for_val_swap(&i.body, pending);
                if let Some(e) = &i.else_ {
                    match e.as_ref() {
                        Stmt::BlockStmt(b) => walk_block_for_val_swap(b, pending),
                        Stmt::IfStmt(inner) => walk_block_for_val_swap(&inner.body, pending),
                        _ => {}
                    }
                }
            }
            Stmt::ForStmt(f) => walk_block_for_val_swap(&f.body, pending),
            Stmt::RangeStmt(r) => walk_block_for_val_swap(&r.body, pending),
            Stmt::SwitchStmt(sw) => {
                for c in &sw.body.list {
                    if let Stmt::CaseClause(cc) = c {
                        check_val_swap(&cc.body, pending);
                    }
                }
            }
            _ => {}
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "gocritic requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<GocriticOptions>("gocritic")
        .cloned()
        .unwrap_or_default();
    let set = enabled_set(&options);

    let mut pending = Vec::new();
    let mut if_else_visited = HashSet::new();
    // Track pointer identity for if-else via id; also use a map for id==0 cases.
    let mut if_else_ptr: HashMap<usize, ()> = HashMap::new();

    for file in pass.files() {
        if enabled(&set, "valSwap") {
            for decl in &file.decls {
                if let guff::ast::Decl::FuncDecl(f) = decl {
                    if let Some(body) = &f.body {
                        walk_block_for_val_swap(body, &mut pending);
                    }
                }
            }
        }

        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::IfStmt(s) => {
                    if enabled(&set, "elseif") {
                        check_elseif(s, &mut pending);
                    }
                    if enabled(&set, "ifElseChain") {
                        let key = s as *const _ as usize;
                        if if_else_ptr.insert(key, ()).is_none() {
                            check_if_else_chain(s, &mut if_else_visited, &mut pending);
                        }
                    }
                }
                NodeRef::SwitchStmt(s) => {
                    if enabled(&set, "singleCaseSwitch") {
                        check_single_case_switch(s, &mut pending);
                    }
                    if enabled(&set, "defaultCaseOrder") {
                        check_default_case_order(s, &mut pending);
                    }
                    if enabled(&set, "switchTrue") {
                        check_switch_true(s, &mut pending);
                    }
                    if enabled(&set, "dupCase") {
                        check_dup_case_switch(s, &mut pending);
                    }
                }
                NodeRef::TypeSwitchStmt(s) if enabled(&set, "singleCaseSwitch") => {
                    check_single_case_type_switch(s, &mut pending);
                }
                NodeRef::BinaryExpr(b) if enabled(&set, "sloppyLen") => {
                    check_sloppy_len(b, &mut pending);
                }
                NodeRef::SliceExpr(s) if enabled(&set, "unslice") => {
                    check_unslice(s, &mut pending);
                }
                NodeRef::StarExpr(s) => {
                    if enabled(&set, "newDeref") {
                        check_new_deref(s, &mut pending);
                    }
                    if enabled(&set, "flagDeref") {
                        check_flag_deref(s, &mut pending);
                    }
                }
                NodeRef::AssignStmt(a) => {
                    if enabled(&set, "appendAssign") {
                        check_append_assign(a, &mut pending);
                    }
                    if enabled(&set, "assignOp") {
                        check_assign_op(a, &mut pending);
                    }
                }
                NodeRef::FuncDecl(f) => {
                    if enabled(&set, "captLocal") {
                        check_capt_local(f, &mut pending);
                    }
                    if enabled(&set, "exitAfterDefer") {
                        check_exit_after_defer(f, &mut pending);
                    }
                }
                NodeRef::CallExpr(c) => {
                    if enabled(&set, "badCall") {
                        check_bad_call(pass, c, &mut pending);
                    }
                    if enabled(&set, "dupArg") {
                        check_dup_arg(pass, c, &mut pending);
                    }
                }
                NodeRef::SelectorExpr(sel) if enabled(&set, "underef") => {
                    if let Expr::ParenExpr(paren) = sel.x.as_ref() {
                        if let Expr::StarExpr(star) = paren.x.as_ref() {
                            if let Some(inner) = expr_text(&star.x) {
                                report(
                                    &mut pending,
                                    sel.sel.pos().0 as u32,
                                    format!(
                                        "could simplify (*{inner}).{} to {inner}.{}",
                                        sel.sel.name, sel.sel.name
                                    ),
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
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
        name: "gocritic",
        doc: "Provides diagnostics that check for bugs, performance and style issues.",
        url: "https://github.com/go-critic/go-critic",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
