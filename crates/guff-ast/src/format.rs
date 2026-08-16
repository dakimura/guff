// Port of Go's go/format package to Rust.
//
// Original: Copyright 2012/2015 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// Entry points:
//* [`source`] — format a whole (or partial) Go source buffer like gofmt
//* [`node`] — format an AST node already in hand

use std::io::{self, Write};
use std::sync::Arc;

use crate::ast::{Decl, File};
use crate::import::sort_imports;
use crate::parser::{Mode as ParserMode, PARSE_COMMENTS, SKIP_OBJECT_RESOLUTION};
use crate::parser_interface::{self, ParseError};
use crate::position::FileSet;
use crate::printer::{self, Config, Mode as PrinterMode, PrintNode, NORMALIZE_NUMBERS, TAB_INDENT, USE_SPACES};
use crate::token::Token;

/// Keep in sync with cmd/gofmt and go/format.
const TAB_WIDTH: i32 = 8;
const PRINTER_MODE: PrinterMode = USE_SPACES | TAB_INDENT | NORMALIZE_NUMBERS;
const PARSER_MODE: ParserMode = ParserMode(PARSE_COMMENTS.0 | SKIP_OBJECT_RESOLUTION.0);

fn default_config() -> Config {
    Config {
        mode: PRINTER_MODE,
        tabwidth: TAB_WIDTH,
        indent: 0,
    }
}

/// Format `node` in canonical gofmt style and write the result to `dst`.
pub fn node<W: Write>(dst: &mut W, fset: &Arc<FileSet>, node: PrintNode<'_>) -> io::Result<()> {
    let config = default_config();

    let needs_sort = match &node {
        PrintNode::File(f) => has_unsorted_imports(f),
        PrintNode::Commented(c) => matches!(c.node.as_ref(), PrintNode::File(f) if has_unsorted_imports(f)),
        _ => false,
    };

    if needs_sort {
        let mut buf = Vec::new();
        config.fprint(&mut buf, fset, match &node {
            PrintNode::File(f) => PrintNode::File(f),
            PrintNode::Commented(c) => match c.node.as_ref() {
                PrintNode::File(f) => PrintNode::File(f),
                other => {
                    // Shouldn't happen given needs_sort.
                    return config.fprint(dst, fset, match other {
                        PrintNode::File(f) => PrintNode::File(f),
                        PrintNode::Expr(e) => PrintNode::Expr(e),
                        PrintNode::Stmt(s) => PrintNode::Stmt(s),
                        PrintNode::Decl(d) => PrintNode::Decl(d),
                        PrintNode::Spec(s) => PrintNode::Spec(s),
                        PrintNode::Stmts(s) => PrintNode::Stmts(s),
                        PrintNode::Decls(d) => PrintNode::Decls(d),
                        PrintNode::Commented(_) => unreachable!(),
                    });
                }
            },
            _ => unreachable!(),
        })?;
        let mut parsed = parser_interface::parse_file(fset, "", Some(&buf), PARSER_MODE)
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("format.Node internal error ({e})"),
                )
            })?;
        sort_imports(fset, &mut parsed);
        return config.fprint(dst, fset, PrintNode::File(&parsed));
    }

    config.fprint(dst, fset, node)
}
/// Format `src` in canonical gofmt style.
///
/// `src` is expected to be a syntactically correct Go source file, or a
/// list of Go declarations or statements (partial source).
pub fn source(src: &[u8]) -> Result<Vec<u8>, FormatError> {
    let fset = Arc::new(FileSet::new());
    let (file, source_adj, indent_adj) = parse(&fset, "", src, true)?;

    let mut file = file;
    if source_adj.is_none() {
        // Complete source file.
        sort_imports(&fset, &mut file);
    }

    format_buf(&fset, &file, source_adj.as_ref(), indent_adj, src, default_config())
}

