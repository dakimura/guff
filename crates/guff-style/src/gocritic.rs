//! Port of [`github.com/go-critic/go-critic`](https://github.com/go-critic/go-critic)
//! (golangci-lint wrapper: `linters.settings.gocritic`).
//!
//! Implemented checkers (**106** = 34 default + 72 enable-all extras):
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
//! - batch 6 (enable-all extras): `dupOption`, `methodExprCall`, `rangeExprCopy`,
//!   `regexpPattern`, `sortSlice`, `sqlQuery`, `typeAssertChain`
//! - batch 7 (enable-all extras): `badRegexp`
//! - batch 8 (enable-all extras): `truncateCmp`, `typeDefFirst`, `deferInLoop`,
//!   `hexLiteral`, `nestingReduce`, `todoCommentWithoutDetail`, `docStub`,
//!   `unnecessaryBlock`, `sloppyReassign`
//! - batch 9 (enable-all extras; prometheus-enabled ruleguard ports):
//!   `httpNoBody`, `preferDecodeRune`, `indexAlloc`, `stringXbytes`,
//!   `preferFilepathJoin`, `stringsCompare`, `zeroByteRepeat`, `badSorting`,
//!   `sliceClear`
//! - batch 10 (enable-all extra): `preferWriteByte`
//! - batch 11 (enable-all extras): `preferFprint`, `preferStringWriter`,
//!   `syncMapLoadAndDelete`, `dynamicFmtString`, `stringConcatSimplify`,
//!   `badSyncOnceFunc`
//! - batch 12 (enable-all extras): `equalFold`, `sprintfQuotedString`,
//!   `timeExprSimplify`, `appendCombine`, `unnecessaryDefer`, `redundantSprint`
//! - batch 13 (enable-all extras): `typeUnparen`, `importShadow`, `unnamedResult`,
//!   `whyNoLint`, `hugeParam`, `rangeValCopy`
//! - batch 14 (enable-all extras): `ptrToRefParam`, `tooManyResultsChecker`,
//!   `evalOrder`, `unlabelStmt`, `returnAfterHttpError`, `exposedSyncMutex`
//! - batch 15 (enable-all extra): `commentedOutCode`
//! - batch 16 (enable-all extras): `badLock`, `externalErrorReassign`,
//!   `uncheckedInlineErr`, `boolExprSimplify` (doubleNegation / invertComparison /
//!   negatedEquals / combineChecks / removeIncDec / foldRanges)
//! - batch 17 (enable-all extra): `regexpSimplify`
//!
//! Settings: `enable-all` / `disable-all` / `enabled-checks` / `disabled-checks`
//! / `enabled-tags` / `disabled-tags` (prometheus-style `enable-all` +
//! `disabled-checks` works; cli-style `disabled-tags: [style]` too), plus the
//! per-check params in [`GocriticCheckSettings`](crate::GocriticCheckSettings).
//!
//! Messages are emitted through [`report`], which prefixes the checker name the
//! way golangci-lint's wrapper does (`fmt.Sprintf("%s: %s", …)`). The prefix is
//! not cosmetic: a target's own `exclusions.rules` regexes match against it, so
//! a message missing it can only ever be a guff-only finding. Nodes embedded in
//! a message go through [`node_text`] (go/printer), not [`expr_text`], because
//! upstream interpolates them with `astfmt`. The full 104-checker fixture in
//! `tests/testdata/gocritic/` was diffed message-for-message against
//! golangci-lint 2.12 with `gocritic.enable-all`.
//!
//! DEFERRED: remaining enable-all extras (`ruleguard` DSL host),
//! wrapperFunc's `strings.Map` / `bytes.Map` / `draw.DrawMask` /
//! `strings.Index(…) >= 0` / `strings.Cut` arms,
//! regexpSimplify Go-only spellings (`[][]`) / full quasilyte/regex Value parity,
//! boolExprSimplify SkipChilds (nested dual-report) / SideEffectFree full parity,
//! badRegexp dangling-anchor / flag edge-case full parity with quasilyte/regex,
//! remaining per-check `settings` params (rangeExprCopy/rangeValCopy/hugeParam
//! sizeThreshold, nestingReduce bodyWidth, truncateCmp skipArchDependent; wired:
//! tooManyResultsChecker maxResults, ifElseChain minThreshold, unnamedResult
//! checkExported),
//! SuggestedFix, caseOrder expression-switch overlap,
//! wrapperFunc/unlambda/typeSwitchVar full type-aware parity,
//! sortSlice SideEffectFree full parity, sqlQuery embedded-field Exec walk,
//! stringXbytes regexp method / bytes.Equal full type parity,
//! preferFprint true `types.Implements(io.Writer)` (arity heuristic like QF1012),
//! redundantSprint true `fmt.Stringer` Implements (method-name heuristic),
//! typeUnparen full astcopy/astequal pretty-print parity.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use guff::ast::{
    AssignStmt, BasicLit, BinaryExpr, BlockStmt, CallExpr, ChanDir, CommentGroup, CompositeLit,
    Decl, DeferStmt, Expr, Field, FieldList, File, ForStmt, FuncDecl, FuncLit, FuncType, Ident,
    IfStmt, ImportSpec, IndexExpr, LabeledStmt, RangeStmt, ReturnStmt, SelectorExpr, SliceExpr,
    Spec, StarExpr, Stmt, SwitchStmt, TypeAssertExpr, TypeSwitchStmt, ValueSpec,
};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::{FileSet, Pos, NO_POS};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_constant::{int64_val, make_from_literal};
use guff_types::alias::unalias_readonly;
use guff_types::api_predicates::{api_identical, api_implements};
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::{basic_kind, BasicKind, IS_FLOAT, IS_INTEGER};
use guff_types::lookup::{lookup_field_or_method, LookupResult};
use guff_types::named::named_obj;
use guff_types::operand::OperandMode;
use guff_types::predicates::is_interface;
use guff_types::signature::signature_results;
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::typestring::type_string;
use guff_types::{default_sizes, TypeId};
use regex::Regex;

use crate::options::GocriticOptions;

#[path = "gocritic_bad_regexp.rs"]
mod gocritic_bad_regexp;
#[path = "gocritic_regexp_simplify.rs"]
mod gocritic_regexp_simplify;

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
    "appendCombine",
    "badLock",
    "badRegexp",
    "badSorting",
    "badSyncOnceFunc",
    "boolExprSimplify",
    "builtinShadow",
    "builtinShadowDecl",
    "commentedOutCode",
    "commentedOutImport",
    "deferInLoop",
    "deferUnlambda",
    "docStub",
    "dupImport",
    "dupOption",
    "dynamicFmtString",
    "emptyDecl",
    "emptyFallthrough",
    "emptyStringTest",
    "equalFold",
    "evalOrder",
    "exposedSyncMutex",
    "externalErrorReassign",
    "filepathJoin",
    "hexLiteral",
    "httpNoBody",
    "hugeParam",
    "importShadow",
    "indexAlloc",
    "initClause",
    "methodExprCall",
    "nestingReduce",
    "nilValReturn",
    "octalLiteral",
    "paramTypeCombine",
    "preferDecodeRune",
    "preferFilepathJoin",
    "preferFprint",
    "preferStringWriter",
    "preferWriteByte",
    "ptrToRefParam",
    "rangeAppendAll",
    "rangeExprCopy",
    "rangeValCopy",
    "redundantSprint",
    "regexpPattern",
    "regexpSimplify",
    "returnAfterHttpError",
    "sliceClear",
    "sloppyReassign",
    "sortSlice",
    "sprintfQuotedString",
    "sqlQuery",
    "stringConcatSimplify",
    "stringXbytes",
    "stringsCompare",
    "syncMapLoadAndDelete",
    "timeExprSimplify",
    "todoCommentWithoutDetail",
    "tooManyResultsChecker",
    "truncateCmp",
    "typeAssertChain",
    "typeDefFirst",
    "typeUnparen",
    "uncheckedInlineErr",
    "unlabelStmt",
    "unnecessaryBlock",
    "unnecessaryDefer",
    "unnamedResult",
    "weakCond",
    "whyNoLint",
    "yodaStyleExpr",
    "zeroByteRepeat",
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

/// go-critic's own tags for every checker this port implements, from
/// `CheckerInfo.Tags` in `checkers/*_checker.go` and `DocTags` in
/// `checkers/rulesdata/rulesdata.go` (go-critic v0.14.4).
///
/// The table has to be **complete and multi-valued**, not a rough grouping:
/// `enabled-tags` unions in every checker carrying the tag, so a missing entry
/// is a silently missing check rather than a cosmetic gap, and most checkers
/// carry two or three tags (`unnamedResult` is style + opinionated +
/// experimental). It is also what [`DEFAULT_CHECKS`] means — see
/// [`is_enabled_by_default`].
const CHECK_TAGS: &[(&str, &[&str])] = &[
    ("appendAssign", &["diagnostic"]),
    ("appendCombine", &["performance"]),
    ("argOrder", &["diagnostic"]),
    ("assignOp", &["style"]),
    ("badCall", &["diagnostic"]),
    ("badCond", &["diagnostic"]),
    ("badLock", &["diagnostic", "experimental"]),
    ("badRegexp", &["diagnostic", "experimental"]),
    ("badSorting", &["diagnostic", "experimental"]),
    ("badSyncOnceFunc", &["diagnostic", "experimental"]),
    ("boolExprSimplify", &["experimental", "style"]),
    ("builtinShadow", &["opinionated", "style"]),
    ("builtinShadowDecl", &["diagnostic", "experimental"]),
    ("captLocal", &["style"]),
    ("caseOrder", &["diagnostic"]),
    ("codegenComment", &["diagnostic"]),
    ("commentFormatting", &["style"]),
    ("commentedOutCode", &["diagnostic", "experimental"]),
    ("commentedOutImport", &["experimental", "style"]),
    ("defaultCaseOrder", &["style"]),
    ("deferInLoop", &["diagnostic", "experimental"]),
    ("deferUnlambda", &["experimental", "style"]),
    ("deprecatedComment", &["diagnostic"]),
    ("docStub", &["experimental", "style"]),
    ("dupArg", &["diagnostic"]),
    ("dupBranchBody", &["diagnostic"]),
    ("dupCase", &["diagnostic"]),
    ("dupImport", &["experimental", "style"]),
    ("dupOption", &["diagnostic", "experimental"]),
    ("dupSubExpr", &["diagnostic"]),
    ("dynamicFmtString", &["diagnostic", "experimental"]),
    ("elseif", &["style"]),
    ("emptyDecl", &["diagnostic", "experimental"]),
    ("emptyFallthrough", &["experimental", "style"]),
    ("emptyStringTest", &["experimental", "style"]),
    ("equalFold", &["experimental", "performance"]),
    ("evalOrder", &["diagnostic", "experimental"]),
    ("exitAfterDefer", &["diagnostic"]),
    ("exposedSyncMutex", &["experimental", "style"]),
    ("externalErrorReassign", &["diagnostic", "experimental"]),
    ("filepathJoin", &["diagnostic", "experimental"]),
    ("flagDeref", &["diagnostic"]),
    ("flagName", &["diagnostic"]),
    ("hexLiteral", &["experimental", "style"]),
    ("httpNoBody", &["experimental", "style"]),
    ("hugeParam", &["performance"]),
    ("ifElseChain", &["style"]),
    ("importShadow", &["opinionated", "style"]),
    ("indexAlloc", &["performance"]),
    ("initClause", &["experimental", "opinionated", "style"]),
    ("mapKey", &["diagnostic"]),
    ("methodExprCall", &["experimental", "style"]),
    ("nestingReduce", &["experimental", "opinionated", "style"]),
    ("newDeref", &["style"]),
    ("nilValReturn", &["diagnostic", "experimental"]),
    ("octalLiteral", &["experimental", "opinionated", "style"]),
    ("offBy1", &["diagnostic"]),
    ("paramTypeCombine", &["opinionated", "style"]),
    ("preferDecodeRune", &["experimental", "performance"]),
    ("preferFilepathJoin", &["experimental", "style"]),
    ("preferFprint", &["experimental", "performance"]),
    ("preferStringWriter", &["experimental", "performance"]),
    ("preferWriteByte", &["experimental", "opinionated", "performance"]),
    ("ptrToRefParam", &["experimental", "opinionated", "style"]),
    ("rangeAppendAll", &["diagnostic", "experimental"]),
    ("rangeExprCopy", &["performance"]),
    ("rangeValCopy", &["performance"]),
    ("redundantSprint", &["experimental", "style"]),
    ("regexpMust", &["style"]),
    ("regexpPattern", &["diagnostic", "experimental"]),
    ("regexpSimplify", &["experimental", "opinionated", "style"]),
    ("returnAfterHttpError", &["diagnostic", "experimental"]),
    ("singleCaseSwitch", &["style"]),
    ("sliceClear", &["experimental", "performance"]),
    ("sloppyLen", &["diagnostic"]),
    ("sloppyReassign", &["diagnostic", "experimental"]),
    ("sloppyTypeAssert", &["diagnostic"]),
    ("sortSlice", &["diagnostic", "experimental"]),
    ("sprintfQuotedString", &["diagnostic", "experimental"]),
    ("sqlQuery", &["diagnostic", "experimental"]),
    ("stringConcatSimplify", &["experimental", "style"]),
    ("stringXbytes", &["performance"]),
    ("stringsCompare", &["experimental", "style"]),
    ("switchTrue", &["style"]),
    ("syncMapLoadAndDelete", &["diagnostic", "experimental"]),
    ("timeExprSimplify", &["experimental", "style"]),
    ("todoCommentWithoutDetail", &["experimental", "opinionated", "style"]),
    ("tooManyResultsChecker", &["experimental", "opinionated", "style"]),
    ("truncateCmp", &["diagnostic", "experimental"]),
    ("typeAssertChain", &["experimental", "style"]),
    ("typeDefFirst", &["experimental", "style"]),
    ("typeSwitchVar", &["style"]),
    ("typeUnparen", &["opinionated", "style"]),
    ("uncheckedInlineErr", &["diagnostic", "experimental"]),
    ("underef", &["style"]),
    ("unlabelStmt", &["experimental", "style"]),
    ("unlambda", &["style"]),
    ("unnamedResult", &["experimental", "opinionated", "style"]),
    ("unnecessaryBlock", &["experimental", "opinionated", "style"]),
    ("unnecessaryDefer", &["diagnostic", "experimental"]),
    ("unslice", &["style"]),
    ("valSwap", &["style"]),
    ("weakCond", &["diagnostic", "experimental"]),
    ("whyNoLint", &["experimental", "style"]),
    ("wrapperFunc", &["style"]),
    ("yodaStyleExpr", &["experimental", "style"]),
    ("zeroByteRepeat", &["performance"]),
];

fn check_tags(name: &str) -> &'static [&'static str] {
    CHECK_TAGS
        .iter()
        .find(|(n, _)| *n == name)
        .map_or(&[][..], |(_, tags)| *tags)
}

/// `isEnabledByDefaultGoCriticChecker`: everything that carries none of the
/// four opt-in tags. This is the definition [`DEFAULT_CHECKS`] spells out, and
/// `gocritic_default_checks_are_exactly_the_untagged_ones` holds the two
/// together.
fn is_enabled_by_default(name: &str) -> bool {
    !check_tags(name)
        .iter()
        .any(|t| matches!(*t, "experimental" | "opinionated" | "performance" | "security"))
}

