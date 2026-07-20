//! `unreachable` — check for unreachable statements.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/unreachable`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{BlockStmt, CallExpr, Expr, FuncDecl, FuncLit, Ident, Stmt};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::expreq::unparen;

fn stmt_key(stmt: &Stmt) -> usize {
    stmt as *const Stmt as usize
}

struct DeadState {
    has_break: HashMap<usize, bool>,
    has_goto: HashMap<String, bool>,
    labels: HashMap<String, usize>,
    break_target: Option<usize>,
    reachable: bool,
    pending: Vec<u32>,
}

impl DeadState {
    fn new() -> Self {
        Self {
            has_break: HashMap::new(),
            has_goto: HashMap::new(),
            labels: HashMap::new(),
            break_target: None,
            reachable: false,
            pending: Vec::new(),
        }
    }

    fn find_labels(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::AssignStmt(_)
            | Stmt::BadStmt(_)
            | Stmt::DeclStmt(_)
            | Stmt::DeferStmt(_)
            | Stmt::EmptyStmt(_)
            | Stmt::ExprStmt(_)
            | Stmt::GoStmt(_)
            | Stmt::IncDecStmt(_)
            | Stmt::ReturnStmt(_)
            | Stmt::SendStmt(_) => {}

            Stmt::BlockStmt(b) => {
                for s in &b.list {
                    self.find_labels(s);
                }
            }

            Stmt::BranchStmt(b) => match b.tok {
                Token::GOTO => {
                    if let Some(label) = &b.label {
                        self.has_goto.insert(label.name.clone(), true);
                    }
                }
                Token::BREAK => {
                    let target = if let Some(label) = &b.label {
                        self.labels.get(&label.name).copied()
                    } else {
                        self.break_target
                    };
                    if let Some(key) = target {
                        self.has_break.insert(key, true);
                    }
                }
                _ => {}
            },

            Stmt::IfStmt(s) => {
                self.find_labels(&Stmt::BlockStmt(s.body.clone()));
                if let Some(else_) = &s.else_ {
                    self.find_labels(else_);
                }
            }

            Stmt::LabeledStmt(s) => {
                self.labels.insert(s.label.name.clone(), stmt_key(&s.stmt));
                self.find_labels(&s.stmt);
            }

            Stmt::ForStmt(s) => {
                let outer = self.break_target;
                self.break_target = Some(stmt_key(stmt));
                self.find_labels(&Stmt::BlockStmt(s.body.clone()));
                self.break_target = outer;
            }

            Stmt::RangeStmt(s) => {
                let outer = self.break_target;
                self.break_target = Some(stmt_key(stmt));
                self.find_labels(&Stmt::BlockStmt(s.body.clone()));
                self.break_target = outer;
            }

            Stmt::SelectStmt(s) => {
                let outer = self.break_target;
                self.break_target = Some(stmt_key(stmt));
                self.find_labels(&Stmt::BlockStmt(s.body.clone()));
                self.break_target = outer;
            }

            Stmt::SwitchStmt(s) => {
                let outer = self.break_target;
                self.break_target = Some(stmt_key(stmt));
                self.find_labels(&Stmt::BlockStmt(s.body.clone()));
                self.break_target = outer;
            }

            Stmt::TypeSwitchStmt(s) => {
                let outer = self.break_target;
                self.break_target = Some(stmt_key(stmt));
                self.find_labels(&Stmt::BlockStmt(s.body.clone()));
                self.break_target = outer;
            }

            Stmt::CommClause(c) => {
                for s in &c.body {
                    self.find_labels(s);
                }
            }

            Stmt::CaseClause(c) => {
                for s in &c.body {
                    self.find_labels(s);
                }
            }
        }
    }

    fn find_dead(&mut self, stmt: &Stmt) {
        if let Stmt::LabeledStmt(s) = stmt {
            if self.has_goto.get(&s.label.name).copied().unwrap_or(false) {
                self.reachable = true;
            }
        }

        if !self.reachable {
            match stmt {
                Stmt::EmptyStmt(_) => {}
                _ => {
                    self.pending.push(stmt.pos().0 as u32);
                    self.reachable = true;
                }
            }
        }

        match stmt {
            Stmt::AssignStmt(_)
            | Stmt::BadStmt(_)
            | Stmt::DeclStmt(_)
            | Stmt::DeferStmt(_)
            | Stmt::EmptyStmt(_)
            | Stmt::GoStmt(_)
            | Stmt::IncDecStmt(_)
            | Stmt::SendStmt(_) => {}

            Stmt::BlockStmt(b) => {
                for s in &b.list {
                    self.find_dead(s);
                }
            }

            Stmt::BranchStmt(b) => match b.tok {
                Token::BREAK | Token::GOTO | Token::FALLTHROUGH | Token::CONTINUE => {
                    self.reachable = false;
                }
                _ => {}
            },

            Stmt::ExprStmt(s) => {
                if is_panic_call(&s.x) {
                    self.reachable = false;
                }
            }

            Stmt::ForStmt(s) => {
                self.find_dead(&Stmt::BlockStmt(s.body.clone()));
                self.reachable = s.cond.is_some() || self.has_break.get(&stmt_key(stmt)).copied().unwrap_or(false);
            }

            Stmt::IfStmt(s) => {
                self.find_dead(&Stmt::BlockStmt(s.body.clone()));
                if let Some(else_) = &s.else_ {
                    let saved = self.reachable;
                    self.reachable = true;
                    self.find_dead(else_);
                    self.reachable = self.reachable || saved;
                } else {
                    self.reachable = true;
                }
            }

            Stmt::LabeledStmt(s) => self.find_dead(&s.stmt),

            Stmt::RangeStmt(s) => {
                self.find_dead(&Stmt::BlockStmt(s.body.clone()));
                self.reachable = true;
            }

            Stmt::ReturnStmt(_) => self.reachable = false,

            Stmt::SelectStmt(s) => {
                let mut any_reachable = false;
                for comm in &s.body.list {
                    let Stmt::CommClause(c) = comm else {
                        continue;
                    };
                    self.reachable = true;
                    for inner in &c.body {
                        self.find_dead(inner);
                    }
                    any_reachable = any_reachable || self.reachable;
                }
                self.reachable =
                    any_reachable || self.has_break.get(&stmt_key(stmt)).copied().unwrap_or(false);
            }

            Stmt::SwitchStmt(s) => {
                let mut any_reachable = false;
                let mut has_default = false;
                for cas in &s.body.list {
                    let Stmt::CaseClause(c) = cas else {
                        continue;
                    };
                    if c.list.is_empty() {
                        has_default = true;
                    }
                    self.reachable = true;
                    for inner in &c.body {
                        self.find_dead(inner);
                    }
                    any_reachable = any_reachable || self.reachable;
                }
                self.reachable = any_reachable
                    || self.has_break.get(&stmt_key(stmt)).copied().unwrap_or(false)
                    || !has_default;
            }

            Stmt::TypeSwitchStmt(s) => {
                let mut any_reachable = false;
                let mut has_default = false;
                for cas in &s.body.list {
                    let Stmt::CaseClause(c) = cas else {
                        continue;
                    };
                    if c.list.is_empty() {
                        has_default = true;
                    }
                    self.reachable = true;
                    for inner in &c.body {
                        self.find_dead(inner);
                    }
                    any_reachable = any_reachable || self.reachable;
                }
                self.reachable = any_reachable
                    || self.has_break.get(&stmt_key(stmt)).copied().unwrap_or(false)
                    || !has_default;
            }

            Stmt::CaseClause(_) | Stmt::CommClause(_) => {}
        }
    }
}

fn is_panic_call(expr: &Expr) -> bool {
    let Expr::CallExpr(CallExpr { fun, .. }) = expr else {
        return false;
    };
    matches!(unparen(fun), Expr::Ident(Ident { name, .. }) if name == "panic")
}

fn check_body(body: &BlockStmt) -> Vec<u32> {
    let mut state = DeadState::new();
    let root = Stmt::BlockStmt(body.clone());
    state.find_labels(&root);
    state.reachable = true;
    state.find_dead(&root);
    state.pending
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "unreachable requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |n| {
        match n {
            NodeRef::FuncDecl(FuncDecl { body: Some(body), .. }) => {
                pending.extend(check_body(body));
            }
            NodeRef::FuncLit(FuncLit { body, .. }) => pending.extend(check_body(body)),
            _ => {}
        }
    });

    // Sort + dedupe so report order is stable across parallel/sequential runs
    // (HashMap-backed walks elsewhere can change visit order of FuncDecl/FuncLit).
    pending.sort_unstable();
    pending.dedup();
    for pos in pending {
        pass.reportf(pos, "unreachable code");
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "unreachable",
        doc: "check for unreachable code",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/unreachable",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
