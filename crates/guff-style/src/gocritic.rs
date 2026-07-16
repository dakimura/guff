//! Port of [`github.com/go-critic/go-critic`](https://github.com/go-critic/go-critic)
//! (golangci-lint wrapper: `linters.settings.gocritic`).
//!
//! Implemented checkers (**50** = 34 default + 16 enable-all extras):
//! - original 18: `appendAssign`, `assignOp`, `badCall`, `captLocal`,
//!   `defaultCaseOrder`, `dupArg`, `dupCase`, `elseif`, `exitAfterDefer`,
//!   `flagDeref`, `ifElseChain`, `newDeref`, `singleCaseSwitch`, `sloppyLen`,
//!   `switchTrue`, `underef`, `unslice`, `valSwap`
//! - batch 2: `argOrder`, `badCond`, `dupBranchBody`, `dupSubExpr`, `flagName`,
//!   `mapKey`, `offBy1`, `regexpMust`, `typeSwitchVar`, `unlambda`, `wrapperFunc`
//! - batch 3: `caseOrder`, `codegenComment`, `commentFormatting`,
//!   `deprecatedComment`, `sloppyTypeAssert`
//! - batch 4 (enable-all extras): `deferUnlambda`, `emptyDecl`, `emptyFallthrough`,
//!   `emptyStringTest`, `initClause`, `nilValReturn`, `octalLiteral`, `yodaStyleExpr`
//! - batch 5 (enable-all extras): `builtinShadow`, `builtinShadowDecl`,
//!   `commentedOutImport`, `dupImport`, `filepathJoin`, `paramTypeCombine`,
//!   `rangeAppendAll`, `weakCond`
//!
//! Settings: `enable-all` / `disable-all` / `enabled-checks` / `disabled-checks`
//! (prometheus-style `enable-all` + `disabled-checks` works).
//!
//! DEFERRED: remaining enable-all extras (badRegexp / dupOption / methodExprCall /
//! rangeExprCopy / regexpPattern / ruleguard / sortSlice / sqlQuery / typeAssertChain),
//! per-check `settings` params, SuggestedFix, caseOrder expression-switch
//! overlap, wrapperFunc/unlambda/typeSwitchVar full type-aware parity.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use guff::ast::{
    AssignStmt, BasicLit, BinaryExpr, BlockStmt, CallExpr, CommentGroup, CompositeLit, Decl,
    DeferStmt, Expr, Field, FieldList, File, FuncDecl, FuncLit, FuncType, Ident, IfStmt, IndexExpr,
    RangeStmt, SliceExpr, Spec, StarExpr, Stmt, SwitchStmt, TypeAssertExpr, TypeSwitchStmt,
    ValueSpec,
};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::{FileSet, Pos};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::api_predicates::{api_identical, api_implements};
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::BasicKind;
use guff_types::predicates::is_interface;
use guff_types::TypeId;
use regex::Regex;

use crate::options::GocriticOptions;

/// Checks enabled by default when neither `enable-all` nor `disable-all` is set
/// (golangci-lint stable list ∩ implemented).
const DEFAULT_CHECKS: &[&str] = &[
    "appendAssign",
    "argOrder",
    "assignOp",
    "badCall",
    "badCond",
    "captLocal",
    "caseOrder",
    "codegenComment",
    "commentFormatting",
    "defaultCaseOrder",
    "deprecatedComment",
    "dupArg",
    "dupBranchBody",
    "dupCase",
    "dupSubExpr",
    "elseif",
    "exitAfterDefer",
    "flagDeref",
    "flagName",
    "ifElseChain",
    "mapKey",
    "newDeref",
    "offBy1",
    "regexpMust",
    "singleCaseSwitch",
    "sloppyLen",
    "sloppyTypeAssert",
    "switchTrue",
    "typeSwitchVar",
    "underef",
    "unlambda",
    "unslice",
    "valSwap",
    "wrapperFunc",
];

/// Experimental / opinionated checkers available via `enable-all` or
/// `enabled-checks` (prometheus enable-all coverage).
const ENABLE_ALL_EXTRA_CHECKS: &[&str] = &[
    "builtinShadow",
    "builtinShadowDecl",
    "commentedOutImport",
    "deferUnlambda",
    "dupImport",
    "emptyDecl",
    "emptyFallthrough",
    "emptyStringTest",
    "filepathJoin",
    "initClause",
    "nilValReturn",
    "octalLiteral",
    "paramTypeCombine",
    "rangeAppendAll",
    "weakCond",
    "yodaStyleExpr",
];

/// All checkers this port implements (used for `enable-all`).
fn implemented_checks() -> impl Iterator<Item = &'static str> {
    DEFAULT_CHECKS
        .iter()
        .copied()
        .chain(ENABLE_ALL_EXTRA_CHECKS.iter().copied())
}

fn is_implemented(name: &str) -> bool {
    DEFAULT_CHECKS.contains(&name) || ENABLE_ALL_EXTRA_CHECKS.contains(&name)
}