/// gofmt output for a file that has already been parsed from the same bytes.
///
/// [`source`] is `parse` → [`sort_imports`] → print, and its parse uses exactly
/// this module's `PARSER_MODE`. A caller that has already parsed the identical
/// bytes with that mode — `guff_fmt`'s shared gci+gofumpt path is the one that
/// matters (PERF_TASKS_V8 §V8-2) — would otherwise pay for a second parse of
/// the file to get an AST it is holding.
///
/// `file` is sorted in place, exactly as [`source`] sorts its own copy. That is
/// the caller's AST, so the sort is visible afterwards; it is idempotent, and
/// every consumer of a gofmt-shaped AST sorts imports itself before printing.
///
/// **`file` must be the whole-file parse of the same bytes.** Handing it an AST
/// parsed from different source, or one of `parse`'s fragment wrappers, returns
/// formatted output for whatever it *was* — not an error.
pub fn source_parsed(fset: &Arc<FileSet>, file: &mut File) -> Result<Vec<u8>, FormatError> {
    sort_imports(fset, file);
    format_buf(fset, file, None, 0, &[], default_config())
}

/// Errors from [`source`] / [`node`].
#[derive(Debug)]
pub enum FormatError {
    Parse(ParseError),
    Io(io::Error),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::Parse(e) => write!(f, "{e}"),
            FormatError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FormatError {}

impl From<ParseError> for FormatError {
    fn from(e: ParseError) -> Self {
        FormatError::Parse(e)
    }
}

impl From<io::Error> for FormatError {
    fn from(e: io::Error) -> Self {
        FormatError::Io(e)
    }
}

type SourceAdj = Box<dyn Fn(&[u8], i32) -> Vec<u8>>;

/// Parse `src` as a Go source file, declaration list, or statement list.
fn parse(
    fset: &Arc<FileSet>,
    filename: &str,
    src: &[u8],
    fragment_ok: bool,
) -> Result<(File, Option<SourceAdj>, i32), FormatError> {
    match parser_interface::parse_file(fset, filename, Some(src), PARSER_MODE) {
        Ok(file) => return Ok((file, None, 0)),
        Err(err) => {
            if !fragment_ok || !err.to_string().contains("expected 'package'") {
                return Err(FormatError::Parse(err));
            }
        }
    }

    // Declaration list: insert a package clause.
    let mut psrc = Vec::with_capacity(src.len() + 16);
    psrc.extend_from_slice(b"package p;");
    psrc.extend_from_slice(src);
    match parser_interface::parse_file(fset, filename, Some(&psrc), PARSER_MODE) {
        Ok(file) => {
            let adj: SourceAdj = Box::new(|src, indent| {
                let skip = indent as usize + b"package p\n".len();
                let body = if skip <= src.len() { &src[skip..] } else { src };
                trim_space(body)
            });
            return Ok((file, Some(adj), 0));
        }
        Err(err) => {
            if !err.to_string().contains("expected declaration") {
                return Err(FormatError::Parse(err));
            }
        }
    }

    // Statement list: wrap in func _() { ... }.
    let mut fsrc = Vec::with_capacity(src.len() + 32);
    fsrc.extend_from_slice(b"package p; func _() {");
    fsrc.extend_from_slice(src);
    fsrc.extend_from_slice(b"\n\n}");
    let file = parser_interface::parse_file(fset, filename, Some(&fsrc), PARSER_MODE)?;
    let adj: SourceAdj = Box::new(|src, indent| {
        let mut indent = indent;
        if indent < 0 {
            indent = 0;
        }
        let prefix_len = 2 * indent as usize + b"package p\n\nfunc _() {".len();
        let body = if prefix_len <= src.len() {
            &src[prefix_len..]
        } else {
            src
        };
        let body = if body.len() >= 2 && body.ends_with(b"}\n") {
            &body[..body.len() - 2]
        } else {
            body
        };
        trim_space(body)
    });
    Ok((file, Some(adj), -1))
}

fn format_buf(
    fset: &Arc<FileSet>,
    file: &File,
    source_adj: Option<&SourceAdj>,
    indent_adj: i32,
    src: &[u8],
    mut cfg: Config,
) -> Result<Vec<u8>, FormatError> {
    if source_adj.is_none() {
        let mut buf = Vec::new();
        cfg.fprint(&mut buf, fset, PrintNode::File(file))?;
        return Ok(buf);
    }

    // Partial source file.
    let mut i = 0usize;
    let mut j = 0usize;
    while j < src.len() && is_space(src[j]) {
        if src[j] == b'\n' {
            i = j + 1;
        }
        j += 1;
    }
    let mut res = src[..i].to_vec();

    let mut indent = 0i32;
    let mut has_space = false;
    for &b in &src[i..j] {
        match b {
            b' ' => has_space = true,
            b'\t' => indent += 1,
            _ => {}
        }
    }
    if indent == 0 && has_space {
        indent = 1;
    }
    for _ in 0..indent {
        res.push(b'\t');
    }

    cfg.indent = indent + indent_adj;
    let mut buf = Vec::new();
    cfg.fprint(&mut buf, fset, PrintNode::File(file))?;
    let out = source_adj.unwrap()(&buf, cfg.indent);

    if out.is_empty() {
        return Ok(src.to_vec());
    }
    res.extend_from_slice(&out);

    let mut end = src.len();
    while end > 0 && is_space(src[end - 1]) {
        end -= 1;
    }
    res.extend_from_slice(&src[end..]);
    Ok(res)
}

fn has_unsorted_imports(file: &File) -> bool {
    for d in &file.decls {
        let Decl::GenDecl(g) = d else {
            return false;
        };
        if g.tok != Some(Token::IMPORT) {
            return false;
        }
        if g.lparen.is_valid() {
            return true;
        }
    }
    false
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

fn trim_space(s: &[u8]) -> Vec<u8> {
    let mut a = 0;
    let mut b = s.len();
    while a < b && is_space(s[a]) {
        a += 1;
    }
    while b > a && is_space(s[b - 1]) {
        b -= 1;
    }
    s[a..b].to_vec()
}

// Re-export printer config bits useful to callers.
pub use printer::{CommentedNode, Config as PrinterConfig};

#[cfg(test)]
mod tests {
    use super::source;

    fn fmt(src: &str) -> String {
        let out = source(src.as_bytes()).expect("format");
        String::from_utf8(out).expect("utf8")
    }

    #[test]
    fn combines_tilde_map_type_params() {
        let src = "package p\nfunc Equal[M1, M2 ~map[K]V, K, V comparable]() {}\n";
        let got = fmt(src);
        assert!(
            got.contains("func Equal[M1, M2 ~map[K]V, K, V comparable]()"),
            "expected combined type params, got:\n{got}"
        );
    }

    #[test]
    fn combines_func_type_params() {
        let src = "package p\nfunc gen(arch string, tags, zero, copy func()) {}\n";
        let got = fmt(src);
        assert!(
            got.contains("func gen(arch string, tags, zero, copy func())"),
            "expected combined func params, got:\n{got}"
        );
    }

    #[test]
    fn aligns_embedded_field_comments() {
        // Multi-field struct: named fields get 1 extra tab before comment,
        // embedded fields get 2 — tabwriter then aligns the comment column.
        let src = "package p\ntype T struct {\n\t*Request // original\n\tPublicKey // public\n}\n";
        let got = fmt(src);
        // After gofmt, both comments share the same column (spaces from tabs).
        let lines: Vec<&str> = got.lines().collect();
        let emb = lines.iter().find(|l| l.contains("*Request")).expect("embedded");
        let named = lines.iter().find(|l| l.contains("PublicKey")).expect("named");
        let emb_c = emb.find("//").expect("emb comment");
        let named_c = named.find("//").expect("named comment");
        assert_eq!(
            emb_c, named_c,
            "comment columns should align:\n{got}"
        );
    }

    #[test]
    fn keeps_mini_block_comment_body() {
        // Regression: strip_common_prefix must not empty short /* */ bodies
        // when the first non-blank inner line initializes the prefix.
        let src = "package p\n\n/*\nmini\n*/\nvar x int\n";
        let got = fmt(src);
        assert!(
            got.contains("mini"),
            "comment body must survive formatting, got:\n{got}"
        );
    }

    #[test]
    fn spaces_around_and_unary_xor() {
        // `x & ^y` must keep blanks around `&` (clash with &^ token).
        let src = "package p\nfunc f(x int) bool { return x & ^3 == 0 }\n";
        let got = fmt(src);
        assert!(
            got.contains("x & ^3 == 0"),
            "expected blanks around &, got:\n{got}"
        );
    }
}
