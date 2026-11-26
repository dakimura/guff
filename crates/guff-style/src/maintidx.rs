//! Port of [`github.com/yagipy/maintidx`](https://github.com/yagipy/maintidx)
//! (golangci-lint wrapper in `pkg/golinters/maintidx`).
//!
//! Reports functions whose maintainability index is strictly below `under`
//! (golangci / upstream default: 20). The index is the Microsoft-normalized
//! combination of cyclomatic complexity, Halstead volume, and lines of code.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{Decl, FuncDecl};
use guff::position::FileSet;
use guff::scope::ObjKind;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::MaintidxOptions;

fn func_pos(fd: &FuncDecl) -> guff::position::Pos {
    fd.ty.pos()
}

fn func_end(fd: &FuncDecl) -> guff::position::Pos {
    fd.body
        .as_ref()
        .map(|b| b.end())
        .unwrap_or_else(|| fd.ty.end())
}

#[derive(Default)]
struct Metrics {
    cyc: usize,
    opt: HashMap<String, usize>,
    opd: HashMap<String, usize>,
}

impl Metrics {
    fn incr_opt(&mut self, sym: &str) {
        *self.opt.entry(sym.to_string()).or_default() += 1;
    }

    fn incr_opd(&mut self, sym: &str) {
        *self.opd.entry(sym.to_string()).or_default() += 1;
    }

    fn incr_opt_if(&mut self, sym: &str, ok: bool) {
        if ok {
            self.incr_opt(sym);
        }
    }