fn enabled_set(opts: &GocriticOptions) -> HashSet<String> {
    let mut set: HashSet<String> = if opts.enable_all {
        implemented_checks().map(|s| s.to_string()).collect()
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
    set.retain(|n| is_implemented(n));
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
        Expr::BinaryExpr(b) => {
            let x = expr_text(&b.x)?;
            let y = expr_text(&b.y)?;
            Some(format!("{x} {} {y}", b.op.as_str()))
        }
        Expr::TypeAssertExpr(a) => {
            let x = expr_text(&a.x)?;
            match &a.ty {
                Some(t) => {
                    let ty = expr_text(t)?;
                    Some(format!("{x}.({ty})"))
                }
                None => Some(format!("{x}.(type)")),
            }
        }
        Expr::FuncLit(_) => None,
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

fn stmt_text(stmt: &Stmt) -> Option<String> {
    match stmt {
        Stmt::ExprStmt(e) => expr_text(&e.x).map(|x| format!("{x};")),
        Stmt::ReturnStmt(r) => {
            let parts: Option<Vec<_>> = r.results.iter().map(expr_text).collect();
            Some(format!("return {};", parts?.join(", ")))
        }
        Stmt::AssignStmt(a) => {
            let lhs: Option<Vec<_>> = a.lhs.iter().map(expr_text).collect();
            let rhs: Option<Vec<_>> = a.rhs.iter().map(expr_text).collect();
            let op = match a.tok {
                Some(Token::DEFINE) => ":=",
                Some(Token::ASSIGN) | None => "=",
                Some(t) => t.as_str(),
            };
            Some(format!("{} {} {};", lhs?.join(", "), op, rhs?.join(", ")))
        }
        Stmt::IncDecStmt(i) => {
            let x = expr_text(&i.x)?;
            let op = if i.tok == Token::INC { "++" } else { "--" };
            Some(format!("{x}{op};"))
        }
        Stmt::BlockStmt(b) => block_text(b),
        Stmt::IfStmt(i) => {
            let cond = expr_text(&i.cond)?;
            let body = block_text(&i.body)?;
            match &i.else_ {
                Some(e) => {
                    let else_t = stmt_text(e)?;
                    Some(format!("if {cond} {body} else {else_t}"))
                }
                None => Some(format!("if {cond} {body}")),
            }
        }
        Stmt::DeferStmt(d) => call_qualified_name(&d.call)
            .or_else(|| expr_text(&d.call.fun))
            .map(|n| format!("defer {n}(...);")),
        Stmt::GoStmt(g) => call_qualified_name(&g.call)
            .or_else(|| expr_text(&g.call.fun))
            .map(|n| format!("go {n}(...);")),
        Stmt::BranchStmt(b) => Some(format!("{};", b.tok.as_str())),
        Stmt::EmptyStmt(_) => Some(";".into()),
        _ => None,
    }
}

fn block_text(body: &BlockStmt) -> Option<String> {
    let parts: Option<Vec<_>> = body.list.iter().map(stmt_text).collect();
    Some(format!("{{{}}}", parts?.join("")))
}

fn check_dup_branch_body(stmt: &IfStmt, pending: &mut Vec<(u32, String)>) {
    let Some(Stmt::BlockStmt(else_body)) = stmt.else_.as_deref() else {
        return;
    };
    let Some(then_t) = block_text(&stmt.body) else {
        return;
    };
    let Some(else_t) = block_text(else_body) else {
        return;
    };
    if then_t == else_t {
        report(
            pending,
            stmt.if_.0 as u32,
            "both branches in if statement have same body",
        );
    }
}

fn check_dup_sub_expr(bin: &BinaryExpr, pending: &mut Vec<(u32, String)>) {
    let watch = matches!(
        bin.op,
        Token::LOR
            | Token::LAND
            | Token::OR
            | Token::AND
            | Token::XOR
            | Token::LSS
            | Token::GTR
            | Token::AndNot
            | Token::REM
            | Token::EQL
            | Token::NEQ
            | Token::LEQ
            | Token::GEQ
            | Token::QUO
            | Token::SUB
    );
    if !watch || !exprs_equal(&bin.x, &bin.y) {
        return;
    }
    // Skip trivial literals like `1 == 1` — still suspicious but less useful;
    // upstream skips floats with side-effect-free check; we keep AST equality.
    report(
        pending,
        bin.op_pos.0 as u32,
        format!(
            "suspicious identical LHS and RHS for `{}` operator",
            bin.op.as_str()
        ),
    );
}

fn unquote_basic_string(value: &str) -> Option<String> {
    if value.len() >= 2 && (value.starts_with('"') || value.starts_with('`')) {
        Some(value[1..value.len() - 1].to_string())
    } else {
        None
    }
}

fn check_flag_name(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    let (pkg_ok, sym) = if let Some(rest) = name.strip_prefix("flag.") {
        (true, rest)
    } else {
        return;
    };
    if !pkg_ok {
        return;
    }
    let arg_idx = match sym {
        "Bool" | "Duration" | "Float64" | "String" | "Int" | "Int64" | "Uint" | "Uint64" => 0usize,
        "BoolVar" | "DurationVar" | "Float64Var" | "StringVar" | "IntVar" | "Int64Var"
        | "UintVar" | "Uint64Var" => 1usize,
        _ => return,
    };
    let Some(arg) = call.args.get(arg_idx) else {
        return;
    };
    let Some(flag) = code::expr_to_string(pass, arg).or_else(|| {
        if let Expr::BasicLit(lit) = arg {
            unquote_basic_string(&lit.value)
        } else {
            None
        }
    }) else {
        return;
    };
    let pos = call.fun.pos().0 as u32;
    if flag.is_empty() {
        report(pending, pos, "empty flag name");
    } else if flag.starts_with('-') {
        report(
            pending,
            pos,
            format!("flag name {flag:?} should not start with a hyphen"),
        );
    } else if flag.contains('=') {
        report(
            pending,
            pos,
            format!("flag name {flag:?} should not contain '='"),
        );
    } else if flag.contains(' ') {
        report(
            pending,
            pos,
            format!("flag name {flag:?} contains whitespace"),
        );
    }
}

fn check_map_key(lit: &CompositeLit, pending: &mut Vec<(u32, String)>) {
    if lit.elts.len() < 2 {
        return;
    }
    let is_map = matches!(lit.ty.as_deref(), Some(Expr::MapType(_)));
    if !is_map {
        return;
    }
    let mut whitespace_key: Option<(u32, String)> = None;
    let mut seen_non_basic = HashSet::new();
    for elt in &lit.elts {
        let Expr::KeyValueExpr(kv) = elt else {
            continue;
        };
        if let Expr::BasicLit(lit) = kv.key.as_ref() {
            let Some(s) = unquote_basic_string(&lit.value) else {
                continue;
            };
            if s.len() < 1 || s == " " || !s.contains(' ') {
                continue;
            }
            let bad = (s.starts_with(' ') && !s.starts_with("  "))
                || (s.ends_with(' ') && !s.ends_with("  "));
            if !bad {
                return;
            }
            if whitespace_key.is_some() {
                return; // more than one → not suspicious
            }
            whitespace_key = Some((kv.key.pos().0 as u32, expr_text(&kv.key).unwrap_or(s)));
        } else if let Some(text) = expr_text(&kv.key) {
            if !seen_non_basic.insert(text.clone()) {
                report(
                    pending,
                    kv.key.pos().0 as u32,
                    format!("suspicious duplicate {text} key"),
                );
            }
        }
    }
    if let Some((pos, key)) = whitespace_key {
        report(pending, pos, format!("suspicious whitespace in {key} key"));
    }
}

fn check_off_by1(index: &IndexExpr, pending: &mut Vec<(u32, String)>) {
    let Expr::CallExpr(call) = index.index.as_ref() else {
        return;
    };
    let is_len = match call.fun.as_ref() {
        Expr::Ident(id) => id.name == "len",
        _ => false,
    };
    if !is_len || call.args.len() != 1 {
        return;
    }
    if !exprs_equal(&index.x, &call.args[0]) {
        return;
    }
    let Some(x) = expr_text(&index.x) else {
        return;
    };
    report(
        pending,
        index.lbrack.0 as u32,
        format!("index expr always panics; maybe you wanted {x}[len({x})-1]?"),
    );
}

fn type_assert_matches(assert: &TypeAssertExpr, want_x: &Expr, want_ty: &Expr) -> bool {
    assert.ty.as_ref().is_some_and(|t| exprs_equal(t, want_ty)) && exprs_equal(&assert.x, want_x)
}

fn find_matching_assert(stmt: &Stmt, want_x: &Expr, want_ty: &Expr) -> bool {
    let mut found = false;
    walk::inspect(walk::stmt_ref(stmt), |n| {
        let Some(n) = n else {
            return true;
        };
        if let NodeRef::TypeAssertExpr(a) = n {
            if type_assert_matches(a, want_x, want_ty) {
                found = true;
            }
        }
        true
    });
    found
}

fn check_type_switch_var(stmt: &TypeSwitchStmt, pending: &mut Vec<(u32, String)>) {
    // Already has `v := x.(type)` form.
    if matches!(stmt.assign.as_ref(), Stmt::AssignStmt(_)) {
        return;
    }
    let Stmt::ExprStmt(es) = stmt.assign.as_ref() else {
        return;
    };
    let Expr::TypeAssertExpr(ta) = &es.x else {
        return;
    };
    if ta.ty.is_some() {
        return; // not `.(type)`
    }
    let x = ta.x.as_ref();
    let mut count = 0;
    for s in &stmt.body.list {
        let Stmt::CaseClause(cc) = s else {
            continue;
        };
        if cc.list.len() != 1 {
            continue;
        }
        if cc
            .body
            .iter()
            .any(|body_stmt| find_matching_assert(body_stmt, x, &cc.list[0]))
        {
            count += 1;
        }
    }
    if count > 0 {
        let msg = if count == 1 { "case" } else { "cases" };
        report(
            pending,
            stmt.switch.0 as u32,
            format!("{count} {msg} can benefit from type switch with assignment"),
        );
    }
}

fn unparen(expr: &Expr) -> &Expr {
    match expr {
        Expr::ParenExpr(p) => unparen(&p.x),
        other => other,
    }
}

fn check_bad_cond_expr(bin: &BinaryExpr, pending: &mut Vec<(u32, String)>) {
    if bin.op != Token::LAND {
        return;
    }
    let Expr::BinaryExpr(lhs) = unparen(&bin.x) else {
        return;
    };
    let Expr::BinaryExpr(rhs) = unparen(&bin.y) else {
        return;
    };
    // `x == a && x == b`
    if lhs.op == Token::EQL && rhs.op == Token::EQL && exprs_equal(&lhs.x, &rhs.x) {
        let text = expr_text(&Expr::BinaryExpr(bin.clone())).unwrap_or_else(|| "cond".into());
        report(
            pending,
            bin.op_pos.0 as u32,
            format!("`{text}` condition is suspicious"),
        );
        return;
    }
    // `x < a && x > b` where a < b (int literals)
    if lhs.op == Token::LSS && rhs.op == Token::GTR && exprs_equal(&lhs.x, &rhs.x) {
        let Some(a) = int_lit_value(&lhs.y) else {
            return;
        };
        let Some(b) = int_lit_value(&rhs.y) else {
            return;
        };
        if a < b {
            let text = expr_text(&Expr::BinaryExpr(bin.clone())).unwrap_or_else(|| "cond".into());
            report(
                pending,
                bin.op_pos.0 as u32,
                format!("`{text}` condition is always false"),
            );
        }
    }
}

fn int_lit_value(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::BasicLit(lit) => lit.value.parse().ok(),
        Expr::UnaryExpr(u) if u.op == Token::SUB => {
            int_lit_value(&u.x).map(|v| -v)
        }
        _ => None,
    }
}

fn check_bad_cond_for(stmt: &guff::ast::ForStmt, pending: &mut Vec<(u32, String)>) {
    let Some(Stmt::AssignStmt(init)) = stmt.init.as_deref() else {
        return;
    };
    if init.tok != Some(Token::DEFINE) || init.lhs.len() != 1 || init.rhs.len() != 1 {
        return;
    }
    if !is_int_lit(&init.rhs[0], 0) {
        return;
    }
    let Expr::Ident(iter) = &init.lhs[0] else {
        return;
    };
    let Some(cond) = &stmt.cond else {
        return;
    };
    let Expr::BinaryExpr(bin) = cond else {
        return;
    };
    let (op_suggest, cond_ok) = match bin.op {
        Token::GTR if matches!(&*bin.x, Expr::Ident(id) if id.name == iter.name) => {
            (Token::LSS, true)
        }
        Token::LSS if matches!(&*bin.y, Expr::Ident(id) if id.name == iter.name) => {
            (Token::GTR, true)
        }
        _ => (Token::LSS, false),
    };
    if !cond_ok {
        return;
    }
    let Some(Stmt::IncDecStmt(post)) = stmt.post.as_deref() else {
        return;
    };
    if post.tok != Token::INC || !matches!(&post.x, Expr::Ident(id) if id.name == iter.name) {
        return;
    }
    let Some(cond_t) = expr_text(cond) else {
        return;
    };
    let suggest = match (bin.op, op_suggest) {
        (Token::GTR, Token::LSS) => cond_t.replacen('>', "<", 1),
        (Token::LSS, Token::GTR) => cond_t.replacen('<', ">", 1),
        _ => return,
    };
    report(
        pending,
        stmt.for_.0 as u32,
        format!("`{cond_t}` in loop; probably meant `{suggest}`?"),
    );
}

fn check_unlambda(fl: &FuncLit, pending: &mut Vec<(u32, String)>) {
    if fl.body.list.len() != 1 {
        return;
    }
    let Stmt::ReturnStmt(ret) = &fl.body.list[0] else {
        return;
    };
    if ret.results.len() != 1 {
        return;
    }
    let Expr::CallExpr(call) = &ret.results[0] else {
        return;
    };
    let Some(callable) = call_qualified_name(call).or_else(|| expr_text(&call.fun)) else {
        return;
    };
    // Skip builtins.
    if matches!(
        callable.as_str(),
        "len" | "cap" | "make" | "new" | "append" | "copy" | "delete" | "panic" | "recover"
            | "close" | "complex" | "real" | "imag" | "min" | "max" | "clear"
    ) {
        return;
    }
    let Some(params) = &fl.ty.params else {
        return;
    };
    let mut expected: Vec<&str> = Vec::new();
    let mut has_ellipsis = false;
    for field in &params.list {
        if matches!(field.ty, Some(Expr::Ellipsis(_))) {
            has_ellipsis = true;
        }
        for name in &field.names {
            expected.push(name.name.as_str());
        }
    }
    if has_ellipsis {
        if !call.ellipsis.is_valid() {
            return;
        }
    }
    if call.args.len() != expected.len() {
        return;
    }
    for (arg, want) in call.args.iter().zip(expected.iter()) {
        match arg {
            Expr::Ident(id) if id.name == *want => {}
            _ => return,
        }
    }
    let Some(lit_text) = expr_text(&Expr::FuncLit(fl.clone())).or_else(|| {
        Some(format!("func(...) {{ return {callable}(...) }}"))
    }) else {
        return;
    };
    report(
        pending,
        fl.ty.func.0 as u32,
        format!("replace `{lit_text}` with `{callable}`"),
    );
}

fn check_regexp_must(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    let suggest = match name.as_str() {
        "regexp.Compile" => "regexp.MustCompile",
        "regexp.CompilePOSIX" => "regexp.MustCompilePOSIX",
        _ => return,
    };
    let Some(pat) = call.args.first() else {
        return;
    };
    let Some(pat_s) = code::expr_to_string(pass, pat).or_else(|| {
        if let Expr::BasicLit(lit) = pat {
            unquote_basic_string(&lit.value)
        } else {
            None
        }
    }) else {
        return;
    };
    report(
        pending,
        call.fun.pos().0 as u32,
        format!("for const patterns like {pat_s:?}, use {suggest}"),
    );
}

fn check_wrapper_func(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    // Method-style: x.Add(-1) / x.Truncate(0)
    if let Expr::SelectorExpr(sel) = call.fun.as_ref() {
        if sel.sel.name == "Add"
            && call.args.len() == 1
            && (code::is_integer_literal(pass, &call.args[0], -1) || is_int_lit(&call.args[0], -1))
        {
            report(
                pending,
                call.fun.pos().0 as u32,
                "use WaitGroup.Done method in `Add(-1)`",
            );
            return;
        }
        if sel.sel.name == "Truncate"
            && call.args.len() == 1
            && (code::is_integer_literal(pass, &call.args[0], 0) || is_int_lit(&call.args[0], 0))
        {
            report(
                pending,
                call.fun.pos().0 as u32,
                "use Buffer.Reset method in `Truncate(0)`",
            );
            return;
        }
    }
    match name.as_str() {
        "strings.SplitN" | "bytes.SplitN"
            if call.args.len() >= 3
                && (code::is_integer_literal(pass, &call.args[2], -1)
                    || is_int_lit(&call.args[2], -1)) =>
        {
            let pkg = if name.starts_with("bytes") {
                "bytes"
            } else {
                "strings"
            };
            report(
                pending,
                call.fun.pos().0 as u32,
                format!("use {pkg}.Split method in `{name}(..., -1)`"),
            );
        }
        "strings.Replace" | "bytes.Replace"
            if call.args.len() >= 4
                && (code::is_integer_literal(pass, &call.args[3], -1)
                    || is_int_lit(&call.args[3], -1)) =>
        {
            let pkg = if name.starts_with("bytes") {
                "bytes"
            } else {
                "strings"
            };
            report(
                pending,
                call.fun.pos().0 as u32,
                format!("use {pkg}.ReplaceAll method in `{name}(..., -1)`"),
            );
        }
        "http.HandlerFunc"
            if call.args.len() == 1
                && matches!(
                    call_qualified_name_of_expr(&call.args[0]).as_deref(),
                    Some("http.NotFound")
                ) =>
        {
            report(
                pending,
                call.fun.pos().0 as u32,
                "use http.NotFoundHandler method in `http.HandlerFunc(http.NotFound)`",
            );
        }
        _ => {}
    }
}

fn call_qualified_name_of_expr(expr: &Expr) -> Option<String> {
    match expr {
        Expr::SelectorExpr(sel) => {
            let x = expr_text(&sel.x)?;
            Some(format!("{x}.{}", sel.sel.name))
        }
        Expr::Ident(id) => Some(id.name.clone()),
        _ => None,
    }
}

fn check_arg_order(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    if call.args.len() < 2 {
        return;
    }
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    let watch = matches!(
        name.as_str(),
        "strings.HasPrefix"
            | "bytes.HasPrefix"
            | "strings.HasSuffix"
            | "bytes.HasSuffix"
            | "strings.Contains"
            | "bytes.Contains"
            | "strings.TrimPrefix"
            | "bytes.TrimPrefix"
            | "strings.TrimSuffix"
            | "bytes.TrimSuffix"
            | "strings.Split"
            | "bytes.Split"
    );
    if !watch {
        return;
    }
    let lit = &call.args[0];
    let s = &call.args[1];
    // First arg is const string/bytes, second is not const, and first is not Ident.
    if matches!(lit, Expr::Ident(_)) {
        return;
    }
    let lit_const = code::expr_to_string(pass, lit).is_some()
        || matches!(lit, Expr::BasicLit(b) if b.value.starts_with('"') || b.value.starts_with('`'));
    if !lit_const {
        return;
    }
    let s_const = code::expr_to_string(pass, s).is_some()
        || matches!(s, Expr::BasicLit(b) if b.value.starts_with('"') || b.value.starts_with('`'));
    if s_const {
        return;
    }
    let Some(lit_t) = expr_text(lit) else {
        return;
    };
    let Some(s_t) = expr_text(s) else {
        return;
    };
    report(
        pending,
        call.fun.pos().0 as u32,
        format!("{lit_t} and {s_t} arguments order looks reversed"),
    );
}

fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn types_identical(pass: &Pass<'_>, a: TypeId, b: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    api_identical(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        a,
        b,
    )
}