/// Port of golangci-lint's `settingsWrapper.inferEnabledChecks`.
///
/// The five steps are applied in this order and the order is the semantics:
///
/// 1. the base set — empty under `disable-all`, everything under `enable-all`,
///    otherwise the default-by-tag set;
/// 2. `enabled-tags` **adds** every checker carrying the tag. It is a union,
///    not a filter: `enabled-tags: [performance]` keeps the whole default set
///    and adds the performance checkers on top. Reading it as "only these
///    tags" empties the result, because no default-on checker carries an
///    opt-in tag by construction;
/// 3. `enabled-checks` adds by name;
/// 4. `disabled-tags` removes by tag;
/// 5. `disabled-checks` removes by name.
///
/// Because 4 and 5 come after 2 and 3, a checker named in `enabled-checks`
/// whose tag is in `disabled-tags` ends up **off**.
fn enabled_set(opts: &GocriticOptions) -> HashSet<String> {
    let mut set: HashSet<String> = if opts.disable_all {
        HashSet::new()
    } else if opts.enable_all {
        implemented_checks().map(|s| s.to_string()).collect()
    } else {
        implemented_checks()
            .filter(|n| is_enabled_by_default(n))
            .map(|s| s.to_string())
            .collect()
    };

    let enabled_tags: HashSet<String> = opts
        .enabled_tags
        .iter()
        .map(|t| t.to_ascii_lowercase())
        .collect();
    if !enabled_tags.is_empty() {
        for name in implemented_checks() {
            if check_tags(name).iter().any(|t| enabled_tags.contains(*t)) {
                set.insert(name.to_string());
            }
        }
    }

    for name in &opts.enabled_checks {
        set.insert(name.clone());
    }

    let disabled_tags: HashSet<String> = opts
        .disabled_tags
        .iter()
        .map(|t| t.to_ascii_lowercase())
        .collect();
    if !disabled_tags.is_empty() {
        set.retain(|name| !check_tags(name).iter().any(|t| disabled_tags.contains(*t)));
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

/// Render `expr` the way go-critic embeds nodes in its warnings.
///
/// Upstream messages interpolate AST nodes through `astfmt`, which is
/// `go/printer` over the real `token.FileSet`; the ruleguard rules spell the
/// same thing `$$` (whole match) or `$x` (a captured operand). [`expr_text`] is
/// a hand-rolled approximation that renders `f(a, b)` as `f(...)` and puts
/// blanks around every binary operator, so any message that embeds a node must
/// go through here instead to stay byte-identical with upstream.
fn node_text(pass: &Pass<'_>, expr: &Expr) -> Option<String> {
    let mut buf: Vec<u8> = Vec::new();
    guff::printer::fprint(&mut buf, pass.fset(), guff::printer::PrintNode::Expr(expr)).ok()?;
    String::from_utf8(buf).ok()
}

fn node_text_stmt(pass: &Pass<'_>, stmt: &Stmt) -> Option<String> {
    let mut buf: Vec<u8> = Vec::new();
    guff::printer::fprint(&mut buf, pass.fset(), guff::printer::PrintNode::Stmt(stmt)).ok()?;
    String::from_utf8(buf).ok()
}

/// [`node_text`] for a call the checker only holds as a `&CallExpr`. This is
/// what the ruleguard rules spell `$$` when the matched pattern is a call.
fn call_text(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    node_text(pass, &Expr::CallExpr(call.clone()))
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
        Expr::UnaryExpr(u) if u.op == Token::AND => {
            let x = expr_text(&u.x)?;
            Some(format!("&{x}"))
        }
        Expr::UnaryExpr(u) if u.op == Token::MUL => {
            let x = expr_text(&u.x)?;
            Some(format!("*{x}"))
        }
        Expr::UnaryExpr(u) if u.op == Token::SUB => {
            let x = expr_text(&u.x)?;
            Some(format!("-{x}"))
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

/// Structural equality matching [`expr_text`] semantics (no String alloc).
///
/// Variants that [`expr_text`] cannot print yield `false`. `StarExpr` and
/// `UnaryExpr(MUL)` both print as `*x`, so they compare equal. Call ellipsis
/// is ignored (not represented in [`expr_text`]).
fn exprs_equal(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(x), Expr::Ident(y)) => x.name == y.name,
        (Expr::BasicLit(x), Expr::BasicLit(y)) => x.value == y.value,
        (Expr::SelectorExpr(x), Expr::SelectorExpr(y)) => {
            x.sel.name == y.sel.name && exprs_equal(&x.x, &y.x)
        }
        (Expr::StarExpr(x), Expr::StarExpr(y)) => exprs_equal(&x.x, &y.x),
        (Expr::StarExpr(x), Expr::UnaryExpr(y)) if y.op == Token::MUL => exprs_equal(&x.x, &y.x),
        (Expr::UnaryExpr(x), Expr::StarExpr(y)) if x.op == Token::MUL => exprs_equal(&x.x, &y.x),
        (Expr::ParenExpr(x), Expr::ParenExpr(y)) => exprs_equal(&x.x, &y.x),
        (Expr::IndexExpr(x), Expr::IndexExpr(y)) => {
            exprs_equal(&x.x, &y.x) && exprs_equal(&x.index, &y.index)
        }
        (Expr::CallExpr(x), Expr::CallExpr(y)) => {
            exprs_equal(&x.fun, &y.fun)
                && x.args.len() == y.args.len()
                && x.args
                    .iter()
                    .zip(y.args.iter())
                    .all(|(a, b)| exprs_equal(a, b))
        }
        (Expr::UnaryExpr(x), Expr::UnaryExpr(y)) => {
            matches!(x.op, Token::NOT | Token::AND | Token::MUL | Token::SUB)
                && x.op == y.op
                && exprs_equal(&x.x, &y.x)
        }
        (Expr::BinaryExpr(x), Expr::BinaryExpr(y)) => {
            x.op == y.op && exprs_equal(&x.x, &y.x) && exprs_equal(&x.y, &y.y)
        }
        (Expr::TypeAssertExpr(x), Expr::TypeAssertExpr(y)) => {
            exprs_equal(&x.x, &y.x)
                && match (&x.ty, &y.ty) {
                    (None, None) => true,
                    (Some(tx), Some(ty)) => exprs_equal(tx, ty),
                    _ => false,
                }
        }
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

/// Emit a finding. golangci-lint's gocritic wrapper renders every warning as
/// `fmt.Sprintf("%s: %s", checkerName, warning)`, so the checker name is part
/// of the message the user — and any `exclusions.rules` regex — sees.
fn report(pending: &mut Vec<(u32, String)>, pos: u32, checker: &str, msg: impl Into<String>) {
    pending.push((pos, format!("{checker}: {}", msg.into())));
}

/// The checker name [`report`] prefixed onto a pending message.
fn checker_of(msg: &str) -> &str {
    msg.split_once(": ").map_or(msg, |(name, _)| name)
}

/// Position of an assignment *statement*, for checkers that warn on the whole
/// statement rather than on one of its operands.
///
/// go-critic passes an `ast.Node` to `ctx.Warn`, and `ast.AssignStmt.Pos()` is
/// the first LHS operand — not the `=` / `:=` token. Reporting `tok_pos` lands
/// a few columns to the right of upstream, which the finding-set gates never
/// saw because their key ignores columns (COMPAT-HARDENING §1).
fn assign_pos(assign: &AssignStmt) -> u32 {
    assign
        .lhs
        .first()
        .map(|e| e.pos())
        .unwrap_or(assign.tok_pos)
        .0 as u32
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
        "elseif",
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
                Stmt::ForStmt(_)
                | Stmt::RangeStmt(_)
                | Stmt::SelectStmt(_)
                | Stmt::SwitchStmt(_)
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
        report(
            pending,
            pos,
            "singleCaseSwitch",
            "found switch with default case only",
        );
    } else if cc.list.len() == 1 {
        report(
            pending,
            pos,
            "singleCaseSwitch",
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
                "defaultCaseOrder",
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
            "switchTrue",
            "replace 'switch $x; true {}' with 'switch $x; {}'",
        );
    } else {
        report(
            pending,
            stmt.switch.0 as u32,
            "switchTrue",
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
    let pos = bin.x.pos().0 as u32;
    match bin.op {
        Token::GEQ if is_int_lit(&bin.y, 0) => {
            if let Some(arg) = expr_text(&call.args[0]) {
                report(
                    pending,
                    pos,
                    "sloppyLen",
                    format!("len({arg}) >= 0 is always true"),
                );
            } else {
                report(pending, pos, "sloppyLen", "len(_) >= 0 is always true");
            }
        }
        Token::LSS if is_int_lit(&bin.y, 0) => {
            if let Some(arg) = expr_text(&call.args[0]) {
                report(
                    pending,
                    pos,
                    "sloppyLen",
                    format!("len({arg}) < 0 is always false"),
                );
            } else {
                report(pending, pos, "sloppyLen", "len(_) < 0 is always false");
            }
        }
        Token::LEQ if is_int_lit(&bin.y, 0) => {
            if let Some(arg) = expr_text(&call.args[0]) {
                report(
                    pending,
                    pos,
                    "sloppyLen",
                    format!("len({arg}) <= 0 can be len({arg}) == 0"),
                );
            } else {
                report(pending, pos, "sloppyLen", "len(_) <= 0 can be len(_) == 0");
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

fn check_unslice(pass: &Pass<'_>, slice: &SliceExpr, pending: &mut Vec<(u32, String)>) {
    if slice.low.is_some() || slice.high.is_some() || slice.max.is_some() || slice.slice3 {
        return;
    }
    // Upstream Type.Is(`[]$_`): only unnamed slice types. Arrays, named slices,
    // and strings must not flag. No AST fallback when type info is missing.
    let Some(typ) = type_of(pass, &slice.x) else {
        return;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    if !matches!(artifacts.types.get(typ), TypeData::Slice(_)) {
        return;
    }
    let Some(x) = expr_text(&slice.x) else {
        return;
    };
    report(
        pending,
        slice.x.pos().0 as u32,
        "unslice",
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
        "newDeref",
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
        // Upstream go-critic only compares when the append base is an Ident,
        // SelectorExpr, IndexExpr, or SliceExpr — not CompositeLit / CallExpr
        // (e.g. `options := append([]string{""}, …)` is intentional).
        match &call.args[0] {
            Expr::SliceExpr(s) => {
                if !exprs_equal(lhs, s.x.as_ref()) {
                    report(
                        pending,
                        call.fun.pos().0 as u32,
                        "appendAssign",
                        "append result not assigned to the same slice",
                    );
                }
            }
            Expr::IndexExpr(_) | Expr::Ident(_) | Expr::SelectorExpr(_) => {
                if !exprs_equal(lhs, &call.args[0]) {
                    report(
                        pending,
                        call.fun.pos().0 as u32,
                        "appendAssign",
                        "append result not assigned to the same slice",
                    );
                }
            }
            _ => {}
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
                report(
                    pending,
                    x.pos().0 as u32,
                    "dupCase",
                    format!("'case {text}' is duplicated"),
                );
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
                    "captLocal",
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

fn check_exit_after_defer(pass: &Pass<'_>, func: &FuncDecl, pending: &mut Vec<(u32, String)>) {
    let Some(body) = &func.body else {
        return;
    };
    let mut defer_pos: Option<(u32, String)> = None;
    let mut found = false;

    /// Upstream renders the whole `defer` statement with `astfmt.Sprint`, but
    /// collapses a function literal to `func(…){...}(...)` so the warning stays
    /// on one line.
    fn defer_label(pass: &Pass<'_>, stmt: &Stmt, d: &DeferStmt) -> String {
        if let Expr::FuncLit(fl) = d.call.fun.as_ref() {
            let sig =
                node_text(pass, &Expr::FuncType(fl.ty.clone())).unwrap_or_else(|| "func()".into());
            return format!("defer {sig}{{...}}(...)");
        }
        node_text_stmt(pass, stmt).unwrap_or_else(|| "defer ...".into())
    }

    fn walk(
        pass: &Pass<'_>,
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
                    *defer_pos = Some((d.defer_.0 as u32, defer_label(pass, s, d)));
                }
                Stmt::ExprStmt(e) => {
                    if let Expr::CallExpr(call) = &e.x {
                        check_exit_call(call, defer_pos, found, pending);
                    }
                }
                Stmt::IfStmt(i) => {
                    walk(pass, &i.body.list, defer_pos, found, pending, false);
                    if !*found {
                        if let Some(e) = &i.else_ {
                            // Don't treat else-branch exits as after defer when
                            // defer was only seen on the if path (upstream skips Else).
                            if defer_pos.is_some() && !in_else {
                                // Still check else if defer already recorded before if.
                            }
                            match e.as_ref() {
                                Stmt::BlockStmt(b) => {
                                    walk(pass, &b.list, defer_pos, found, pending, true)
                                }
                                Stmt::IfStmt(_) => walk(
                                    pass,
                                    std::slice::from_ref(e.as_ref()),
                                    defer_pos,
                                    found,
                                    pending,
                                    true,
                                ),
                                _ => {}
                            }
                        }
                    }
                }
                Stmt::BlockStmt(b) => walk(pass, &b.list, defer_pos, found, pending, in_else),
                Stmt::ForStmt(f) => walk(pass, &f.body.list, defer_pos, found, pending, false),
                Stmt::RangeStmt(r) => walk(pass, &r.body.list, defer_pos, found, pending, false),
                Stmt::SwitchStmt(sw) => {
                    for c in &sw.body.list {
                        if let Stmt::CaseClause(cc) = c {
                            walk(pass, &cc.body, defer_pos, found, pending, false);
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
                "exitAfterDefer",
                format!("{name} will exit, and `{defer_label}` will not run"),
            );
            *found = true;
        }
    }

    walk(pass, &body.list, &mut defer_pos, &mut found, pending, false);
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
    min_threshold: usize,
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
    // `count_if_else_len` is non-negative by construction (0 on give-up).
    if usize::try_from(count_if_else_len(stmt)).unwrap_or(0) >= min_threshold {
        report(
            pending,
            stmt.if_.0 as u32,
            "ifElseChain",
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
        if a.lhs.len() != 1
            || a.rhs.len() != 1
            || b.lhs.len() != 1
            || b.rhs.len() != 1
            || c.lhs.len() != 1
            || c.rhs.len() != 1
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
            assign_pos(a),
            "valSwap",
            format!("can re-write as `{y_t}, {x_t} = {x_t}, {y_t}`"),
        );
    }
}

fn check_flag_deref(pass: &Pass<'_>, star: &StarExpr, pending: &mut Vec<(u32, String)>) {
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
    // Upstream interpolates the whole deref expression, arguments included.
    let whole =
        node_text(pass, &Expr::StarExpr(star.clone())).unwrap_or_else(|| format!("*{name}(...)"));
    report(
        pending,
        star.star.0 as u32,
        "flagDeref",
        format!("immediate deref in {whole} is most likely an error; consider using {suggest}"),
    );
}

fn check_bad_call(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    // Builtin `append` only — go-critic's `append($_)` does not match methods
    // named `append` (e.g. `cmd.append(app)`).
    if matches!(call.fun.as_ref(), Expr::Ident(id) if id.name == "append")
        && call.args.len() == 1
        && !call.ellipsis.is_valid()
    {
        report(
            pending,
            call.fun.pos().0 as u32,
            "badCall",
            "no-op append call, probably missing arguments",
        );
    }

    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    match name.as_str() {
        n if (n == "filepath.Join"
            || n == "path/filepath.Join"
            || n.ends_with("/filepath.Join"))
            && call.args.len() == 1 =>
        {
            report(
                pending,
                call.fun.pos().0 as u32,
                "badCall",
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
                        "badCall",
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
        Token::AndNot => {
            format!("replace `{x_t} = {x_t} &^ {y_t}` with `{x_t} &^= {y_t}`")
        }
        _ => return,
    };
    report(pending, assign_pos(assign), "assignOp", msg);
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
        let whole = call_text(pass, call).unwrap_or_else(|| format!("{name}(...)"));
        report(
            pending,
            call.fun.pos().0 as u32,
            "dupArg",
            format!("suspicious duplicated args in {whole}"),
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
            // The init statement is part of the comparison: dropping it made
            // `if err := f(a); err != nil {…}` and `if err := f(b); err != nil
            // {…}` render identically, so `dupBranchBody` reported branches
            // that differ only in the call (kubernetes `wsstream/stream.go`).
            let init = match &i.init {
                Some(s) => format!("{} ", stmt_text(s)?),
                None => String::new(),
            };
            match &i.else_ {
                Some(e) => {
                    let else_t = stmt_text(e)?;
                    Some(format!("if {init}{cond} {body} else {else_t}"))
                }
                None => Some(format!("if {init}{cond} {body}")),
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
            "dupBranchBody",
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
        bin.x.pos().0 as u32,
        "dupSubExpr",
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
        report(pending, pos, "flagName", "empty flag name");
    } else if flag.starts_with('-') {
        report(
            pending,
            pos,
            "flagName",
            format!("flag name {flag:?} should not start with a hyphen"),
        );
    } else if flag.contains('=') {
        report(
            pending,
            pos,
            "flagName",
            format!("flag name {flag:?} should not contain '='"),
        );
    } else if flag.contains(' ') {
        report(
            pending,
            pos,
            "flagName",
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
                    "mapKey",
                    format!("suspicious duplicate {text} key"),
                );
            }
        }
    }
    if let Some((pos, key)) = whitespace_key {
        report(
            pending,
            pos,
            "mapKey",
            format!("suspicious whitespace in {key} key"),
        );
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
        index.x.pos().0 as u32,
        "offBy1",
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
            "typeSwitchVar",
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
            bin.x.pos().0 as u32,
            "badCond",
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
                bin.x.pos().0 as u32,
                "badCond",
                format!("`{text}` condition is always false"),
            );
        }
    }
}

fn int_lit_value(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::BasicLit(lit) => lit.value.parse().ok(),
        Expr::UnaryExpr(u) if u.op == Token::SUB => int_lit_value(&u.x).map(|v| -v),
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
        "badCond",
        format!("`{cond_t}` in loop; probably meant `{suggest}`?"),
    );
}

fn check_unlambda(pass: &Pass<'_>, fl: &FuncLit, pending: &mut Vec<(u32, String)>) {
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
    // Upstream `qualifiedName`: only simple `pkg.Func` / `ident` / `recv.Method`
    // where recv is an Ident — skip `LoadedData().HasSection` etc.
    if !is_simple_unlambda_callable(&call.fun) {
        return;
    }
    let Some(callable) = call_qualified_name(call).or_else(|| expr_text(&call.fun)) else {
        return;
    };
    // Skip builtins.
    if matches!(
        callable.as_str(),
        "len"
            | "cap"
            | "make"
            | "new"
            | "append"
            | "copy"
            | "delete"
            | "panic"
            | "recover"
            | "close"
            | "complex"
            | "real"
            | "imag"
            | "min"
            | "max"
            | "clear"
    ) {
        return;
    }
    // Skip type conversions (Fun is a type name, not a callable).
    if is_type_expr(pass, &call.fun) {
        return;
    }
    // Skip when Fun captures Vars that aren't non-pointer struct method values
    // (upstream #888 / #1007 — e.g. `externalURL.String`, local func vars).
    if unlambda_fun_has_disallowed_vars(pass, &call.fun) {
        return;
    }
    // Require identical types between the func lit and the callable.
    let Some(info) = pass.types_info() else {
        return;
    };
    let Some(fn_ty) = info.types.get(&fl.id).map(|tv| tv.typ) else {
        return;
    };
    let Some(callable_ty) = type_of(pass, &call.fun) else {
        return;
    };
    if !types_identical(pass, fn_ty, callable_ty) {
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
    // Upstream renders the literal with `astfmt`, i.e. the real source text:
    // `func(s string) string { return strings.TrimSpace(s) }`. `expr_text` has
    // no case for a func literal's body, so it has to go through go/printer.
    let lit = Expr::FuncLit(fl.clone());
    let Some(lit_text) = node_text(pass, &lit)
        .or_else(|| expr_text(&lit))
        .or_else(|| Some(format!("func(...) {{ return {callable}(...) }}")))
    else {
        return;
    };
    report(
        pending,
        fl.ty.func.0 as u32,
        "unlambda",
        format!("replace `{lit_text}` with `{callable}`"),
    );
}

/// Upstream unlambda only handles simple callables (`fn`, `pkg.Fn`, `x.Method`
/// with Ident receiver) — not `LoadedData().HasSection`.
fn is_simple_unlambda_callable(fun: &Expr) -> bool {
    let fun = unparen_expr(fun);
    match fun {
        Expr::Ident(_) => true,
        Expr::SelectorExpr(sel) => matches!(unparen_expr(&sel.x), Expr::Ident(_)),
        _ => false,
    }
}

fn unparen_expr(e: &Expr) -> &Expr {
    let mut cur = e;
    while let Expr::ParenExpr(p) = cur {
        cur = &p.x;
    }
    cur
}

/// go-critic underef with default `skipRecvDeref: true`.
fn check_underef(pass: &Pass<'_>, sel: &SelectorExpr, pending: &mut Vec<(u32, String)>) {
    if is_ptr_recv_method_call(pass, sel) {
        return;
    }
    let Expr::ParenExpr(paren) = sel.x.as_ref() else {
        return;
    };
    let Expr::StarExpr(star) = paren.x.as_ref() else {
        return;
    };
    let Some(inner) = expr_text(&star.x) else {
        return;
    };
    report(
        pending,
        sel.x.pos().0 as u32,
        "underef",
        format!(
            "could simplify (*{inner}).{} to {inner}.{}",
            sel.sel.name, sel.sel.name
        ),
    );
}

/// True when `sel` is a method with a pointer receiver (go-critic
/// `isPtrRecvMethodCall`).
fn is_ptr_recv_method_call(pass: &Pass<'_>, sel: &SelectorExpr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(obj) = info.uses.get(&sel.sel.id).copied() else {
        return false;
    };
    let ObjectData::Func(f) = artifacts.objects.get(obj) else {
        return false;
    };
    let Some(sig) = f.typ() else {
        return false;
    };
    let Some(recv) = guff_types::signature::signature_recv(&artifacts.types, sig) else {
        return false;
    };
    let Some(recv_ty) = recv.typ(&artifacts.objects) else {
        return false;
    };
    let u = recv_ty.underlying(&artifacts.types);
    matches!(artifacts.types.get(u), TypeData::Pointer(_))
}

/// Upstream unlambda: skip if `Fun` contains a `Var` whose underlying type is
/// not a (non-pointer) struct — only those are safe method-value receivers.
fn unlambda_fun_has_disallowed_vars(pass: &Pass<'_>, fun: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut found = false;
    walk::inspect(walk::expr_ref(fun), |n| {
        let Some(n) = n else {
            return true;
        };
        if found {
            return false;
        }
        let NodeRef::Ident(id) = n else {
            return true;
        };
        let Some(&obj) = info.uses.get(&id.id) else {
            return true;
        };
        let ObjectData::Var(v) = artifacts.objects.get(obj) else {
            return true;
        };
        let under = v.typ().underlying(&artifacts.types);
        // Permit only non-pointer struct method values (`typep.IsStruct`).
        if !matches!(artifacts.types.get(under), TypeData::Struct(_)) {
            found = true;
            return false;
        }
        true
    });
    found
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
        "regexpMust",
        format!("for const patterns like {pat_s:?}, use {suggest}"),
    );
}

/// The upstream `bytes.SplitN` rule is `bytes.SplitN(b, []byte("."), -1)`:
/// `b` and `"."` are written without `$`, so ruleguard matches them literally.
fn is_bytes_splitn_literal_form(call: &CallExpr) -> bool {
    if call.args.len() != 3 {
        return false;
    }
    if !matches!(&call.args[0], Expr::Ident(id) if id.name == "b") {
        return false;
    }
    let Some(inner) = is_byte_slice_conv(&call.args[1]) else {
        return false;
    };
    matches!(inner, Expr::BasicLit(lit) if lit.value == "\".\"")
}

fn check_wrapper_func(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    // Method-style: x.Add(-1) / x.Truncate(0) — only on the concrete types
    // upstream wrapperFunc matches (`sync.WaitGroup`, `bytes.Buffer`).
    if let Expr::SelectorExpr(sel) = call.fun.as_ref() {
        if sel.sel.name == "Add"
            && call.args.len() == 1
            && (code::is_integer_literal(pass, &call.args[0], -1) || is_int_lit(&call.args[0], -1))
            && type_is_sync_wait_group(pass, &sel.x)
        {
            let whole = call_text(pass, call).unwrap_or_else(|| "Add(-1)".into());
            report(
                pending,
                call.fun.pos().0 as u32,
                "wrapperFunc",
                format!("use WaitGroup.Done method in `{whole}`"),
            );
            return;
        }
        if sel.sel.name == "Truncate"
            && call.args.len() == 1
            && (code::is_integer_literal(pass, &call.args[0], 0) || is_int_lit(&call.args[0], 0))
            && type_is_bytes_buffer(pass, &sel.x)
        {
            let whole = call_text(pass, call).unwrap_or_else(|| "Truncate(0)".into());
            report(
                pending,
                call.fun.pos().0 as u32,
                "wrapperFunc",
                format!("use Buffer.Reset method in `{whole}`"),
            );
            return;
        }
    }
    match name.as_str() {
        // `strings.SplitN($_, $_, -1)` is a general pattern upstream, but the
        // bytes twin is written `bytes.SplitN(b, []byte("."), -1)` — literal
        // identifiers, so it only ever fires on that exact spelling.
        "strings.SplitN" | "bytes.SplitN"
            if call.args.len() >= 3
                && (code::is_integer_literal(pass, &call.args[2], -1)
                    || is_int_lit(&call.args[2], -1))
                && (name == "strings.SplitN" || is_bytes_splitn_literal_form(call)) =>
        {
            let pkg = if name.starts_with("bytes") {
                "bytes"
            } else {
                "strings"
            };
            let whole = call_text(pass, call).unwrap_or_else(|| format!("{name}(..., -1)"));
            report(
                pending,
                call.fun.pos().0 as u32,
                "wrapperFunc",
                format!("use {pkg}.Split method in `{whole}`"),
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
            let whole = call_text(pass, call).unwrap_or_else(|| format!("{name}(..., -1)"));
            report(
                pending,
                call.fun.pos().0 as u32,
                "wrapperFunc",
                format!("use {pkg}.ReplaceAll method in `{whole}`"),
            );
        }
        "http.HandlerFunc"
            if call.args.len() == 1
                && matches!(
                    call_qualified_name_of_expr(&call.args[0]).as_deref(),
                    Some("http.NotFound")
                ) =>
        {
            let whole =
                call_text(pass, call).unwrap_or_else(|| "http.HandlerFunc(http.NotFound)".into());
            report(
                pending,
                call.fun.pos().0 as u32,
                "wrapperFunc",
                format!("use http.NotFoundHandler method in `{whole}`"),
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
        "argOrder",
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
    api_identical(&mut types, &artifacts.objects, &artifacts.packages, a, b)
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
                    "caseOrder",
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
                        "caseOrder",
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
                assert.x.pos().0 as u32,
                "sloppyTypeAssert",
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
            assert.x.pos().0 as u32,
            "sloppyTypeAssert",
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
            // Prefixes are ASCII; compare via bytes so a multi-byte char in
            // `text` cannot panic on a mid-codepoint slice (`text[..p.len()]`).
            if text.len() >= p.len()
                && text.as_bytes()[..p.len()].eq_ignore_ascii_case(p.as_bytes())
            {
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
            "commentFormatting",
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
            let prefix = l
                .get(..DEPRECATED_PREFIX.len())
                .unwrap_or(DEPRECATED_PREFIX);
            report(
                pending,
                comment.slash.0 as u32,
                "deprecatedComment",
                format!("use `Deprecated: ` (note the casing) instead of `{prefix}`"),
            );
            return;
        }
        if l.starts_with("Deprecated, ") {
            report(
                pending,
                comment.slash.0 as u32,
                "deprecatedComment",
                "use `:` instead of `,` in `Deprecated, `",
            );
            return;
        }
        for pat in deprecated_common_patterns() {
            if l.get(..pat.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(pat))
            {
                report(
                    pending,
                    comment.slash.0 as u32,
                    "deprecatedComment",
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
                    "deprecatedComment",
                    format!("typo in `{word}`; should be `Deprecated`"),
                );
                return;
            }
        }
        if l.starts_with(DEPRECATED_PREFIX) && !prev.is_empty() {
            report(pending, comment.slash.0 as u32, "deprecatedComment", "`Deprecated: ` notices should be in a dedicated paragraph, separated from the rest");
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
                "codegenComment",
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

fn run_comment_checks(pass: &Pass<'_>, set: &HashSet<String>, pending: &mut Vec<(u32, String)>) {
    let need_codegen = enabled(set, "codegenComment");
    let need_fmt = enabled(set, "commentFormatting");
    let need_depr = enabled(set, "deprecatedComment");
    let need_commented_code = enabled(set, "commentedOutCode");
    let need_commented_import = enabled(set, "commentedOutImport");
    let need_todo = enabled(set, "todoCommentWithoutDetail");
    let need_doc_stub = enabled(set, "docStub");
    let need_why_nolint = enabled(set, "whyNoLint");
    if !need_codegen
        && !need_fmt
        && !need_depr
        && !need_commented_code
        && !need_commented_import
        && !need_todo
        && !need_doc_stub
        && !need_why_nolint
    {
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
                    if let Some(mapped) = code::remap_reparsed_pos(pass.fset(), file.pos(), &re_fset, Pos(pos as i64))
                        .map(|p| p.0 as u32) {
                        pending.push((mapped, msg));
                    }
                }
            }
        }

        if need_fmt {
            for cg in &parsed.comments {
                let mut local = Vec::new();
                check_comment_formatting(cg, &mut local);
                for (pos, msg) in local {
                    if let Some(mapped) = code::remap_reparsed_pos(pass.fset(), file.pos(), &re_fset, Pos(pos as i64))
                        .map(|p| p.0 as u32) {
                        pending.push((mapped, msg));
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
                    if let Some(mapped) = code::remap_reparsed_pos(pass.fset(), file.pos(), &re_fset, Pos(pos as i64))
                        .map(|p| p.0 as u32) {
                        pending.push((mapped, msg));
                    }
                }
            }
        }

        if need_commented_code {
            let mut local = Vec::new();
            check_commented_out_code(&parsed, &mut local);
            for (pos, msg) in local {
                if let Some(mapped) = code::remap_reparsed_pos(pass.fset(), file.pos(), &re_fset, Pos(pos as i64))
                        .map(|p| p.0 as u32) {
                    pending.push((mapped, msg));
                }
            }
        }

        if need_commented_import {
            let mut local = Vec::new();
            check_commented_out_import(&parsed, &mut local);
            for (pos, msg) in local {
                if let Some(mapped) = code::remap_reparsed_pos(pass.fset(), file.pos(), &re_fset, Pos(pos as i64))
                        .map(|p| p.0 as u32) {
                    pending.push((mapped, msg));
                }
            }
        }

        if need_todo {
            for cg in &parsed.comments {
                let mut local = Vec::new();
                check_todo_comment_without_detail(cg, &mut local);
                for (pos, msg) in local {
                    if let Some(mapped) = code::remap_reparsed_pos(pass.fset(), file.pos(), &re_fset, Pos(pos as i64))
                        .map(|p| p.0 as u32) {
                        pending.push((mapped, msg));
                    }
                }
            }
        }

        if need_doc_stub {
            let mut local = Vec::new();
            check_doc_stub(&parsed, &mut local);
            for (pos, msg) in local {
                if let Some(mapped) = code::remap_reparsed_pos(pass.fset(), file.pos(), &re_fset, Pos(pos as i64))
                        .map(|p| p.0 as u32) {
                    pending.push((mapped, msg));
                }
            }
        }

        if need_why_nolint {
            for cg in &parsed.comments {
                let mut local = Vec::new();
                check_why_no_lint(cg, &mut local);
                for (pos, msg) in local {
                    if let Some(mapped) = code::remap_reparsed_pos(pass.fset(), file.pos(), &re_fset, Pos(pos as i64))
                        .map(|p| p.0 as u32) {
                        pending.push((mapped, msg));
                    }
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
                        "commentedOutImport",
                        format!("remove commented-out \"{path}\" import"),
                    );
                }
            }
        }
    }
}

fn function_for_local_comment<'a>(file: &'a File, cg: &CommentGroup) -> Option<&'a FuncDecl> {
    file.decls.iter().find_map(|decl| {
        let Decl::FuncDecl(f) = decl else {
            return None;
        };
        let body = f.body.as_ref()?;
        if body.lbrace.0 < cg.pos().0 && cg.end().0 < body.rbrace.0 {
            Some(f)
        } else {
            None
        }
    })
}

fn is_commented_out_code_permitted_expr(expr: &Expr) -> bool {
    match expr {
        Expr::CallExpr(_) => false,
        Expr::UnaryExpr(u) => u.op != Token::ARROW,
        _ => true,
    }
}

fn is_commented_out_code_permitted_stmt(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::ExprStmt(s) => is_commented_out_code_permitted_expr(&s.x),
        Stmt::LabeledStmt(s) => is_commented_out_code_permitted_stmt(&s.stmt),
        Stmt::DeclStmt(s) => match &s.decl {
            Decl::GenDecl(g) => g.tok == Some(Token::TYPE),
            _ => false,
        },
        Stmt::EmptyStmt(_) => true,
        _ => false,
    }
}

fn parsed_commented_out_code_stmts(text: &str) -> Option<Vec<Stmt>> {
    let src = format!("package p\nfunc _() {{\n{text}\n}}\n");
    let fset = FileSet::new();
    let parsed = parse_file(
        &fset,
        "commented_out_code.go",
        src.as_bytes(),
        guff::parser::Mode::NONE,
    )
    .ok()?;
    let decl = parsed.decls.into_iter().find_map(|decl| match decl {
        Decl::FuncDecl(f) => Some(f),
        _ => None,
    })?;
    decl.body
        .map(|body| body.list)
        .filter(|stmts| !stmts.is_empty())
}

fn check_commented_out_code(file: &File, pending: &mut Vec<(u32, String)>) {
    static NOT_QUITE_FUNC_CALL_RE: OnceLock<Regex> = OnceLock::new();
    let not_quite_func_call = NOT_QUITE_FUNC_CALL_RE
        .get_or_init(|| Regex::new(r"\w+\s+\([^)]*\)\s*$").expect("commentedOutCode call RE"));

    for cg in &file.comments {
        let Some(func) = function_for_local_comment(file, cg) else {
            continue;
        };
        let text = cg.text();
        if text.contains("TODO")
            || text.contains("http://")
            || text.contains("https://")
            || text.contains("e.g. ")
            || (func.name.name.starts_with("Example") && text.contains("Output:"))
        {
            continue;
        }
        if text.chars().count() < 15
            && !text.contains("print")
            && !text.contains("fmt.")
            && !text.contains("log.")
        {
            continue;
        }
        if not_quite_func_call.is_match(&text) {
            continue;
        }
        let Some(stmts) = parsed_commented_out_code_stmts(&text) else {
            continue;
        };
        if stmts.iter().all(is_commented_out_code_permitted_stmt) {
            continue;
        }
        report(
            pending,
            cg.pos().0 as u32,
            "commentedOutCode",
            "may want to remove commented-out code",
        );
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
        bin.x.pos().0 as u32,
        "emptyStringTest",
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
                            "emptyFallthrough",
                            "remove empty case containing only fallthrough to default case",
                        );
                    } else if !cc.list.is_empty() {
                        report(
                            pending,
                            bs.tok_pos.0 as u32,
                            "emptyFallthrough",
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
    report(pending, g.tok_pos.0 as u32, "emptyDecl", msg);
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
        "octalLiteral",
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
                "nilValReturn",
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
        bin.x.pos().0 as u32,
        "yodaStyleExpr",
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
        "deferUnlambda",
        format!("can rewrite as `{rewrite}`"),
    );
}

fn check_init_clause(name: &str, init: Option<&Stmt>, pos: u32, pending: &mut Vec<(u32, String)>) {
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
        "initClause",
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

fn warn_builtin_shadow(ident: &Ident, checker: &str, pending: &mut Vec<(u32, String)>) {
    if is_builtin_name(&ident.name) {
        report(
            pending,
            ident.pos().0 as u32,
            checker,
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
            warn_builtin_shadow(name, "builtinShadow", pending);
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
            warn_builtin_shadow(id, "builtinShadow", pending);
        }
    }
}

fn check_builtin_shadow_value_spec(spec: &ValueSpec, pending: &mut Vec<(u32, String)>) {
    for name in &spec.names {
        warn_builtin_shadow(name, "builtinShadow", pending);
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
            warn_builtin_shadow(&f.name, "builtinShadowDecl", pending);
        }
        Decl::GenDecl(g) => {
            for spec in &g.specs {
                match spec {
                    Spec::ValueSpec(vs) => {
                        for name in &vs.names {
                            warn_builtin_shadow(name, "builtinShadowDecl", pending);
                        }
                    }
                    Spec::TypeSpec(ts) => {
                        warn_builtin_shadow(&ts.name, "builtinShadowDecl", pending)
                    }
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
        by_path.entry(imp.path.value.clone()).or_default().push(imp);
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
            // `ast.ImportSpec.Pos()` is the alias when there is one, so an
            // aliased duplicate reports on the alias, not on the path literal.
            let pos = imp
                .name
                .as_ref()
                .map_or(imp.path.value_pos, |name| name.pos());
            report(
                pending,
                pos.0 as u32,
                "dupImport",
                msg.clone(),
            );
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
                "filepathJoin",
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
        "paramTypeCombine",
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
                "rangeAppendAll",
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
        Expr::KeyValueExpr(kv) => contains_index_of(&kv.key, x) || contains_index_of(&kv.value, x),
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
        bin.x.pos().0 as u32,
        "weakCond",
        format!("suspicious `{whole}`; nil check may not be enough, check for len"),
    );
}

fn signature_of(pass: &Pass<'_>, typ: TypeId) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Signature(_) => Some(typ),
        _ => None,
    }
}

fn is_option_func_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(sig) = signature_of(pass, typ) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let TypeData::Signature(s) = artifacts.types.get(sig) else {
        return false;
    };
    tuple_len(&artifacts.types, s.params()) > 0
}

fn check_dup_option(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    if call.args.is_empty() || call.ellipsis != NO_POS {
        return;
    }
    let Some(fun_ty) = type_of(pass, &call.fun) else {
        return;
    };
    let Some(sig) = signature_of(pass, fun_ty) else {
        return;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let TypeData::Signature(s) = artifacts.types.get(sig) else {
        return;
    };
    if !s.variadic() {
        return;
    }
    let nparams = tuple_len(&artifacts.types, s.params());
    if nparams == 0 {
        return;
    }
    let last = nparams - 1;
    if last > call.args.len() {
        return;
    }
    let last_param = tuple_at(&artifacts.types, s.params().unwrap(), last);
    let Some(last_ty) = last_param.typ(&artifacts.objects) else {
        return;
    };
    let last_ty = unalias_readonly(&artifacts.types, last_ty);
    let TypeData::Slice(slice) = artifacts.types.get(last_ty) else {
        return;
    };
    if !is_option_func_type(pass, slice.elem()) {
        return;
    }
    let mut seen = HashSet::new();
    for arg in &call.args[last..] {
        let Some(code) = expr_text(arg) else {
            continue;
        };
        if !seen.insert(code.clone()) {
            report(
                pending,
                arg.pos().0 as u32,
                "dupOption",
                format!("function argument `{code}` is duplicated"),
            );
        }
    }
}

fn is_type_expr(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    info.types
        .get(&expr.id())
        .is_some_and(|tv| tv.mode == OperandMode::TypeExpr)
}

fn check_method_expr_call(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return;
    };
    if call.args.is_empty() {
        return;
    }
    if matches!(&call.args[0], Expr::Ident(id) if id.name == "nil") {
        return;
    }
    if !is_type_expr(pass, &sel.x) {
        return;
    }
    let Some(fun_t) = expr_text(&call.fun) else {
        return;
    };
    let mut recv = &call.args[0];
    if let Expr::UnaryExpr(u) = recv {
        if u.op == Token::AND {
            recv = &u.x;
        }
    }
    let Some(recv_t) = expr_text(recv) else {
        return;
    };
    report(
        pending,
        call.fun.pos().0 as u32,
        "methodExprCall",
        format!(
            "consider to change `{fun_t}` to `{recv_t}.{}`",
            sel.sel.name
        ),
    );
}

const RANGE_EXPR_COPY_SIZE_THRESHOLD: i64 = 512;

fn check_range_expr_copy(pass: &Pass<'_>, rs: &RangeStmt, pending: &mut Vec<(u32, String)>) {
    if rs.key.is_none() || rs.value.is_none() {
        return;
    }
    let Some(info) = pass.types_info() else {
        return;
    };
    let Some(tv) = info.types.get(&rs.x.id()) else {
        return;
    };
    if tv.mode != OperandMode::Variable {
        return;
    }
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let typ = unalias_readonly(&artifacts.types, tv.typ);
    if !matches!(artifacts.types.get(typ), TypeData::Array(_)) {
        return;
    }
    let sizes = pass.pkg().types_sizes.unwrap_or_else(default_sizes);
    let size = sizes.sizeof(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
    );
    if size < RANGE_EXPR_COPY_SIZE_THRESHOLD {
        return;
    }
    let Some(x_t) = expr_text(&rs.x) else {
        return;
    };
    report(
        pending,
        rs.for_.0 as u32,
        "rangeExprCopy",
        format!("copy of {x_t} ({size} bytes) can be avoided with &{x_t}"),
    );
}

fn domain_pattern_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"[^\\]\.(com|org|info|net|ru|de)\b").expect("domain pattern regex")
    })
}

fn check_regexp_pattern(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    match name.as_str() {
        "regexp.Compile"
        | "regexp.CompilePOSIX"
        | "regexp.MustCompile"
        | "regexp.MustCompilePOSIX"
        | "regexp.MustCompilePosix" => {}
        _ => return,
    }
    if call.args.is_empty() {
        return;
    }
    let Some(pat) = code::expr_to_string(pass, &call.args[0]).or_else(|| match &call.args[0] {
        Expr::BasicLit(b) if b.kind == Some(Token::STRING) => {
            Some(b.value.trim_matches(|c| c == '"' || c == '`').to_string())
        }
        _ => None,
    }) else {
        return;
    };
    if let Some(caps) = domain_pattern_re().captures(&pat) {
        let domain = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        report(
            pending,
            call.args[0].pos().0 as u32,
            "regexpPattern",
            format!("'.{domain}' should probably be '\\.{domain}'"),
        );
    }
}

fn check_bad_regexp(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    match name.as_str() {
        "regexp.Compile" | "regexp.MustCompile" => {}
        _ => return,
    }
    if call.args.is_empty() {
        return;
    }
    let Some(pat) = code::expr_to_string(pass, &call.args[0]).or_else(|| match &call.args[0] {
        Expr::BasicLit(b) if b.kind == Some(Token::STRING) => {
            Some(b.value.trim_matches(|c| c == '"' || c == '`').to_string())
        }
        _ => None,
    }) else {
        return;
    };
    let pos = call.args[0].pos().0 as u32;
    for msg in gocritic_bad_regexp::check_pattern(&pat) {
        report(pending, pos, "badRegexp", msg);
    }
}

fn check_regexp_simplify(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    match name.as_str() {
        "regexp.Compile" | "regexp.MustCompile" => {}
        _ => return,
    }
    if call.args.is_empty() {
        return;
    }
    let Some(pat) = code::expr_to_string(pass, &call.args[0]).or_else(|| match &call.args[0] {
        Expr::BasicLit(b) if b.kind == Some(Token::STRING) => {
            Some(b.value.trim_matches(|c| c == '"' || c == '`').to_string())
        }
        _ => None,
    }) else {
        return;
    };
    if let Some(simplified) = gocritic_regexp_simplify::simplify(&pat) {
        report(
            pending,
            call.args[0].pos().0 as u32,
            "regexpSimplify",
            format!("can re-write `{pat}` as `{simplified}`"),
        );
    }
}

fn side_effect_free_approx(expr: &Expr) -> bool {
    match expr {
        Expr::CallExpr(_) => false,
        Expr::UnaryExpr(u) if u.op == Token::ARROW => false,
        Expr::ParenExpr(p) => side_effect_free_approx(&p.x),
        Expr::UnaryExpr(u) => side_effect_free_approx(&u.x),
        Expr::BinaryExpr(b) => side_effect_free_approx(&b.x) && side_effect_free_approx(&b.y),
        Expr::SelectorExpr(s) => side_effect_free_approx(&s.x),
        Expr::IndexExpr(ix) => side_effect_free_approx(&ix.x) && side_effect_free_approx(&ix.index),
        Expr::SliceExpr(s) => {
            side_effect_free_approx(&s.x)
                && s.low.as_ref().is_none_or(|e| side_effect_free_approx(e))
                && s.high.as_ref().is_none_or(|e| side_effect_free_approx(e))
                && s.max.as_ref().is_none_or(|e| side_effect_free_approx(e))
        }
        Expr::StarExpr(s) => side_effect_free_approx(&s.x),
        Expr::TypeAssertExpr(a) => side_effect_free_approx(&a.x),
        Expr::IndexListExpr(ix) => {
            side_effect_free_approx(&ix.x) && ix.indices.iter().all(side_effect_free_approx)
        }
        Expr::KeyValueExpr(kv) => {
            side_effect_free_approx(&kv.key) && side_effect_free_approx(&kv.value)
        }
        Expr::CompositeLit(lit) => lit.elts.iter().all(side_effect_free_approx),
        _ => true,
    }
}

fn contains_expr(tree: &Expr, needle: &Expr) -> bool {
    if exprs_equal(tree, needle) {
        return true;
    }
    match tree {
        Expr::ParenExpr(p) => contains_expr(&p.x, needle),
        Expr::UnaryExpr(u) => contains_expr(&u.x, needle),
        Expr::BinaryExpr(b) => contains_expr(&b.x, needle) || contains_expr(&b.y, needle),
        Expr::CallExpr(c) => {
            contains_expr(&c.fun, needle) || c.args.iter().any(|a| contains_expr(a, needle))
        }
        Expr::SelectorExpr(s) => contains_expr(&s.x, needle),
        Expr::IndexExpr(ix) => contains_expr(&ix.x, needle) || contains_expr(&ix.index, needle),
        Expr::SliceExpr(s) => {
            contains_expr(&s.x, needle)
                || s.low.as_ref().is_some_and(|e| contains_expr(e, needle))
                || s.high.as_ref().is_some_and(|e| contains_expr(e, needle))
                || s.max.as_ref().is_some_and(|e| contains_expr(e, needle))
        }
        Expr::StarExpr(s) => contains_expr(&s.x, needle),
        Expr::TypeAssertExpr(a) => contains_expr(&a.x, needle),
        Expr::IndexListExpr(ix) => {
            contains_expr(&ix.x, needle) || ix.indices.iter().any(|i| contains_expr(i, needle))
        }
        Expr::KeyValueExpr(kv) => {
            contains_expr(&kv.key, needle) || contains_expr(&kv.value, needle)
        }
        Expr::CompositeLit(lit) => lit.elts.iter().any(|e| contains_expr(e, needle)),
        _ => false,
    }
}

fn contains_index_ident(tree: &Expr, index_name: &str) -> bool {
    match tree {
        Expr::IndexExpr(ix) => {
            matches!(ix.index.as_ref(), Expr::Ident(id) if id.name == index_name)
                || contains_index_ident(&ix.x, index_name)
                || contains_index_ident(&ix.index, index_name)
        }
        Expr::ParenExpr(p) => contains_index_ident(&p.x, index_name),
        Expr::UnaryExpr(u) => contains_index_ident(&u.x, index_name),
        Expr::BinaryExpr(b) => {
            contains_index_ident(&b.x, index_name) || contains_index_ident(&b.y, index_name)
        }
        Expr::CallExpr(c) => {
            contains_index_ident(&c.fun, index_name)
                || c.args.iter().any(|a| contains_index_ident(a, index_name))
        }
        Expr::SelectorExpr(s) => contains_index_ident(&s.x, index_name),
        Expr::SliceExpr(s) => {
            contains_index_ident(&s.x, index_name)
                || s.low
                    .as_ref()
                    .is_some_and(|e| contains_index_ident(e, index_name))
                || s.high
                    .as_ref()
                    .is_some_and(|e| contains_index_ident(e, index_name))
                || s.max
                    .as_ref()
                    .is_some_and(|e| contains_index_ident(e, index_name))
        }
        Expr::StarExpr(s) => contains_index_ident(&s.x, index_name),
        Expr::TypeAssertExpr(a) => contains_index_ident(&a.x, index_name),
        Expr::IndexListExpr(ix) => {
            contains_index_ident(&ix.x, index_name)
                || ix
                    .indices
                    .iter()
                    .any(|i| contains_index_ident(i, index_name))
        }
        Expr::KeyValueExpr(kv) => {
            contains_index_ident(&kv.key, index_name) || contains_index_ident(&kv.value, index_name)
        }
        Expr::CompositeLit(lit) => lit.elts.iter().any(|e| contains_index_ident(e, index_name)),
        _ => false,
    }
}

fn unwrap_slice_arg(expr: &Expr) -> &Expr {
    match unparen(expr) {
        Expr::SliceExpr(s) => unwrap_slice_arg(&s.x),
        other => other,
    }
}

fn sort_less_params(ty: &FuncType) -> Option<(&Ident, &Ident)> {
    let params = ty.params.as_ref()?;
    let mut idents = Vec::new();
    for field in &params.list {
        for name in &field.names {
            idents.push(name);
        }
    }
    if idents.len() == 2 {
        Some((idents[0], idents[1]))
    } else {
        None
    }
}

fn check_sort_slice(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    if call.args.len() != 2 {
        return;
    }
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    if name != "sort.Slice" && name != "sort.SliceStable" {
        return;
    }
    let slice = unwrap_slice_arg(&call.args[0]);
    if !side_effect_free_approx(slice) {
        return;
    }
    let Expr::FuncLit(less) = &call.args[1] else {
        return;
    };
    let Some((ivar, jvar)) = sort_less_params(&less.ty) else {
        return;
    };
    if less.body.list.len() != 1 {
        return;
    }
    let Stmt::ReturnStmt(ret) = &less.body.list[0] else {
        return;
    };
    if ret.results.len() != 1 {
        return;
    }
    let cmp = unparen(&ret.results[0]);
    let Expr::BinaryExpr(bin) = cmp else {
        return;
    };
    if !matches!(bin.op, Token::LSS | Token::LEQ | Token::GTR | Token::GEQ) {
        return;
    }
    if !side_effect_free_approx(cmp) {
        return;
    }
    if !contains_expr(&bin.x, slice) && !contains_expr(&bin.y, slice) {
        let Some(slice_t) = expr_text(slice) else {
            return;
        };
        report(
            pending,
            bin.x.pos().0 as u32,
            "sortSlice",
            format!("cmp func must use {slice_t} slice in comparison"),
        );
    }
    if contains_index_ident(&bin.x, &jvar.name) && contains_index_ident(&bin.y, &ivar.name) {
        report(
            pending,
            bin.x.pos().0 as u32,
            "sortSlice",
            format!(
                "unusual order of {{{},{}}} params in comparison",
                ivar.name, jvar.name
            ),
        );
    }
}

fn type_is_rows_like(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Pointer(p) => type_is_rows_like(pass, p.elem()),
        TypeData::Named(_) => named_obj(&artifacts.types, typ).name(&artifacts.objects) == "Rows",
        _ => false,
    }
}

fn func_is_exec(pass: &Pass<'_>, func_oid: guff_types::arena::ObjectId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let ObjectData::Func(f) = artifacts.objects.get(func_oid) else {
        return false;
    };
    if f.name() != "Exec" {
        return false;
    }
    let Some(sig) = f.typ() else {
        return false;
    };
    let TypeData::Signature(s) = artifacts.types.get(sig) else {
        return false;
    };
    if tuple_len(&artifacts.types, s.results()) != 2 {
        return false;
    }
    let nparams = tuple_len(&artifacts.types, s.params());
    if nparams == 0 {
        return false;
    }
    let first = tuple_at(&artifacts.types, s.params().unwrap(), 0);
    let Some(first_ty) = first.typ(&artifacts.objects) else {
        return false;
    };
    let first_ty = unalias_readonly(&artifacts.types, first_ty);
    matches!(
        artifacts.types.get(first_ty),
        TypeData::Basic(b) if b.kind() == BasicKind::String
    )
}

fn type_has_exec_method(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Pointer(p) => type_has_exec_method(pass, p.elem()),
        TypeData::Named(n) => {
            for i in 0..n.num_methods() {
                if func_is_exec(pass, n.method(i)) {
                    return true;
                }
            }
            if let Some(under) = n.underlying() {
                let under = unalias_readonly(&artifacts.types, under);
                match artifacts.types.get(under) {
                    TypeData::Interface(iface) => {
                        for i in 0..iface.num_explicit_methods() {
                            if func_is_exec(pass, iface.explicit_method(i)) {
                                return true;
                            }
                        }
                    }
                    TypeData::Struct(st) => {
                        for i in 0..st.num_fields() {
                            let oid = st.field(i);
                            let ObjectData::Var(v) = artifacts.objects.get(oid) else {
                                continue;
                            };
                            if v.embedded() && type_has_exec_method(pass, v.typ()) {
                                return true;
                            }
                        }
                    }
                    _ => {}
                }
            }
            false
        }
        TypeData::Interface(iface) => {
            for i in 0..iface.num_explicit_methods() {
                if func_is_exec(pass, iface.explicit_method(i)) {
                    return true;
                }
            }
            false
        }
        TypeData::Struct(st) => {
            for i in 0..st.num_fields() {
                let oid = st.field(i);
                let ObjectData::Var(v) = artifacts.objects.get(oid) else {
                    continue;
                };
                if v.embedded() && type_has_exec_method(pass, v.typ()) {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

fn query_call_is_rows_query(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return false;
    };
    match sel.sel.name.as_str() {
        "Query" | "QueryContext" | "Queryx" | "QueryxContext" => {}
        _ => return false,
    }
    let Some(fun_ty) = type_of(pass, &call.fun) else {
        return false;
    };
    let Some(sig) = signature_of(pass, fun_ty) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let TypeData::Signature(s) = artifacts.types.get(sig) else {
        return false;
    };
    if tuple_len(&artifacts.types, s.results()) != 2 {
        return false;
    }
    let first = tuple_at(&artifacts.types, s.results().unwrap(), 0);
    let Some(first_ty) = first.typ(&artifacts.objects) else {
        return false;
    };
    type_is_rows_like(pass, first_ty)
}

fn check_sql_query(pass: &Pass<'_>, assign: &AssignStmt, pending: &mut Vec<(u32, String)>) {
    if assign.lhs.len() != 2 || assign.rhs.len() != 1 {
        return;
    }
    let Expr::Ident(first) = &assign.lhs[0] else {
        return;
    };
    if first.name != "_" {
        return;
    }
    let Expr::CallExpr(call) = &assign.rhs[0] else {
        return;
    };
    if !query_call_is_rows_query(pass, call) {
        return;
    }
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return;
    };
    let Some(recv_ty) = type_of(pass, &sel.x) else {
        return;
    };
    let Some(recv_t) = expr_text(&sel.x) else {
        return;
    };
    if type_has_exec_method(pass, recv_ty) {
        report(
            pending,
            sel.x.pos().0 as u32,
            "sqlQuery",
            format!("use {recv_t}.Exec() if returned result is not needed"),
        );
    } else {
        report(
            pending,
            sel.x.pos().0 as u32,
            "sqlQuery",
            "ignoring Query() rows result may lead to a connection leak",
        );
    }
}

fn type_assert_from_if(stmt: &IfStmt) -> Option<&TypeAssertExpr> {
    let init = stmt.init.as_ref()?;
    let Stmt::AssignStmt(assign) = init.as_ref() else {
        return None;
    };
    if assign.tok != Some(Token::DEFINE) || assign.lhs.len() != 2 || assign.rhs.len() != 1 {
        return None;
    }
    if !matches!(&assign.lhs[0], Expr::Ident(_)) {
        return None;
    }
    if !exprs_equal(&assign.lhs[1], &stmt.cond) {
        return None;
    }
    match &assign.rhs[0] {
        Expr::TypeAssertExpr(a) => Some(a),
        _ => None,
    }
}

fn count_type_assert_chain(stmt: &IfStmt, assertion: &TypeAssertExpr) -> usize {
    let mut seen_types = HashSet::new();
    let Some(first_ty) = assertion.ty.as_ref().and_then(|t| expr_text(t)) else {
        return 0;
    };
    seen_types.insert(first_ty);
    let x = &assertion.x;
    let mut count = 1;
    let mut cur = stmt;
    loop {
        let Some(Stmt::IfStmt(else_if)) = cur.else_.as_deref() else {
            return count;
        };
        let Some(next) = type_assert_from_if(else_if) else {
            return count;
        };
        let Some(ty_t) = next.ty.as_ref().and_then(|t| expr_text(t)) else {
            return count;
        };
        if !seen_types.insert(ty_t) {
            return 0;
        }
        if !exprs_equal(x, &next.x) {
            return 0;
        }
        count += 1;
        cur = else_if;
    }
}

fn check_type_assert_chain(
    stmt: &IfStmt,
    visited: &mut HashSet<usize>,
    pending: &mut Vec<(u32, String)>,
) {
    let key = stmt as *const _ as usize;
    if !visited.insert(key) {
        return;
    }
    let Some(assertion) = type_assert_from_if(stmt) else {
        return;
    };
    if count_type_assert_chain(stmt, assertion) >= 2 {
        // Mark nested else-ifs visited so we only warn once at the chain head.
        let mut cur = stmt;
        while let Some(Stmt::IfStmt(else_if)) = cur.else_.as_deref() {
            visited.insert(else_if as *const _ as usize);
            cur = else_if;
        }
        report(
            pending,
            stmt.if_.0 as u32,
            "typeAssertChain",
            "rewrite if-else to type switch statement",
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

const NESTING_REDUCE_BODY_WIDTH: usize = 5;

fn trunc_cast_name(expr: &Expr) -> Option<&str> {
    let Expr::CallExpr(call) = expr else {
        return None;
    };
    let Expr::Ident(id) = call.fun.as_ref() else {
        return None;
    };
    match id.name.as_str() {
        "int8" | "int16" | "int32" | "uint8" | "uint16" | "uint32" => Some(id.name.as_str()),
        _ => None,
    }
}

fn check_truncate_cmp(pass: &Pass<'_>, bin: &BinaryExpr, pending: &mut Vec<(u32, String)>) {
    match bin.op {
        Token::LSS | Token::GTR | Token::LEQ | Token::GEQ | Token::EQL | Token::NEQ => {}
        _ => return,
    }
    if matches!(bin.x.as_ref(), Expr::BasicLit(_)) || matches!(bin.y.as_ref(), Expr::BasicLit(_)) {
        return;
    }
    let left = trunc_cast_name(bin.x.as_ref()).is_some();
    let right = trunc_cast_name(bin.y.as_ref()).is_some();
    match (left, right) {
        (true, false) => check_truncate_cmp_side(pass, bin.x.as_ref(), bin.y.as_ref(), pending),
        (false, true) => check_truncate_cmp_side(pass, bin.y.as_ref(), bin.x.as_ref(), pending),
        _ => {}
    }
}

fn check_truncate_cmp_side(
    pass: &Pass<'_>,
    cast_expr: &Expr,
    other: &Expr,
    pending: &mut Vec<(u32, String)>,
) {
    let Expr::CallExpr(xcast) = cast_expr else {
        return;
    };
    if xcast.args.len() != 1 {
        return;
    }
    let x = &xcast.args[0];
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let Some(x_typ) = type_of(pass, x) else {
        return;
    };
    let Some(y_typ) = type_of(pass, other) else {
        return;
    };
    let x_under = unalias_readonly(&artifacts.types, x_typ).underlying(&artifacts.types);
    let y_under = unalias_readonly(&artifacts.types, y_typ).underlying(&artifacts.types);
    let TypeData::Basic(xb) = artifacts.types.get(x_under) else {
        return;
    };
    let TypeData::Basic(yb) = artifacts.types.get(y_under) else {
        return;
    };
    if !xb.info().contains(IS_INTEGER) || xb.info().0 != yb.info().0 {
        return;
    }
    let sizes = pass.pkg().types_sizes.unwrap_or_else(default_sizes);
    let xsize = sizes.sizeof(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        x_under,
    );
    let ysize = sizes.sizeof(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        y_under,
    );
    if xsize <= ysize {
        return;
    }
    // skipArchDependent=true (golangci / go-critic default)
    match basic_kind(&artifacts.types, x_under) {
        BasicKind::Int | BasicKind::Uint | BasicKind::Uintptr => return,
        _ => {}
    }
    let suggest = type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        x_under,
        None,
    );
    report(
        pending,
        cast_expr.pos().0 as u32,
        "truncateCmp",
        format!(
            "truncation in comparison {}->{} bit; cast the other operand to {suggest} instead",
            xsize * 8,
            ysize * 8
        ),
    );
}

fn receiver_type_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::StarExpr(s) => receiver_type_name(&s.x),
        Expr::Ident(id) => Some(id.name.as_str()),
        Expr::IndexExpr(ix) => receiver_type_name(&ix.x),
        Expr::IndexListExpr(ix) => receiver_type_name(&ix.x),
        _ => None,
    }
}

fn check_type_def_first(file: &File, pending: &mut Vec<(u32, String)>) {
    let mut tracked: HashSet<String> = HashSet::new();
    for decl in &file.decls {
        match decl {
            Decl::FuncDecl(f) => {
                let Some(recv) = &f.recv else {
                    continue;
                };
                let Some(field) = recv.list.first() else {
                    continue;
                };
                let Some(ty) = &field.ty else {
                    continue;
                };
                if let Some(name) = receiver_type_name(ty) {
                    tracked.insert(name.to_string());
                }
            }
            Decl::GenDecl(g) if g.tok == Some(Token::TYPE) => {
                for spec in &g.specs {
                    let Spec::TypeSpec(ts) = spec else {
                        continue;
                    };
                    if tracked.contains(&ts.name.name) {
                        report(
                            pending,
                            g.tok_pos.0 as u32,
                            "typeDefFirst",
                            format!(
                                "definition of type '{}' should appear before its methods",
                                ts.name.name
                            ),
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

fn check_defer_in_loop_func(f: &FuncDecl, pending: &mut Vec<(u32, String)>) {
    let Some(body) = &f.body else {
        return;
    };
    walk_defer_in_loop_stmts(&body.list, false, pending);
}

fn walk_defer_in_loop_stmts(stmts: &[Stmt], in_for: bool, pending: &mut Vec<(u32, String)>) {
    for stmt in stmts {
        match stmt {
            Stmt::DeferStmt(d) if in_for => {
                report(
                    pending,
                    d.defer_.0 as u32,
                    "deferInLoop",
                    "Possible resource leak, 'defer' is called in the 'for' loop",
                );
            }
            Stmt::ForStmt(fs) => {
                walk_defer_in_loop_stmts(&fs.body.list, true, pending);
            }
            Stmt::RangeStmt(rs) => {
                walk_defer_in_loop_stmts(&rs.body.list, true, pending);
            }
            Stmt::BlockStmt(b) => walk_defer_in_loop_stmts(&b.list, in_for, pending),
            Stmt::IfStmt(s) => {
                walk_defer_in_loop_stmts(&s.body.list, in_for, pending);
                if let Some(els) = &s.else_ {
                    walk_defer_in_loop_stmts(std::slice::from_ref(els.as_ref()), in_for, pending);
                }
            }
            Stmt::SwitchStmt(s) => walk_defer_in_loop_stmts(&s.body.list, in_for, pending),
            Stmt::TypeSwitchStmt(s) => walk_defer_in_loop_stmts(&s.body.list, in_for, pending),
            Stmt::SelectStmt(s) => walk_defer_in_loop_stmts(&s.body.list, in_for, pending),
            Stmt::CaseClause(c) => walk_defer_in_loop_stmts(&c.body, in_for, pending),
            Stmt::CommClause(c) => walk_defer_in_loop_stmts(&c.body, in_for, pending),
            Stmt::GoStmt(g) => {
                if let Expr::FuncLit(fl) = g.call.fun.as_ref() {
                    walk_defer_in_loop_stmts(&fl.body.list, false, pending);
                }
            }
            Stmt::DeferStmt(d) => {
                if let Expr::FuncLit(fl) = d.call.fun.as_ref() {
                    walk_defer_in_loop_stmts(&fl.body.list, false, pending);
                }
            }
            Stmt::ExprStmt(es) => {
                if let Expr::FuncLit(fl) = &es.x {
                    walk_defer_in_loop_stmts(&fl.body.list, false, pending);
                }
            }
            Stmt::AssignStmt(a) => {
                for rhs in &a.rhs {
                    if let Expr::FuncLit(fl) = rhs {
                        walk_defer_in_loop_stmts(&fl.body.list, false, pending);
                    }
                }
            }
            _ => {}
        }
    }
}

fn check_hex_literal(lit: &BasicLit, pending: &mut Vec<(u32, String)>) {
    if lit.kind != Some(Token::INT) || lit.value.len() < 3 {
        return;
    }
    if lit.value.starts_with("0X") {
        let suggest = format!("0x{}", &lit.value[2..]);
        report(
            pending,
            lit.pos().0 as u32,
            "hexLiteral",
            format!("prefer 0x over 0X, s/{}/{suggest}/", lit.value),
        );
        return;
    }
    if !lit.value.starts_with("0x") {
        return;
    }
    let digits = &lit.value[2..];
    let lower = digits.to_ascii_lowercase();
    let upper = digits.to_ascii_uppercase();
    if digits != lower.as_str() && digits != upper.as_str() {
        report(
            pending,
            lit.pos().0 as u32,
            "hexLiteral",
            "don't mix hex literal letter digits casing",
        );
    }
}

fn check_nesting_reduce_for(body: &BlockStmt, pending: &mut Vec<(u32, String)>) {
    if body.list.len() != 1 {
        return;
    }
    let Stmt::IfStmt(ifs) = &body.list[0] else {
        return;
    };
    if ifs.else_.is_some() {
        return;
    }
    if ifs.body.list.len() >= NESTING_REDUCE_BODY_WIDTH {
        report(
            pending,
            ifs.if_.0 as u32,
            "nestingReduce",
            "invert if cond, replace body with `continue`, move old body after the statement",
        );
    }
}

fn check_todo_comment_without_detail(cg: &CommentGroup, pending: &mut Vec<(u32, String)>) {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"^(?://|/\*)?\s*(TODO|FIX|FIXME|BUG)\s*(?:\*/)?$").expect("todo regex")
    });
    for c in &cg.list {
        if re.is_match(&c.text) {
            report(
                pending,
                c.pos().0 as u32,
                "todoCommentWithoutDetail",
                "may want to add detail/assignee to this TODO/FIXME/BUG comment",
            );
            break;
        }
    }
}

fn check_doc_stub(file: &File, pending: &mut Vec<(u32, String)>) {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)^\.\.\.$|^\.$|^xxx\.?$|^whatever\.?$").expect("doc stub regex")
    });
    for decl in &file.decls {
        match decl {
            Decl::FuncDecl(f) => {
                // Upstream warns on the declaration node: `ast.FuncDecl.Pos()`
                // is the `func` keyword. (`ast.TypeSpec.Pos()` below is the
                // name, so the two arms legitimately differ.)
                visit_doc_stub(
                    f.ty.pos().0 as u32,
                    &f.name.name,
                    f.doc.as_ref(),
                    false,
                    re,
                    pending,
                );
            }
            Decl::GenDecl(g) if g.tok == Some(Token::TYPE) => {
                if g.specs.len() == 1 {
                    if let Spec::TypeSpec(ts) = &g.specs[0] {
                        visit_doc_stub(
                            ts.name.pos().0 as u32,
                            &ts.name.name,
                            g.doc.as_ref().or(ts.doc.as_ref()),
                            true,
                            re,
                            pending,
                        );
                    }
                }
                for spec in &g.specs {
                    let Spec::TypeSpec(ts) = spec else {
                        continue;
                    };
                    visit_doc_stub(
                        ts.name.pos().0 as u32,
                        &ts.name.name,
                        ts.doc.as_ref(),
                        true,
                        re,
                        pending,
                    );
                }
            }
            _ => {}
        }
    }
}

fn visit_doc_stub(
    pos: u32,
    sym: &str,
    doc: Option<&CommentGroup>,
    article: bool,
    re: &Regex,
    pending: &mut Vec<(u32, String)>,
) {
    if !is_exported(sym) {
        return;
    }
    let Some(doc) = doc else {
        return;
    };
    let Some(first) = doc.list.first() else {
        return;
    };
    let Some(rest) = first.text.strip_prefix("//") else {
        return;
    };
    let mut line = rest.trim();
    if article {
        for a in ["The ", "An ", "A "] {
            if let Some(stripped) = line.strip_prefix(a) {
                line = stripped;
                break;
            }
        }
    }
    let Some(after_name) = line.strip_prefix(sym) else {
        return;
    };
    let after = after_name.trim();
    if re.is_match(after) {
        report(
            pending,
            pos,
            "docStub",
            "silencing go lint doc-comment warnings is unadvised",
        );
    }
}

fn block_has_definitions(block: &BlockStmt) -> bool {
    for stmt in &block.list {
        match stmt {
            Stmt::AssignStmt(a) if a.tok == Some(Token::DEFINE) => return true,
            Stmt::DeclStmt(d) => {
                if let Decl::GenDecl(g) = &d.decl {
                    if !g.specs.is_empty() {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn check_unnecessary_block_in_list(stmts: &[Stmt], pending: &mut Vec<(u32, String)>) {
    for stmt in stmts {
        if let Stmt::BlockStmt(b) = stmt {
            if !block_has_definitions(b) {
                report(
                    pending,
                    b.lbrace.0 as u32,
                    "unnecessaryBlock",
                    "block doesn't have definitions, can be simply deleted",
                );
            }
        }
    }
}

fn check_unnecessary_block_case(body: &[Stmt], pending: &mut Vec<(u32, String)>) {
    if body.len() == 1 {
        if let Stmt::BlockStmt(b) = &body[0] {
            report(
                pending,
                b.lbrace.0 as u32,
                "unnecessaryBlock",
                "case statement doesn't require a block statement",
            );
            return;
        }
    }
    check_unnecessary_block_in_list(body, pending);
}

fn check_sloppy_reassign(ifs: &IfStmt, pending: &mut Vec<(u32, String)>) {
    let Some(init) = &ifs.init else {
        return;
    };
    let Stmt::AssignStmt(assign) = init.as_ref() else {
        return;
    };
    if assign.tok != Some(Token::ASSIGN) {
        return;
    }
    if assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return;
    }
    if ifs.body.list.len() != 1 {
        return;
    }
    let Expr::Ident(re_assigned) = &assign.lhs[0] else {
        return;
    };
    let cond_ok = match &ifs.cond {
        Expr::BinaryExpr(b)
            if b.op == Token::NEQ
                && matches!(b.x.as_ref(), Expr::Ident(id) if id.name == re_assigned.name)
                && matches!(b.y.as_ref(), Expr::Ident(id) if id.name == "nil") =>
        {
            true
        }
        _ => false,
    };
    if !cond_ok {
        return;
    }
    let Stmt::ReturnStmt(ret) = &ifs.body.list[0] else {
        return;
    };
    let returns_err = ret
        .results
        .iter()
        .any(|r| matches!(r, Expr::Ident(id) if id.name == re_assigned.name));
    if !returns_err {
        return;
    }
    let Some(rhs) = expr_text(&assign.rhs[0]) else {
        return;
    };
    report(
        pending,
        assign_pos(assign),
        "sloppyReassign",
        format!(
            "re-assignment to `{}` can be replaced with `{} := {rhs}`",
            re_assigned.name, re_assigned.name
        ),
    );
}

fn is_slice_type_named(expr: &Expr, elt: &str) -> bool {
    let Expr::ArrayType(at) = expr else {
        return false;
    };
    if at.len.is_some() {
        return false;
    }
    matches!(at.elt.as_ref(), Expr::Ident(id) if id.name == elt)
}

fn is_string_conv(expr: &Expr) -> Option<&Expr> {
    let Expr::CallExpr(call) = expr else {
        return None;
    };
    if call.args.len() != 1 {
        return None;
    }
    match call.fun.as_ref() {
        Expr::Ident(id) if id.name == "string" => Some(&call.args[0]),
        _ => None,
    }
}

fn is_byte_slice_conv(expr: &Expr) -> Option<&Expr> {
    let Expr::CallExpr(call) = expr else {
        return None;
    };
    if call.args.len() != 1 {
        return None;
    }
    if is_slice_type_named(&call.fun, "byte") || is_slice_type_named(&call.fun, "uint8") {
        Some(&call.args[0])
    } else {
        None
    }
}

fn is_rune_slice_conv(expr: &Expr) -> Option<&Expr> {
    let Expr::CallExpr(call) = expr else {
        return None;
    };
    if call.args.len() != 1 {
        return None;
    }
    if is_slice_type_named(&call.fun, "rune") {
        Some(&call.args[0])
    } else {
        None
    }
}

fn is_nil_ident(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(id) if id.name == "nil")
}

fn is_os_path_separator_string_conv(expr: &Expr) -> bool {
    let Expr::CallExpr(call) = expr else {
        return false;
    };
    if call.args.len() != 1 {
        return false;
    }
    let Expr::Ident(fun) = call.fun.as_ref() else {
        return false;
    };
    if fun.name != "string" {
        return false;
    }
    matches!(
        call_qualified_name_of_expr(&call.args[0]).as_deref(),
        Some("os.PathSeparator")
    )
}

fn is_byte_slice_typed(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let TypeData::Slice(slice) = artifacts.types.get(typ) else {
        return false;
    };
    let elem = unalias_readonly(&artifacts.types, slice.elem());
    match artifacts.types.get(elem) {
        TypeData::Basic(b) => matches!(b.kind(), BasicKind::Uint8 | BasicKind::UntypedInt),
        _ => false,
    }
}

/// go-critic `stringXbytes` Where: `m["re"].Type.Is("*regexp.Regexp")` (stdlib only).
fn is_stdlib_regexp_recv(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let rendered = type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    );
    rendered == "*regexp.Regexp"
}

fn check_http_no_body(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    let nil_idx = match name.as_str() {
        "http.NewRequest" | "net/http.NewRequest" if call.args.len() == 3 => 2,
        "http.NewRequestWithContext" | "net/http.NewRequestWithContext" if call.args.len() == 4 => {
            3
        }
        "httptest.NewRequest" | "net/http/httptest.NewRequest" if call.args.len() == 3 => 2,
        _ => return,
    };
    if !is_nil_ident(&call.args[nil_idx]) && !code::is_nil(pass, &call.args[nil_idx]) {
        return;
    }
    report(
        pending,
        call.fun.pos().0 as u32,
        "httpNoBody",
        "http.NoBody should be preferred to the nil request body",
    );
}

fn check_prefer_decode_rune(pass: &Pass<'_>, ix: &IndexExpr, pending: &mut Vec<(u32, String)>) {
    if !is_int_lit(&ix.index, 0) {
        return;
    }
    let Some(s) = is_rune_slice_conv(&ix.x) else {
        return;
    };
    if !is_string_typed(pass, s) {
        // AST fallback: still report when conversion is clearly []rune(stringish)
        if !matches!(
            s,
            Expr::Ident(_) | Expr::BasicLit(_) | Expr::SelectorExpr(_)
        ) {
            return;
        }
    }
    let Some(s_t) = expr_text(s) else {
        return;
    };
    report(
        pending,
        ix.x.pos().0 as u32,
        "preferDecodeRune",
        format!("consider replacing []rune({s_t})[0] with utf8.DecodeRuneInString({s_t})"),
    );
}

fn check_prefer_write_byte(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return;
    };
    if sel.sel.name != "WriteRune" || call.args.len() != 1 {
        return;
    }
    let Expr::BasicLit(lit) = &call.args[0] else {
        return;
    };
    if lit.kind != Some(Token::CHAR) {
        return;
    }
    let value = make_from_literal(&lit.value, Token::CHAR, 0);
    let (rune, exact) = int64_val(&value);
    if !exact || !(0..0x80).contains(&rune) {
        return;
    }

    let Some(receiver_type) = type_of(pass, &sel.x) else {
        return;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let mut types = artifacts.types.clone();
    let LookupResult::Found { obj, .. } = lookup_field_or_method(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        receiver_type,
        false, // method set of declared type (Implements io.ByteWriter)
        None,
        "WriteByte",
    ) else {
        return;
    };
    let ObjectData::Func(method) = artifacts.objects.get(obj) else {
        return;
    };
    let Some(method_type) = method.typ() else {
        return;
    };
    let TypeData::Signature(sig) = artifacts.types.get(method_type) else {
        return;
    };
    if tuple_len(&artifacts.types, sig.params()) != 1
        || tuple_len(&artifacts.types, sig.results()) != 1
    {
        return;
    }
    let param = tuple_at(&artifacts.types, sig.params().unwrap(), 0);
    let result = tuple_at(&artifacts.types, sig.results().unwrap(), 0);
    let Some(param_type) = param.typ(&artifacts.objects) else {
        return;
    };
    let Some(result_type) = result.typ(&artifacts.objects) else {
        return;
    };
    if basic_kind(&artifacts.types, param_type) != BasicKind::Uint8
        || type_string(
            &artifacts.types,
            &artifacts.objects,
            &artifacts.packages,
            result_type,
            None,
        ) != "error"
    {
        return;
    }

    let receiver = expr_text(&sel.x).unwrap_or_else(|| "writer".to_string());
    let rune = expr_text(&call.args[0]).unwrap_or_else(|| lit.value.clone());
    report(
        pending,
        call.fun.pos().0 as u32,
        "preferWriteByte",
        format!("consider writing single byte rune {rune} with {receiver}.WriteByte({rune})"),
    );
}

fn check_index_alloc(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    if name != "strings.Index" && !name.ends_with("/strings.Index") {
        return;
    }
    if call.args.len() != 2 {
        return;
    }
    let Some(x) = is_string_conv(&call.args[0]) else {
        return;
    };
    let Some(x_t) = node_text(pass, x) else {
        return;
    };
    let Some(y_t) = node_text(pass, &call.args[1]) else {
        return;
    };
    // `Report("consider replacing $$ with bytes.Index($x, []byte($y))")`.
    let Some(whole) = call_text(pass, call) else {
        return;
    };
    report(
        pending,
        call.fun.pos().0 as u32,
        "indexAlloc",
        format!("consider replacing {whole} with bytes.Index({x_t}, []byte({y_t}))"),
    );
}

fn check_string_xbytes(pass: &Pass<'_>, n: NodeRef<'_>, pending: &mut Vec<(u32, String)>) {
    match n {
        NodeRef::CallExpr(call) => {
            // copy(_, []byte(s))
            let is_copy = match call.fun.as_ref() {
                Expr::Ident(id) => id.name == "copy",
                _ => false,
            };
            if is_copy && call.args.len() == 2 {
                if let Some(s) = is_byte_slice_conv(&call.args[1]) {
                    // Report("can simplify `[]byte($s)` to `$s`")
                    if let Some(s_t) = node_text(pass, s) {
                        report(
                            pending,
                            call.fun.pos().0 as u32,
                            "stringXbytes",
                            format!("can simplify `[]byte({s_t})` to `{s_t}`"),
                        );
                    }
                }
            }
            // len(string(b))
            if matches!(call.fun.as_ref(), Expr::Ident(id) if id.name == "len")
                && call.args.len() == 1
            {
                if let Some(b) = is_string_conv(&call.args[0]) {
                    if is_byte_slice_typed(pass, b)
                        || matches!(b, Expr::Ident(_) | Expr::SelectorExpr(_))
                    {
                        // Suggest(`len($b)`), no Report — golangci renders a
                        // suggestion-only rule as `suggestion: <replacement>`.
                        if let Some(b_t) = node_text(pass, b) {
                            report(
                                pending,
                                call.fun.pos().0 as u32,
                                "stringXbytes",
                                format!("suggestion: len({b_t})"),
                            );
                        }
                    }
                }
            }
            // $re.Match([]byte($s)) and friends — go-critic requires
            // `*regexp.Regexp` (stdlib). Forks like github.com/grafana/regexp
            // share the API but must not match.
            if let Expr::SelectorExpr(sel) = call.fun.as_ref() {
                if matches!(
                    sel.sel.name.as_str(),
                    "Match" | "FindIndex" | "FindAllIndex"
                ) && !call.args.is_empty()
                {
                    if let Some(s) = is_byte_slice_conv(&call.args[0]) {
                        if is_stdlib_regexp_recv(pass, &sel.x) {
                            let method = match sel.sel.name.as_str() {
                                "Match" => "MatchString",
                                "FindIndex" => "FindStringIndex",
                                "FindAllIndex" => "FindAllStringIndex",
                                _ => unreachable!(),
                            };
                            // Suggest(`$re.<method>($s)`) — FindAllIndex also
                            // carries the trailing `$n` operand.
                            let rest: Option<Vec<String>> =
                                call.args[1..].iter().map(|a| node_text(pass, a)).collect();
                            if let (Some(re_t), Some(s_t), Some(rest)) =
                                (node_text(pass, &sel.x), node_text(pass, s), rest)
                            {
                                let mut args = vec![s_t];
                                args.extend(rest);
                                report(
                                    pending,
                                    call.args[0].pos().0 as u32,
                                    "stringXbytes",
                                    format!("suggestion: {re_t}.{method}({})", args.join(", ")),
                                );
                            }
                        }
                    }
                }
            }
        }
        NodeRef::BinaryExpr(bin) => {
            // string(b) == "" / != ""
            if matches!(bin.op, Token::EQL | Token::NEQ) {
                if let (Some(b), true) = (is_string_conv(&bin.x), is_string_lit_empty(&bin.y)) {
                    if is_byte_slice_typed(pass, b)
                        || matches!(b, Expr::Ident(_) | Expr::SelectorExpr(_))
                    {
                        let op = if bin.op == Token::EQL { "==" } else { "!=" };
                        // Suggest(`len($b) == 0`) / Suggest(`len($b) != 0`).
                        if let Some(b_t) = node_text(pass, b) {
                            report(
                                pending,
                                bin.x.pos().0 as u32,
                                "stringXbytes",
                                format!("suggestion: len({b_t}) {op} 0"),
                            );
                        }
                    }
                }
            }
            // string(x) == string(y) for []byte
            if matches!(bin.op, Token::EQL | Token::NEQ) {
                if let (Some(x), Some(y)) = (is_string_conv(&bin.x), is_string_conv(&bin.y)) {
                    let both_bytes = (is_byte_slice_typed(pass, x)
                        || matches!(x, Expr::Ident(_) | Expr::SelectorExpr(_)))
                        && (is_byte_slice_typed(pass, y)
                            || matches!(y, Expr::Ident(_) | Expr::SelectorExpr(_)));
                    if both_bytes {
                        // Suggest(`bytes.Equal($x, $y)`) / `!bytes.Equal(...)`.
                        let bang = if bin.op == Token::EQL { "" } else { "!" };
                        if let (Some(x_t), Some(y_t)) = (node_text(pass, x), node_text(pass, y)) {
                            report(
                                pending,
                                bin.x.pos().0 as u32,
                                "stringXbytes",
                                format!("suggestion: {bang}bytes.Equal({x_t}, {y_t})"),
                            );
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn is_string_lit_empty(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::BasicLit(lit)
            if lit.kind == Some(Token::STRING) && (lit.value == "\"\"" || lit.value == "``")
    )
}

fn check_prefer_filepath_join(pass: &Pass<'_>, bin: &BinaryExpr, pending: &mut Vec<(u32, String)>) {
    if bin.op != Token::ADD {
        return;
    }
    let Expr::BinaryExpr(left) = bin.x.as_ref() else {
        return;
    };
    if left.op != Token::ADD {
        return;
    }
    if !is_os_path_separator_string_conv(&left.y) {
        return;
    }
    if !is_string_typed(pass, &left.x)
        && !matches!(
            left.x.as_ref(),
            Expr::Ident(_) | Expr::BasicLit(_) | Expr::SelectorExpr(_)
        )
    {
        return;
    }
    if !is_string_typed(pass, &bin.y)
        && !matches!(
            bin.y.as_ref(),
            Expr::Ident(_) | Expr::BasicLit(_) | Expr::SelectorExpr(_)
        )
    {
        return;
    }
    let Some(x_t) = node_text(pass, &left.x) else {
        return;
    };
    let Some(y_t) = node_text(pass, &bin.y) else {
        return;
    };
    // Report("filepath.Join($x, $y) should be preferred to the $$").
    let Some(whole) = node_text(pass, &Expr::BinaryExpr(bin.clone())) else {
        return;
    };
    report(
        pending,
        bin.x.pos().0 as u32,
        "preferFilepathJoin",
        format!("filepath.Join({x_t}, {y_t}) should be preferred to the {whole}"),
    );
}

fn check_strings_compare(pass: &Pass<'_>, bin: &BinaryExpr, pending: &mut Vec<(u32, String)>) {
    let call = match bin.x.as_ref() {
        Expr::CallExpr(c) => c,
        _ => return,
    };
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    if name != "strings.Compare" && !name.ends_with("/strings.Compare") {
        return;
    }
    if call.args.len() != 2 {
        return;
    }
    let Some(s1) = node_text(pass, &call.args[0]) else {
        return;
    };
    let Some(s2) = node_text(pass, &call.args[1]) else {
        return;
    };
    let suggest = match bin.op {
        Token::EQL if is_int_lit(&bin.y, 0) => format!("{s1} == {s2}"),
        Token::EQL if is_int_lit(&bin.y, -1) => format!("{s1} < {s2}"),
        Token::EQL if is_int_lit(&bin.y, 1) => format!("{s1} > {s2}"),
        Token::LSS if is_int_lit(&bin.y, 0) => format!("{s1} < {s2}"),
        Token::GTR if is_int_lit(&bin.y, 0) => format!("{s1} > {s2}"),
        _ => return,
    };
    // Every arm is `Suggest`-only upstream.
    report(
        pending,
        bin.x.pos().0 as u32,
        "stringsCompare",
        format!("suggestion: {suggest}"),
    );
}

fn is_zero_byte_composite(expr: &Expr) -> bool {
    let Expr::CompositeLit(lit) = expr else {
        return false;
    };
    if lit.elts.len() != 1 {
        return false;
    }
    let ty_ok = lit
        .ty
        .as_ref()
        .map(|t| is_slice_type_named(t, "byte") || is_slice_type_named(t, "uint8"))
        .unwrap_or(true);
    ty_ok && is_int_lit(&lit.elts[0], 0)
}

fn check_zero_byte_repeat(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    if name != "bytes.Repeat" && !name.ends_with("/bytes.Repeat") {
        return;
    }
    if call.args.len() != 2 {
        return;
    }
    if !is_zero_byte_composite(&call.args[0]) {
        return;
    }
    let Some(n) = expr_text(&call.args[1]) else {
        return;
    };
    report(
        pending,
        call.fun.pos().0 as u32,
        "zeroByteRepeat",
        format!("avoid bytes.Repeat([]byte{{0}}, {n}); consider using make([]byte, {n}) instead"),
    );
}

fn check_bad_sorting(pass: &Pass<'_>, assign: &AssignStmt, pending: &mut Vec<(u32, String)>) {
    if assign.tok != Some(Token::ASSIGN) {
        return;
    }
    if assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return;
    }
    let Expr::CallExpr(call) = &assign.rhs[0] else {
        return;
    };
    if call.args.len() != 1 || !exprs_equal(&assign.lhs[0], &call.args[0]) {
        return;
    }
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    let (needle, suggest) = if name.ends_with("sort.IntSlice") || name == "sort.IntSlice" {
        ("sort.IntSlice", "sort.Ints")
    } else if name.ends_with("sort.Float64Slice") || name == "sort.Float64Slice" {
        ("sort.Float64Slice", "sort.Float64s")
    } else if name.ends_with("sort.StringSlice") || name == "sort.StringSlice" {
        ("sort.StringSlice", "sort.Strings")
    } else {
        return;
    };
    report(
        pending,
        assign_pos(assign),
        "badSorting",
        format!("suspicious {needle} usage, maybe {suggest} was intended?"),
    );
}

fn check_slice_clear(fs: &ForStmt, pending: &mut Vec<(u32, String)>) {
    // for i := 0; i < len(xs); i++ { xs[i] = 0 }
    let Some(init) = fs.init.as_deref() else {
        return;
    };
    let Stmt::AssignStmt(init_as) = init else {
        return;
    };
    if init_as.lhs.len() != 1 || init_as.rhs.len() != 1 || !is_int_lit(&init_as.rhs[0], 0) {
        return;
    }
    let Expr::Ident(i_id) = &init_as.lhs[0] else {
        return;
    };
    let Some(cond) = fs.cond.as_ref() else {
        return;
    };
    let Expr::BinaryExpr(cond_bin) = cond else {
        return;
    };
    if cond_bin.op != Token::LSS {
        return;
    }
    if !matches!(cond_bin.x.as_ref(), Expr::Ident(id) if id.name == i_id.name) {
        return;
    }
    let Some(xs) = len_arg(&cond_bin.y) else {
        return;
    };
    let Some(post) = fs.post.as_deref() else {
        return;
    };
    let post_ok = match post {
        Stmt::IncDecStmt(inc) => {
            inc.tok == Token::INC && matches!(&inc.x, Expr::Ident(id) if id.name == i_id.name)
        }
        _ => false,
    };
    if !post_ok {
        return;
    }
    if fs.body.list.len() != 1 {
        return;
    }
    let Stmt::AssignStmt(body_as) = &fs.body.list[0] else {
        return;
    };
    if body_as.lhs.len() != 1 || body_as.rhs.len() != 1 || !is_int_lit(&body_as.rhs[0], 0) {
        return;
    }
    let Expr::IndexExpr(ix) = &body_as.lhs[0] else {
        return;
    };
    if !exprs_equal(&ix.x, xs) {
        return;
    }
    if !matches!(ix.index.as_ref(), Expr::Ident(id) if id.name == i_id.name) {
        return;
    }
    report(
        pending,
        fs.for_.0 as u32,
        "sliceClear",
        "rewrite as for-range so compiler can recognize this pattern",
    );
}

const SPRINT_FNS: &[&str] = &["fmt.Sprint", "fmt.Sprintf", "fmt.Sprintln"];

fn method_result_count(pass: &Pass<'_>, typ: TypeId, name: &str) -> Option<usize> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let mut types = artifacts.types.clone();
    let resolved = unalias_readonly(&artifacts.types, typ);
    match lookup_field_or_method(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        resolved,
        true,
        None,
        name,
    ) {
        LookupResult::Found { obj, .. }
            if matches!(artifacts.objects.get(obj), ObjectData::Func(_)) =>
        {
            let sig = obj.typ(&artifacts.objects)?;
            let results = signature_results(&artifacts.types, sig);
            Some(tuple_len(&artifacts.types, results))
        }
        _ => None,
    }
}

fn implements_writer_arity(pass: &Pass<'_>, recv: &Expr) -> bool {
    let Some(typ) = type_of(pass, recv) else {
        return false;
    };
    method_result_count(pass, typ, "Write") == Some(2)
}

fn implements_string_writer_arity(pass: &Pass<'_>, recv: &Expr) -> bool {
    let Some(typ) = type_of(pass, recv) else {
        return false;
    };
    method_result_count(pass, typ, "WriteString") == Some(2)
}

fn sprint_to_fprint(name: &str) -> Option<&'static str> {
    match name {
        "fmt.Sprint" => Some("Fprint"),
        "fmt.Sprintf" => Some("Fprintf"),
        "fmt.Sprintln" => Some("Fprintln"),
        _ => None,
    }
}

fn is_fmt_sprint_call<'a>(pass: &Pass<'_>, expr: &'a Expr) -> Option<(&'static str, &'a [Expr])> {
    let Expr::CallExpr(inner) = expr else {
        return None;
    };
    if !code::is_call_to_any(pass, inner, SPRINT_FNS) {
        return None;
    }
    let name = code::call_name(pass, &inner.fun)?;
    let fprint = sprint_to_fprint(&name)?;
    Some((fprint, &inner.args))
}

/// Render the `$args` capture of the `fmt.Sprint*` operand — upstream splices
/// the variadic match straight into `fmt.Fprint*($w, $args)`, so an empty
/// capture really does leave a dangling `, `.
fn fprint_args(pass: &Pass<'_>, args: &[Expr]) -> String {
    args.iter()
        .map(|a| node_text(pass, a).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(", ")
}

fn check_prefer_fprint(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    // $w.Write([]byte(fmt.Sprint*(...)))
    if let Expr::SelectorExpr(sel) = call.fun.as_ref() {
        if sel.sel.name == "Write"
            && code::is_method_val(pass, sel, "Write")
            && call.args.len() == 1
        {
            if let Some(inner_s) = is_byte_slice_conv(&call.args[0]) {
                if let Some((fprint, args)) = is_fmt_sprint_call(pass, inner_s) {
                    if implements_writer_arity(pass, &sel.x) {
                        // Report("fmt.Fprint*($w, $args) should be preferred to the $$").
                        let w = expr_text(&sel.x).unwrap_or_else(|| "w".into());
                        let args = fprint_args(pass, args);
                        let whole = call_text(pass, call).unwrap_or_default();
                        report(
                            pending,
                            call.fun.pos().0 as u32,
                            "preferFprint",
                            format!("fmt.{fprint}({w}, {args}) should be preferred to the {whole}"),
                        );
                        return;
                    }
                }
            }
        }
        // $w.WriteString(fmt.Sprint*(...))
        if sel.sel.name == "WriteString"
            && code::is_method_val(pass, sel, "WriteString")
            && call.args.len() == 1
        {
            if let Some((fprint, args)) = is_fmt_sprint_call(pass, &call.args[0]) {
                if implements_string_writer_arity(pass, &sel.x)
                    && implements_writer_arity(pass, &sel.x)
                {
                    // Suggest-only rule: `suggestion: fmt.Fprint*($w, $args)`.
                    let w = expr_text(&sel.x).unwrap_or_else(|| "w".into());
                    let args = fprint_args(pass, args);
                    report(
                        pending,
                        call.fun.pos().0 as u32,
                        "preferFprint",
                        format!("suggestion: fmt.{fprint}({w}, {args})"),
                    );
                    return;
                }
            }
        }
    }

    // io.WriteString($w, fmt.Sprint*(...))
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    if name != "io.WriteString" && !name.ends_with("/io.WriteString") {
        return;
    }
    if call.args.len() != 2 {
        return;
    }
    let Some((fprint, args)) = is_fmt_sprint_call(pass, &call.args[1]) else {
        return;
    };
    // Suggest-only rule: `suggestion: fmt.Fprint*($w, $args)`.
    let w = expr_text(&call.args[0]).unwrap_or_else(|| "w".into());
    let args = fprint_args(pass, args);
    report(
        pending,
        call.fun.pos().0 as u32,
        "preferFprint",
        format!("suggestion: fmt.{fprint}({w}, {args})"),
    );
}

/// `preferStringWriter` overlaps `preferFprint` on `Write([]byte(fmt.Sprint*))`
/// and `io.WriteString(w, fmt.Sprint*)`: upstream runs both checkers and emits
/// both warnings. Which one the user ends up seeing is decided later, by
/// `issues.uniq-by-line` keeping the first finding on the line — and only when
/// that option is on. Suppressing here instead would drop the finding outright
/// under `uniq-by-line: false`.
fn check_prefer_string_writer(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    // $w.Write([]byte($s)) where $w implements StringWriter
    if let Expr::SelectorExpr(sel) = call.fun.as_ref() {
        if sel.sel.name == "Write"
            && code::is_method_val(pass, sel, "Write")
            && call.args.len() == 1
        {
            if let Some(s) = is_byte_slice_conv(&call.args[0]) {
                if implements_string_writer_arity(pass, &sel.x) {
                    let w = expr_text(&sel.x).unwrap_or_else(|| "w".into());
                    let s_t = node_text(pass, s).unwrap_or_else(|| "s".into());
                    let whole = call_text(pass, call).unwrap_or_default();
                    report(
                        pending,
                        call.fun.pos().0 as u32,
                        "preferStringWriter",
                        format!("{w}.WriteString({s_t}) should be preferred to the {whole}"),
                    );
                    return;
                }
            }
        }
    }

    // io.WriteString($w, $s) where $w implements StringWriter
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    if name != "io.WriteString" && !name.ends_with("/io.WriteString") {
        return;
    }
    if call.args.len() != 2 {
        return;
    }
    if !implements_string_writer_arity(pass, &call.args[0]) {
        return;
    }
    let w = expr_text(&call.args[0]).unwrap_or_else(|| "w".into());
    let s = node_text(pass, &call.args[1]).unwrap_or_else(|| "s".into());
    let whole = call_text(pass, call).unwrap_or_default();
    report(
        pending,
        call.fun.pos().0 as u32,
        "preferStringWriter",
        format!("{w}.WriteString({s}) should be preferred to the {whole}"),
    );
}

fn type_is_sync_map_ptr(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let s = type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    );
    s == "*sync.Map" || (s.starts_with('*') && s.ends_with("/sync.Map"))
}

fn type_is_sync_wait_group(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let s = type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    );
    // `Where(m["wg"].Type.Is("sync.WaitGroup"))` is an exact type match: a
    // `*sync.WaitGroup` receiver does not match, and upstream really does skip
    // it (the overwhelmingly common spelling).
    s == "sync.WaitGroup" || s.ends_with("/sync.WaitGroup")
}

fn type_is_bytes_buffer(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let s = type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    );
    // Exact match, as with [`type_is_sync_wait_group`]: upstream's
    // `Where(m["buf"].Type.Is("bytes.Buffer"))` does not match `*bytes.Buffer`.
    s == "bytes.Buffer" || s.ends_with("/bytes.Buffer")
}

fn is_sync_map_method_call<'a>(
    pass: &Pass<'_>,
    call: &'a CallExpr,
    method: &str,
) -> Option<&'a Expr> {
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return None;
    };
    if sel.sel.name != method || call.args.len() != 1 {
        return None;
    }
    if !type_is_sync_map_ptr(pass, &sel.x) {
        return None;
    }
    Some(&sel.x)
}

fn check_sync_map_load_and_delete(
    pass: &Pass<'_>,
    stmts: &[Stmt],
    pending: &mut Vec<(u32, String)>,
) {
    for window in stmts.windows(2) {
        let (Stmt::AssignStmt(asgn), Stmt::IfStmt(ifs)) = (&window[0], &window[1]) else {
            continue;
        };
        if asgn.tok != Some(Token::DEFINE) || asgn.lhs.len() != 2 || asgn.rhs.len() != 1 {
            continue;
        }
        if ifs.init.is_some() || ifs.else_.is_some() {
            continue;
        }
        let Expr::Ident(_ok_id) = &ifs.cond else {
            continue;
        };
        if !exprs_equal(&asgn.lhs[1], &ifs.cond) {
            continue;
        }
        let Expr::CallExpr(load) = &asgn.rhs[0] else {
            continue;
        };
        let Some(m) = is_sync_map_method_call(pass, load, "Load") else {
            continue;
        };
        if ifs.body.list.is_empty() {
            continue;
        }
        let Stmt::ExprStmt(first) = &ifs.body.list[0] else {
            continue;
        };
        let Expr::CallExpr(del) = &first.x else {
            continue;
        };
        let Some(m2) = is_sync_map_method_call(pass, del, "Delete") else {
            continue;
        };
        if !exprs_equal(m, m2) || !exprs_equal(&load.args[0], &del.args[0]) {
            continue;
        }
        let m_t = expr_text(m).unwrap_or_else(|| "m".into());
        report(
            pending,
            assign_pos(asgn),
            "syncMapLoadAndDelete",
            format!("use {m_t}.LoadAndDelete to perform load+delete operations atomically"),
        );
    }
}

fn check_dynamic_fmt_string(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    if name != "fmt.Errorf" && !name.ends_with("/fmt.Errorf") {
        return;
    }
    if call.args.len() != 1 {
        return;
    }
    let arg = &call.args[0];
    // fmt.Errorf($f($*args))
    if let Expr::CallExpr(inner) = arg {
        // Report(`use errors.New($f($*args)) or fmt.Errorf("%s", $f($*args)) instead`)
        // — `$f($*args)` is the whole inner call, argument list included.
        let inner_t = call_text(pass, inner).unwrap_or_else(|| "f()".into());
        report(
            pending,
            call.fun.pos().0 as u32,
            "dynamicFmtString",
            format!("use errors.New({inner_t}) or fmt.Errorf(\"%s\", {inner_t}) instead"),
        );
        return;
    }
    // fmt.Errorf($f) where !$f.Const
    if is_const_expr(pass, arg) {
        return;
    }
    let f_t = node_text(pass, arg).unwrap_or_else(|| "f".into());
    report(
        pending,
        call.fun.pos().0 as u32,
        "dynamicFmtString",
        format!("use errors.New({f_t}) or fmt.Errorf(\"%s\", {f_t}) instead"),
    );
}

fn is_string_slice_composite(expr: &Expr) -> Option<&[Expr]> {
    let Expr::CompositeLit(lit) = expr else {
        return None;
    };
    let ty_ok = match lit.ty.as_deref() {
        Some(Expr::ArrayType(at)) if at.len.is_none() => {
            matches!(at.elt.as_ref(), Expr::Ident(id) if id.name == "string")
        }
        _ => false,
    };
    if !ty_ok {
        return None;
    }
    Some(&lit.elts)
}

fn check_string_concat_simplify(
    pass: &Pass<'_>,
    call: &CallExpr,
    pending: &mut Vec<(u32, String)>,
) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    if name != "strings.Join" && !name.ends_with("/strings.Join") {
        return;
    }
    if call.args.len() != 2 {
        return;
    }
    let Some(elts) = is_string_slice_composite(&call.args[0]) else {
        return;
    };
    let glue = &call.args[1];
    let empty_glue = matches!(glue, Expr::BasicLit(lit) if lit.value == "\"\"");
    let suggest = match elts {
        [x, y] if empty_glue => {
            let (Some(x_t), Some(y_t)) = (node_text(pass, x), node_text(pass, y)) else {
                return;
            };
            format!("{x_t} + {y_t}")
        }
        [x, y, z] if empty_glue => {
            let (Some(x_t), Some(y_t), Some(z_t)) =
                (node_text(pass, x), node_text(pass, y), node_text(pass, z))
            else {
                return;
            };
            format!("{x_t} + {y_t} + {z_t}")
        }
        [x, y] => {
            let (Some(x_t), Some(y_t), Some(g_t)) = (
                node_text(pass, x),
                node_text(pass, y),
                node_text(pass, glue),
            ) else {
                return;
            };
            format!("{x_t} + {g_t} + {y_t}")
        }
        _ => return,
    };
    report(
        pending,
        call.fun.pos().0 as u32,
        "stringConcatSimplify",
        // Every arm is `Suggest`-only upstream.
        format!("suggestion: {suggest}"),
    );
}

fn is_sync_once_func_call(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return false;
    };
    (name == "sync.OnceFunc" || name.ends_with("/sync.OnceFunc")) && call.args.len() == 1
}

fn check_bad_sync_once_func_call(
    pass: &Pass<'_>,
    call: &CallExpr,
    pending: &mut Vec<(u32, String)>,
) {
    // sync.OnceFunc($x)()
    let Expr::CallExpr(inner) = call.fun.as_ref() else {
        return;
    };
    if !is_sync_once_func_call(pass, inner) {
        return;
    }
    let x_t = expr_text(&inner.args[0]).unwrap_or_else(|| "f".into());
    report(
        pending,
        call.fun.pos().0 as u32,
        "badSyncOnceFunc",
        format!(
            "possible sync.OnceFunc misuse, consider to assign sync.OnceFunc({x_t}) to a variable"
        ),
    );
}

fn check_bad_sync_once_func_stmts(
    pass: &Pass<'_>,
    stmts: &[Stmt],
    pending: &mut Vec<(u32, String)>,
) {
    for s in stmts {
        let Stmt::ExprStmt(e) = s else {
            continue;
        };
        let Expr::CallExpr(call) = &e.x else {
            continue;
        };
        // Immediate call is handled separately; skip here.
        if matches!(call.fun.as_ref(), Expr::CallExpr(_)) {
            continue;
        }
        if !is_sync_once_func_call(pass, call) {
            continue;
        }
        let x_t = expr_text(&call.args[0]).unwrap_or_else(|| "f".into());
        report(
            pending,
            call.fun.pos().0 as u32,
            "badSyncOnceFunc",
            format!("possible sync.OnceFunc misuse, sync.OnceFunc({x_t}) result is not used"),
        );
    }
}

fn case_fold_call_arg<'a>(pass: &Pass<'_>, expr: &'a Expr, want: &str) -> Option<&'a Expr> {
    let Expr::CallExpr(call) = unparen(expr) else {
        return None;
    };
    let name = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call))?;
    if (name == want || name.ends_with(&format!("/{want}"))) && call.args.len() == 1 {
        Some(&call.args[0])
    } else {
        None
    }
}

fn check_equal_fold_strings(pass: &Pass<'_>, bin: &BinaryExpr, pending: &mut Vec<(u32, String)>) {
    if bin.op != Token::EQL && bin.op != Token::NEQ {
        return;
    }
    let left_lower = case_fold_call_arg(pass, &bin.x, "strings.ToLower");
    let left_upper = case_fold_call_arg(pass, &bin.x, "strings.ToUpper");
    let right_lower = case_fold_call_arg(pass, &bin.y, "strings.ToLower");
    let right_upper = case_fold_call_arg(pass, &bin.y, "strings.ToUpper");

    let (x, y) = match (
        left_lower.is_some() || left_upper.is_some(),
        right_lower.is_some() || right_upper.is_some(),
    ) {
        (true, true) => {
            // Both sides folded: require same case transform (ToLower/ToLower or ToUpper/ToUpper).
            let left_is_lower = left_lower.is_some();
            let right_is_lower = right_lower.is_some();
            if left_is_lower != right_is_lower {
                return;
            }
            (
                left_lower.or(left_upper).unwrap(),
                right_lower.or(right_upper).unwrap(),
            )
        }
        (true, false) => (left_lower.or(left_upper).unwrap(), unparen(&bin.y)),
        (false, true) => (unparen(&bin.x), right_lower.or(right_upper).unwrap()),
        (false, false) => return,
    };

    if !side_effect_free_approx(x) || !side_effect_free_approx(y) {
        return;
    }
    let (Some(xt), Some(yt)) = (expr_text(x), expr_text(y)) else {
        return;
    };
    if xt == yt {
        return;
    }
    let suggest = if bin.op == Token::NEQ {
        format!("!strings.EqualFold({xt}, {yt})")
    } else {
        format!("strings.EqualFold({xt}, {yt})")
    };
    report(
        pending,
        bin.x.pos().0 as u32,
        "equalFold",
        format!("consider replacing with {suggest}"),
    );
}

fn check_equal_fold_bytes(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    if name != "bytes.Equal" && !name.ends_with("/bytes.Equal") {
        return;
    }
    if call.args.len() != 2 {
        return;
    }
    let a = &call.args[0];
    let b = &call.args[1];
    let a_lower = case_fold_call_arg(pass, a, "bytes.ToLower");
    let a_upper = case_fold_call_arg(pass, a, "bytes.ToUpper");
    let b_lower = case_fold_call_arg(pass, b, "bytes.ToLower");
    let b_upper = case_fold_call_arg(pass, b, "bytes.ToUpper");

    let (x, y) = match (
        a_lower.is_some() || a_upper.is_some(),
        b_lower.is_some() || b_upper.is_some(),
    ) {
        (true, true) => {
            let a_is_lower = a_lower.is_some();
            let b_is_lower = b_lower.is_some();
            if a_is_lower != b_is_lower {
                return;
            }
            (a_lower.or(a_upper).unwrap(), b_lower.or(b_upper).unwrap())
        }
        (true, false) => (a_lower.or(a_upper).unwrap(), unparen(b)),
        (false, true) => (unparen(a), b_lower.or(b_upper).unwrap()),
        (false, false) => return,
    };

    if !side_effect_free_approx(x) || !side_effect_free_approx(y) {
        return;
    }
    let (Some(xt), Some(yt)) = (expr_text(x), expr_text(y)) else {
        return;
    };
    if xt == yt {
        return;
    }
    report(
        pending,
        call.fun.pos().0 as u32,
        "equalFold",
        format!("consider replacing with bytes.EqualFold({xt}, {yt})"),
    );
}

fn check_sprintf_quoted_string(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    if name != "fmt.Sprintf" && !name.ends_with("/fmt.Sprintf") {
        return;
    }
    if call.args.is_empty() {
        return;
    }
    let Expr::BasicLit(lit) = &call.args[0] else {
        return;
    };
    let v = lit.value.as_str();
    if v.len() < 2 {
        return;
    }
    let quoted_pct_s = Regex::new(r#"^`.*"%s".*`$"#).unwrap();
    let escaped_pct_s = Regex::new(r#"^".*\\"%s\\".*"$"#).unwrap();
    // The `%#q` / backquoted arm is unreachable upstream: it is a second
    // `m.Match("fmt.Sprintf($s, $*_)")` with the *same* syntax pattern as the
    // first, and ruleguard keeps only one rule per pattern. Verified against
    // golangci-lint 2.12 — `fmt.Sprintf("foo `+"`%s`"+` bar", s)` reports nothing.
    if quoted_pct_s.is_match(v) || escaped_pct_s.is_match(v) {
        report(
            pending,
            call.fun.pos().0 as u32,
            "sprintfQuotedString",
            r#"use %q instead of "%s" for quoted strings"#,
        );
    }
}

fn type_is_time(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut typ = unalias_readonly(&artifacts.types, typ);
    if let TypeData::Pointer(p) = artifacts.types.get(typ) {
        typ = unalias_readonly(&artifacts.types, p.elem());
    }
    let s = type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    );
    s == "time.Time" || s.ends_with("/time.Time")
}

fn method_recv_named<'a>(pass: &Pass<'_>, expr: &'a Expr, method: &str) -> Option<&'a Expr> {
    let Expr::CallExpr(call) = unparen(expr) else {
        return None;
    };
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return None;
    };
    if sel.sel.name != method || !call.args.is_empty() {
        return None;
    }
    // Prefer typed method when available; fall back to selector name.
    if code::is_method_val(pass, sel, method) || type_of(pass, &sel.x).is_some() {
        Some(&sel.x)
    } else {
        None
    }
}

/// The Go version go-critic's version-gated checks see.
///
/// golangci-lint calls `linterCtx.SetGoVersion(settings.Go)` with `run.go`, so
/// the configured version wins over the module's own go directive. With no
/// `run.go` the loader detects the module version and sets the same thing, so
/// falling back to it here is the same answer.
fn gocritic_go_version(pass: &Pass<'_>) -> String {
    pass.settings::<GocriticOptions>("gocritic")
        .and_then(|o| o.go.clone())
        .map(|v| {
            if v.starts_with("go") {
                v
            } else {
                format!("go{v}")
            }
        })
        .filter(|v| v != "go")
        .unwrap_or_else(|| code::module_go_version(pass))
}

fn check_time_expr_simplify(pass: &Pass<'_>, bin: &BinaryExpr, pending: &mut Vec<(u32, String)>) {
    if code::version_compare(&gocritic_go_version(pass), "go1.17") < 0 {
        return;
    }
    if !is_int_lit(&bin.y, 1000) {
        return;
    }
    let Some(whole) = expr_text(&Expr::BinaryExpr(bin.clone())) else {
        return;
    };
    if bin.op == Token::QUO {
        if let Some(recv) = method_recv_named(pass, &bin.x, "Unix") {
            if type_is_time(pass, recv) {
                let t = expr_text(recv).unwrap_or_else(|| "t".into());
                report(
                    pending,
                    bin.x.pos().0 as u32,
                    "timeExprSimplify",
                    format!("use {t}.UnixMilli() instead of {whole}"),
                );
            }
        }
    } else if bin.op == Token::MUL {
        if let Some(recv) = method_recv_named(pass, &bin.x, "UnixNano") {
            if type_is_time(pass, recv) {
                let t = expr_text(recv).unwrap_or_else(|| "t".into());
                report(
                    pending,
                    bin.x.pos().0 as u32,
                    "timeExprSimplify",
                    format!("use {t}.UnixMicro() instead of {whole}"),
                );
            }
        }
    }
}

fn match_append_assign<'a>(stmt: &'a Stmt, slice: Option<&Expr>) -> Option<&'a CallExpr> {
    let Stmt::AssignStmt(assign) = stmt else {
        return None;
    };
    if assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return None;
    }
    let Expr::CallExpr(call) = &assign.rhs[0] else {
        return None;
    };
    let Expr::Ident(fun) = call.fun.as_ref() else {
        return None;
    };
    if fun.name != "append" || call.ellipsis.is_valid() {
        return None;
    }
    if call.args.is_empty() || !exprs_equal(&assign.lhs[0], &call.args[0]) {
        return None;
    }
    if let Some(prev) = slice {
        if !exprs_equal(prev, &call.args[0]) {
            return None;
        }
    }
    Some(call)
}

fn check_append_combine(stmts: &[Stmt], pending: &mut Vec<(u32, String)>) {
    let mut cause_pos: Option<u32> = None;
    let mut slice: Option<&Expr> = None;
    let mut chain = 0usize;

    let flush = |cause_pos: &mut Option<u32>,
                 slice: &mut Option<&Expr>,
                 chain: &mut usize,
                 pending: &mut Vec<(u32, String)>| {
        if *chain > 1 {
            if let Some(pos) = *cause_pos {
                report(
                    pending,
                    pos,
                    "appendCombine",
                    format!("can combine chain of {chain} appends into one"),
                );
            }
        }
        *chain = 0;
        *slice = None;
        *cause_pos = None;
    };

    for stmt in stmts {
        match match_append_assign(stmt, slice) {
            Some(call) => {
                if chain == 0 {
                    chain = 1;
                    slice = Some(&call.args[0]);
                    cause_pos = Some(stmt.pos().0 as u32);
                } else {
                    chain += 1;
                }
            }
            None => flush(&mut cause_pos, &mut slice, &mut chain, pending),
        }
    }
    flush(&mut cause_pos, &mut slice, &mut chain, pending);
}

fn is_trivial_return(pass: &Pass<'_>, ret: &ReturnStmt) -> bool {
    ret.results.iter().all(|e| is_const_expr(pass, e))
}

fn defer_stmt_summary(d: &DeferStmt) -> String {
    if matches!(d.call.fun.as_ref(), Expr::FuncLit(_)) {
        "defer func(){...}(...)".into()
    } else {
        let call = Expr::CallExpr(d.call.clone());
        match expr_text(&call) {
            Some(t) => format!("defer {t}"),
            None => "defer ...".into(),
        }
    }
}

fn check_defer_before_return(
    pass: &Pass<'_>,
    body: &BlockStmt,
    is_func: bool,
    pending: &mut Vec<(u32, String)>,
) {
    let mut explicit_return = false;
    let mut ret_index = body.list.len();
    for (i, stmt) in body.list.iter().enumerate() {
        let Stmt::ReturnStmt(ret) = stmt else {
            continue;
        };
        explicit_return = true;
        if !is_trivial_return(pass, ret) {
            continue;
        }
        ret_index = i;
        break;
    }
    if ret_index == 0 {
        return;
    }
    let Some(Stmt::DeferStmt(d)) = body.list.get(ret_index - 1) else {
        return;
    };
    if is_func || explicit_return {
        let summary = defer_stmt_summary(d);
        report(
            pending,
            d.defer_.0 as u32,
            "unnecessaryDefer",
            format!("{summary} is placed just before return"),
        );
    }
}

fn walk_unnecessary_defer_stmt(pass: &Pass<'_>, stmt: &Stmt, pending: &mut Vec<(u32, String)>) {
    match stmt {
        Stmt::BlockStmt(b) => check_unnecessary_defer_block(pass, b, false, pending),
        Stmt::IfStmt(i) => {
            check_unnecessary_defer_block(pass, &i.body, false, pending);
            if let Some(e) = &i.else_ {
                walk_unnecessary_defer_stmt(pass, e, pending);
            }
        }
        Stmt::ForStmt(f) => check_unnecessary_defer_block(pass, &f.body, false, pending),
        Stmt::RangeStmt(r) => check_unnecessary_defer_block(pass, &r.body, false, pending),
        Stmt::SwitchStmt(s) => {
            for c in &s.body.list {
                if let Stmt::CaseClause(cc) = c {
                    // Case bodies are stmt lists; wrap via synthetic checks.
                    let mut explicit = false;
                    let mut ret_index = cc.body.len();
                    for (i, st) in cc.body.iter().enumerate() {
                        if let Stmt::ReturnStmt(ret) = st {
                            explicit = true;
                            if is_trivial_return(pass, ret) {
                                ret_index = i;
                                break;
                            }
                        }
                    }
                    if ret_index > 0 {
                        if let Some(Stmt::DeferStmt(d)) = cc.body.get(ret_index - 1) {
                            if explicit {
                                let summary = defer_stmt_summary(d);
                                report(
                                    pending,
                                    d.defer_.0 as u32,
                                    "unnecessaryDefer",
                                    format!("{summary} is placed just before return"),
                                );
                            }
                        }
                    }
                    for st in &cc.body {
                        walk_unnecessary_defer_stmt(pass, st, pending);
                    }
                }
            }
        }
        Stmt::TypeSwitchStmt(s) => {
            for c in &s.body.list {
                if let Stmt::CaseClause(cc) = c {
                    for st in &cc.body {
                        walk_unnecessary_defer_stmt(pass, st, pending);
                    }
                }
            }
        }
        Stmt::SelectStmt(s) => {
            for c in &s.body.list {
                if let Stmt::CommClause(cc) = c {
                    for st in &cc.body {
                        walk_unnecessary_defer_stmt(pass, st, pending);
                    }
                }
            }
        }
        Stmt::DeferStmt(d) => {
            if let Expr::FuncLit(fl) = d.call.fun.as_ref() {
                check_unnecessary_defer_block(pass, &fl.body, true, pending);
            }
        }
        Stmt::ExprStmt(e) => {
            if let Expr::CallExpr(call) = &e.x {
                if let Expr::FuncLit(fl) = call.fun.as_ref() {
                    check_unnecessary_defer_block(pass, &fl.body, true, pending);
                }
            }
            if let Expr::FuncLit(fl) = &e.x {
                check_unnecessary_defer_block(pass, &fl.body, true, pending);
            }
        }
        Stmt::GoStmt(g) => {
            if let Expr::FuncLit(fl) = g.call.fun.as_ref() {
                check_unnecessary_defer_block(pass, &fl.body, true, pending);
            }
        }
        Stmt::AssignStmt(a) => {
            for rhs in &a.rhs {
                if let Expr::FuncLit(fl) = rhs {
                    check_unnecessary_defer_block(pass, &fl.body, true, pending);
                }
            }
        }
        _ => {}
    }
}

fn check_unnecessary_defer_block(
    pass: &Pass<'_>,
    body: &BlockStmt,
    is_func: bool,
    pending: &mut Vec<(u32, String)>,
) {
    check_defer_before_return(pass, body, is_func, pending);
    for stmt in &body.list {
        walk_unnecessary_defer_stmt(pass, stmt, pending);
    }
}

fn check_unnecessary_defer_func(pass: &Pass<'_>, f: &FuncDecl, pending: &mut Vec<(u32, String)>) {
    if let Some(body) = &f.body {
        check_unnecessary_defer_block(pass, body, true, pending);
    }
}

fn has_string_method(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    // DEFERRED: true fmt.Stringer Implements; method-name + arity heuristic.
    method_result_count(pass, typ, "String") == Some(1)
}

fn type_is_reflect_value(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let s = type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        unalias_readonly(&artifacts.types, typ),
        None,
    );
    s == "reflect.Value" || s.ends_with("/reflect.Value")
}

fn check_redundant_sprint(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    let arg = if name == "fmt.Sprint" || name.ends_with("/fmt.Sprint") {
        if call.args.len() != 1 {
            return;
        }
        &call.args[0]
    } else if name == "fmt.Sprintf" || name.ends_with("/fmt.Sprintf") {
        if call.args.len() != 2 {
            return;
        }
        let Expr::BasicLit(lit) = &call.args[0] else {
            return;
        };
        if lit.value != "\"%s\"" && lit.value != "\"%v\"" {
            return;
        }
        &call.args[1]
    } else {
        return;
    };

    if type_is_reflect_value(pass, arg) {
        return;
    }
    let Some(arg_t) = expr_text(arg) else {
        return;
    };
    if is_string_typed(pass, arg) {
        report(
            pending,
            call.fun.pos().0 as u32,
            "redundantSprint",
            format!("{arg_t} is already string"),
        );
        return;
    }
    if has_string_method(pass, arg) {
        report(
            pending,
            call.fun.pos().0 as u32,
            "redundantSprint",
            format!("use {arg_t}.String() instead"),
        );
    }
}

// --- batch 13: typeUnparen / importShadow / unnamedResult / whyNoLint /
// hugeParam / rangeValCopy -------------------------------------------------

fn is_stdlib_import_path(path: &str) -> bool {
    let elem = path.split('/').next().unwrap_or(path);
    !elem.contains('.')
}

fn import_local_name(imp: &ImportSpec) -> Option<(String, String)> {
    let path = imp.path.value.trim_matches('"').to_string();
    if path == "C" {
        return None;
    }
    let name = if let Some(n) = &imp.name {
        if n.name == "." || n.name == "_" {
            return None;
        }
        n.name.clone()
    } else {
        path.rsplit('/').next().unwrap_or(&path).to_string()
    };
    Some((name, path))
}

fn collect_import_names(file: &File) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for imp in &file.imports {
        if let Some((name, path)) = import_local_name(imp) {
            out.insert(name, path);
        }
    }
    out
}

