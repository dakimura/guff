//! Native `goimports` — PERF_TASKS Task 1d (format-only / group+sort).
//!
//! Implements `goimports -format-only` behavior from
//! `golang.org/x/tools/internal/imports`: merge import decls, sort by
//! (group, path, name), insert blank lines between groups, then gofmt.
//!
//! Does **not** add/remove imports (no module resolution). On the prometheus
//! corpus, `goimports -format-only` is byte-identical to full `goimports`
//! (725/725), so this is enough for that harness. Files that need missing
//! import fixes still require the subprocess path / future work.

use std::cmp::Ordering;
use std::sync::Arc;

use guff::ast::{Decl, Spec};
use guff::format::{self as go_format, FormatError as AstFormatError};
use guff::parser::{Mode as ParserMode, PARSE_COMMENTS, SKIP_OBJECT_RESOLUTION};
use guff::parser_interface;
use guff::token::Token;
use guff::{FileSet, Pos};

use crate::native::NativeOptions;
use crate::runner::FormatError;

const PARSER_MODE: ParserMode = ParserMode(PARSE_COMMENTS.0 | SKIP_OBJECT_RESOLUTION.0);
const C_IMPORT: &str = "\"C\"";

/// Format `src` like `goimports -format-only -local …`.
pub fn format(src: &[u8], opts: &NativeOptions) -> Result<Vec<u8>, FormatError> {
    match format_inner(src, opts) {
        Ok(out) => Ok(out),
        Err(AstFormatError::Parse(e)) => Err(FormatError::Message {
            formatter: "native-goimports".into(),
            path: path_label(opts),
            message: e.to_string(),
        }),
        Err(AstFormatError::Io(e)) => Err(FormatError::Io {
            formatter: "native-goimports".into(),
            path: path_label(opts),
            source: e,
        }),
    }
}

fn path_label(opts: &NativeOptions) -> String {
    if opts.filename.is_empty() {
        "<standard input>".into()
    } else {
        opts.filename.clone()
    }
}

fn format_inner(src: &[u8], opts: &NativeOptions) -> Result<Vec<u8>, AstFormatError> {
    let local = opts.local_prefixes.join(",");
    let mut parsed = match parse_imports(src, &opts.filename)? {
        Some(p) => p,
        None => return go_format::source(src),
    };

    if parsed.imports.len() <= 1 {
        // goimports does not force `import (` around a lone import.
        return go_format::source(src);
    }

    parsed.imports.sort_by(|a, b| cmp_import(&local, a, b));
    let dist = reconstruct(src, &parsed, &local);
    let dist: Vec<u8> = dist.into_iter().filter(|&b| b != b'\r').collect();
    go_format::source(&dist)
}

#[derive(Debug, Clone)]
struct Imp {
    start: usize,
    end: usize,
    name: String,
    path: String,
    comment: String,
}

#[derive(Debug)]
struct Parsed {
    imports: Vec<Imp>,
    head_end: usize,
    tail_start: usize,
    /// Optional C import block to keep before the merged import `(`.
    c_chunks: Vec<(usize, usize)>,
}

fn parse_imports(src: &[u8], filename: &str) -> Result<Option<Parsed>, AstFormatError> {
    let fset = Arc::new(FileSet::new());
    let name = if filename.is_empty() {
        "goimports.go"
    } else {
        filename
    };
    let file = parser_interface::parse_file(&fset, name, Some(src), PARSER_MODE)?;
    if file.imports.is_empty() {
        return Ok(None);
    }

    let f = fset
        .file(file.package)
        .ok_or_else(|| io_err("missing file in FileSet".into()))?;

    let mut head_end = 0usize;
    let mut tail_start = 0usize;
    let mut imports = Vec::new();
    let mut c_chunks = Vec::new();

    for decl in &file.decls {
        let Decl::GenDecl(gen) = decl else {
            continue;
        };
        if gen.tok != Some(Token::IMPORT) {
            break; // imports are first
        }

        let is_c = gen.specs.iter().any(|s| {
            matches!(s, Spec::ImportSpec(i) if i.path.value == C_IMPORT)
        });

        if head_end == 0 {
            if is_c {
                if let Some(doc) = &gen.doc {
                    head_end = pos_start(&f, doc.pos());
                } else {
                    head_end = pos_start(&f, gen.tok_pos);
                }
            } else {
                head_end = pos_start(&f, gen.tok_pos);
            }
        }
        tail_start = pos_gci_end(&f, decl.end(), src.len());

        if is_c {
            // Keep the whole C import decl (doc + import "C") as a raw chunk.
            let start = if let Some(doc) = &gen.doc {
                pos_start(&f, doc.pos())
            } else {
                pos_start(&f, gen.tok_pos)
            };
            let end = pos_gci_end(&f, decl.end(), src.len());
            c_chunks.push((start, end));
            // Non-C specs in the same decl (rare) still get collected below.
        }

        for spec in &gen.specs {
            let Spec::ImportSpec(imp) = spec else {
                continue;
            };
            if imp.path.value == C_IMPORT {
                continue;
            }
            let (start, end, name) = import_range(&f, imp, src.len());
            imports.push(Imp {
                start,
                end,
                name,
                path: trim_quotes(&imp.path.value),
                comment: imp
                    .comment
                    .as_ref()
                    .map(|c| c.text())
                    .unwrap_or_default(),
            });
        }
    }

    Ok(Some(Parsed {
        imports,
        head_end,
        tail_start,
        c_chunks,
    }))
}