fn type_implements(pass: &Pass<'_>, v: TypeId, iface: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        v,
        iface,
    )
}

fn type_is_interface(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    is_interface(&artifacts.types, typ)
}

fn check_case_order(pass: &Pass<'_>, stmt: &TypeSwitchStmt, pending: &mut Vec<(u32, String)>) {
    // DEFERRED: expression-switch overlapping ranges (upstream TODO).
    struct IfaceSeen {
        node_text: String,
        typ: TypeId,
    }
    let mut ifaces: Vec<IfaceSeen> = Vec::new();
    for clause in &stmt.body.list {
        let Stmt::CaseClause(cc) = clause else {
            continue;
        };
        for x in &cc.list {
            let Some(typ) = type_of(pass, x) else {
                let concrete = expr_text(x).unwrap_or_else(|| "?".into());
                report(
                    pending,
                    cc.case.0 as u32,
                    format!("type is not defined {concrete}"),
                );
                return;
            };
            for iface in &ifaces {
                if type_implements(pass, typ, iface.typ) {
                    let concrete = expr_text(x).unwrap_or_else(|| "?".into());
                    report(
                        pending,
                        cc.case.0 as u32,
                        format!(
                            "case {concrete} must go before the {} case",
                            iface.node_text
                        ),
                    );
                    break;
                }
            }
            if type_is_interface(pass, typ) {
                ifaces.push(IfaceSeen {
                    node_text: expr_text(x).unwrap_or_else(|| "?".into()),
                    typ,
                });
            }
        }
    }
}

