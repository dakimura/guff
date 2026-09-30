//! Shared if-else chain analysis (`superfluous-else` / `indent-error-flow` / `early-return`).

use guff::ast::{AssignStmt, BlockStmt, CallExpr, Expr, IfStmt, Stmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;
use crate::util::unparen;

/// Arguments shared by early-return / indent-error-flow / superfluous-else
/// (mirrors revive `internal/ifelse.Args`).
#[derive(Debug, Clone, Copy, Default)]
struct Args {
    /// Do not suggest refactorings that would enlarge variable scope.
    preserve_scope: bool,
    /// early-return only: allow introducing a new jump to reduce nesting.
    allow_jump: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BranchKind {
    Empty,
    Return,
    Continue,
    Break,
    Goto,
    Panic,
    Exit,
    Regular,
}

impl BranchKind {
    fn deviates(self) -> bool {
        !matches!(self, Self::Empty | Self::Regular)
    }

    fn is_empty(self) -> bool {
        self == Self::Empty
    }

    fn returns(self) -> bool {
        self == Self::Return
    }

    fn long_string(self) -> &'static str {
        match self {
            Self::Empty => "an empty block",
            Self::Regular => "a regular statement",
            Self::Return => "a return statement",
            Self::Continue => "a continue statement",
            Self::Break => "a break statement",
            Self::Goto => "a goto statement",
            // Panic / Exit never reach here: `Branch::long_string` renders them
            // from the call name (upstream `Branch.LongString`).
            Self::Panic => "a function call that panics",
            Self::Exit => "a function call that exits the program",
        }
    }
}

#[derive(Debug, Clone)]
struct Branch {
    kind: BranchKind,
    has_decls: bool,
    /// Name of the function called at the end of the branch, for `Panic` and
    /// `Exit`. Upstream's `Branch.Call` — both its `String` and `LongString`
    /// render it, so `panic()` and `os.Exit()` do not share a message.
    call: Option<String>,
}

impl Branch {
    fn is_empty(&self) -> bool {
        self.kind.is_empty()
    }

    fn short_string(&self) -> String {
        match self.kind {
            BranchKind::Empty => "{ }".into(),
            BranchKind::Regular => "{ ... }".into(),
            BranchKind::Return => "{ ... return }".into(),
            BranchKind::Continue => "{ ... continue }".into(),
            BranchKind::Break => "{ ... break }".into(),
            BranchKind::Goto => "{ ... goto }".into(),
            BranchKind::Panic | BranchKind::Exit => {
                format!("{{ ... {}() }}", self.call.as_deref().unwrap_or("panic"))
            }
        }
    }

    /// Upstream `Branch.LongString`: `call to <fn> function` for the two kinds
    /// that carry a call, the bare kind name otherwise.
    fn long_string(&self) -> String {
        match self.kind {
            BranchKind::Panic | BranchKind::Exit => {
                format!("call to {} function", self.call.as_deref().unwrap_or("panic"))
            }
            kind => kind.long_string().to_string(),
        }
    }

    fn is_short(&self) -> bool {
        // Approximation: empty blocks are short; detailed stmt analysis is DEFERRED.
        self.is_empty()
    }
}

struct Chain {
    if_branch: Branch,
    has_else: bool,
    else_branch: Branch,
    has_initializer: bool,
    has_prior_non_deviating: bool,
    at_block_end: bool,
    block_end_kind: BranchKind,
}

pub fn apply_indent_error_flow(pass: &Pass<'_>) -> Vec<Failure> {
    apply_one(pass, "indent-error-flow", indent_error_flow_args(pass), check_indent_error_flow)
}

pub fn apply_superfluous_else(pass: &Pass<'_>) -> Vec<Failure> {
    apply_one(pass, "superfluous-else", superfluous_else_args(pass), check_superfluous_else)
}

pub fn apply_early_return(pass: &Pass<'_>) -> Vec<Failure> {
    apply_one(pass, "early-return", early_return_args(pass), check_early_return)
}

fn indent_error_flow_args(pass: &Pass<'_>) -> Args {
    Args {
        preserve_scope: config::rule_has_string_option(pass, "indent-error-flow", "preserveScope"),
        allow_jump: false,
    }
}

fn superfluous_else_args(pass: &Pass<'_>) -> Args {
    Args {
        preserve_scope: config::rule_has_string_option(pass, "superfluous-else", "preserveScope"),
        allow_jump: false,
    }
}

fn early_return_args(pass: &Pass<'_>) -> Args {
    Args {
        preserve_scope: config::rule_has_string_option(pass, "early-return", "preserveScope"),
        allow_jump: config::rule_has_string_option(pass, "early-return", "allowJump"),
    }
}

/// Run all enabled ifelse-family rules in a single pruned file walk.
pub fn run_enabled(pass: &Pass<'_>) -> std::collections::HashMap<&'static str, Vec<Failure>> {
    let settings = config::effective_settings(pass);
    let all = config::all_rules();
    let enabled = |name: &str| settings.rule_enabled(name, config::DEFAULT_RULES, all);

    let mut rules: Vec<(&'static str, Args, fn(&Chain, Args) -> Option<String>)> = Vec::new();
    if enabled("indent-error-flow") {
        rules.push((
            "indent-error-flow",
            indent_error_flow_args(pass),
            check_indent_error_flow,
        ));
    }
    if enabled("superfluous-else") {
        rules.push((
            "superfluous-else",
            superfluous_else_args(pass),
            check_superfluous_else,
        ));
    }
    if enabled("early-return") {
        rules.push(("early-return", early_return_args(pass), check_early_return));
    }
    if rules.is_empty() {
        return std::collections::HashMap::new();
    }

    let mut by_rule: std::collections::HashMap<&'static str, Vec<Failure>> =
        rules.iter().map(|(n, _, _)| (*n, Vec::new())).collect();

    for (rule, args, check) in &rules {
        let failures = by_rule.get_mut(rule).unwrap();
        let mut v = Visitor { rule, args: *args, failures, check: *check };
        for file in pass.files() {
            v.walk(NodeRef::File(file));
        }
    }
    by_rule
}

fn apply_one(
    pass: &Pass<'_>,
    rule: &'static str,
    args: Args,
    check: fn(&Chain, Args) -> Option<String>,
) -> Vec<Failure> {
    let mut failures = Vec::new();
    let mut v = Visitor { rule, args, failures: &mut failures, check };
    for file in pass.files() {
        v.walk(NodeRef::File(file));
    }
    failures
}

/// Port of revive `internal/ifelse.visitor` (v1.15.0).
///
/// `Visit` claims six node kinds and walks everything else: a function body
/// (either kind) ends in a return, a `for` / `range` body in a continue, a
/// `case` clause in a break, any other block in nothing. Inside a claimed
/// block, `visitBlock` hands `if` statements to `visitIf` and walks every
/// other statement through `Visit` again — which is how an if-else chain
/// inside a func literal, a loop or a `switch` case is reached. guff used to
/// visit only the function bodies and the `if`s directly in them, so grype's
/// `if tt.wantErr { …; return } else { … }` inside a `t.Run` closure was
/// never looked at.
///
/// Two things fall out of that shape and are kept on purpose. An `if` met by
/// the walk rather than by `visitBlock` — the direct child of a `select`
/// clause, whose body is a statement list and not a block — is not a chain;
/// only its bodies are visited. And `visitIf` never walks the condition or
/// the initializer, so a func literal there is not visited.
struct Visitor<'f> {
    rule: &'static str,
    args: Args,
    failures: &'f mut Vec<Failure>,
    check: fn(&Chain, Args) -> Option<String>,
}

impl Visitor<'_> {
    /// `ast.Walk(v, node)` with `Visit`.
    fn walk(&mut self, node: NodeRef<'_>) {
        walk::inspect(node, |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::FuncDecl(f) => {
                    if let Some(body) = &f.body {
                        self.visit_block(&body.list, BranchKind::Return);
                    }
                    false
                }
                NodeRef::FuncLit(f) => {
                    self.visit_block(&f.body.list, BranchKind::Return);
                    false
                }
                NodeRef::ForStmt(f) => {
                    self.visit_block(&f.body.list, BranchKind::Continue);
                    false
                }
                NodeRef::RangeStmt(r) => {
                    self.visit_block(&r.body.list, BranchKind::Continue);
                    false
                }
                NodeRef::CaseClause(c) => {
                    self.visit_block(&c.body, BranchKind::Break);
                    false
                }
                NodeRef::BlockStmt(b) => {
                    self.visit_block(&b.list, BranchKind::Regular);
                    false
                }
                _ => true,
            }
        });
    }

    /// `visitBlock`: only the **last** statement's chain is at the block end,
    /// and only it carries the block's end kind — every other chain starts
    /// from the zero `Chain` (`Empty`), and passes that on to the blocks
    /// nested inside it.
    fn visit_block(&mut self, stmts: &[Stmt], end_kind: BranchKind) {
        for (i, stmt) in stmts.iter().enumerate() {
            let Stmt::IfStmt(if_stmt) = stmt else {
                self.walk(walk::stmt_ref(stmt));
                continue;
            };
            let mut chain = Chain::zero();
            if i + 1 == stmts.len() {
                chain.at_block_end = true;
                chain.block_end_kind = end_kind;
            }
            self.visit_if(if_stmt, chain);
        }
    }

    fn visit_if(&mut self, if_stmt: &IfStmt, mut chain: Chain) {
        // Other if-else chains nested inside this `if { }` block.
        self.visit_block(&if_stmt.body.list, chain.block_end_kind);

        if matches!(
            if_stmt.init.as_deref(),
            Some(Stmt::AssignStmt(AssignStmt {
                tok: Some(Token::DEFINE),
                ..
            }))
        ) {
            chain.has_initializer = true;
        }
        chain.if_branch = block_branch(&if_stmt.body);

        let Some(else_stmt) = &if_stmt.else_ else {
            if self.args.allow_jump {
                self.check_rule(if_stmt, None, &chain);
            }
            return;
        };

        match else_stmt.as_ref() {
            Stmt::IfStmt(else_if) => {
                if !chain.if_branch.kind.deviates() {
                    chain.has_prior_non_deviating = true;
                }
                self.visit_if(else_if, chain);
            }
            Stmt::BlockStmt(else_block) => {
                // Other if-else chains nested inside this `else { }` block.
                self.visit_block(&else_block.list, chain.block_end_kind);
                chain.has_else = true;
                chain.else_branch = block_branch(else_block);
                self.check_rule(if_stmt, Some(else_block), &chain);
            }
            _ => {}
        }
    }

    fn check_rule(&mut self, if_stmt: &IfStmt, else_block: Option<&BlockStmt>, chain: &Chain) {
        let Some(mut message) = (self.check)(chain, self.args) else {
            return;
        };
        if chain.has_initializer {
            message.push_str(" (move short variable declaration to its own line if necessary)");
        }
        // Upstream's ifelse framework lets each rule pick a target:
        // early-return points at the `if`, indent-error-flow and
        // superfluous-else point at the *else* (`ifelse.TargetElse`). Go's AST
        // has no position for the `else` keyword, so the else branch's own
        // Pos() — its `{` — is what gets reported.
        let pos = match else_block {
            Some(b) if target_else(self.rule) => b.lbrace.0 as u32,
            _ => if_stmt.if_.0 as u32,
        };
        self.failures.push(Failure {
            rule: self.rule,
            pos,
            message,
            ..Failure::default()
        });
    }
}

