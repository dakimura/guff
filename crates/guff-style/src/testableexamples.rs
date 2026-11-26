//! Port of [`github.com/maratori/testableexamples`](https://github.com/maratori/testableexamples)
//! (golangci-lint wrapper in `pkg/golinters/testableexamples`).
//!
//! Checks that Go example functions are testable: they must have an
//! `// Output:` (or `// Unordered output:`) comment so `go test` can
//! validate them. Logic mirrors `go/doc.Examples` + `exampleOutput`.
//!
//! Re-parses with `PARSE_COMMENTS` because load uses `Mode::NONE` and
//! drops `file.Comments`.
//!
//! No `linters.settings` keys (upstream has none).

use std::fs;
use std::sync::{Arc, OnceLock};

use regex::Regex;

use guff::ast::{BlockStmt, CommentGroup, Decl, File};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::{FileSet, Pos};
use guff::token::Token;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

const MESSAGE: &str = "missing output for example, go test can't validate it";

fn output_prefix() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^[[:space:]]*(unordered )?output:").expect("regex"))
}

fn is_test(name: &str, prefix: &str) -> bool {
    if !name.starts_with(prefix) {
        return false;
    }
    if name.len() == prefix.len() {
        return true;
    }
    let rest = &name[prefix.len()..];
    let ch = rest.chars().next().unwrap_or('\0');
    !ch.is_lowercase()
}

/// Last comment group whose span lies entirely inside `body`.
fn last_comment_in_body<'a>(
    body: &BlockStmt,
    comments: &'a [CommentGroup],
) -> Option<&'a CommentGroup> {
    let pos = body.pos().0;
    let end = body.end().0;
    let mut last = None;
    for cg in comments {
        if cg.pos().0 < pos {
            continue;
        }
        if cg.end().0 > end {
            break;
        }
        last = Some(cg);
    }
    last
}

/// Returns `(output, has_output_comment)` for an example body.
fn example_output(body: &BlockStmt, comments: &[CommentGroup]) -> (String, bool) {
    let Some(last) = last_comment_in_body(body, comments) else {
        return (String::new(), false);
    };
    let text = last.text();
    let Some(caps) = output_prefix().find(&text) else {
        return (String::new(), false);
    };
    let mut out = text[caps.end()..].to_string();
    // Strip zero or more spaces followed by \n or a single space (go/doc).
    out = out.trim_start_matches(' ').to_string();
    if out.starts_with('\n') {
        out = out[1..].to_string();
    }
    (out, true)
}

/// Report positions for examples that lack an Output comment (`go/doc.Examples`).
fn examples_missing_output(file: &File) -> Vec<Pos> {
    let mut has_tests = false;
    let mut num_decl = 0usize;
    let mut bodies: Vec<&BlockStmt> = Vec::new();
    let mut report_at: Vec<Pos> = Vec::new();

    for decl in &file.decls {
        match decl {
            Decl::GenDecl(g) if g.tok == Some(Token::IMPORT) => continue,
            Decl::GenDecl(_) => {
                num_decl += 1;
            }
            Decl::FuncDecl(f) => {
                num_decl += 1;
                if f.recv.is_some() {
                    continue;
                }
                let name = f.name.name.as_str();
                if is_test(name, "Test") || is_test(name, "Benchmark") || is_test(name, "Fuzz") {
                    has_tests = true;
                    continue;
                }
                if !is_test(name, "Example") {
                    continue;
                }
                if let Some(params) = f.ty.params.as_ref() {
                    if !params.list.is_empty() {
                        continue;
                    }
                }
                if let Some(results) = f.ty.results.as_ref() {
                    if !results.list.is_empty() {
                        continue;
                    }
                }
                let Some(body) = f.body.as_ref() else {
                    continue;
                };
                bodies.push(body);
                report_at.push(body.pos());
            }
            Decl::BadDecl(_) => {}
        }
    }

    // Whole-file example: one Example, other top-level decls, no Test/Benchmark/Fuzz.
    if !has_tests && num_decl > 1 && bodies.len() == 1 {
        report_at[0] = file.name.pos();
    }

    let mut missing = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        let (output, has_output) = example_output(body, &file.comments);
        // EmptyOutput := output == "" && hasOutput (e.g. `// Output:` alone).
        let empty_output = output.is_empty() && has_output;
        if output.is_empty() && !empty_output {
            missing.push(report_at[i]);
        }
    }
    missing
}

fn reparse(path: &std::path::Path) -> Option<(Arc<FileSet>, File)> {
    let src = fs::read(path).ok()?;
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, &src, PARSE_COMMENTS).ok()?;
    Some((fset, file))
}

/// Map a reparsed file position onto the pass FileSet via line/column.
fn map_pos(pass: &Pass<'_>, pass_file: &File, re_fset: &FileSet, re_pos: Pos) -> u32 {
    let Some(ft) = pass.fset().file(pass_file.pos()) else {
        return re_pos.0 as u32;
    };
    let re_p = re_fset.position(re_pos);
    if re_p.line < 1 || re_p.line as usize > ft.line_count() {
        return pass_file.pos().0 as u32;
    }
    ft.line_start(re_p.line as usize).0 as u32 + (re_p.column as u32).saturating_sub(1)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "testableexamples requires inspect analyzer".to_string())?;

    let mut pending: Vec<(u32, String)> = Vec::new();
    let paths = pass.pkg().compiled_go_files.clone();
    let n = pass.files().len();

    for i in 0..n {
        let Some(path) = paths.get(i) else {
            continue;
        };
        let filename = path.to_string_lossy();
        if !filename.ends_with("_test.go") {
            continue;
        }
        let Some((re_fset, parsed)) = reparse(path) else {
            continue;
        };
        let pass_file = &pass.files()[i];
        for re_pos in examples_missing_output(&parsed) {
            let mapped = map_pos(pass, pass_file, &re_fset, re_pos);
            pending.push((mapped, MESSAGE.to_string()));
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
        name: "testableexamples",
        doc: "linter checks if examples are testable (have an expected output)",
        url: "https://github.com/maratori/testableexamples",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_test_naming() {
        assert!(is_test("Example", "Example"));
        assert!(is_test("ExampleFoo", "Example"));
        assert!(is_test("Example_foo", "Example"));
        assert!(!is_test("Examplefoo", "Example")); // lower-case after prefix
        assert!(!is_test("NotExample", "Example"));
        assert!(is_test("Test", "Test"));
        assert!(!is_test("Testiness", "Test"));
    }

    #[test]
    fn output_prefix_matches() {
        let re = output_prefix();
        assert!(re.is_match("Output: hello\n"));
        assert!(re.is_match("output:\n"));
        assert!(re.is_match("Unordered output: x\n"));
        assert!(re.is_match("  Output: y\n"));
        assert!(!re.is_match("Out: hello\n"));
    }
}
