//! `buildtag` — check `//go:build` and `// +build` directives.

use std::sync::OnceLock;

use guff::ast::File;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::buildconstraint::{is_go_build_line, is_plus_build_line, parse_go_build, parse_plus_build};

struct Checker {
    go_build_ok: bool,
    plus_build_ok: bool,
    go_build_seen: bool,
    pending: Vec<(u32, String)>,
}

impl Checker {
    fn new() -> Self {
        Self {
            go_build_ok: true,
            plus_build_ok: true,
            go_build_seen: false,
            pending: Vec::new(),
        }
    }

    fn check_go_file(&mut self, f: &File) {
        for group in &f.comments {
            if group.end().0 + 1 >= f.package.0 {
                self.plus_build_ok = false;
            }
            if group.pos().0 >= f.package.0 {
                self.go_build_ok = false;
            }
            for c in &group.list {
                if !c.text.starts_with("//") {
                    self.plus_build_ok = false;
                }
                self.comment(c.slash.0 as u32, &c.text);
            }
        }
    }

    fn comment(&mut self, pos: u32, text: &str) {
        if text.contains("+build") {
            self.plus_build_line(pos, text);
        }
        if text.contains("//go:build") || text.contains("// go:build") {
            self.go_build_line(pos, text);
        }
    }

    fn go_build_line(&mut self, pos: u32, line: &str) {
        if !is_go_build_line(line) {
            if line.contains("go:build") {
                self.pending.push((
                    pos,
                    "malformed //go:build line (space between // and go:build)".into(),
                ));
            }
            return;
        }
        if !self.go_build_ok {
            self.pending.push((pos, "misplaced //go:build comment".into()));
            return;
        }
        if self.go_build_seen {
            self.pending.push((pos, "unexpected extra //go:build line".into()));
            return;
        }
        self.go_build_seen = true;
        let trimmed = line.split(" // ERROR ").next().unwrap_or(line);
        if let Err(e) = parse_go_build(trimmed) {
            self.pending.push((pos, e));
        }
    }

    fn plus_build_line(&mut self, pos: u32, line: &str) {
        let trimmed = line.trim();
        if !is_plus_build_line(trimmed) {
            if self.plus_build_ok && !trimmed.starts_with("// want") {
                self.pending.push((pos, "possible malformed +build comment".into()));
            }
            return;
        }
        if !self.plus_build_ok {
            self.pending.push((pos, "misplaced +build comment".into()));
        }
        let trimmed = trimmed.split(" // ERROR ").next().unwrap_or(trimmed);
        if let Err(e) = parse_plus_build(trimmed) {
            self.pending.push((pos, e));
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut checker = Checker::new();
    for file in pass.files() {
        checker.check_go_file(file);
    }
    for (pos, message) in checker.pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "buildtag",
        doc: "check //go:build and // +build directives",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/buildtag",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![],
        fact_types: vec![],
    })
}