impl Chain {
    /// Go's zero `Chain`.
    fn zero() -> Self {
        let empty = Branch {
            kind: BranchKind::Empty,
            has_decls: false,
            call: None,
        };
        Chain {
            if_branch: empty.clone(),
            has_else: false,
            else_branch: empty,
            has_initializer: false,
            has_prior_non_deviating: false,
            at_block_end: false,
            block_end_kind: BranchKind::Empty,
        }
    }
}

/// Rules whose failure is attached to the else branch rather than the `if`
/// (`ifelse.TargetElse` upstream).
fn target_else(rule: &str) -> bool {
    matches!(rule, "indent-error-flow" | "superfluous-else")
}

fn block_branch(block: &BlockStmt) -> Branch {
    if block.list.is_empty() {
        return Branch {
            kind: BranchKind::Empty,
            has_decls: false,
            call: None,
        };
    }
    let mut branch = stmt_branch(block.list.last().expect("non-empty"));
    branch.has_decls = block_has_decls(&block.list);
    branch
}

fn block_has_decls(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::DeclStmt(_) => true,
        Stmt::AssignStmt(a) => a.tok == Some(Token::DEFINE),
        _ => false,
    })
}

fn stmt_branch(stmt: &Stmt) -> Branch {
    match stmt {
        Stmt::ReturnStmt(_) => Branch {
            kind: BranchKind::Return,
            has_decls: false,
            call: None,
        },
        Stmt::BranchStmt(b) => Branch {
            kind: match b.tok {
                Token::BREAK => BranchKind::Break,
                Token::CONTINUE => BranchKind::Continue,
                Token::GOTO => BranchKind::Goto,
                _ => BranchKind::Regular,
            },
            has_decls: false,
            call: None,
        },
        Stmt::BlockStmt(b) => block_branch(b),
        Stmt::ExprStmt(e) => {
            if let Some((kind, call)) = deviating_call(&e.x) {
                Branch {
                    kind,
                    has_decls: false,
                    call: Some(call),
                }
            } else {
                Branch {
                    kind: BranchKind::Regular,
                    has_decls: false,
                    call: None,
                }
            }
        }
        Stmt::EmptyStmt(_) => Branch {
            kind: BranchKind::Empty,
            has_decls: false,
            call: None,
        },
        Stmt::LabeledStmt(l) => stmt_branch(&l.stmt),
        _ => Branch {
            kind: BranchKind::Regular,
            has_decls: false,
            call: None,
        },
    }
}