fn warn_import_shadow(
    id: &Ident,
    imports: &HashMap<String, String>,
    pending: &mut Vec<(u32, String)>,
) {
    if id.name == "_" {
        return;
    }
    let Some(path) = imports.get(&id.name) else {
        return;
    };
    if is_stdlib_import_path(path) {
        report(
            pending,
            id.pos().0 as u32,
            "importShadow",
            format!("shadow of imported package '{}'", id.name),
        );
    } else {
        report(
            pending,
            id.pos().0 as u32,
            "importShadow",
            format!("shadow of imported from '{}' package '{}'", path, id.name),
        );
    }
}

fn check_import_shadow_fields(
    fields: Option<&FieldList>,
    imports: &HashMap<String, String>,
    pending: &mut Vec<(u32, String)>,
) {
    let Some(fl) = fields else {
        return;
    };
    for f in &fl.list {
        for name in &f.names {
            warn_import_shadow(name, imports, pending);
        }
    }
}

fn check_import_shadow_func(
    pass: &Pass<'_>,
    f: &FuncDecl,
    imports: &HashMap<String, String>,
    pending: &mut Vec<(u32, String)>,
) {
    // `astwalk.walkSignature` order: params, results, then the receiver.
    // Results were missing entirely, so a named result shadowing an import
    // went unreported (upstream flags `func C() (os int)`).
    check_import_shadow_fields(f.ty.params.as_ref(), imports, pending);
    check_import_shadow_fields(f.ty.results.as_ref(), imports, pending);
    check_import_shadow_fields(f.recv.as_ref(), imports, pending);
    let Some(body) = &f.body else {
        return;
    };
    // Mirrors `astwalk.localDefWalker.walkFuncBody`, whose two cases both end
    // in `return false`. What it does *not* visit is as load-bearing as what
    // it does — all three of these are unreported upstream, measured against
    // golangci-lint 2.12.2:
    //
    //   for os, strings := range m {}   // a RangeStmt is not an AssignStmt
    //   f = func() { os := 1 }          // non-define assign: not descended
    //   var g = func() { os := 1 }      // GenDecl: not descended
    //
    // while a closure reached any other way (`func() { os := 1 }()`) is.
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        match n {
            NodeRef::AssignStmt(a) => {
                if a.tok == Some(Token::DEFINE) {
                    for lhs in &a.lhs {
                        if let Expr::Ident(id) = lhs {
                            if is_def_ident(pass, id) {
                                warn_import_shadow(id, imports, pending);
                            }
                        }
                    }
                }
                // `return false` for *either* token: upstream stops here even
                // when the assignment defines nothing.
                false
            }
            NodeRef::ValueSpec(vs) => {
                for name in &vs.names {
                    warn_import_shadow(name, imports, pending);
                }
                false
            }
            _ => true,
        }
    });
}