fn check_sloppy_type_assert(
    pass: &Pass<'_>,
    assert: &TypeAssertExpr,
    pending: &mut Vec<(u32, String)>,
) {
    if assert.ty.is_none() {
        return;
    }
    let info = match pass.types_info() {
        Some(i) => i,
        None => return,
    };
    let Some(to_tav) = info.types.get(&assert.id) else {
        // Fall back to the asserted type expression.
        let Some(ty_expr) = assert.ty.as_ref() else {
            return;
        };
        let Some(to_type) = type_of(pass, ty_expr) else {
            return;
        };
        let Some(from_type) = type_of(pass, &assert.x) else {
            return;
        };
        if types_identical(pass, to_type, from_type) {
            report(
                pending,
                assert.lparen.0 as u32,
                "type assertion from/to types are identical",
            );
        }
        return;
    };
    let Some(from_type) = type_of(pass, &assert.x) else {
        return;
    };
    if types_identical(pass, to_tav.typ, from_type) {
        report(
            pending,
            assert.lparen.0 as u32,
            "type assertion from/to types are identical",
        );
    }
}

fn codegen_bad_comment_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let patterns = [
            r"this (?:file|code) (?:was|is) auto(?:matically)? generated",
            r"this (?:file|code) (?:was|is) generated automatically",
            r"this (?:file|code) (?:was|is) generated by",
            r"this (?:file|code) (?:was|is) (?:auto(?:matically)? )?generated",
            r"this (?:file|code) (?:was|is) generated",
            r"code in this file (?:was|is) auto(?:matically)? generated",
            r"generated (?:file|code) - do not edit",
        ];
        Regex::new(&format!("(?i){}", patterns.join("|"))).expect("codegenComment RE")
    })
}

fn comment_fmt_key_value_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^//[\w-]+:.*$").expect("commentFormatting key:value RE"))
}

const COMMENT_FMT_PARTS: &[&str] = &[
    "//go:generate ",
    "//line /",
    "//nolint ",
    "//noinspection ",
    "//region",
    "//endregion",
    "//<editor-fold",
    "//</editor-fold",
    "//export ",
    "///",
    "//+",
    "//#",
    "//-",
    "//!",
];

fn check_comment_formatting(cg: &CommentGroup, pending: &mut Vec<(u32, String)>) {
    if cg.list.first().is_some_and(|c| c.text.starts_with("/*")) {
        return;
    }
    'outer: for comment in &cg.list {
        let text = comment.text.as_str();
        if text.len() <= "// ".len() {
            continue;
        }
        for p in COMMENT_FMT_PARTS {
            if text.len() >= p.len() && text[..p.len()].eq_ignore_ascii_case(p) {
                continue 'outer;
            }
        }
        if text.eq_ignore_ascii_case("//nolint") {
            continue;
        }
        if comment_fmt_key_value_re().is_match(text) {
            continue;
        }
        let rest = &text["//".len()..];
        let Some(r) = rest.chars().next() else {
            continue;
        };
        if matches!(r, '+' | '-' | '#' | '!') || r.is_whitespace() {
            continue;
        }
        report(
            pending,
            comment.slash.0 as u32,
            "put a space between `//` and comment text",
        );
        return;
    }
}

const DEPRECATED_PREFIX: &str = "Deprecated: ";

fn deprecated_common_patterns() -> &'static [&'static str] {
    &[
        "this type is deprecated",
        "this function is deprecated",
        "[[deprecated]]",
        "note: deprecated",
        "deprecated in",
        "deprecated. use",
        "deprecated! use",
        "deprecated use",
    ]
}

fn deprecated_common_typos() -> &'static [&'static str] {
    &[
        "DPRECATED: ",
        "DERECATED: ",
        "DEPECATED: ",
        "DEPEKATED: ",
        "DEPRCATED: ",
        "DEPREATED: ",
        "DEPRECTED: ",
        "DEPRECAED: ",
        "DEPRECATD: ",
        "DEPRECATE: ",
        "DERPECATE: ",
        "DERPECATED: ",
        "DEPREACTED: ",
    ]
}

fn check_deprecated_comment(doc: &CommentGroup, pending: &mut Vec<(u32, String)>) {
    let mut prev = String::new();
    for comment in &doc.list {
        if comment.text.starts_with("/*") {
            continue;
        }
        let raw_line = comment.text.strip_prefix("//").unwrap_or(&comment.text);
        let l = raw_line.trim();
        if raw_line.len() < DEPRECATED_PREFIX.len() {
            prev = l.to_string();
            continue;
        }
        let upcase = l.to_uppercase();
        if upcase.starts_with("DEPRECATED: ") && !l.starts_with(DEPRECATED_PREFIX) {
            let prefix = &l[.."DEPRECATED: ".len()];
            report(
                pending,
                comment.slash.0 as u32,
                format!("use `Deprecated: ` (note the casing) instead of `{prefix}`"),
            );
            return;
        }
        if l.starts_with("Deprecated, ") {
            report(
                pending,
                comment.slash.0 as u32,
                "use `:` instead of `,` in `Deprecated, `",
            );
            return;
        }
        for pat in deprecated_common_patterns() {
            if l.len() >= pat.len() && l[..pat.len()].eq_ignore_ascii_case(pat) {
                report(
                    pending,
                    comment.slash.0 as u32,
                    "the proper format is `Deprecated: `",
                );
                return;
            }
        }
        for typo in deprecated_common_typos() {
            if upcase.starts_with(typo) {
                let word = l.split(':').next().unwrap_or(l);
                report(
                    pending,
                    comment.slash.0 as u32,
                    format!("typo in `{word}`; should be `Deprecated`"),
                );
                return;
            }
        }
        if l.starts_with(DEPRECATED_PREFIX) && !prev.is_empty() {
            report(
                pending,
                comment.slash.0 as u32,
                "`Deprecated: ` notices should be in a dedicated paragraph, separated from the rest",
            );
            return;
        }
        prev = l.to_string();
    }
}