fn io_err(msg: String) -> AstFormatError {
    AstFormatError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, msg))
}

fn trim_quotes(v: &str) -> String {
    v.trim_matches('"').to_string()
}

fn pos_start(file: &guff::File, pos: Pos) -> usize {
    file.offset(pos) as usize
}

fn pos_gci_end(file: &guff::File, pos: Pos, src_len: usize) -> usize {
    (file.offset(pos) as usize + 1).min(src_len)
}

fn import_range(
    file: &guff::File,
    imp: &guff::ast::ImportSpec,
    src_len: usize,
) -> (usize, usize, String) {
    let start = if let Some(doc) = &imp.doc {
        pos_start(file, doc.pos())
    } else if let Some(name) = &imp.name {
        pos_start(file, name.pos())
    } else {
        pos_start(file, imp.path.value_pos)
    };
    let name = imp
        .name
        .as_ref()
        .map(|n| n.name.clone())
        .unwrap_or_default();
    let end = if let Some(cg) = &imp.comment {
        pos_gci_end(file, cg.end(), src_len)
    } else {
        pos_gci_end(file, imp.path.end(), src_len)
    };
    (start, end, name)
}

/// Port of `golang.org/x/tools/internal/imports.importGroup`.
fn import_group(local_prefix: &str, import_path: &str) -> i32 {
    if !local_prefix.is_empty() {
        for p in local_prefix.split(',') {
            let p = p.trim();
            if p.is_empty() {
                continue;
            }
            if import_path.starts_with(p) || p.trim_end_matches('/') == import_path {
                return 3;
            }
        }
    }
    if import_path.starts_with("appengine") {
        return 2;
    }
    let first = import_path.split('/').next().unwrap_or("");
    if first.contains('.') {
        return 1;
    }
    0
}

fn cmp_import(local: &str, a: &Imp, b: &Imp) -> Ordering {
    let ga = import_group(local, &a.path);
    let gb = import_group(local, &b.path);
    match ga.cmp(&gb) {
        Ordering::Equal => {}
        o => return o,
    }
    match a.path.cmp(&b.path) {
        Ordering::Equal => {}
        o => return o,
    }
    match a.name.cmp(&b.name) {
        Ordering::Equal => {}
        o => return o,
    }
    a.comment.cmp(&b.comment)
}

fn reconstruct(src: &[u8], parsed: &Parsed, local: &str) -> Vec<u8> {
    let imports = &parsed.imports;
    let mut body: Vec<u8> = Vec::new();
    let mut first = true;
    let mut last_group = -1i32;

    for imp in imports {
        let g = import_group(local, &imp.path);
        if !first && g != last_group {
            body.push(b'\n');
        }
        if !first {
            body.push(b'\t');
        } else {
            first = false;
        }
        last_group = g;
        let end = imp.end.min(src.len());
        let start = imp.start.min(end);
        body.extend_from_slice(&src[start..end]);
    }

    let mut head = src[..parsed.head_end.min(src.len())].to_vec();

    // Emit C import decls first (goimports keeps them separate / unmerged).
    for &(cs, ce) in &parsed.c_chunks {
        let cs = cs.min(src.len());
        let ce = ce.min(src.len()).max(cs);
        // Avoid duplicating if C was already in the head region.
        if cs >= parsed.head_end {
            head.extend_from_slice(&src[cs..ce]);
            if !head.ends_with(b"\n") {
                head.push(b'\n');
            }
        }
    }

    if !imports.is_empty() {
        head.extend_from_slice(b"import (");
        head.push(b'\n');
        body.push(b')');
        body.push(b'\n');
    }

    let tail_start = parsed.tail_start.min(src.len());
    let mut dist = Vec::with_capacity(head.len() + body.len() + src.len() - tail_start);
    dist.extend_from_slice(&head);
    dist.extend_from_slice(&body);
    dist.extend_from_slice(&src[tail_start..]);
    dist
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(local: &str) -> NativeOptions {
        NativeOptions {
            local_prefixes: if local.is_empty() {
                vec![]
            } else {
                vec![local.into()]
            },
            filename: "p.go".into(),
            ..Default::default()
        }
    }

    #[test]
    fn groups_local_after_third_party() {
        let src = br#"package p

import (
	"github.com/org/project/pkg"
	"github.com/foo/bar"
	"fmt"
)

func f() {}
"#;
        let out = format(src, &opts("github.com/org/project")).unwrap();
        let s = String::from_utf8(out).unwrap();
        let fmt_pos = s.find("\"fmt\"").unwrap();
        let bar_pos = s.find("\"github.com/foo/bar\"").unwrap();
        let pkg_pos = s.find("\"github.com/org/project/pkg\"").unwrap();
        assert!(fmt_pos < bar_pos && bar_pos < pkg_pos, "got:\n{s}");
        assert!(
            s.contains("\"github.com/foo/bar\"\n\n\t\"github.com/org/project/pkg\""),
            "got:\n{s}"
        );
    }

    #[test]
    fn merges_top_level_import() {
        let src = br#"package p

import __yyfmt__ "fmt"

import (
	"math"

	"github.com/foo/bar"
)

func f() {}
"#;
        let out = format(src, &opts("")).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("__yyfmt__"), "got:\n{s}");
        assert!(!s.contains("import __yyfmt__"), "should merge:\n{s}");
        assert!(s.contains("import ("), "got:\n{s}");
    }
}