fn type_expr_text(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::SelectorExpr(sel) => {
            let x = type_expr_text(&sel.x)?;
            Some(format!("{x}.{}", sel.sel.name))
        }
        Expr::StarExpr(s) => {
            let x = type_expr_text(&s.x)?;
            Some(format!("*{x}"))
        }
        Expr::ParenExpr(p) => type_expr_text(&p.x).map(|inner| format!("({inner})")),
        Expr::ArrayType(a) => {
            let elt = type_expr_text(&a.elt)?;
            match &a.len {
                None => Some(format!("[]{elt}")),
                Some(len) => {
                    let n = expr_text(len)?;
                    Some(format!("[{n}]{elt}"))
                }
            }
        }
        Expr::MapType(m) => {
            let k = type_expr_text(&m.key)?;
            let v = type_expr_text(&m.value)?;
            Some(format!("map[{k}]{v}"))
        }
        Expr::ChanType(c) => {
            let v = type_expr_text(&c.value)?;
            let prefix = match c.dir.0 {
                d if d == ChanDir::SEND.0 => "chan<- ",
                d if d == ChanDir::RECV.0 => "<-chan ",
                _ => "chan ",
            };
            Some(format!("{prefix}{v}"))
        }
        Expr::FuncType(f) => {
            let params = field_list_type_text(f.params.as_ref())?;
            let results = match &f.results {
                None => String::new(),
                Some(r) if r.list.len() == 1 && r.list[0].names.is_empty() => {
                    let t = type_expr_text(r.list[0].ty.as_ref()?)?;
                    format!(" {t}")
                }
                Some(r) => format!(" ({})", field_list_type_text(Some(r))?),
            };
            Some(format!("func({params}){results}"))
        }
        Expr::StructType(_) => Some("struct{...}".to_string()),
        Expr::InterfaceType(_) => Some("interface{...}".to_string()),
        Expr::Ellipsis(e) => {
            let elt = e.elt.as_ref().and_then(|x| type_expr_text(x))?;
            Some(format!("...{elt}"))
        }
        _ => expr_text(expr),
    }
}