fn check_codegen_comment(doc: &CommentGroup, pending: &mut Vec<(u32, String)>) {
    let re = codegen_bad_comment_re();
    for comment in &doc.list {
        if re.is_match(&comment.text) {
            report(
                pending,
                comment.slash.0 as u32,
                "comment should match `Code generated .* DO NOT EDIT.` regexp",
            );
            return;
        }
    }
}

fn reparse_with_comments(path: &Path) -> Option<(Arc<FileSet>, File)> {
    let src = fs::read(path).ok()?;
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, &src, PARSE_COMMENTS).ok()?;
    Some((fset, file))
}

fn line_pos(fset: &FileSet, file_pos: Pos, line: i64) -> Option<u32> {
    let ft = fset.file(file_pos)?;
    if line < 1 || line as usize > ft.line_count() {
        return None;
    }
    Some(ft.line_start(line as usize).0 as u32)
}

fn declaration_docs(file: &File) -> Vec<&CommentGroup> {
    let mut out = Vec::new();
    if let Some(doc) = &file.doc {
        out.push(doc);
    }
    for decl in &file.decls {
        match decl {
            Decl::GenDecl(g) => {
                if let Some(doc) = &g.doc {
                    out.push(doc);
                }
            }
            Decl::FuncDecl(f) => {
                if let Some(doc) = &f.doc {
                    out.push(doc);
                }
            }
            Decl::BadDecl(_) => {}
        }
    }
    out
}

fn run_comment_checks(
    pass: &Pass<'_>,
    set: &HashSet<String>,
    pending: &mut Vec<(u32, String)>,
) {
    let need_codegen = enabled(set, "codegenComment");
    let need_fmt = enabled(set, "commentFormatting");
    let need_depr = enabled(set, "deprecatedComment");
    let need_commented_import = enabled(set, "commentedOutImport");
    if !need_codegen && !need_fmt && !need_depr && !need_commented_import {
        return;
    }

    let paths: Vec<_> = pass.pkg().compiled_go_files.clone();
    let n = pass.files().len();
    for i in 0..n {
        let file = &pass.files()[i];
        let Some(path) = paths.get(i) else {
            continue;
        };
        let Some((re_fset, parsed)) = reparse_with_comments(path) else {
            continue;
        };

        if need_codegen {
            if let Some(doc) = &parsed.doc {
                let mut local = Vec::new();
                check_codegen_comment(doc, &mut local);
                for (pos, msg) in local {
                    // pos is from reparse fset; remap via line.
                    let line = re_fset.position(Pos(pos as i64)).line;
                    if let Some(mapped) = line_pos(pass.fset(), file.pos(), line) {
                        report(pending, mapped, msg);
                    }
                }
            }
        }

        if need_fmt {
            for cg in &parsed.comments {
                let mut local = Vec::new();
                check_comment_formatting(cg, &mut local);
                for (pos, msg) in local {
                    let line = re_fset.position(Pos(pos as i64)).line;
                    if let Some(mapped) = line_pos(pass.fset(), file.pos(), line) {
                        report(pending, mapped, msg);
                    }
                }
            }
        }

        if need_depr {
            let docs = declaration_docs(&parsed);
            for doc in docs {
                let mut local = Vec::new();
                check_deprecated_comment(doc, &mut local);
                for (pos, msg) in local {
                    let line = re_fset.position(Pos(pos as i64)).line;
                    if let Some(mapped) = line_pos(pass.fset(), file.pos(), line) {
                        report(pending, mapped, msg);
                    }
                }
            }
        }

        if need_commented_import {
            let mut local = Vec::new();
            check_commented_out_import(&parsed, &mut local);
            for (pos, msg) in local {
                let line = re_fset.position(Pos(pos as i64)).line;
                if let Some(mapped) = line_pos(pass.fset(), file.pos(), line) {
                    report(pending, mapped, msg);
                }
            }
        }
    }
}

fn check_commented_out_import(file: &File, pending: &mut Vec<(u32, String)>) {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"(?m)^(?://|/\*)?\s*"([a-zA-Z0-9_/]+)"\s*(?:\*/)?$"#).unwrap()
    });
    for decl in &file.decls {
        let Decl::GenDecl(g) = decl else {
            break;
        };
        if g.tok != Some(Token::IMPORT) {
            break;
        }
        if !g.lparen.is_valid() {
            continue;
        }
        for cg in &file.comments {
            if cg.pos().0 > g.rparen.0 {
                break;
            }
            if cg.pos().0 < g.lparen.0 {
                continue;
            }
            for comment in &cg.list {
                for caps in re.captures_iter(&comment.text) {
                    let path = &caps[1];
                    report(
                        pending,
                        comment.slash.0 as u32,
                        format!("remove commented-out \"{path}\" import"),
                    );
                }
            }
        }
    }
}

fn is_string_typed(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Basic(b) => {
            matches!(b.kind(), BasicKind::String | BasicKind::UntypedString)
        }
        _ => false,
    }
}

fn len_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::CallExpr(call) = expr else {
        return None;
    };
    let Expr::Ident(id) = call.fun.as_ref() else {
        return None;
    };
    if id.name != "len" || call.args.len() != 1 {
        return None;
    }
    Some(&call.args[0])
}

fn check_empty_string_test(pass: &Pass<'_>, bin: &BinaryExpr, pending: &mut Vec<(u32, String)>) {
    let Some(arg) = len_arg(&bin.x) else {
        return;
    };
    if !is_string_typed(pass, arg) {
        return;
    }
    let Some(arg_t) = expr_text(arg) else {
        return;
    };
    let Some(x_t) = expr_text(&bin.x) else {
        return;
    };
    let Some(y_t) = expr_text(&bin.y) else {
        return;
    };
    let whole = format!("{x_t} {} {y_t}", bin.op.as_str());
    let suggest = match bin.op {
        Token::NEQ | Token::GTR if is_int_lit(&bin.y, 0) => format!("{arg_t} != \"\""),
        Token::EQL | Token::LEQ if is_int_lit(&bin.y, 0) => format!("{arg_t} == \"\""),
        _ => return,
    };
    report(
        pending,
        bin.op_pos.0 as u32,
        format!("replace `{whole}` with `{suggest}`"),
    );
}

fn check_empty_fallthrough(stmt: &SwitchStmt, pending: &mut Vec<(u32, String)>) {
    let mut prev_case_default = false;
    for s in stmt.body.list.iter().rev() {
        let Stmt::CaseClause(cc) = s else {
            continue;
        };
        let mut warn = false;
        if cc.body.len() == 1 {
            if let Stmt::BranchStmt(bs) = &cc.body[0] {
                if bs.tok == Token::FALLTHROUGH {
                    warn = true;
                    if prev_case_default {
                        report(
                            pending,
                            bs.tok_pos.0 as u32,
                            "remove empty case containing only fallthrough to default case",
                        );
                    } else if !cc.list.is_empty() {
                        report(
                            pending,
                            bs.tok_pos.0 as u32,
                            "replace empty case containing only fallthrough with expression list",
                        );
                    }
                }
            }
        }
        if !warn {
            prev_case_default = cc.list.is_empty();
        }
    }
}

fn check_empty_decl(g: &guff::ast::GenDecl, pending: &mut Vec<(u32, String)>) {
    if !g.lparen.is_valid() || !g.specs.is_empty() {
        return;
    }
    let msg = match g.tok {
        Some(Token::VAR) => "empty var() block",
        Some(Token::CONST) => "empty const() block",
        Some(Token::TYPE) => "empty type() block",
        _ => return,
    };
    report(pending, g.tok_pos.0 as u32, msg);
}