/// The deviating call ending a branch, with the name upstream renders in the
/// message (`panic`, `os.Exit`, `log.Fatalf`, …).
fn deviating_call(expr: &Expr) -> Option<(BranchKind, String)> {
    let Expr::CallExpr(CallExpr { fun, .. }) = unparen(expr) else {
        return None;
    };
    match unparen(fun) {
        Expr::Ident(id) if id.name == "panic" => Some((BranchKind::Panic, "panic".into())),
        Expr::SelectorExpr(sel) => {
            let pkg = match unparen(&sel.x) {
                Expr::Ident(id) => id.name.as_str(),
                _ => return None,
            };
            let name = format!("{pkg}.{}", sel.sel.name);
            match (pkg, sel.sel.name.as_str()) {
                ("os", "Exit") => Some((BranchKind::Exit, name)),
                ("log", "Fatal" | "Fatalf" | "Fatalln" | "Panic" | "Panicf" | "Panicln") => {
                    let kind =
                        if matches!(sel.sel.name.as_str(), "Fatal" | "Fatalf" | "Fatalln") {
                            BranchKind::Exit
                        } else {
                            BranchKind::Panic
                        };
                    Some((kind, name))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn check_indent_error_flow(chain: &Chain, args: Args) -> Option<String> {
    if !chain.has_else || !chain.if_branch.kind.deviates() || chain.has_prior_non_deviating {
        return None;
    }
    if !chain.if_branch.kind.returns() {
        return None;
    }
    if args.preserve_scope
        && !chain.at_block_end
        && (chain.has_initializer || chain.else_branch.has_decls)
    {
        return None;
    }
    Some("if block ends with a return statement, so drop this else and outdent its block".into())
}

fn check_superfluous_else(chain: &Chain, args: Args) -> Option<String> {
    if !chain.has_else || !chain.if_branch.kind.deviates() || chain.has_prior_non_deviating {
        return None;
    }
    if chain.if_branch.kind.returns() {
        return None;
    }
    if args.preserve_scope
        && !chain.at_block_end
        && (chain.has_initializer || chain.else_branch.has_decls)
    {
        return None;
    }
    Some(format!(
        "if block ends with {}, so drop this else and outdent its block",
        chain.if_branch.long_string()
    ))
}

fn check_early_return(chain: &Chain, args: Args) -> Option<String> {
    if chain.has_else {
        if !chain.else_branch.kind.deviates() {
            return None;
        }
    } else if !args.allow_jump
        || !chain.at_block_end
        || !chain.block_end_kind.deviates()
        || chain.if_branch.is_short()
    {
        return None;
    }

    if chain.has_prior_non_deviating && !chain.if_branch.is_empty() {
        return None;
    }

    if chain.has_else && chain.if_branch.kind.deviates() {
        return None;
    }

    if args.preserve_scope
        && !chain.at_block_end
        && (chain.has_initializer || chain.if_branch.has_decls)
    {
        return None;
    }

    if !chain.has_else {
        return Some(format!(
            "if c {{ ... }} can be rewritten if !c {{ {} }} ... to reduce nesting",
            chain.block_end_kind.long_string()
        ));
    }

    let else_str = chain.else_branch.short_string();
    if chain.if_branch.is_empty() {
        return Some(format!(
            "if c {{ }} else {else_str} can be simplified to if !c {else_str}"
        ));
    }
    Some(format!(
        "if c {{ ... }} else {else_str} can be simplified to if !c {else_str} ..."
    ))
}