fn field_list_type_text(fl: Option<&FieldList>) -> Option<String> {
    let Some(fl) = fl else {
        return Some(String::new());
    };
    let mut parts = Vec::new();
    for f in &fl.list {
        let ty = type_expr_text(f.ty.as_ref()?)?;
        if f.names.is_empty() {
            parts.push(ty);
        } else {
            let names: Vec<_> = f.names.iter().map(|n| n.name.as_str()).collect();
            parts.push(format!("{} {ty}", names.join(", ")));
        }
    }
    Some(parts.join(", "))
}

fn type_expr_text_stripped(expr: &Expr) -> Option<String> {
    match expr {
        Expr::ParenExpr(p) => type_expr_text_stripped(&p.x),
        Expr::StarExpr(s) => {
            let x = type_expr_text_stripped(&s.x)?;
            Some(format!("*{x}"))
        }
        Expr::ArrayType(a) => {
            let elt = type_expr_text_stripped(&a.elt)?;
            match &a.len {
                None => Some(format!("[]{elt}")),
                Some(len) => {
                    let n = expr_text(len)?;
                    Some(format!("[{n}]{elt}"))
                }
            }
        }
        Expr::MapType(m) => {
            let k = type_expr_text_stripped(&m.key)?;
            let v = type_expr_text_stripped(&m.value)?;
            Some(format!("map[{k}]{v}"))
        }
        Expr::ChanType(c) => {
            let any = ChanDir::SEND.0 | ChanDir::RECV.0;
            if let Expr::ParenExpr(inner) = c.value.as_ref() {
                if let Expr::ChanType(nested) = inner.x.as_ref() {
                    if nested.dir.0 != any || c.dir.0 != any {
                        let v = type_expr_text_stripped(&inner.x)?;
                        let prefix = match c.dir.0 {
                            d if d == ChanDir::SEND.0 => "chan<- ",
                            d if d == ChanDir::RECV.0 => "<-chan ",
                            _ => "chan ",
                        };
                        return Some(format!("{prefix}({v})"));
                    }
                }
            }
            let v = type_expr_text_stripped(&c.value)?;
            let prefix = match c.dir.0 {
                d if d == ChanDir::SEND.0 => "chan<- ",
                d if d == ChanDir::RECV.0 => "<-chan ",
                _ => "chan ",
            };
            Some(format!("{prefix}{v}"))
        }
        Expr::FuncType(f) => {
            let params = field_list_type_text_stripped(f.params.as_ref())?;
            let results = match &f.results {
                None => String::new(),
                Some(r) if r.list.len() == 1 && r.list[0].names.is_empty() => {
                    let t = type_expr_text_stripped(r.list[0].ty.as_ref()?)?;
                    format!(" {t}")
                }
                Some(r) => format!(" ({})", field_list_type_text_stripped(Some(r))?),
            };
            Some(format!("func({params}){results}"))
        }
        Expr::StructType(s) => {
            // Keep display token; nested field types checked separately when
            // VisitTypeExpr would only walk nested fields for struct/interface.
            let _ = s;
            Some("struct{...}".to_string())
        }
        Expr::InterfaceType(i) => {
            let _ = i;
            Some("interface{...}".to_string())
        }
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::SelectorExpr(sel) => {
            let x = type_expr_text_stripped(&sel.x)?;
            Some(format!("{x}.{}", sel.sel.name))
        }
        Expr::Ellipsis(e) => {
            let elt = e.elt.as_ref().and_then(|x| type_expr_text_stripped(x))?;
            Some(format!("...{elt}"))
        }
        _ => type_expr_text(expr),
    }
}