fn check_octal_literal(lit: &BasicLit, pending: &mut Vec<(u32, String)>) {
    if lit.kind != Some(Token::INT) {
        return;
    }
    let v = lit.value.as_str();
    if !v.starts_with('0') || v.len() == 1 {
        return;
    }
    let second = v.as_bytes()[1];
    // Old-style octal: 0[0-7]... — skip 0x/0X/0b/0B/0o/0O.
    if !second.is_ascii_digit() {
        return;
    }
    report(
        pending,
        lit.pos().0 as u32,
        format!("use new octal literal style, 0o{}", &v[1..]),
    );
}

fn check_nil_val_return(pass: &Pass<'_>, stmt: &IfStmt, pending: &mut Vec<(u32, String)>) {
    if stmt.body.list.len() != 1 {
        return;
    }
    let Stmt::ReturnStmt(ret) = &stmt.body.list[0] else {
        return;
    };
    let Expr::BinaryExpr(expr) = &stmt.cond else {
        return;
    };
    if expr.op != Token::EQL {
        return;
    }
    if !code::is_nil(pass, &expr.y) {
        return;
    }
    for res in &ret.results {
        if exprs_equal(&expr.x, res) {
            let Some(val) = expr_text(&expr.x) else {
                continue;
            };
            report(
                pending,
                ret.return_.0 as u32,
                format!("returned expr is always nil; replace {val} with nil"),
            );
            break;
        }
    }
}

fn check_yoda_style(bin: &BinaryExpr, pending: &mut Vec<(u32, String)>) {
    if bin.op != Token::EQL && bin.op != Token::NEQ {
        return;
    }
    let lhs_const = matches!(&*bin.x, Expr::BasicLit(_))
        || matches!(&*bin.x, Expr::Ident(id) if id.name == "nil");
    let rhs_lit = matches!(&*bin.y, Expr::BasicLit(_));
    if !lhs_const || rhs_lit {
        return;
    }
    let Some(x_t) = expr_text(&bin.x) else {
        return;
    };
    let Some(y_t) = expr_text(&bin.y) else {
        return;
    };
    let op = if bin.op == Token::EQL { "==" } else { "!=" };
    report(
        pending,
        bin.op_pos.0 as u32,
        format!("consider to change order in expression to {y_t} {op} {x_t}"),
    );
}

fn is_const_expr(pass: &Pass<'_>, expr: &Expr) -> bool {
    if let Some(info) = pass.types_info() {
        if let Some(tav) = info.types.get(&expr.id()) {
            if tav.val.is_some() {
                return true;
            }
        }
    }
    match expr {
        Expr::BasicLit(_) => true,
        Expr::ParenExpr(p) => is_const_expr(pass, &p.x),
        Expr::UnaryExpr(u)
            if matches!(
                u.op,
                Token::ADD | Token::SUB | Token::XOR | Token::NOT | Token::AND
            ) =>
        {
            is_const_expr(pass, &u.x)
        }
        Expr::BinaryExpr(b) => is_const_expr(pass, &b.x) && is_const_expr(pass, &b.y),
        _ => false,
    }
}

fn is_pkg_name(pass: &Pass<'_>, id: &Ident) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(&obj) = info.uses.get(&id.id) else {
        return false;
    };
    matches!(artifacts.objects.get(obj), ObjectData::PkgName(_))
}

fn check_defer_unlambda(pass: &Pass<'_>, d: &DeferStmt, pending: &mut Vec<(u32, String)>) {
    let call = &d.call;
    if !call.args.is_empty() {
        return;
    }
    let Expr::FuncLit(fl) = call.fun.as_ref() else {
        return;
    };
    if fl.body.list.len() != 1 {
        return;
    }
    let Stmt::ExprStmt(es) = &fl.body.list[0] else {
        return;
    };
    let Expr::CallExpr(inner) = &es.x else {
        return;
    };
    if !inner.args.iter().all(|a| is_const_expr(pass, a)) {
        return;
    }
    let args = inner
        .args
        .iter()
        .filter_map(expr_text)
        .collect::<Vec<_>>()
        .join(", ");
    let rewrite = match inner.fun.as_ref() {
        Expr::Ident(id) if id.name == "panic" || id.name == "recover" => return,
        Expr::Ident(id) => {
            if args.is_empty() {
                format!("defer {}()", id.name)
            } else {
                format!("defer {}({args})", id.name)
            }
        }
        Expr::SelectorExpr(sel) => {
            let Expr::Ident(pkg) = sel.x.as_ref() else {
                return;
            };
            if !is_pkg_name(pass, pkg) {
                return;
            }
            if args.is_empty() {
                format!("defer {}.{}()", pkg.name, sel.sel.name)
            } else {
                format!("defer {}.{}({args})", pkg.name, sel.sel.name)
            }
        }
        _ => return,
    };
    report(
        pending,
        d.defer_.0 as u32,
        format!("can rewrite as `{rewrite}`"),
    );
}

fn check_init_clause(
    name: &str,
    init: Option<&Stmt>,
    pos: u32,
    pending: &mut Vec<(u32, String)>,
) {
    let Some(init) = init else {
        return;
    };
    if matches!(init, Stmt::AssignStmt(_)) {
        return;
    }
    let clause = match init {
        Stmt::ExprStmt(e) => expr_text(&e.x).unwrap_or_else(|| "…".into()),
        _ => "…".into(),
    };
    report(
        pending,
        pos,
        format!("consider to move `{clause}` before {name}"),
    );
}

fn is_builtin_name(name: &str) -> bool {
    matches!(
        name,
        "any"
            | "bool"
            | "byte"
            | "comparable"
            | "complex64"
            | "complex128"
            | "error"
            | "float32"
            | "float64"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "rune"
            | "string"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uintptr"
            | "true"
            | "false"
            | "iota"
            | "nil"
            | "append"
            | "cap"
            | "clear"
            | "close"
            | "complex"
            | "copy"
            | "delete"
            | "imag"
            | "len"
            | "make"
            | "min"
            | "max"
            | "new"
            | "panic"
            | "print"
            | "println"
            | "real"
            | "recover"
    )
}

fn warn_builtin_shadow(ident: &Ident, pending: &mut Vec<(u32, String)>) {
    if is_builtin_name(&ident.name) {
        report(
            pending,
            ident.pos().0 as u32,
            format!("shadowing of predeclared identifier: {}", ident.name),
        );
    }
}

fn check_builtin_shadow_fields(fields: Option<&FieldList>, pending: &mut Vec<(u32, String)>) {
    let Some(fl) = fields else {
        return;
    };
    for field in &fl.list {
        for name in &field.names {
            warn_builtin_shadow(name, pending);
        }
    }
}

fn is_def_ident(pass: &Pass<'_>, id: &Ident) -> bool {
    let Some(info) = pass.types_info() else {
        // Without types info, treat DEFINE LHS idents as defs (best-effort).
        return true;
    };
    info.defs.get(&id.id).copied().flatten().is_some()
}

fn check_builtin_shadow_assign(pass: &Pass<'_>, a: &AssignStmt, pending: &mut Vec<(u32, String)>) {
    if a.tok != Some(Token::DEFINE) {
        return;
    }
    for lhs in &a.lhs {
        let Expr::Ident(id) = lhs else {
            continue;
        };
        if is_def_ident(pass, id) {
            warn_builtin_shadow(id, pending);
        }
    }
}

fn check_builtin_shadow_value_spec(spec: &ValueSpec, pending: &mut Vec<(u32, String)>) {
    for name in &spec.names {
        warn_builtin_shadow(name, pending);
    }
}