    fn analyze(&mut self, n: NodeRef<'_>) {
        // Cyclomatic complexity (pkg/cyc).
        match n {
            NodeRef::IfStmt(_) | NodeRef::ForStmt(_) | NodeRef::RangeStmt(_) => {
                self.cyc += 1;
            }
            NodeRef::CaseClause(c) if !c.list.is_empty() => {
                self.cyc += 1;
            }
            NodeRef::CommClause(c) if c.comm.is_some() => {
                self.cyc += 1;
            }
            NodeRef::BinaryExpr(b) if b.op == Token::LAND || b.op == Token::LOR => {
                self.cyc += 1;
            }
            _ => {}
        }

        // Halstead volume operators / operands (pkg/halstvol).
        match n {
            NodeRef::FuncDecl(fd) => self.handle_func_decl(fd),
            NodeRef::GenDecl(gd) => self.handle_gen_decl(gd),
            NodeRef::ParenExpr(e) => {
                self.incr_opt_if("()", e.lparen.is_valid() && e.rparen.is_valid());
            }
            NodeRef::IndexExpr(e) => {
                // Upstream counts index brackets as "{}" (intentional parity).
                self.incr_opt_if("{}", e.lbrack.is_valid() && e.rbrack.is_valid());
            }
            NodeRef::SliceExpr(e) => {
                self.incr_opt_if("[]", e.lbrack.is_valid() && e.rbrack.is_valid());
            }
            NodeRef::TypeAssertExpr(e) => {
                self.incr_opt_if("()", e.lparen.is_valid() && e.rparen.is_valid());
            }
            NodeRef::CallExpr(e) => {
                self.incr_opt_if("()", e.lparen.is_valid() && e.rparen.is_valid());
                self.incr_opt_if("...", e.ellipsis.is_valid());
            }
            NodeRef::StarExpr(e) => self.incr_opt_if("*", e.star.is_valid()),
            NodeRef::UnaryExpr(e) => {
                if e.op.is_operator() {
                    self.incr_opt(&e.op.to_string());
                } else {
                    self.incr_opd(&e.op.to_string());
                }
            }
            NodeRef::BinaryExpr(e) => self.incr_opt(&e.op.to_string()),
            NodeRef::KeyValueExpr(e) => self.incr_opt_if(":", e.colon.is_valid()),
            NodeRef::BasicLit(lit) => {
                if lit.kind.map(|k| k.is_literal()).unwrap_or(false) {
                    self.incr_opd(&lit.value);
                } else {
                    self.incr_opt(&lit.value);
                }
            }
            NodeRef::CompositeLit(lit) => {
                self.incr_opt_if("{}", lit.lbrace.is_valid() && lit.rbrace.is_valid());
            }
            NodeRef::Ident(id) => self.handle_ident(id),
            NodeRef::Ellipsis(e) => self.incr_opt_if("...", e.ellipsis.is_valid()),
            NodeRef::FuncType(ft) => {
                self.incr_opt_if("func", ft.func.is_valid());
                self.incr_opt("()");
            }
            NodeRef::ChanType(ct) => {
                self.incr_opt_if("chan", ct.begin.is_valid());
                self.incr_opt_if("<-", ct.arrow.is_valid());
            }
            NodeRef::SendStmt(s) => self.incr_opt_if("<-", s.arrow.is_valid()),
            NodeRef::IncDecStmt(s) => {
                self.incr_opt_if(&s.tok.to_string(), s.tok.is_operator());
            }
            NodeRef::AssignStmt(s) => {
                if let Some(tok) = s.tok {
                    if tok.is_operator() {
                        self.incr_opt(&tok.to_string());
                    }
                }
            }
            NodeRef::GoStmt(s) => self.incr_opt_if("go", s.go_.is_valid()),
            NodeRef::DeferStmt(s) => self.incr_opt_if("defer", s.defer_.is_valid()),
            NodeRef::ReturnStmt(s) => self.incr_opt_if("return", s.return_.is_valid()),
            NodeRef::BranchStmt(s) => {
                if s.tok.is_operator() {
                    self.incr_opt(&s.tok.to_string());
                } else {
                    self.incr_opd(&s.tok.to_string());
                }
            }
            NodeRef::BlockStmt(s) => {
                self.incr_opt_if("{}", s.lbrace.is_valid() && s.rbrace.is_valid());
            }
            NodeRef::IfStmt(s) => {
                self.incr_opt_if("if", s.if_.is_valid());
                if s.else_.is_some() {
                    self.incr_opt("else");
                }
            }
            NodeRef::SwitchStmt(s) => self.incr_opt_if("switch", s.switch.is_valid()),
            NodeRef::SelectStmt(s) => self.incr_opt_if("select", s.select_.is_valid()),
            NodeRef::ForStmt(s) => self.incr_opt_if("for", s.for_.is_valid()),
            NodeRef::RangeStmt(s) => {
                self.incr_opt_if("for", s.for_.is_valid());
                if s.key.is_some() {
                    if let Some(tok) = s.tok {
                        if tok.is_operator() {
                            self.incr_opt(&tok.to_string());
                        } else {
                            self.incr_opd(&tok.to_string());
                        }
                    }
                }
                self.incr_opt("range");
            }
            NodeRef::CaseClause(cc) => {
                if cc.list.is_empty() {
                    self.incr_opt("default");
                }
                self.incr_opt_if(":", cc.colon.is_valid());
            }
            _ => {}
        }
    }

    fn handle_func_decl(&mut self, fd: &FuncDecl) {
        if fd.recv.is_none() {
            // Receiver methods count the name via *ast.Ident instead.
            self.incr_opt(&fd.name.name);
        } else {
            self.incr_opt("()");
        }
    }

    fn handle_gen_decl(&mut self, gd: &guff::ast::GenDecl) {
        if gd.lparen.is_valid() && gd.rparen.is_valid() {
            self.incr_opt("()");
        }
        if let Some(tok) = gd.tok {
            if tok.is_operator() {
                self.incr_opt(&tok.to_string());
            } else {
                self.incr_opd(&tok.to_string());
            }
        }
    }

    fn handle_ident(&mut self, id: &guff::ast::Ident) {
        let obj = id.obj.lock().unwrap().clone();
        match obj {
            None => self.incr_opt(&id.name),
            Some(obj) => {
                // Upstream skips function objects (`ObjKind.String() == "func"`).
                if obj.kind != ObjKind::Fun {
                    self.incr_opd(&id.name);
                }
            }
        }
    }