fn field_list_type_text_stripped(fl: Option<&FieldList>) -> Option<String> {
    let Some(fl) = fl else {
        return Some(String::new());
    };
    let mut parts = Vec::new();
    for f in &fl.list {
        let ty = type_expr_text_stripped(f.ty.as_ref()?)?;
        if f.names.is_empty() {
            parts.push(ty);
        } else {
            let names: Vec<_> = f.names.iter().map(|n| n.name.as_str()).collect();
            parts.push(format!("{} {ty}", names.join(", ")));
        }
    }
    Some(parts.join(", "))
}

fn check_type_unparen_compare(e: &Expr, pending: &mut Vec<(u32, String)>) {
    let (Some(before), Some(after)) = (type_expr_text(e), type_expr_text_stripped(e)) else {
        return;
    };
    if before != after {
        report(
            pending,
            e.pos().0 as u32,
            "typeUnparen",
            format!("could simplify {before} to {after}"),
        );
    }
}

fn check_type_unparen_root(e: &Expr, pending: &mut Vec<(u32, String)>) {
    match e {
        Expr::ParenExpr(p) => match p.x.as_ref() {
            Expr::StructType(_) => {
                report(
                    pending,
                    p.lparen.0 as u32,
                    "typeUnparen",
                    "could simplify (struct{...}) to struct{...}",
                );
            }
            Expr::InterfaceType(_) => {
                report(
                    pending,
                    p.lparen.0 as u32,
                    "typeUnparen",
                    "could simplify (interface{...}) to interface{...}",
                );
            }
            _ => check_type_unparen_compare(e, pending),
        },
        Expr::StructType(s) => {
            for field in &s.fields.list {
                if let Some(ty) = &field.ty {
                    check_type_unparen_root(ty, pending);
                }
            }
        }
        Expr::InterfaceType(i) => {
            for field in &i.methods.list {
                if let Some(ty) = &field.ty {
                    check_type_unparen_root(ty, pending);
                }
            }
        }
        _ => check_type_unparen_compare(e, pending),
    }
}

fn check_type_unparen_file(file: &File, pending: &mut Vec<(u32, String)>) {
    for decl in &file.decls {
        match decl {
            Decl::GenDecl(g) => {
                for spec in &g.specs {
                    if let Spec::TypeSpec(ts) = spec {
                        check_type_unparen_root(&ts.ty, pending);
                    }
                    if let Spec::ValueSpec(vs) = spec {
                        if let Some(ty) = &vs.ty {
                            check_type_unparen_root(ty, pending);
                        }
                    }
                }
            }
            Decl::FuncDecl(f) => {
                check_import_shadow_fields_types(&f.ty, pending);
                if let Some(recv) = &f.recv {
                    for field in &recv.list {
                        if let Some(ty) = &field.ty {
                            check_type_unparen_root(ty, pending);
                        }
                    }
                }
                if let Some(body) = &f.body {
                    walk::inspect(NodeRef::BlockStmt(body), |n| {
                        let Some(n) = n else {
                            return true;
                        };
                        if let NodeRef::ValueSpec(vs) = n {
                            if let Some(ty) = &vs.ty {
                                check_type_unparen_root(ty, pending);
                            }
                        }
                        if let NodeRef::FuncLit(fl) = n {
                            check_import_shadow_fields_types(&fl.ty, pending);
                        }
                        true
                    });
                }
            }
            _ => {}
        }
    }
}

fn check_import_shadow_fields_types(ty: &FuncType, pending: &mut Vec<(u32, String)>) {
    for fl in [&ty.type_params, &ty.params, &ty.results] {
        if let Some(list) = fl {
            for field in &list.list {
                if let Some(t) = &field.ty {
                    check_type_unparen_root(t, pending);
                }
            }
        }
    }
}

fn ast_type_name(ty: &Expr) -> String {
    match ty {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => sel.sel.name.clone(),
        Expr::StarExpr(s) => ast_type_name(&s.x),
        Expr::ArrayType(a) => ast_type_name(&a.elt),
        Expr::ParenExpr(p) => ast_type_name(&p.x),
        _ => String::new(),
    }
}

fn ast_qualified_name(ty: &Expr) -> String {
    match ty {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => {
            format!("{}.{}", expr_text(&sel.x).unwrap_or_default(), sel.sel.name)
        }
        Expr::ParenExpr(p) => ast_qualified_name(&p.x),
        _ => String::new(),
    }
}

fn is_error_type_expr(ty: &Expr) -> bool {
    ast_qualified_name(ty) == "error"
}

fn is_bool_type_expr(ty: &Expr) -> bool {
    ast_qualified_name(ty) == "bool"
}

fn result_num_fields(results: &FieldList) -> usize {
    let mut n = 0;
    for f in &results.list {
        if f.names.is_empty() {
            n += 1;
        } else {
            n += f.names.len();
        }
    }
    n
}

fn check_unnamed_result(f: &FuncDecl, check_exported: bool, pending: &mut Vec<(u32, String)>) {
    // Upstream: `if c.checkExported && !ast.IsExported(name) { return }` — the
    // param *narrows* the check to exported funcs rather than adding them.
    if check_exported && !is_exported(&f.name.name) {
        return;
    }
    // checkExported default false → only exported funcs (upstream inverted naming:
    // checkExported=false means skip unexported... wait:
    // `if c.checkExported && !ast.IsExported` → return
    // So when checkExported is false (default), it does NOT return for unexported —
    // it checks ALL functions. When checkExported is true, it only checks exported.
    // Actually: `if c.checkExported && !ast.IsExported(decl.Name.Name) { return }`
    // Default checkExported=false → never returns early → checks all. OK.

    let Some(results) = &f.ty.results else {
        return;
    };
    if results.list.is_empty() {
        return;
    }
    if results.list[0].names.first().is_some() {
        return; // named results
    }

    let fields: Vec<&Expr> = results.list.iter().filter_map(|f| f.ty.as_ref()).collect();
    if fields.is_empty() {
        return;
    }

    if result_num_fields(results) == 2 {
        if fields.len() < 2 {
            return;
        }
        let typ1 = fields[0];
        let typ2 = fields[1];
        let name1 = ast_type_name(typ1);
        let name2 = ast_type_name(typ2);
        let cond = (name1 != name2 && !name2.is_empty())
            || (!is_error_type_expr(typ1) && is_error_type_expr(typ2))
            || (!is_bool_type_expr(typ1) && is_bool_type_expr(typ2));
        if !cond {
            report(
                pending,
                f.ty.func.0 as u32,
                "unnamedResult",
                "consider giving a name to these results",
            );
        }
        return;
    }

    let mut seen: HashMap<String, bool> = HashMap::new();
    for (i, typ) in fields.iter().enumerate() {
        let name = ast_type_name(typ);
        let is_last = i + 1 == fields.len();
        let cond = !seen.get(&name).copied().unwrap_or(false)
            || (is_last && (is_error_type_expr(typ) || is_bool_type_expr(typ)));
        if !cond {
            report(
                pending,
                f.ty.func.0 as u32,
                "unnamedResult",
                "consider giving a name to these results",
            );
            return;
        }
        seen.insert(name, true);
    }
}

fn check_why_no_lint(cg: &CommentGroup, pending: &mut Vec<(u32, String)>) {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r"^// *nolint(?::[^ ]+)? *(.*)$").expect("whyNoLint regex"));
    if cg.list.first().is_some_and(|c| c.text.starts_with("/*")) {
        return;
    }
    for comment in &cg.list {
        let Some(caps) = re.captures(&comment.text) else {
            continue;
        };
        let rest = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        if !rest.starts_with("//") || rest.trim_start_matches("//").is_empty() {
            report(
                pending,
                cg.pos().0 as u32,
                "whyNoLint",
                "include an explanation for nolint directive",
            );
            return;
        }
    }
}

const HUGE_PARAM_SIZE_THRESHOLD: i64 = 80;
const RANGE_VAL_COPY_SIZE_THRESHOLD: i64 = 128;

