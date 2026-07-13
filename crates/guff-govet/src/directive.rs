//! `directive` — check Go toolchain directives such as `//go:debug`.

use std::sync::OnceLock;

use guff::ast::File;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::govet_util::{is_in_test_file, is_main_package};

struct Checker {
    filename: String,
    file: Option<File>,
    in_header: bool,
    pending: Vec<(u32, String)>,
}

impl Checker {
    fn new(filename: String, file: Option<File>) -> Self {
        Self {
            filename,
            file,
            in_header: true,
            pending: Vec::new(),
        }
    }

    fn check_go_file(&mut self, f: &File) {
        for group in &f.comments {
            if group.pos().0 >= f.package.0 {
                self.in_header = false;
            }
            for c in &group.list {
                self.comment(c.slash.0 as u32, &c.text);
            }
        }
    }

    fn comment(&mut self, pos: u32, line: &str) {
        if !line.starts_with("//go:") {
            return;
        }
        let trimmed = line.split(" // ERROR ").next().unwrap_or(line);
        let verb = trimmed
            .split_whitespace()
            .next()
            .unwrap_or(trimmed);
        match verb {
            "//go:build" => {}
            "//go:debug" => {
                if self.file.is_none() {
                    self.pending.push((
                        pos,
                        "//go:debug directive only valid in Go source files".into(),
                    ));
                    return;
                }
                let file = self.file.as_ref().unwrap();
                let is_test = self.filename.ends_with("_test.go");
                if file.name.name != "main" && !is_test {
                    self.pending.push((
                        pos,
                        "//go:debug directive only valid in package main or test".into(),
                    ));
                } else if !self.in_header {
                    self.pending.push((
                        pos,
                        "//go:debug directive only valid before package declaration".into(),
                    ));
                }
            }
            _ => {}
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut pending = Vec::new();
    for (i, file) in pass.files().iter().enumerate() {
        let filename = pass
            .pkg()
            .compiled_go_files
            .get(i)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "file.go".into());
        let mut checker = Checker::new(filename.clone(), Some(file.clone()));
        checker.check_go_file(file);
        pending.extend(checker.pending);
    }
    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    let _ = is_main_package(pass);
    let _ = is_in_test_file(pass, 0);
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "directive",
        doc: "check Go toolchain directives such as //go:debug",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/directive",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![],
        fact_types: vec![],
    })
}
