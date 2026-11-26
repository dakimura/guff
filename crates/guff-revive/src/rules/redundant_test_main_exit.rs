//! `redundant-test-main-exit` — warn on `os.Exit` in `TestMain`.

use std::collections::HashMap;

use guff::ast::{Decl, Expr, FuncDecl, SelectorExpr, Spec, Stmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{file_is_test, go_version_at_least, is_ident, is_pkg_dot_name, unparen};

pub struct Checker {
    skip_file: bool,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn try_new(pass: &Pass<'_>) -> Option<Self> {
        if !go_version_at_least(pass, 1, 15) {
            return None;
        }
        Some(Self {
            skip_file: true,
            failures: Vec::new(),
        })
    }

    /// Upstream returns unless `file.IsTest()`, a filename check — so the rule
    /// runs on `foo_test.go` in `package foo`, which asking the *package* name
    /// answered "no" for. That is most internal test files, and this rule only
    /// ever fires in one.
    pub fn on_file(&mut self, file_is_test: bool) {
        self.skip_file = !file_is_test;
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        if self.skip_file {
            return;
        }
        let NodeRef::FuncDecl(f) = n else {
            return;
        };
        // revive v1.17.0: only `func TestMain(m *testing.M)`, and only an
        // Exit whose argument is the result of `m.Run()`.
        if f.recv.is_some() || f.name.name != "TestMain" || f.ty.results.is_some() {
            return;
        }
        let Some(body) = &f.body else {
            return;
        };
        let Some(m_name) = testing_m_param(f) else {
            return;
        };
        let mut w = RunResults {
            m_name,
            writes: HashMap::new(),
            run_results: HashMap::new(),
        };
        walk::inspect(NodeRef::BlockStmt(body), |n| {
            if let Some(n) = n {
                w.count_writes(n);
            }
            true
        });
        w.collect_run_results(&body.list);
        walk::inspect(NodeRef::BlockStmt(body), |n| {
            let Some(NodeRef::CallExpr(call)) = n else {
                return true;
            };
            let pkg = if is_pkg_dot_name(&call.fun, "os", "Exit") {
                "os"
            } else if is_pkg_dot_name(&call.fun, "syscall", "Exit") {
                "syscall"
            } else {
                return true;
            };
            if call.args.len() != 1 || !w.is_run_result(&call.args[0]) {
                return true;
            }
            self.failures.push(Failure {
                rule: "redundant-test-main-exit",
                pos: call.pos().0 as u32,
                message: format!(
                    "redundant call to {pkg}.Exit in TestMain function, the test runner will handle it automatically as of Go 1.15"
                ),
                ..Failure::default()
            });
            true
        });
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let Some(mut c) = Checker::try_new(pass) else {
        return Vec::new();
    };
    for file in pass.files() {
        c.on_file(file_is_test(pass, file));
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}

/// `FuncSignatureIs(fd, "TestMain", []string{"*testing.M"}, nil)` and exactly
/// one parameter name: that name.
fn testing_m_param(f: &FuncDecl) -> Option<String> {
    let params = f.ty.params.as_ref()?;
    let [field] = params.list.as_slice() else {
        return None;
    };
    let [name] = field.names.as_slice() else {
        return None;
    };
    let Expr::StarExpr(star) = field.ty.as_ref()? else {
        return None;
    };
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = star.x.as_ref() else {
        return None;
    };
    (is_ident(x, "testing") && sel.name == "M").then(|| name.name.clone())
}

struct RunResults {
    /// Name of the `*testing.M` parameter.
    m_name: String,
    /// Writes to each variable: only one written exactly once, from `m.Run`,
    /// holds its result.
    writes: HashMap<String, u32>,
    /// End of the unconditional statement assigning `m.Run()` to a variable.
    run_results: HashMap<String, u32>,
}

impl RunResults {
    /// `countWrites`.
    fn count_writes(&mut self, n: NodeRef<'_>) {
        match n {
            NodeRef::AssignStmt(a) => a.lhs.iter().for_each(|l| self.count_write(l)),
            NodeRef::ValueSpec(vs) if !vs.values.is_empty() => {
                for name in &vs.names {
                    self.count_name(&name.name);
                }
            }
            NodeRef::IncDecStmt(s) => self.count_write(&s.x),
            NodeRef::UnaryExpr(u) if u.op == Token::AND => self.count_write(&u.x),
            NodeRef::RangeStmt(r) => {
                for e in [r.key.as_ref(), r.value.as_ref()].into_iter().flatten() {
                    self.count_write(e);
                }
            }
            _ => {}
        }
    }

    fn count_write(&mut self, target: &Expr) {
        if let Expr::Ident(id) = unparen(target) {
            self.count_name(&id.name);
        }
    }

    fn count_name(&mut self, name: &str) {
        if name != "_" {
            *self.writes.entry(name.to_string()).or_default() += 1;
        }
    }

    /// `collectRunResults`: variables assigned from `m.Run` by a top-level
    /// statement of the body, which runs before anything after it.
    fn collect_run_results(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            let (name, value) = match stmt {
                Stmt::AssignStmt(a)
                    if a.lhs.len() == 1
                        && a.rhs.len() == 1
                        && matches!(a.tok, Some(Token::ASSIGN) | Some(Token::DEFINE)) =>
                {
                    let Expr::Ident(id) = unparen(&a.lhs[0]) else {
                        continue;
                    };
                    (&id.name, &a.rhs[0])
                }
                Stmt::DeclStmt(d) => {
                    let Decl::GenDecl(gd) = &d.decl else { continue };
                    let [Spec::ValueSpec(vs)] = gd.specs.as_slice() else {
                        continue;
                    };
                    let ([name], [value]) = (vs.names.as_slice(), vs.values.as_slice()) else {
                        continue;
                    };
                    (&name.name, value)
                }
                _ => continue,
            };
            if self.is_run_call(value) {
                self.run_results.insert(name.clone(), stmt.end().0 as u32);
            }
        }
    }

    /// `isRunResult`: `m.Run()` itself, or a variable whose only write, before
    /// `expr`, is the result of `m.Run()`.
    fn is_run_result(&self, expr: &Expr) -> bool {
        let expr = unparen(expr);
        let Expr::Ident(id) = expr else {
            return self.is_run_call(expr);
        };
        self.run_results.get(&id.name).is_some_and(|&at| {
            self.writes.get(&id.name) == Some(&1) && at < expr.pos().0 as u32
        })
    }

    fn is_run_call(&self, expr: &Expr) -> bool {
        let Expr::CallExpr(call) = unparen(expr) else {
            return false;
        };
        let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = call.fun.as_ref() else {
            return false;
        };
        sel.name == "Run" && is_ident(unparen(x), &self.m_name)
    }
}