/// Upstream `checkers.isUnitTestFunc`: `func TestXxx(*testing.T)` returning
/// nothing.
///
/// `rangeValCopy` and `rangeExprCopy` are the only two checkers that take a
/// `skipTestFuncs` param, and it **defaults to true** — so this is not an
/// unwired setting but part of their default behaviour. Their `EnterFunc`
/// returns false for such a function, which prunes the whole subtree (nested
/// `t.Run(..., func(t *testing.T) { ... })` closures included).
fn is_unit_test_func(pass: &Pass<'_>, decl: &FuncDecl) -> bool {
    if !decl.name.name.starts_with("Test") {
        return false;
    }
    if decl.ty.results.as_ref().is_some_and(|r| !r.list.is_empty()) {
        return false;
    }
    let Some(params) = decl.ty.params.as_ref() else {
        return false;
    };
    // `sig.Params().Len() == 1`: one field holding one name (`t *testing.T`)
    // or one unnamed field (`*testing.T`).
    if params.list.len() != 1 || params.list[0].names.len() > 1 {
        return false;
    }
    let Some(ty) = params.list[0].ty.as_ref() else {
        return false;
    };
    let Some(typ) = type_of(pass, ty) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    // Upstream compares `sig.Params().At(0).Type().String()`, so a renamed
    // import (`import tst "testing"`) still reads as `*testing.T`.
    type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    ) == "*testing.T"
}

fn is_stringer_method(decl: &FuncDecl) -> bool {
    if decl.recv.is_none() || decl.name.name != "String" {
        return false;
    }
    if decl.ty.params.as_ref().is_some_and(|p| !p.list.is_empty()) {
        return false;
    }
    let Some(results) = &decl.ty.results else {
        return false;
    };
    if results.list.len() != 1 {
        return false;
    }
    matches!(
        results.list[0].ty.as_ref(),
        Some(Expr::Ident(id)) if id.name == "string"
    )
}

fn check_huge_param_fields(
    pass: &Pass<'_>,
    fields: Option<&FieldList>,
    pending: &mut Vec<(u32, String)>,
) {
    let Some(fl) = fields else {
        return;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let Some(info) = pass.types_info() else {
        return;
    };
    let sizes = pass.pkg().types_sizes.unwrap_or_else(default_sizes);
    for f in &fl.list {
        for id in &f.names {
            let typ = info
                .defs
                .get(&id.id)
                .copied()
                .flatten()
                .and_then(|oid| match artifacts.objects.get(oid) {
                    ObjectData::Var(v) => Some(v.typ()),
                    _ => None,
                })
                .or_else(|| f.ty.as_ref().and_then(|ty| type_of(pass, ty)));
            let Some(typ) = typ else {
                continue;
            };
            let size = sizes.sizeof(
                &artifacts.types,
                &artifacts.objects,
                &artifacts.packages,
                typ,
            );
            if size >= HUGE_PARAM_SIZE_THRESHOLD {
                report(
                    pending,
                    id.pos().0 as u32,
                    "hugeParam",
                    format!(
                        "{} is heavy ({size} bytes); consider passing it by pointer",
                        id.name
                    ),
                );
            }
        }
    }
}

fn check_huge_param(pass: &Pass<'_>, f: &FuncDecl, pending: &mut Vec<(u32, String)>) {
    if is_stringer_method(f) {
        return;
    }
    check_huge_param_fields(pass, f.recv.as_ref(), pending);
    check_huge_param_fields(pass, f.ty.params.as_ref(), pending);
}

fn check_range_val_copy(pass: &Pass<'_>, rs: &RangeStmt, pending: &mut Vec<(u32, String)>) {
    let Some(value) = &rs.value else {
        return;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let typ = type_of(pass, value)
        .or_else(|| {
            let info = pass.types_info()?;
            let Expr::Ident(id) = value else {
                return None;
            };
            let oid = info.defs.get(&id.id).copied().flatten()?;
            match artifacts.objects.get(oid) {
                ObjectData::Var(v) => Some(v.typ()),
                _ => None,
            }
        })
        .or_else(|| {
            // Infer from range expression: []T / [N]T → T
            let x_typ = type_of(pass, &rs.x)?;
            let x_typ = unalias_readonly(&artifacts.types, x_typ);
            match artifacts.types.get(x_typ) {
                TypeData::Slice(s) => Some(s.elem()),
                TypeData::Array(a) => Some(a.elem()),
                _ => None,
            }
        });
    let Some(typ) = typ else {
        return;
    };
    let sizes = pass.pkg().types_sizes.unwrap_or_else(default_sizes);
    let size = sizes.sizeof(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
    );
    if size >= RANGE_VAL_COPY_SIZE_THRESHOLD {
        report(
            pending,
            rs.for_.0 as u32,
            "rangeValCopy",
            format!("each iteration copies {size} bytes (consider pointers or indexing)"),
        );
    }
}

// --- batch 14: ptrToRefParam / tooManyResultsChecker / evalOrder /
// unlabelStmt / returnAfterHttpError / exposedSyncMutex --------------------

fn is_ref_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Map(_) | TypeData::Chan(_) | TypeData::Interface(_) => true,
        TypeData::Named(_) => {
            let u = typ.underlying(&artifacts.types);
            matches!(artifacts.types.get(u), TypeData::Interface(_))
        }
        _ => false,
    }
}

fn check_ptr_to_ref_param_fields(
    pass: &Pass<'_>,
    fields: Option<&FieldList>,
    pending: &mut Vec<(u32, String)>,
) {
    let Some(fl) = fields else {
        return;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    for f in &fl.list {
        let Some(ty_expr) = &f.ty else {
            continue;
        };
        let Some(typ) = type_of(pass, ty_expr) else {
            continue;
        };
        let typ = unalias_readonly(&artifacts.types, typ);
        let TypeData::Pointer(p) = artifacts.types.get(typ) else {
            continue;
        };
        if !is_ref_type(pass, p.elem()) {
            continue;
        }
        if f.names.is_empty() {
            let ty_text = expr_text(ty_expr).unwrap_or_else(|| "?".into());
            report(
                pending,
                f.pos().0 as u32,
                "ptrToRefParam",
                format!("consider to make non-pointer type for `{ty_text}`"),
            );
        } else {
            for id in &f.names {
                report(
                    pending,
                    id.pos().0 as u32,
                    "ptrToRefParam",
                    format!("consider `{}' to be of non-pointer type", id.name),
                );
            }
        }
    }
}

fn check_ptr_to_ref_param(pass: &Pass<'_>, f: &FuncDecl, pending: &mut Vec<(u32, String)>) {
    check_ptr_to_ref_param_fields(pass, f.ty.params.as_ref(), pending);
    check_ptr_to_ref_param_fields(pass, f.ty.results.as_ref(), pending);
}

fn check_too_many_results(f: &FuncDecl, max_results: usize, pending: &mut Vec<(u32, String)>) {
    let Some(results) = &f.ty.results else {
        return;
    };
    if result_num_fields(results) > max_results {
        report(
            pending,
            f.ty.func.0 as u32,
            "tooManyResultsChecker",
            format!(
                "function has more than {max_results} results, consider to simplify the function"
            ),
        );
    }
}

fn call_contains_addr_of(call: &CallExpr, id: &Ident) -> bool {
    let mut found = false;
    walk::inspect(NodeRef::CallExpr(call), |n| {
        let Some(n) = n else {
            return true;
        };
        if let NodeRef::UnaryExpr(u) = n {
            if u.op == Token::AND {
                if let Expr::Ident(x) = u.x.as_ref() {
                    if x.name == id.name {
                        found = true;
                    }
                }
            }
        }
        true
    });
    found
}

fn has_ptr_recv(pass: &Pass<'_>, sel: &guff::ast::SelectorExpr) -> bool {
    let Some(recv_type) = type_of(pass, &sel.x) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    let LookupResult::Found { obj, .. } = lookup_field_or_method(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        recv_type,
        true,
        None,
        &sel.sel.name,
    ) else {
        return false;
    };
    let ObjectData::Func(method) = artifacts.objects.get(obj) else {
        return false;
    };
    let Some(method_type) = method.typ() else {
        return false;
    };
    let TypeData::Signature(sig) = artifacts.types.get(method_type) else {
        return false;
    };
    let Some(recv) = sig.recv() else {
        return false;
    };
    let ObjectData::Var(v) = artifacts.objects.get(recv) else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, v.typ());
    matches!(artifacts.types.get(typ), TypeData::Pointer(_))
}

fn check_eval_order(pass: &Pass<'_>, ret: &ReturnStmt, pending: &mut Vec<(u32, String)>) {
    if ret.results.len() < 2 {
        return;
    }
    for res in &ret.results {
        let Expr::Ident(id) = res else {
            continue;
        };
        for other in &ret.results {
            let Expr::CallExpr(call) = other else {
                continue;
            };
            if let Expr::SelectorExpr(sel) = call.fun.as_ref() {
                if exprs_equal(&sel.x, res) && has_ptr_recv(pass, sel) {
                    let call_text = expr_text(other).unwrap_or_else(|| "call".into());
                    report(
                        pending,
                        call.fun.pos().0 as u32,
                        "evalOrder",
                        format!("may want to evaluate {call_text} before the return statement"),
                    );
                }
            }
            if call_contains_addr_of(call, id) {
                let call_text = expr_text(other).unwrap_or_else(|| "call".into());
                report(
                    pending,
                    call.fun.pos().0 as u32,
                    "evalOrder",
                    format!("may want to evaluate {call_text} before the return statement"),
                );
            }
        }
    }
}

fn stmt_contains_goto(body: &BlockStmt) -> bool {
    let mut found = false;
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        if let Some(NodeRef::BranchStmt(br)) = n {
            if br.tok == Token::GOTO {
                found = true;
            }
        }
        true
    });
    found
}

fn can_break_from_stmt(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::RangeStmt(_)
            | Stmt::ForStmt(_)
            | Stmt::SwitchStmt(_)
            | Stmt::TypeSwitchStmt(_)
            | Stmt::SelectStmt(_)
    )
}

fn block_stmt_of(stmt: &Stmt) -> Option<&BlockStmt> {
    match stmt {
        Stmt::RangeStmt(s) => Some(&s.body),
        Stmt::ForStmt(s) => Some(&s.body),
        Stmt::SwitchStmt(s) => Some(&s.body),
        Stmt::TypeSwitchStmt(s) => Some(&s.body),
        Stmt::SelectStmt(s) => Some(&s.body),
        _ => None,
    }
}

fn is_loop_stmt(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::ForStmt(_) | Stmt::RangeStmt(_))
}

fn uses_label_in_block(body: &BlockStmt, label: &str) -> bool {
    let mut found = false;
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        if let Some(NodeRef::BranchStmt(br)) = n {
            if br.label.as_ref().is_some_and(|l| l.name == label)
                && (br.tok == Token::CONTINUE || br.tok == Token::BREAK)
            {
                found = true;
            }
        }
        true
    });
    found
}

fn nested_breakable_uses_label(body: &BlockStmt, label: &str) -> bool {
    let mut found = false;
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        let (is_breakable, block) = match n {
            NodeRef::RangeStmt(s) => (true, Some(&s.body)),
            NodeRef::ForStmt(s) => (true, Some(&s.body)),
            NodeRef::SwitchStmt(s) => (true, Some(&s.body)),
            NodeRef::TypeSwitchStmt(s) => (true, Some(&s.body)),
            NodeRef::SelectStmt(s) => (true, Some(&s.body)),
            _ => (false, None),
        };
        if is_breakable {
            if let Some(block) = block {
                if uses_label_in_block(block, label) {
                    found = true;
                }
            }
        }
        true
    });
    found
}

fn find_labeled_continue(body: &BlockStmt, label: &str) -> Option<u32> {
    let mut found: Option<u32> = None;
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        if matches!(n, NodeRef::SelectStmt(_)) {
            return false; // skip select subtrees (upstream FindNode skip)
        }
        if let NodeRef::BranchStmt(br) = n {
            if br.tok == Token::CONTINUE && br.label.as_ref().is_some_and(|l| l.name == label) {
                found = Some(br.tok_pos.0 as u32);
            }
        }
        true
    });
    found
}

fn check_unlabel_stmt(labeled: &LabeledStmt, pending: &mut Vec<(u32, String)>) {
    if !can_break_from_stmt(&labeled.stmt) {
        return;
    }
    let Some(body) = block_stmt_of(&labeled.stmt) else {
        return;
    };
    let name = &labeled.label.name;

    if !nested_breakable_uses_label(body, name) {
        report(
            pending,
            labeled.label.pos().0 as u32,
            "unlabelStmt",
            format!("label {name} is redundant"),
        );
        return;
    }

    if !is_loop_stmt(&labeled.stmt) {
        return;
    }
    if body.list.is_empty() {
        return;
    }
    let last = body.list.last().unwrap();
    if !is_loop_stmt(last) {
        return;
    }
    let Some(inner_body) = block_stmt_of(last) else {
        return;
    };
    if let Some(pos) = find_labeled_continue(inner_body, name) {
        report(
            pending,
            pos,
            "unlabelStmt",
            format!("change `continue {name}` to `break`"),
        );
    }
}

fn check_unlabel_stmt_func(f: &FuncDecl, pending: &mut Vec<(u32, String)>) {
    let Some(body) = &f.body else {
        return;
    };
    if stmt_contains_goto(body) {
        return;
    }
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        if let Some(NodeRef::LabeledStmt(labeled)) = n {
            check_unlabel_stmt(labeled, pending);
        }
        true
    });
}

fn check_return_after_http_error(pass: &Pass<'_>, stmt: &IfStmt, pending: &mut Vec<(u32, String)>) {
    let last = stmt.body.list.last();
    let Some(Stmt::ExprStmt(es)) = last else {
        return;
    };
    let Expr::CallExpr(call) = &es.x else {
        return;
    };
    let Some(name) = code::call_name(pass, &call.fun).or_else(|| call_qualified_name(call)) else {
        return;
    };
    if name != "http.Error" && !name.ends_with("/http.Error") {
        return;
    }
    if call.args.len() != 3 {
        return;
    }
    report(
        pending,
        call.args[0].pos().0 as u32,
        "returnAfterHttpError",
        "Possibly return is missed after the http.Error call",
    );
}

fn sync_mutex_embed_text(ty: &Expr) -> Option<String> {
    match ty {
        Expr::SelectorExpr(sel) => {
            let Expr::Ident(pkg) = sel.x.as_ref() else {
                return None;
            };
            if pkg.name != "sync" {
                return None;
            }
            if sel.sel.name == "Mutex" || sel.sel.name == "RWMutex" {
                return Some(format!("sync.{}", sel.sel.name));
            }
            None
        }
        Expr::StarExpr(s) => {
            let inner = sync_mutex_embed_text(&s.x)?;
            Some(format!("*{inner}"))
        }
        _ => None,
    }
}

fn check_exposed_sync_mutex(file: &File, pending: &mut Vec<(u32, String)>) {
    for decl in &file.decls {
        let Decl::GenDecl(g) = decl else {
            continue;
        };
        if g.tok != Some(Token::TYPE) {
            continue;
        }
        // The rules are written as `m.Match("type $x struct { …; sync.Mutex; … }")`,
        // which is a whole *declaration* pattern: it only matches a single-spec
        // `type` decl, and it reports at the `type` keyword — not at the field.
        let [Spec::TypeSpec(ts)] = g.specs.as_slice() else {
            continue;
        };
        if !is_exported(&ts.name.name) {
            continue;
        }
        let Expr::StructType(st) = &ts.ty else {
            continue;
        };
        for field in &st.fields.list {
            if !field.names.is_empty() {
                continue; // only embedded fields
            }
            let Some(ty) = &field.ty else {
                continue;
            };
            if let Some(text) = sync_mutex_embed_text(ty) {
                report(
                    pending,
                    g.tok_pos.0 as u32,
                    "exposedSyncMutex",
                    format!("don't embed {text}"),
                );
            }
        }
    }
}

// badLock / externalErrorReassign / uncheckedInlineErr / boolExprSimplify ------

fn mutex_method_call(call: &CallExpr) -> Option<(&Expr, &str)> {
    if !call.args.is_empty() {
        return None;
    }
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return None;
    };
    match sel.sel.name.as_str() {
        "Lock" | "Unlock" | "RLock" | "RUnlock" => Some((&sel.x, sel.sel.name.as_str())),
        _ => None,
    }
}

fn stmt_mutex_call(stmt: &Stmt) -> Option<(&Expr, &str, bool /* deferred */, u32)> {
    match stmt {
        Stmt::ExprStmt(es) => {
            let Expr::CallExpr(call) = &es.x else {
                return None;
            };
            let (recv, method) = mutex_method_call(call)?;
            Some((recv, method, false, es.x.pos().0 as u32))
        }
        Stmt::DeferStmt(d) => {
            let (recv, method) = mutex_method_call(&d.call)?;
            Some((recv, method, true, d.call.pos().0 as u32))
        }
        _ => None,
    }
}

fn check_bad_lock(stmts: &[Stmt], pending: &mut Vec<(u32, String)>) {
    for window in stmts.windows(2) {
        let Some((mu1, m1, deferred1, _)) = stmt_mutex_call(&window[0]) else {
            continue;
        };
        if deferred1 {
            continue;
        }
        let Some((mu2, m2, deferred2, pos2)) = stmt_mutex_call(&window[1]) else {
            continue;
        };
        if !exprs_equal(mu1, mu2) {
            continue;
        }
        let mu_t = expr_text(mu1).unwrap_or_else(|| "mu".into());
        match (m1, m2, deferred2) {
            ("Lock", "Unlock", false) | ("RLock", "RUnlock", false) => {
                report(
                    pending,
                    pos2,
                    "badLock",
                    "defer is missing, mutex is unlocked immediately",
                );
            }
            ("Lock", "RUnlock", true) => {
                report(
                    pending,
                    pos2,
                    "badLock",
                    "suspicious unlock, maybe Unlock was intended?",
                );
            }
            ("RLock", "Unlock", true) => {
                report(
                    pending,
                    pos2,
                    "badLock",
                    "suspicious unlock, maybe RUnlock was intended?",
                );
            }
            ("Lock", "Lock", true) => {
                report(
                    pending,
                    pos2,
                    "badLock",
                    format!("maybe defer {mu_t}.Unlock() was intended?"),
                );
            }
            ("RLock", "RLock", true) => {
                report(
                    pending,
                    pos2,
                    "badLock",
                    format!("maybe defer {mu_t}.RUnlock() was intended?"),
                );
            }
            _ => {}
        }
    }
}

/// `typep.HasBoolKind`: the type is `*types.Basic` **with kind exactly
/// `types.Bool`**.
///
/// Not "has the boolean property" — `types.UntypedBool` is a different kind and
/// fails this, which is load-bearing rather than pedantic. A comparison keeps
/// its untyped-bool type wherever nothing gives it a typed context, and the one
/// place that happens for whole expressions is an `if` or `for` **condition**:
/// `_ = x+1 > y` is `bool` (the assignment defaults it) and `if x+1 > y` is
/// `untyped bool`. So `boolExprSimplify` says nothing about a condition that is
/// exactly one comparison, and does report `if x+1 > y && ok`, where the `ok`
/// operand makes the whole condition typed.
fn type_is_boolean(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ).underlying(&artifacts.types);
    let TypeData::Basic(b) = artifacts.types.get(typ) else {
        return false;
    };
    b.kind() == BasicKind::Bool
}

fn type_has_float(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ).underlying(&artifacts.types);
    let TypeData::Basic(b) = artifacts.types.get(typ) else {
        return false;
    };
    b.info().contains(IS_FLOAT)
}

fn universe_error_type(pass: &Pass<'_>) -> Option<TypeId> {
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

fn implements_error(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(err) = universe_error_type(pass) else {
        return false;
    };
    type_implements(pass, typ, err)
}

fn check_external_error_reassign(
    pass: &Pass<'_>,
    assign: &AssignStmt,
    pending: &mut Vec<(u32, String)>,
) {
    if assign.tok != Some(Token::ASSIGN) || assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return;
    }
    let Expr::SelectorExpr(sel) = &assign.lhs[0] else {
        return;
    };
    let Expr::Ident(pkg) = sel.x.as_ref() else {
        return;
    };
    if !is_pkg_name(pass, pkg) {
        return;
    }
    let Some(typ) = type_of(pass, &assign.lhs[0]) else {
        return;
    };
    if !implements_error(pass, typ) {
        return;
    }
    report(
        pending,
        assign_pos(assign),
        "externalErrorReassign",
        "suspicious reassignment of error from another package",
    );
}

fn ident_type(pass: &Pass<'_>, id: &Ident) -> Option<TypeId> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if let Some(Some(oid)) = info.defs.get(&id.id).copied() {
        if let ObjectData::Var(v) = artifacts.objects.get(oid) {
            return Some(v.typ());
        }
    }
    if let Some(oid) = info.uses.get(&id.id).copied() {
        if let ObjectData::Var(v) = artifacts.objects.get(oid) {
            return Some(v.typ());
        }
    }
    info.types.get(&id.id).map(|tav| tav.typ)
}

fn check_unchecked_inline_err(pass: &Pass<'_>, ifs: &IfStmt, pending: &mut Vec<(u32, String)>) {
    let Some(Stmt::AssignStmt(init)) = ifs.init.as_deref() else {
        return;
    };
    if !matches!(init.tok, Some(Token::DEFINE) | Some(Token::ASSIGN)) {
        return;
    }
    if init.rhs.len() != 1 || !matches!(init.rhs[0], Expr::CallExpr(_)) {
        return;
    }
    // Last LHS is the returned error (`err` / `_, err`).
    let Some(err_lhs) = init.lhs.last() else {
        return;
    };
    let Expr::Ident(err_id) = err_lhs else {
        return;
    };
    let Some(err_typ) = ident_type(pass, err_id) else {
        return;
    };
    if !implements_error(pass, err_typ) {
        return;
    }

    let Expr::BinaryExpr(cond) = &ifs.cond else {
        return;
    };
    if cond.op != Token::NEQ {
        return;
    }
    let err2 = if is_nil_ident(&cond.y) {
        cond.x.as_ref()
    } else if is_nil_ident(&cond.x) {
        cond.y.as_ref()
    } else {
        return;
    };
    let Expr::Ident(err2_id) = err2 else {
        return;
    };
    if err_id.name == err2_id.name {
        return;
    }
    let Some(err2_typ) = ident_type(pass, err2_id) else {
        return;
    };
    if !implements_error(pass, err2_typ) {
        return;
    }
    report(
        pending,
        err_id.pos().0 as u32,
        "uncheckedInlineErr",
        format!(
            "{} error is unchecked, maybe intended to check it instead of {}",
            err_id.name, err2_id.name
        ),
    );
}

fn negate_cmp_op(op: Token) -> Option<&'static str> {
    match op {
        Token::EQL => Some("!="),
        Token::NEQ => Some("=="),
        Token::LSS => Some(">="),
        Token::GTR => Some("<="),
        Token::LEQ => Some(">"),
        Token::GEQ => Some("<"),
        _ => None,
    }
}

fn negate_op_str(op: &str) -> Option<&'static str> {
    match op {
        "==" => Some("!="),
        "!=" => Some("=="),
        "<" => Some(">="),
        ">" => Some("<="),
        "<=" => Some(">"),
        ">=" => Some("<"),
        _ => None,
    }
}

fn expr_contains_float_cmp(pass: &Pass<'_>, expr: &Expr) -> bool {
    match expr {
        Expr::ParenExpr(p) => expr_contains_float_cmp(pass, &p.x),
        Expr::UnaryExpr(u) => expr_contains_float_cmp(pass, &u.x),
        Expr::BinaryExpr(b) => {
            if matches!(
                b.op,
                Token::EQL | Token::NEQ | Token::LSS | Token::GTR | Token::LEQ | Token::GEQ
            ) {
                if type_of(pass, &b.x).is_some_and(|t| type_has_float(pass, t))
                    || type_of(pass, &b.y).is_some_and(|t| type_has_float(pass, t))
                {
                    return true;
                }
            }
            expr_contains_float_cmp(pass, &b.x) || expr_contains_float_cmp(pass, &b.y)
        }
        _ => false,
    }
}

/// Approximate `typep.SideEffectFree` for combineChecks / foldRanges.
fn expr_is_safe(expr: &Expr) -> bool {
    match unparen_expr(expr) {
        Expr::Ident(_) | Expr::BasicLit(_) => true,
        Expr::SelectorExpr(s) => expr_is_safe(&s.x),
        Expr::IndexExpr(i) => expr_is_safe(&i.x) && expr_is_safe(&i.index),
        Expr::StarExpr(s) => expr_is_safe(&s.x),
        Expr::UnaryExpr(u)
            if matches!(
                u.op,
                Token::ADD | Token::SUB | Token::NOT | Token::XOR | Token::AND | Token::MUL
            ) =>
        {
            expr_is_safe(&u.x)
        }
        Expr::BinaryExpr(b) => expr_is_safe(&b.x) && expr_is_safe(&b.y),
        Expr::ParenExpr(p) => expr_is_safe(&p.x),
        _ => false,
    }
}

fn basic_lit_is_one(expr: &Expr) -> bool {
    matches!(unparen_expr(expr), Expr::BasicLit(lit) if lit.value == "1")
}

fn int64_lit(expr: &Expr) -> Option<i64> {
    let Expr::BasicLit(lit) = unparen_expr(expr) else {
        return None;
    };
    lit.value.parse().ok()
}

