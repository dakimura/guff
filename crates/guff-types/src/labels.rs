//! Port of `labels.go` — the label-checking 2nd pass over a function body.
//!
//! Reports: `DuplicateLabel`, `UndeclaredLabel` (a `goto` to a label that is
//! declared nowhere in the function), `UnusedLabel`, and `MisplacedLabel` (a
//! labeled `break`/`continue` whose target is not an appropriate enclosing
//! statement).
//!
//! ## Simplifications
//!
//! - We have no `Label` object kind, so labels are tracked by name (positions
//!   for diagnostics). `recordDef`/`recordUse` are no-ops (Info, §18b).
//! - The forward-jump analysis (`JumpOverDecl` / `JumpIntoBlock` — a `goto`
//!   that jumps over a variable declaration or into a block) is **deferred**:
//!   a `goto` to any label declared somewhere in the function is accepted.

use crate::hash::{HashMap, HashSet};

use guff::ast::{BlockStmt, Stmt};
use guff::token::Token;
use guff_types_errors::Code;

use crate::check::Checker;

/// The kind of statement a label may be attached to for `break`/`continue`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LabelKind {
    /// `for` / `range` — valid target for both `break` and `continue`.
    Loop,
    /// `switch` / `select` — valid target for `break` only.
    SwitchSelect,
}

/// If `s` is a statement that a labeled `break` (and, for `Loop`, `continue`)
/// may target, return its kind.
fn breakable_kind(s: &Stmt) -> Option<LabelKind> {
    match s {
        Stmt::ForStmt(_) | Stmt::RangeStmt(_) => Some(LabelKind::Loop),
        Stmt::SwitchStmt(_) | Stmt::TypeSwitchStmt(_) | Stmt::SelectStmt(_) => {
            Some(LabelKind::SwitchSelect)
        }
        _ => None,
    }
}

impl Checker {
    /// Check correct label use in `body`. Equivalent to `Checker.labels`.
    pub fn labels(&mut self, body: &BlockStmt) {
        // Collect every label declared anywhere in the function (labels are
        // function-scoped); report duplicates.
        let mut all: HashMap<String, u32> = HashMap::default();
        self.collect_labels(&body.list, &mut all);

        // Validate every labeled branch and record which labels are used. (Run
        // even when no labels are declared: a `goto L` to a nonexistent label
        // must still report UndeclaredLabel.)
        let mut used: HashSet<String> = HashSet::default();
        let mut enclosing: Vec<(String, LabelKind)> = Vec::new();
        self.label_branches_list(&body.list, &all, &mut enclosing, &mut used);

        // spec: "It is illegal to define a label that is never used."
        let mut unused: Vec<(String, u32)> = all
            .iter()
            .filter(|(n, _)| !used.contains(*n))
            .map(|(n, p)| (n.clone(), *p))
            .collect();
        unused.sort_by_key(|(_, p)| *p);
        for (name, pos) in unused {
            self.error(
                pos,
                Code::UnusedLabel,
                format!("label {} declared and not used", name),
            );
        }
    }

    /// Pass 1: collect all label declarations, reporting `DuplicateLabel`.
    fn collect_labels(&mut self, list: &[Stmt], all: &mut HashMap<String, u32>) {
        for s in list {
            self.collect_labels_stmt(s, all);
        }
    }

    fn collect_labels_stmt(&mut self, s: &Stmt, all: &mut HashMap<String, u32>) {
        match s {
            Stmt::LabeledStmt(l) => {
                let name = l.label.name.clone();
                if name != "_" {
                    let pos = l.label.pos().0 as u32;
                    if all.contains_key(&name) {
                        self.error(
                            pos,
                            Code::DuplicateLabel,
                            format!("label {} already declared", name),
                        );
                    } else {
                        all.insert(name, pos);
                    }
                }
                self.collect_labels_stmt(&l.stmt, all);
            }
            Stmt::BlockStmt(b) => self.collect_labels(&b.list, all),
            Stmt::IfStmt(i) => {
                self.collect_labels(&i.body.list, all);
                if let Some(e) = &i.else_ {
                    self.collect_labels_stmt(e, all);
                }
            }
            Stmt::ForStmt(f) => self.collect_labels(&f.body.list, all),
            Stmt::RangeStmt(r) => self.collect_labels(&r.body.list, all),
            Stmt::SwitchStmt(sw) => self.collect_labels_clauses(&sw.body.list, all),
            Stmt::TypeSwitchStmt(sw) => self.collect_labels_clauses(&sw.body.list, all),
            Stmt::SelectStmt(sel) => self.collect_labels_clauses(&sel.body.list, all),
            _ => {}
        }
    }