    fn halstead_volume(&self) -> f64 {
        let dist_opt = self.opt.len();
        let dist_opd = self.opd.len();
        let sum_opt: usize = self.opt.values().sum();
        let sum_opd: usize = self.opd.values().sum();
        let vocab = dist_opt + dist_opd;
        let length = sum_opt + sum_opd;
        if vocab == 0 {
            return 0.0;
        }
        (length as f64) * (vocab as f64).log2()
    }

    /// Microsoft maintainability index, normalized to 0..=171 scale then 0..=100.
    fn maint_idx(&self, loc: usize) -> i32 {
        let vol = self.halstead_volume();
        let loc = loc.max(1) as f64;
        let orig = 171.0 - 5.2 * vol.ln() - 0.23 * (self.cyc as f64) - 16.2 * loc.ln();
        let norm = (orig * 100.0 / 171.0).max(0.0);
        norm as i32
    }
}

fn loc(fset: &FileSet, fd: &FuncDecl) -> usize {
    let start = fset.position(func_pos(fd)).line;
    let end = fset.position(func_end(fd)).line;
    (end - start + 1).max(1) as usize
}

fn analyze_func(fset: &FileSet, fd: &FuncDecl) -> (usize, f64, i32) {
    let mut m = Metrics {
        cyc: 1,
        ..Metrics::default()
    };
    walk::preorder(NodeRef::FuncDecl(fd), |n| {
        m.analyze(n);
        true
    });
    let vol = m.halstead_volume();
    let mi = m.maint_idx(loc(fset, fd));
    (m.cyc, vol, mi)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "maintidx requires inspect analyzer".to_string())?;

    let under = pass
        .settings::<MaintidxOptions>("maintidx")
        .copied()
        .unwrap_or_default()
        .under;

    let fset = pass.fset().clone();
    let mut pending: Vec<(u32, String)> = Vec::new();

    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            let (cyc, vol, mi) = analyze_func(&fset, fd);
            if mi < under as i32 {
                pending.push((
                    // `pass.Reportf(n.Pos(), …)` where `n` is the FuncDecl:
                    // the `func` keyword, not the name it prints in the message.
                    fd.ty.pos().0 as u32,
                    format!(
                        "Function name: {}, Cyclomatic Complexity: {}, Halstead Volume: {:.2}, Maintainability Index: {}",
                        fd.name.name, cyc, vol, mi
                    ),
                ));
            }
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "maintidx",
        doc: "Measures the maintainability index of each function.",
        url: "https://github.com/yagipy/maintidx",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff::parser::{parse_file, Mode};
    use guff::position::FileSet;

    fn parse_one(src: &str) -> (std::sync::Arc<FileSet>, FuncDecl) {
        let fset = FileSet::new();
        let file = parse_file(&fset, "t.go", src.as_bytes(), Mode::NONE).expect("parse");
        let Decl::FuncDecl(fd) = file.decls.into_iter().next().expect("func") else {
            panic!("expected FuncDecl");
        };
        (fset, fd)
    }

    #[test]
    fn f2_matches_upstream_halstead_and_mi() {
        // Upstream testdata want: Cyc 1, Vol 18.09, MI 80
        let src = r#"
package p
func f2() {
	print("Hello, World")
}
"#;
        let (fset, fd) = parse_one(src);
        let (cyc, vol, mi) = analyze_func(&fset, &fd);
        assert_eq!(cyc, 1);
        assert!((vol - 18.09).abs() < 0.01, "vol={vol}");
        assert_eq!(mi, 80);
    }

    #[test]
    fn empty_method_matches_upstream() {
        // Upstream: Cyc 1, Vol 22.46, MI 83
        let src = r#"
package p
type t1 struct{}
func (t *t1) receive() {
}
"#;
        let fset = FileSet::new();
        let file = parse_file(&fset, "t.go", src.as_bytes(), Mode::NONE).expect("parse");
        let fd = file
            .decls
            .into_iter()
            .find_map(|d| match d {
                Decl::FuncDecl(f) => Some(f),
                _ => None,
            })
            .expect("method");
        let (cyc, vol, mi) = analyze_func(&fset, &fd);
        assert_eq!(cyc, 1);
        assert!((vol - 22.46).abs() < 0.01, "vol={vol}");
        assert_eq!(mi, 83);
    }
}