/// `removeIncDec`: `x > y-1` → `x >= y`, `x+1 > y` → `x >= y`, etc.
fn try_remove_inc_dec(cmp: &BinaryExpr) -> Option<(String, &'static str, String)> {
    let match_one_way = |op: Token, x: &Expr, y: &Expr| -> bool {
        let Expr::BinaryExpr(xb) = unparen_expr(x) else {
            return false;
        };
        if xb.op != op || !basic_lit_is_one(&xb.y) {
            return false;
        }
        // Balanced ±1 on both sides is intentional — skip.
        if let Expr::BinaryExpr(yb) = unparen_expr(y) {
            if yb.op == op && basic_lit_is_one(&yb.y) {
                return false;
            }
        }
        true
    };

    let replace = |lhs_op: Token,
                   rhs_op: Token,
                   replacement: &'static str|
     -> Option<(String, &'static str, String)> {
        // `matchOneWay(lhsOp, lhs, rhs)` → strip ±1 from left
        if match_one_way(lhs_op, &cmp.x, &cmp.y) {
            let Expr::BinaryExpr(lhs) = unparen_expr(&cmp.x) else {
                return None;
            };
            return Some((expr_text(&lhs.x)?, replacement, expr_text(&cmp.y)?));
        }
        // `matchOneWay(rhsOp, rhs, lhs)` → strip ±1 from right
        if match_one_way(rhs_op, &cmp.y, &cmp.x) {
            let Expr::BinaryExpr(rhs) = unparen_expr(&cmp.y) else {
                return None;
            };
            return Some((expr_text(&cmp.x)?, replacement, expr_text(&rhs.x)?));
        }
        None
    };

    match cmp.op {
        // `x > y-1` / `x+1 > y` → `x >= y`
        Token::GTR => replace(Token::ADD, Token::SUB, ">="),
        // `x >= y+1` / `x-1 >= y` → `x > y`
        Token::GEQ => replace(Token::SUB, Token::ADD, ">"),
        // `x < y+1` / `x-1 < y` → `x <= y`
        Token::LSS => replace(Token::SUB, Token::ADD, "<="),
        // `x <= y-1` / `x+1 <= y` → `x < y`
        Token::LEQ => replace(Token::ADD, Token::SUB, "<"),
        _ => None,
    }
}

/// `foldRanges`: `x > 10 && x < 12` → `x == 11`, `x < 11 || x > 11` → `x != 11`.
fn try_fold_ranges(e: &BinaryExpr, has_floats: bool) -> Option<String> {
    if has_floats {
        return None;
    }
    let lhs = match unparen_expr(&e.x) {
        Expr::BinaryExpr(b) => b,
        _ => return None,
    };
    let rhs = match unparen_expr(&e.y) {
        Expr::BinaryExpr(b) => b,
        _ => return None,
    };
    if !expr_is_safe(&lhs.x) || !expr_is_safe(&rhs.x) {
        return None;
    }
    if !exprs_equal(&lhs.x, &rhs.x) {
        return None;
    }
    let c1 = int64_lit(&lhs.y)?;
    let c2 = int64_lit(&rhs.y)?;

    // (lhsOp, rhsOp, rhsDiff=c2-c1, resDelta)
    let match_comb =
        |lhs_op: Token, rhs_op: Token, rhs_diff: i64, res_delta: i64| -> Option<String> {
            if lhs.op != lhs_op || rhs.op != rhs_op {
                return None;
            }
            if c2 - c1 != rhs_diff {
                return None;
            }
            let x = expr_text(&lhs.x)?;
            let v = c1 + res_delta;
            let op = match e.op {
                Token::LAND => "==",
                Token::LOR => "!=",
                _ => return None,
            };
            Some(format!("{x} {op} {v}"))
        };

    match e.op {
        Token::LAND => {
            // `x > c && x < c+2` → `x == c+1`
            match_comb(Token::GTR, Token::LSS, 2, 1)
                // `x >= c && x < c+1` → `x == c`
                .or_else(|| match_comb(Token::GEQ, Token::LSS, 1, 0))
                // `x > c && x <= c+1` → `x == c+1`
                .or_else(|| match_comb(Token::GTR, Token::LEQ, 1, 1))
                // `x >= c && x <= c` → `x == c`
                .or_else(|| match_comb(Token::GEQ, Token::LEQ, 0, 0))
        }
        Token::LOR => {
            // `x < c || x > c` → `x != c`
            match_comb(Token::LSS, Token::GTR, 0, 0)
                // `x <= c || x > c+1` → `x != c+1`
                .or_else(|| match_comb(Token::LEQ, Token::GTR, 1, 1))
                // `x < c || x >= c+1` → `x != c`
                .or_else(|| match_comb(Token::LSS, Token::GEQ, 1, 0))
                // `x <= c || x >= c+2` → `x != c+1`
                .or_else(|| match_comb(Token::LEQ, Token::GEQ, 2, 1))
        }
        _ => None,
    }
}

fn simplify_bool_expr(expr: &Expr, has_floats: bool) -> Option<String> {
    let expr = unparen_expr(expr);
    match expr {
        Expr::UnaryExpr(u) if u.op == Token::NOT => {
            let x = unparen_expr(&u.x);
            // doubleNegation: !!x → x
            if let Expr::UnaryExpr(u2) = x {
                if u2.op == Token::NOT {
                    let inner = unparen_expr(&u2.x);
                    return simplify_bool_expr(inner, has_floats).or_else(|| expr_text(inner));
                }
            }
            // invertComparison: !(a op b) → a negated_op b
            // Apply removeIncDec on the comparison first (upstream Apply post-order).
            if !has_floats {
                if let Expr::BinaryExpr(cmp) = x {
                    if let Some((lx, op, ly)) = try_remove_inc_dec(cmp) {
                        if let Some(neg) = negate_op_str(op) {
                            return Some(format!("{lx} {neg} {ly}"));
                        }
                    } else if let Some(neg) = negate_cmp_op(cmp.op) {
                        let lx =
                            simplify_bool_expr(&cmp.x, has_floats).or_else(|| expr_text(&cmp.x))?;
                        let ly =
                            simplify_bool_expr(&cmp.y, has_floats).or_else(|| expr_text(&cmp.y))?;
                        return Some(format!("{lx} {neg} {ly}"));
                    }
                }
            }
            // nested: !{simplified}
            if let Some(sx) = simplify_bool_expr(&u.x, has_floats) {
                let orig_x = expr_text(&u.x)?;
                if sx != orig_x {
                    return Some(format!("!{sx}"));
                }
            }
            None
        }
        Expr::BinaryExpr(b) => {
            // negatedEquals: !x == !y → x == y
            if b.op == Token::EQL {
                let lx = unparen_expr(&b.x);
                let ry = unparen_expr(&b.y);
                if let (Expr::UnaryExpr(nx), Expr::UnaryExpr(ny)) = (lx, ry) {
                    if nx.op == Token::NOT && ny.op == Token::NOT {
                        let x =
                            simplify_bool_expr(&nx.x, has_floats).or_else(|| expr_text(&nx.x))?;
                        let y =
                            simplify_bool_expr(&ny.x, has_floats).or_else(|| expr_text(&ny.x))?;
                        return Some(format!("{x} == {y}"));
                    }
                }
            }
            // combineChecks: x > y || x == y → x >= y (and permutations)
            if b.op == Token::LOR {
                let lhs = unparen_expr(&b.x);
                let rhs = unparen_expr(&b.y);
                if let (Expr::BinaryExpr(l), Expr::BinaryExpr(r)) = (lhs, rhs) {
                    if exprs_equal(&l.x, &r.x)
                        && exprs_equal(&l.y, &r.y)
                        && expr_is_safe(&l.x)
                        && expr_is_safe(&l.y)
                    {
                        let comb = match (l.op, r.op) {
                            (Token::GTR, Token::EQL) | (Token::EQL, Token::GTR) => Some(">="),
                            (Token::LSS, Token::EQL) | (Token::EQL, Token::LSS) => Some("<="),
                            _ => None,
                        };
                        if let Some(op) = comb {
                            let x = expr_text(&l.x)?;
                            let y = expr_text(&l.y)?;
                            return Some(format!("{x} {op} {y}"));
                        }
                    }
                }
            }
            // removeIncDec
            if let Some((lx, op, ly)) = try_remove_inc_dec(b) {
                return Some(format!("{lx} {op} {ly}"));
            }
            // foldRanges
            if let Some(s) = try_fold_ranges(b, has_floats) {
                return Some(s);
            }
            // Recurse into sides then rebuild if either side simplified.
            let sx = simplify_bool_expr(&b.x, has_floats);
            let sy = simplify_bool_expr(&b.y, has_floats);
            if sx.is_some() || sy.is_some() {
                let lx = sx.or_else(|| expr_text(&b.x))?;
                let ly = sy.or_else(|| expr_text(&b.y))?;
                return Some(format!("{lx} {} {ly}", b.op.as_str()));
            }
            None
        }
        _ => None,
    }
}

/// `reported_end` is the end offset of the last expression this checker warned
/// about, and suppresses reports on that expression's operands.
///
/// Upstream simplifies an expression *recursively* and warns once, on the
/// outermost node — `(a >= b+1) && x` is reported as `(a > b) && x`, not as a
/// warning on the outer expression plus a second one on `a >= b+1`. The walk is
/// pre-order, so any expression starting before the last reported expression
/// ends is one of its operands.
fn check_bool_expr_simplify(
    pass: &Pass<'_>,
    expr: &Expr,
    reported_end: &mut u32,
    pending: &mut Vec<(u32, String)>,
) {
    if !matches!(expr, Expr::UnaryExpr(_) | Expr::BinaryExpr(_)) {
        return;
    }
    if (expr.pos().0 as u32) < *reported_end {
        return;
    }
    let Some(typ) = type_of(pass, expr) else {
        return;
    };
    if !type_is_boolean(pass, typ) {
        return;
    }
    let has_floats = expr_contains_float_cmp(pass, expr);
    let Some(orig) = expr_text(expr) else {
        return;
    };
    let Some(simplified) = simplify_bool_expr(expr, has_floats) else {
        return;
    };
    if simplified == orig {
        return;
    }
    // The "before" half of the message is the untouched source expression, so
    // render it through go/printer: gofmt drops the blanks around a nested
    // higher-precedence operator (`a < b+1`, not `a < b + 1`) and [`expr_text`]
    // does not. `simplified` is built as a string by the rewriter above, hence
    // the guard still compares the two `expr_text` renderings.
    let orig = node_text(pass, expr).unwrap_or(orig);
    *reported_end = expr.end().0 as u32;
    report(
        pending,
        expr.pos().0 as u32,
        "boolExprSimplify",
        format!("can simplify `{orig}` to `{simplified}`"),
    );
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
    let params = options.check_settings.clone();

    let mut pending = Vec::new();
    let mut if_else_visited = HashSet::new();
    // Track pointer identity for if-else via id; also use a map for id==0 cases.
    let mut if_else_ptr: HashMap<usize, ()> = HashMap::new();
    let mut type_assert_visited: HashSet<usize> = HashSet::new();
    // End offset of the last expression `boolExprSimplify` reported — see
    // [`check_bool_expr_simplify`].
    let mut bool_expr_reported_end: u32 = 0;

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
        if enabled(&set, "typeUnparen") {
            check_type_unparen_file(file, &mut pending);
        }
        if enabled(&set, "importShadow") {
            let imports = collect_import_names(file);
            for decl in &file.decls {
                if let Decl::FuncDecl(f) = decl {
                    check_import_shadow_func(pass, f, &imports, &mut pending);
                }
            }
        }
        if enabled(&set, "exposedSyncMutex") {
            check_exposed_sync_mutex(file, &mut pending);
        }
        if enabled(&set, "unlabelStmt") {
            for decl in &file.decls {
                if let Decl::FuncDecl(f) = decl {
                    check_unlabel_stmt_func(f, &mut pending);
                }
            }
        }

        if enabled(&set, "typeDefFirst") {
            check_type_def_first(file, &mut pending);
        }
        if enabled(&set, "deferInLoop") {
            for decl in &file.decls {
                if let Decl::FuncDecl(f) = decl {
                    check_defer_in_loop_func(f, &mut pending);
                }
            }
        }
        if enabled(&set, "unnecessaryDefer") {
            for decl in &file.decls {
                if let Decl::FuncDecl(f) = decl {
                    check_unnecessary_defer_func(pass, f, &mut pending);
                }
            }
        }

        // `rangeValCopy` / `rangeExprCopy` prune unit-test functions in
        // `EnterFunc`, which stops upstream's walk from descending into them.
        // guff walks the file flat, so record the body spans up front and skip
        // range statements that fall inside one.
        let mut test_func_bodies: Vec<(u32, u32)> = Vec::new();
        if enabled(&set, "rangeValCopy") || enabled(&set, "rangeExprCopy") {
            for decl in &file.decls {
                if let Decl::FuncDecl(f) = decl {
                    if let Some(body) = f.body.as_ref() {
                        if is_unit_test_func(pass, f) {
                            test_func_bodies.push((body.lbrace.0 as u32, body.rbrace.0 as u32));
                        }
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
                    if enabled(&set, "dupBranchBody") {
                        check_dup_branch_body(s, &mut pending);
                    }
                    if enabled(&set, "ifElseChain") {
                        let key = s as *const _ as usize;
                        if if_else_ptr.insert(key, ()).is_none() {
                            check_if_else_chain(
                                s,
                                params.if_else_chain_min_threshold,
                                &mut if_else_visited,
                                &mut pending,
                            );
                        }
                    }
                    if enabled(&set, "nilValReturn") {
                        check_nil_val_return(pass, s, &mut pending);
                    }
                    if enabled(&set, "initClause") {
                        check_init_clause("if", s.init.as_deref(), s.if_.0 as u32, &mut pending);
                    }
                    if enabled(&set, "typeAssertChain") {
                        check_type_assert_chain(s, &mut type_assert_visited, &mut pending);
                    }
                    if enabled(&set, "sloppyReassign") {
                        check_sloppy_reassign(s, &mut pending);
                    }
                    if enabled(&set, "returnAfterHttpError") {
                        check_return_after_http_error(pass, s, &mut pending);
                    }
                    if enabled(&set, "uncheckedInlineErr") {
                        check_unchecked_inline_err(pass, s, &mut pending);
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
                NodeRef::ForStmt(s) => {
                    if enabled(&set, "badCond") {
                        check_bad_cond_for(s, &mut pending);
                    }
                    if enabled(&set, "nestingReduce") {
                        check_nesting_reduce_for(&s.body, &mut pending);
                    }
                    if enabled(&set, "sliceClear") {
                        check_slice_clear(s, &mut pending);
                    }
                }
                NodeRef::RangeStmt(s) => {
                    if enabled(&set, "rangeAppendAll") {
                        check_range_append_all(pass, s, &mut pending);
                    }
                    let in_test_func = test_func_bodies
                        .iter()
                        .any(|(lo, hi)| s.for_.0 as u32 >= *lo && (s.for_.0 as u32) <= *hi);
                    if enabled(&set, "rangeExprCopy") && !in_test_func {
                        check_range_expr_copy(pass, s, &mut pending);
                    }
                    if enabled(&set, "rangeValCopy") && !in_test_func {
                        check_range_val_copy(pass, s, &mut pending);
                    }
                    if enabled(&set, "nestingReduce") {
                        check_nesting_reduce_for(&s.body, &mut pending);
                    }
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
                    if enabled(&set, "truncateCmp") {
                        check_truncate_cmp(pass, b, &mut pending);
                    }
                    if enabled(&set, "preferFilepathJoin") {
                        check_prefer_filepath_join(pass, b, &mut pending);
                    }
                    if enabled(&set, "stringsCompare") {
                        check_strings_compare(pass, b, &mut pending);
                    }
                    if enabled(&set, "stringXbytes") {
                        check_string_xbytes(pass, NodeRef::BinaryExpr(b), &mut pending);
                    }
                    if enabled(&set, "equalFold") {
                        check_equal_fold_strings(pass, b, &mut pending);
                    }
                    if enabled(&set, "timeExprSimplify") {
                        check_time_expr_simplify(pass, b, &mut pending);
                    }
                    if enabled(&set, "boolExprSimplify") {
                        check_bool_expr_simplify(
                            pass,
                            &Expr::BinaryExpr(b.clone()),
                            &mut bool_expr_reported_end,
                            &mut pending,
                        );
                    }
                }
                NodeRef::UnaryExpr(u) if enabled(&set, "boolExprSimplify") => {
                    check_bool_expr_simplify(
                        pass,
                        &Expr::UnaryExpr(u.clone()),
                        &mut bool_expr_reported_end,
                        &mut pending,
                    );
                }
                NodeRef::BasicLit(lit) => {
                    if enabled(&set, "octalLiteral") {
                        check_octal_literal(lit, &mut pending);
                    }
                    if enabled(&set, "hexLiteral") {
                        check_hex_literal(lit, &mut pending);
                    }
                }
                NodeRef::DeferStmt(d) if enabled(&set, "deferUnlambda") => {
                    check_defer_unlambda(pass, d, &mut pending);
                }
                NodeRef::GenDecl(g) if enabled(&set, "emptyDecl") => {
                    check_empty_decl(g, &mut pending);
                }
                NodeRef::SliceExpr(s) if enabled(&set, "unslice") => {
                    check_unslice(pass, s, &mut pending);
                }
                NodeRef::IndexExpr(ix) => {
                    if enabled(&set, "offBy1") {
                        check_off_by1(ix, &mut pending);
                    }
                    if enabled(&set, "preferDecodeRune") {
                        check_prefer_decode_rune(pass, ix, &mut pending);
                    }
                }
                NodeRef::CompositeLit(lit) if enabled(&set, "mapKey") => {
                    check_map_key(lit, &mut pending);
                }
                NodeRef::FuncLit(fl) if enabled(&set, "unlambda") => {
                    check_unlambda(pass, fl, &mut pending);
                }
                NodeRef::StarExpr(s) => {
                    if enabled(&set, "newDeref") {
                        check_new_deref(s, &mut pending);
                    }
                    if enabled(&set, "flagDeref") {
                        check_flag_deref(pass, s, &mut pending);
                    }
                }
                NodeRef::AssignStmt(a) => {
                    if enabled(&set, "appendAssign") {
                        check_append_assign(a, &mut pending);
                    }
                    if enabled(&set, "assignOp") {
                        check_assign_op(a, &mut pending);
                    }
                    if enabled(&set, "sqlQuery") {
                        check_sql_query(pass, a, &mut pending);
                    }
                    if enabled(&set, "badSorting") {
                        check_bad_sorting(pass, a, &mut pending);
                    }
                    if enabled(&set, "externalErrorReassign") {
                        check_external_error_reassign(pass, a, &mut pending);
                    }
                }
                NodeRef::FuncDecl(f) => {
                    if enabled(&set, "captLocal") {
                        check_capt_local(f, &mut pending);
                    }
                    if enabled(&set, "exitAfterDefer") {
                        check_exit_after_defer(pass, f, &mut pending);
                    }
                    if enabled(&set, "builtinShadow") {
                        check_builtin_shadow_func(pass, f, &mut pending);
                    }
                    if enabled(&set, "paramTypeCombine") {
                        check_param_type_combine(pass, f, &mut pending);
                    }
                    if enabled(&set, "unnamedResult") {
                        check_unnamed_result(f, params.unnamed_result_check_exported, &mut pending);
                    }
                    if enabled(&set, "hugeParam") {
                        check_huge_param(pass, f, &mut pending);
                    }
                    if enabled(&set, "ptrToRefParam") {
                        check_ptr_to_ref_param(pass, f, &mut pending);
                    }
                    if enabled(&set, "tooManyResultsChecker") {
                        check_too_many_results(f, params.too_many_results_max, &mut pending);
                    }
                }
                NodeRef::ReturnStmt(r) if enabled(&set, "evalOrder") => {
                    check_eval_order(pass, r, &mut pending);
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
                    if enabled(&set, "dupOption") {
                        check_dup_option(pass, c, &mut pending);
                    }
                    if enabled(&set, "methodExprCall") {
                        check_method_expr_call(pass, c, &mut pending);
                    }
                    if enabled(&set, "regexpPattern") {
                        check_regexp_pattern(pass, c, &mut pending);
                    }
                    if enabled(&set, "badRegexp") {
                        check_bad_regexp(pass, c, &mut pending);
                    }
                    if enabled(&set, "regexpSimplify") {
                        check_regexp_simplify(pass, c, &mut pending);
                    }
                    if enabled(&set, "sortSlice") {
                        check_sort_slice(pass, c, &mut pending);
                    }
                    if enabled(&set, "httpNoBody") {
                        check_http_no_body(pass, c, &mut pending);
                    }
                    if enabled(&set, "indexAlloc") {
                        check_index_alloc(pass, c, &mut pending);
                    }
                    if enabled(&set, "preferWriteByte") {
                        check_prefer_write_byte(pass, c, &mut pending);
                    }
                    if enabled(&set, "preferFprint") {
                        check_prefer_fprint(pass, c, &mut pending);
                    }
                    if enabled(&set, "preferStringWriter") {
                        check_prefer_string_writer(pass, c, &mut pending);
                    }
                    if enabled(&set, "dynamicFmtString") {
                        check_dynamic_fmt_string(pass, c, &mut pending);
                    }
                    if enabled(&set, "stringConcatSimplify") {
                        check_string_concat_simplify(pass, c, &mut pending);
                    }
                    if enabled(&set, "badSyncOnceFunc") {
                        check_bad_sync_once_func_call(pass, c, &mut pending);
                    }
                    if enabled(&set, "zeroByteRepeat") {
                        check_zero_byte_repeat(pass, c, &mut pending);
                    }
                    if enabled(&set, "equalFold") {
                        check_equal_fold_bytes(pass, c, &mut pending);
                    }
                    if enabled(&set, "sprintfQuotedString") {
                        check_sprintf_quoted_string(pass, c, &mut pending);
                    }
                    if enabled(&set, "redundantSprint") {
                        check_redundant_sprint(pass, c, &mut pending);
                    }
                    if enabled(&set, "stringXbytes") {
                        check_string_xbytes(pass, NodeRef::CallExpr(c), &mut pending);
                    }
                }
                NodeRef::SelectorExpr(sel) if enabled(&set, "underef") => {
                    check_underef(pass, sel, &mut pending);
                }
                NodeRef::TypeAssertExpr(a) if enabled(&set, "sloppyTypeAssert") => {
                    check_sloppy_type_assert(pass, a, &mut pending);
                }
                NodeRef::BlockStmt(b) => {
                    if enabled(&set, "unnecessaryBlock") {
                        check_unnecessary_block_in_list(&b.list, &mut pending);
                    }
                    if enabled(&set, "syncMapLoadAndDelete") {
                        check_sync_map_load_and_delete(pass, &b.list, &mut pending);
                    }
                    if enabled(&set, "badSyncOnceFunc") {
                        check_bad_sync_once_func_stmts(pass, &b.list, &mut pending);
                    }
                    if enabled(&set, "appendCombine") {
                        check_append_combine(&b.list, &mut pending);
                    }
                    if enabled(&set, "badLock") {
                        check_bad_lock(&b.list, &mut pending);
                    }
                }
                NodeRef::CaseClause(c) => {
                    if enabled(&set, "unnecessaryBlock") {
                        check_unnecessary_block_case(&c.body, &mut pending);
                    }
                    if enabled(&set, "appendCombine") {
                        check_append_combine(&c.body, &mut pending);
                    }
                    if enabled(&set, "badLock") {
                        check_bad_lock(&c.body, &mut pending);
                    }
                }
                NodeRef::CommClause(c) => {
                    if enabled(&set, "unnecessaryBlock") {
                        check_unnecessary_block_case(&c.body, &mut pending);
                    }
                    if enabled(&set, "appendCombine") {
                        check_append_combine(&c.body, &mut pending);
                    }
                    if enabled(&set, "badLock") {
                        check_bad_lock(&c.body, &mut pending);
                    }
                }
                _ => {}
            }
            true
        });
    }

    run_comment_checks(pass, &set, &mut pending);

    // go-critic runs one checker at a time, in checker-name order, so its
    // warnings reach golangci-lint grouped by checker. That order is load
    // bearing: `issues.uniq-by-line` (on by default) keeps only the *first*
    // gocritic issue per source line, so when two checkers fire on one line the
    // alphabetically earlier name wins — `tooManyResultsChecker` over
    // `unnamedResult`, `preferFprint` over `preferStringWriter`. guff walks
    // every checker in a single pass, so the tiebreak has to be restored here.
    //
    // Sorting by (line, checker) rather than by checker alone picks the same
    // winner per line while leaving the emitted order in source order — grouping
    // a file's findings by checker would be the faithful *pipeline* order, but
    // golangci-lint sorts before display and guff does not. Within one checker
    // the sort is stable, so walk order decides (two `captLocal` names on one
    // line keep their left-to-right order, as upstream does).
    let line_of = |pos: u32| {
        let p = pass.fset().position(Pos(pos as i64));
        (p.filename, p.line)
    };
    pending.sort_by(|a, b| {
        line_of(a.0)
            .cmp(&line_of(b.0))
            .then_with(|| checker_of(&a.1).cmp(checker_of(&b.1)))
    });

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

#[cfg(test)]
mod selector_tests {
    use super::*;

    #[test]
    fn gocritic_default_checks_are_exactly_the_untagged_ones() {
        // `DEFAULT_CHECKS` is a hand-written list; upstream has no such list at
        // all, only the predicate. Keeping the two in step by hand is what
        // fails silently when a checker is ported — so assert the derivation
        // instead of the list.
        let mut derived: Vec<&str> = implemented_checks().filter(|n| is_enabled_by_default(n)).collect();
        derived.sort_unstable();
        let mut declared: Vec<&str> = DEFAULT_CHECKS.to_vec();
        declared.sort_unstable();
        assert_eq!(derived, declared);
    }

    #[test]
    fn gocritic_every_implemented_check_has_tags() {
        // A checker with no tags is enabled by default (it carries no opt-in
        // tag) and can never be reached by `enabled-tags` — the two halves of
        // the same omission, and neither one looks like a bug from the outside.
        let untagged: Vec<&str> = implemented_checks()
            .filter(|n| check_tags(n).is_empty())
            .collect();
        assert!(untagged.is_empty(), "checkers with no tags: {untagged:?}");
    }
}