    /// Collect labels from the bodies of `case`/`comm` clauses.
    fn collect_labels_clauses(&mut self, clauses: &[Stmt], all: &mut HashMap<String, u32>) {
        for c in clauses {
            match c {
                Stmt::CaseClause(cc) => self.collect_labels(&cc.body, all),
                Stmt::CommClause(cc) => self.collect_labels(&cc.body, all),
                _ => {}
            }
        }
    }

    /// Pass 2: walk the body validating labeled branches and marking used
    /// labels. `enclosing` is the stack of labeled breakable statements in
    /// scope at the current point.
    fn label_branches_list(
        &mut self,
        list: &[Stmt],
        all: &HashMap<String, u32>,
        enclosing: &mut Vec<(String, LabelKind)>,
        used: &mut HashSet<String>,
    ) {
        for s in list {
            self.label_branches_stmt(s, all, enclosing, used);
        }
    }

    fn label_branches_stmt(
        &mut self,
        s: &Stmt,
        all: &HashMap<String, u32>,
        enclosing: &mut Vec<(String, LabelKind)>,
        used: &mut HashSet<String>,
    ) {
        match s {
            Stmt::LabeledStmt(l) => {
                // A label on a breakable statement is a `break`/`continue`
                // target inside that statement.
                let pushed = match breakable_kind(&l.stmt) {
                    Some(k) if l.label.name != "_" => {
                        enclosing.push((l.label.name.clone(), k));
                        true
                    }
                    _ => false,
                };
                self.label_branches_stmt(&l.stmt, all, enclosing, used);
                if pushed {
                    enclosing.pop();
                }
            }

            Stmt::BranchStmt(b) => {
                let label = match &b.label {
                    Some(l) => l,
                    None => return, // unlabeled: checked in the 1st pass (stmt)
                };
                let name = label.name.as_str();
                let pos = label.pos().0 as u32;
                match b.tok {
                    Token::BREAK => {
                        if enclosing.iter().any(|(n, _)| n == name) {
                            used.insert(name.to_string());
                        } else {
                            self.error(
                                pos,
                                Code::MisplacedLabel,
                                format!("invalid break label {}", name),
                            );
                        }
                    }
                    Token::CONTINUE => {
                        if enclosing
                            .iter()
                            .any(|(n, k)| n == name && *k == LabelKind::Loop)
                        {
                            used.insert(name.to_string());
                        } else {
                            self.error(
                                pos,
                                Code::MisplacedLabel,
                                format!("invalid continue label {}", name),
                            );
                        }
                    }
                    Token::GOTO => {
                        if all.contains_key(name) {
                            used.insert(name.to_string());
                        } else {
                            self.error(
                                pos,
                                Code::UndeclaredLabel,
                                format!("label {} not declared", name),
                            );
                        }
                    }
                    _ => {}
                }
            }

            Stmt::BlockStmt(b) => self.label_branches_list(&b.list, all, enclosing, used),
            Stmt::IfStmt(i) => {
                self.label_branches_list(&i.body.list, all, enclosing, used);
                if let Some(e) = &i.else_ {
                    self.label_branches_stmt(e, all, enclosing, used);
                }
            }
            Stmt::ForStmt(f) => self.label_branches_list(&f.body.list, all, enclosing, used),
            Stmt::RangeStmt(r) => self.label_branches_list(&r.body.list, all, enclosing, used),
            Stmt::SwitchStmt(sw) => {
                self.label_branches_clauses(&sw.body.list, all, enclosing, used)
            }
            Stmt::TypeSwitchStmt(sw) => {
                self.label_branches_clauses(&sw.body.list, all, enclosing, used)
            }
            Stmt::SelectStmt(sel) => {
                self.label_branches_clauses(&sel.body.list, all, enclosing, used)
            }
            _ => {}
        }
    }

    fn label_branches_clauses(
        &mut self,
        clauses: &[Stmt],
        all: &HashMap<String, u32>,
        enclosing: &mut Vec<(String, LabelKind)>,
        used: &mut HashSet<String>,
    ) {
        for c in clauses {
            match c {
                Stmt::CaseClause(cc) => self.label_branches_list(&cc.body, all, enclosing, used),
                Stmt::CommClause(cc) => self.label_branches_list(&cc.body, all, enclosing, used),
                _ => {}
            }
        }
    }
}