fn check_builtin_shadow_func(pass: &Pass<'_>, f: &FuncDecl, pending: &mut Vec<(u32, String)>) {
    check_builtin_shadow_fields(f.recv.as_ref(), pending);
    check_builtin_shadow_fields(f.ty.params.as_ref(), pending);
    check_builtin_shadow_fields(f.ty.results.as_ref(), pending);
    let Some(body) = &f.body else {
        return;
    };
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        match n {
            NodeRef::AssignStmt(a) => check_builtin_shadow_assign(pass, a, pending),
            NodeRef::GenDecl(g) => {
                for spec in &g.specs {
                    if let Spec::ValueSpec(vs) = spec {
                        check_builtin_shadow_value_spec(vs, pending);
                    }
                }
            }
            _ => {}
        }
        true
    });
}

fn check_builtin_shadow_decl(decl: &Decl, pending: &mut Vec<(u32, String)>) {
    match decl {
        Decl::FuncDecl(f) if f.recv.is_none() => {
            warn_builtin_shadow(&f.name, pending);
        }
        Decl::GenDecl(g) => {
            for spec in &g.specs {
                match spec {
                    Spec::ValueSpec(vs) => {
                        for name in &vs.names {
                            warn_builtin_shadow(name, pending);
                        }
                    }
                    Spec::TypeSpec(ts) => warn_builtin_shadow(&ts.name, pending),
                    Spec::ImportSpec(_) => {}
                }
            }
        }
        _ => {}
    }
}

fn check_dup_import(pass: &Pass<'_>, file: &File, pending: &mut Vec<(u32, String)>) {
    let mut by_path: HashMap<String, Vec<&guff::ast::ImportSpec>> = HashMap::new();
    for imp in &file.imports {
        by_path
            .entry(imp.path.value.clone())
            .or_default()
            .push(imp);
    }
    for import_list in by_path.values() {
        if import_list.len() < 2 {
            continue;
        }
        let mut lines: Vec<i64> = import_list
            .iter()
            .map(|imp| pass.fset().position(imp.path.value_pos).line)
            .collect();
        lines.sort_unstable();
        let mut msg = format!(
            "package is imported {} times under different aliases on lines",
            import_list.len()
        );
        for (idx, line) in lines.iter().enumerate() {
            if idx == lines.len() - 1 && lines.len() > 1 {
                msg.push_str(" and");
            } else if idx > 0 {
                msg.push(',');
            }
            msg.push_str(&format!(" {line}"));
        }
        for imp in import_list {
            report(pending, imp.path.value_pos.0 as u32, msg.clone());
        }
    }
}

fn check_filepath_join(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    if !(name == "filepath.Join"
        || name == "path/filepath.Join"
        || name.ends_with("/filepath.Join"))
    {
        return;
    }
    for arg in &call.args {
        let Expr::BasicLit(lit) = arg else {
            continue;
        };
        if lit.value.contains('/') || lit.value.contains('\\') {
            let Some(text) = expr_text(arg) else {
                continue;
            };
            report(
                pending,
                lit.value_pos.0 as u32,
                format!("{text} contains a path separator"),
            );
        }
    }
}

fn field_type_text(field: &Field) -> Option<String> {
    field.ty.as_ref().and_then(expr_text)
}

fn format_field_list(fields: &[(Vec<String>, String)]) -> String {
    fields
        .iter()
        .map(|(names, ty)| {
            if names.is_empty() {
                ty.clone()
            } else {
                format!("{} {ty}", names.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn optimize_named_fields(fields: &[Field]) -> Option<Vec<(Vec<String>, String)>> {
    if fields.len() < 2 || fields[0].names.is_empty() {
        return None;
    }
    let mut out: Vec<(Vec<String>, String)> = Vec::new();
    for field in fields {
        let names: Vec<String> = field.names.iter().map(|n| n.name.clone()).collect();
        let ty = field_type_text(field)?;
        if let Some(last) = out.last_mut() {
            if last.1 == ty {
                last.0.extend(names);
                continue;
            }
        }
        out.push((names, ty));
    }
    if out.len() == fields.len() {
        None
    } else {
        Some(out)
    }
}

fn format_func_type_like(ty: &FuncType, params: Option<&str>, results: Option<&str>) -> String {
    let mut s = String::from("func");
    if let Some(tp) = &ty.type_params {
        let parts: Vec<String> = tp
            .list
            .iter()
            .filter_map(|f| {
                let names = f
                    .names
                    .iter()
                    .map(|n| n.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let t = field_type_text(f)?;
                if names.is_empty() {
                    Some(t)
                } else {
                    Some(format!("{names} {t}"))
                }
            })
            .collect();
        s.push('[');
        s.push_str(&parts.join(", "));
        s.push(']');
    }
    s.push('(');
    if let Some(p) = params {
        s.push_str(p);
    } else if let Some(p) = &ty.params {
        let cur: Vec<_> = p
            .list
            .iter()
            .filter_map(|f| {
                let names: Vec<_> = f.names.iter().map(|n| n.name.clone()).collect();
                let t = field_type_text(f)?;
                Some((names, t))
            })
            .collect();
        s.push_str(&format_field_list(&cur));
    }
    s.push(')');
    if let Some(r) = results {
        if r.contains(',') || r.contains(' ') {
            s.push_str(&format!(" ({r})"));
        } else {
            s.push(' ');
            s.push_str(r);
        }
    } else if let Some(r) = &ty.results {
        let cur: Vec<_> = r
            .list
            .iter()
            .filter_map(|f| {
                let names: Vec<_> = f.names.iter().map(|n| n.name.clone()).collect();
                let t = field_type_text(f)?;
                Some((names, t))
            })
            .collect();
        let text = format_field_list(&cur);
        if r.list.len() > 1 || r.list.first().is_some_and(|f| !f.names.is_empty()) {
            s.push_str(&format!(" ({text})"));
        } else {
            s.push(' ');
            s.push_str(&text);
        }
    }
    s
}

fn params_are_multi_line(pass: &Pass<'_>, params: &FieldList) -> bool {
    if !params.opening.is_valid() || !params.closing.is_valid() {
        return false;
    }
    let start = pass.fset().position(params.opening).line;
    let end = pass.fset().position(params.closing).line;
    start != end
}

fn check_param_type_combine(pass: &Pass<'_>, f: &FuncDecl, pending: &mut Vec<(u32, String)>) {
    let opt_params = f.ty.params.as_ref().and_then(|p| {
        if params_are_multi_line(pass, p) {
            None
        } else {
            optimize_named_fields(&p.list)
        }
    });
    let opt_results = f.ty.results.as_ref().and_then(|r| {
        if params_are_multi_line(pass, r) {
            None
        } else {
            optimize_named_fields(&r.list)
        }
    });
    if opt_params.is_none() && opt_results.is_none() {
        return;
    }
    let before = format_func_type_like(&f.ty, None, None);
    let after_params = opt_params.as_ref().map(|p| format_field_list(p));
    let after_results = opt_results.as_ref().map(|r| format_field_list(r));
    let after = if opt_results.is_none() {
        format_func_type_like(&f.ty, after_params.as_deref(), None)
    } else if opt_params.is_none() {
        format_func_type_like(&f.ty, None, after_results.as_deref())
    } else {
        format_func_type_like(&f.ty, after_params.as_deref(), after_results.as_deref())
    };
    if before == after {
        return;
    }
    report(
        pending,
        f.ty.pos().0 as u32,
        format!("{before} could be replaced with {after}"),
    );
}

fn is_slice_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::CompositeLit(_))
}

fn check_range_append_all(pass: &Pass<'_>, rs: &RangeStmt, pending: &mut Vec<(u32, String)>) {
    if rs.body.list.is_empty() {
        return;
    }
    let Expr::Ident(range_id) = &rs.x else {
        return;
    };
    let Some(range_obj) = code::object_of(pass, range_id) else {
        return;
    };
    walk::inspect(NodeRef::BlockStmt(&rs.body), |n| {
        let Some(n) = n else {
            return true;
        };
        let NodeRef::CallExpr(call) = n else {
            return true;
        };
        if call.args.len() != 2 || !call.ellipsis.is_valid() {
            return true;
        }
        let is_append = match call.fun.as_ref() {
            Expr::Ident(id) => id.name == "append",
            _ => false,
        };
        if !is_append || is_slice_literal(&call.args[0]) {
            return true;
        }
        let Expr::Ident(from) = &call.args[1] else {
            return true;
        };
        if code::object_of(pass, from) == Some(range_obj) {
            report(
                pending,
                from.pos().0 as u32,
                format!("append all `{}` data while range it", from.name),
            );
        }
        true
    });
}

fn is_slice_typed(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    matches!(artifacts.types.get(typ), TypeData::Slice(_))
}

fn contains_index_of(tree: &Expr, x: &Expr) -> bool {
    match tree {
        Expr::IndexExpr(ix) => exprs_equal(x, &ix.x) || contains_index_of(&ix.index, x),
        Expr::ParenExpr(p) => contains_index_of(&p.x, x),
        Expr::UnaryExpr(u) => contains_index_of(&u.x, x),
        Expr::BinaryExpr(b) => contains_index_of(&b.x, x) || contains_index_of(&b.y, x),
        Expr::CallExpr(c) => {
            contains_index_of(&c.fun, x) || c.args.iter().any(|a| contains_index_of(a, x))
        }
        Expr::SelectorExpr(s) => contains_index_of(&s.x, x),
        Expr::SliceExpr(s) => {
            contains_index_of(&s.x, x)
                || s.low.as_ref().is_some_and(|e| contains_index_of(e, x))
                || s.high.as_ref().is_some_and(|e| contains_index_of(e, x))
                || s.max.as_ref().is_some_and(|e| contains_index_of(e, x))
        }
        Expr::StarExpr(s) => contains_index_of(&s.x, x),
        Expr::TypeAssertExpr(a) => contains_index_of(&a.x, x),
        Expr::IndexListExpr(ix) => {
            contains_index_of(&ix.x, x) || ix.indices.iter().any(|i| contains_index_of(i, x))
        }
        Expr::KeyValueExpr(kv) => {
            contains_index_of(&kv.key, x) || contains_index_of(&kv.value, x)
        }
        Expr::CompositeLit(lit) => lit.elts.iter().any(|e| contains_index_of(e, x)),
        _ => false,
    }
}

fn check_weak_cond(pass: &Pass<'_>, bin: &BinaryExpr, pending: &mut Vec<(u32, String)>) {
    let lhs = unparen(&bin.x);
    let rhs = unparen(&bin.y);
    let Expr::BinaryExpr(lhs_bin) = lhs else {
        return;
    };
    if !code::is_nil(pass, &lhs_bin.y) {
        return;
    }
    if !is_slice_typed(pass, &lhs_bin.x) {
        return;
    }
    let pat1 = bin.op == Token::LAND && lhs_bin.op == Token::NEQ;
    let pat2 = bin.op == Token::LOR && lhs_bin.op == Token::EQL;
    if !pat1 && !pat2 {
        return;
    }
    if !contains_index_of(rhs, &lhs_bin.x) {
        return;
    }
    let Some(x_t) = expr_text(&bin.x) else {
        return;
    };
    let Some(y_t) = expr_text(&bin.y) else {
        return;
    };
    let whole = format!("{x_t} {} {y_t}", bin.op.as_str());
    report(
        pending,
        bin.op_pos.0 as u32,
        format!("suspicious `{whole}`; nil check may not be enough, check for len"),
    );
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
        if enabled(&set, "builtinShadowDecl") {
            for decl in &file.decls {
                check_builtin_shadow_decl(decl, &mut pending);
            }
        }
        if enabled(&set, "dupImport") {
            check_dup_import(pass, file, &mut pending);
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
                    if enabled(&set, "dupBranchBody") {
                        check_dup_branch_body(s, &mut pending);
                    }
                    if enabled(&set, "ifElseChain") {
                        let key = s as *const _ as usize;
                        if if_else_ptr.insert(key, ()).is_none() {
                            check_if_else_chain(s, &mut if_else_visited, &mut pending);
                        }
                    }
                    if enabled(&set, "nilValReturn") {
                        check_nil_val_return(pass, s, &mut pending);
                    }
                    if enabled(&set, "initClause") {
                        check_init_clause("if", s.init.as_deref(), s.if_.0 as u32, &mut pending);
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
                    if enabled(&set, "emptyFallthrough") {
                        check_empty_fallthrough(s, &mut pending);
                    }
                    if enabled(&set, "initClause") {
                        check_init_clause(
                            "switch",
                            s.init.as_deref(),
                            s.switch.0 as u32,
                            &mut pending,
                        );
                    }
                }
                NodeRef::TypeSwitchStmt(s) => {
                    if enabled(&set, "singleCaseSwitch") {
                        check_single_case_type_switch(s, &mut pending);
                    }
                    if enabled(&set, "typeSwitchVar") {
                        check_type_switch_var(s, &mut pending);
                    }
                    if enabled(&set, "caseOrder") {
                        check_case_order(pass, s, &mut pending);
                    }
                }
                NodeRef::ForStmt(s) if enabled(&set, "badCond") => {
                    check_bad_cond_for(s, &mut pending);
                }
                NodeRef::RangeStmt(s) if enabled(&set, "rangeAppendAll") => {
                    check_range_append_all(pass, s, &mut pending);
                }
                NodeRef::BinaryExpr(b) => {
                    if enabled(&set, "sloppyLen") {
                        check_sloppy_len(b, &mut pending);
                    }
                    if enabled(&set, "dupSubExpr") {
                        check_dup_sub_expr(b, &mut pending);
                    }
                    if enabled(&set, "badCond") {
                        check_bad_cond_expr(b, &mut pending);
                    }
                    if enabled(&set, "emptyStringTest") {
                        check_empty_string_test(pass, b, &mut pending);
                    }
                    if enabled(&set, "yodaStyleExpr") {
                        check_yoda_style(b, &mut pending);
                    }
                    if enabled(&set, "weakCond") {
                        check_weak_cond(pass, b, &mut pending);
                    }
                }
                NodeRef::BasicLit(lit) if enabled(&set, "octalLiteral") => {
                    check_octal_literal(lit, &mut pending);
                }
                NodeRef::DeferStmt(d) if enabled(&set, "deferUnlambda") => {
                    check_defer_unlambda(pass, d, &mut pending);
                }
                NodeRef::GenDecl(g) if enabled(&set, "emptyDecl") => {
                    check_empty_decl(g, &mut pending);
                }
                NodeRef::SliceExpr(s) if enabled(&set, "unslice") => {
                    check_unslice(s, &mut pending);
                }
                NodeRef::IndexExpr(ix) if enabled(&set, "offBy1") => {
                    check_off_by1(ix, &mut pending);
                }
                NodeRef::CompositeLit(lit) if enabled(&set, "mapKey") => {
                    check_map_key(lit, &mut pending);
                }
                NodeRef::FuncLit(fl) if enabled(&set, "unlambda") => {
                    check_unlambda(fl, &mut pending);
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
                    if enabled(&set, "builtinShadow") {
                        check_builtin_shadow_func(pass, f, &mut pending);
                    }
                    if enabled(&set, "paramTypeCombine") {
                        check_param_type_combine(pass, f, &mut pending);
                    }
                }
                NodeRef::CallExpr(c) => {
                    if enabled(&set, "badCall") {
                        check_bad_call(pass, c, &mut pending);
                    }
                    if enabled(&set, "dupArg") {
                        check_dup_arg(pass, c, &mut pending);
                    }
                    if enabled(&set, "flagName") {
                        check_flag_name(pass, c, &mut pending);
                    }
                    if enabled(&set, "argOrder") {
                        check_arg_order(pass, c, &mut pending);
                    }
                    if enabled(&set, "regexpMust") {
                        check_regexp_must(pass, c, &mut pending);
                    }
                    if enabled(&set, "wrapperFunc") {
                        check_wrapper_func(pass, c, &mut pending);
                    }
                    if enabled(&set, "filepathJoin") {
                        check_filepath_join(pass, c, &mut pending);
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
                NodeRef::TypeAssertExpr(a) if enabled(&set, "sloppyTypeAssert") => {
                    check_sloppy_type_assert(pass, a, &mut pending);
                }
                _ => {}
            }
            true
        });
    }

    run_comment_checks(pass, &set, &mut pending);

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
