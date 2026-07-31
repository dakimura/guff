//! Native `gci` — PERF_TASKS Task 1e.
//!
//! Port of `github.com/daixiang0/gci` v0.14 `LoadFormat`: parse → extract
//! import byte ranges → assign to sections → reconstruct `import (` block →
//! native gofmt. Does not add/remove imports; only reorders groups.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, OnceLock};

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

static STANDARD_PACKAGES: OnceLock<HashSet<&'static str>> = OnceLock::new();

fn standard_packages() -> &'static HashSet<&'static str> {
    STANDARD_PACKAGES.get_or_init(|| {
        include_str!("gci_std_packages.txt")
            .lines()
            .filter(|l| !l.is_empty())
            .collect()
    })
}

/// Format `src` like `gci print` with sections / order flags from `opts`.
pub fn format(src: &[u8], opts: &NativeOptions) -> Result<Vec<u8>, FormatError> {
    match format_inner(src, opts) {
        Ok(out) => Ok(out),
        Err(AstFormatError::Parse(e)) => Err(FormatError::Message {
            formatter: "native-gci".into(),
            path: path_label(opts),
            message: e.to_string(),
        }),
        Err(AstFormatError::Io(e)) => Err(FormatError::Io {
            formatter: "native-gci".into(),
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
    let sections = resolve_sections(opts)?;
    let parsed = parse_imports(src, &opts.filename)?;
    let Some(parsed) = parsed else {
        // No imports (or only generated skip — we don't skip by default).
        return Ok(src.to_vec());
    };
    format_from_parsed_imports(src, &parsed, &sections)
}

/// B-10: share one skip-object parse with gofumpt. Returns `(gci_out, gofumpt_out)`.
pub(crate) fn format_shared_with_gofumpt(
    src: &[u8],
    gci_opts: &NativeOptions,
    fumpt_opts: &NativeOptions,
) -> (Result<Vec<u8>, FormatError>, Result<Vec<u8>, FormatError>) {
    let map_err = |e: AstFormatError, which: &str| -> FormatError {
        match e {
            AstFormatError::Parse(e) => FormatError::Message {
                formatter: which.into(),
                path: path_label(gci_opts),
                message: e.to_string(),
            },
            AstFormatError::Io(e) => FormatError::Io {
                formatter: which.into(),
                path: path_label(gci_opts),
                source: e,
            },
        }
    };

    let sections = match resolve_sections(gci_opts) {
        Ok(s) => s,
        Err(e) => {
            let err = map_err(e, "native-gci");
            return (Err(err), Err(FormatError::Message {
                formatter: "native-gofumpt".into(),
                path: path_label(fumpt_opts),
                message: "shared parse aborted after gci section resolve".into(),
            }));
        }
    };

    let fset = Arc::new(FileSet::new());
    // gofumpt inserts a dummy base file; keep gci's parse filename for positions.
    let _ = fset.add_file("gofumpt_base.go", 1, 10);
    let name = if gci_opts.filename.is_empty() {
        "gci.go"
    } else {
        gci_opts.filename.as_str()
    };
    let mut file = match parser_interface::parse_file(&fset, name, Some(src), PARSER_MODE) {
        Ok(f) => f,
        Err(e) => {
            let gci_err = map_err(AstFormatError::Parse(e), "native-gci");
            let fumpt_err = FormatError::Message {
                formatter: "native-gofumpt".into(),
                path: path_label(fumpt_opts),
                message: gci_err.to_string(),
            };
            return (Err(gci_err), Err(fumpt_err));
        }
    };

    let gci_out = match extract_imports(src, &fset, &file) {
        Ok(None) => Ok(src.to_vec()),
        Ok(Some(parsed)) => format_from_parsed_imports(src, &parsed, &sections)
            .map_err(|e| map_err(e, "native-gci")),
        Err(e) => Err(map_err(e, "native-gci")),
    };

    let fumpt_out = super::gofumpt::format_parsed(&fset, &mut file, fumpt_opts);

    (gci_out, fumpt_out)
}

fn format_from_parsed_imports(
    src: &[u8],
    parsed: &ParsedImports,
    sections: &[Section],
) -> Result<Vec<u8>, AstFormatError> {
    // gci: do not reformat when ≤1 non-C import.
    if parsed.imports.len() <= 1 {
        return Ok(src.to_vec());
    }

    let grouped = assign_sections(&parsed.imports, sections)?;
    let dist = reconstruct(src, parsed, sections, &grouped);
    // Match gci: strip CR, then gofmt.
    let dist: Vec<u8> = dist.into_iter().filter(|&b| b != b'\r').collect();
    go_format::source(&dist)
}

// ---- section model ---------------------------------------------------------

#[derive(Debug, Clone)]
enum Section {
    Standard,
    Default,
    Prefix(String),
    Blank,
    Dot,
    Alias,
    LocalModule(Vec<String>),
    NewLine,
}

impl Section {
    fn type_name(&self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Default => "default",
            Self::Prefix(_) => "custom",
            Self::Blank => "blank",
            Self::Dot => "dot",
            Self::Alias => "alias",
            Self::LocalModule(_) => "localmodule",
            Self::NewLine => "newline",
        }
    }

    fn key(&self) -> String {
        match self {
            Self::Standard => "standard".into(),
            Self::Default => "default".into(),
            Self::Prefix(p) => format!("prefix({p})"),
            Self::Blank => "blank".into(),
            Self::Dot => "dot".into(),
            Self::Alias => "alias".into(),
            Self::LocalModule(_) => "localmodule".into(),
            Self::NewLine => "newline".into(),
        }
    }

    fn default_order_rank(&self) -> i32 {
        match self {
            Self::Standard => 0,
            Self::Default => 1,
            Self::Prefix(_) => 2,
            Self::Blank => 3,
            Self::Dot => 4,
            Self::Alias => 5,
            Self::LocalModule(_) => 6,
            Self::NewLine => 7,
        }
    }

    fn match_specificity(&self, imp: &GciImport) -> Specificity {
        match self {
            Self::Standard => {
                if standard_packages().contains(imp.path.as_str()) {
                    Specificity::Standard
                } else {
                    Specificity::Mismatch
                }
            }
            Self::Default => Specificity::Default,
            Self::Prefix(prefixes) => {
                let mut best = 0usize;
                let mut matched = false;
                for prefix in prefixes.split(',') {
                    let prefix = prefix.trim();
                    if prefix.is_empty() {
                        continue;
                    }
                    if imp.path.starts_with(prefix) && prefix.len() > best {
                        best = prefix.len();
                        matched = true;
                    }
                }
                if matched {
                    Specificity::Match(best)
                } else {
                    Specificity::Mismatch
                }
            }
            Self::Blank => {
                if imp.name == "_" {
                    Specificity::Name
                } else {
                    Specificity::Mismatch
                }
            }
            Self::Dot => {
                if imp.name == "." {
                    Specificity::Name
                } else {
                    Specificity::Mismatch
                }
            }
            Self::Alias => {
                if !imp.name.is_empty() && imp.name != "." && imp.name != "_" {
                    Specificity::Name
                } else {
                    Specificity::Mismatch
                }
            }
            Self::LocalModule(paths) => {
                for path in paths {
                    if &imp.path == path || imp.path.starts_with(&format!("{path}/")) {
                        return Specificity::LocalModule;
                    }
                }
                Specificity::Mismatch
            }
            Self::NewLine => Specificity::Mismatch,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Specificity {
    Mismatch,
    Default,
    Standard,
    Match(usize),
    Name,
    LocalModule,
}

impl Specificity {
    fn class(self) -> u8 {
        match self {
            Self::Mismatch => 0,
            Self::Default => 10,
            Self::Standard => 20,
            Self::Match(_) => 30,
            Self::Name => 40,
            Self::LocalModule => 50,
        }
    }

    fn is_more_specific(self, than: Self) -> bool {
        if self.class() != than.class() {
            return self.class() > than.class();
        }
        match (self, than) {
            (Self::Match(a), Self::Match(b)) => a > b,
            _ => false,
        }
    }

    fn equal(self, other: Self) -> bool {
        !self.is_more_specific(other) && !other.is_more_specific(self)
    }
}

fn resolve_sections(opts: &NativeOptions) -> Result<Vec<Section>, AstFormatError> {
    let raw = if opts.gci_sections.is_empty() {
        vec!["standard".into(), "default".into()]
    } else {
        opts.gci_sections.clone()
    };
    let mut sections = parse_section_strings(&raw, opts)?;
    if !opts.gci_custom_order {
        sections.sort_by(|a, b| {
            let ta = a.type_name();
            let tb = b.type_name();
            if opts.gci_no_lex_order || ta != tb {
                a.default_order_rank().cmp(&b.default_order_rank())
            } else {
                a.key().cmp(&b.key())
            }
        });
    }
    Ok(sections)
}

fn parse_section_strings(
    raw: &[String],
    opts: &NativeOptions,
) -> Result<Vec<Section>, AstFormatError> {
    let mut list = Vec::new();
    let mut err = String::new();
    for d in raw {
        let s = d.to_ascii_lowercase();
        if s.is_empty() {
            continue;
        }
        if s == "default" {
            list.push(Section::Default);
        } else if s == "standard" {
            list.push(Section::Standard);
        } else if s == "newline" {
            list.push(Section::NewLine);
        } else if s.starts_with("prefix(") && d.len() > 8 {
            list.push(Section::Prefix(d[7..d.len() - 1].to_string()));
        } else if s.starts_with("commentline(") && d.len() > 13 {
            // gci treats commentline(...) like a custom prefix section.
            list.push(Section::Prefix(d[12..d.len() - 1].to_string()));
        } else if s == "dot" {
            list.push(Section::Dot);
        } else if s == "blank" {
            list.push(Section::Blank);
        } else if s == "alias" {
            list.push(Section::Alias);
        } else if s == "localmodule" {
            let paths = find_local_module_paths(opts)?;
            list.push(Section::LocalModule(paths));
        } else {
            err.push(' ');
            err.push_str(&s);
        }
    }
    if !err.is_empty() {
        return Err(io_err(format!("invalid gci section params:{err}")));
    }
    Ok(list)
}

fn find_local_module_paths(opts: &NativeOptions) -> Result<Vec<String>, AstFormatError> {
    // Match gci CLI: GOMOD env, else ./go.mod from cwd. When --filename is set,
    // also try walking from that file (helps harness / golangci paths).
    if let Ok(gomod) = std::env::var("GOMOD") {
        if !gomod.is_empty() && gomod != "/dev/null" {
            if let Some(p) = read_module_path(Path::new(&gomod))? {
                return Ok(vec![p]);
            }
        }
    }
    if let Some(p) = read_module_path(Path::new("go.mod"))? {
        return Ok(vec![p]);
    }
    if !opts.filename.is_empty() && opts.filename != "<standard input>" {
        if let Some(p) = find_go_mod_upwards(Path::new(&opts.filename))? {
            return Ok(vec![p]);
        }
    }
    Err(io_err(
        "could not find module path for `localModule` configuration".into(),
    ))
}

fn find_go_mod_upwards(start: &Path) -> Result<Option<String>, AstFormatError> {
    let mut dir = if start.is_file() || start.extension().is_some() {
        start.parent().unwrap_or(start).to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        let candidate = dir.join("go.mod");
        if let Some(p) = read_module_path(&candidate)? {
            return Ok(Some(p));
        }
        if !dir.pop() {
            return Ok(None);
        }
    }
}

fn read_module_path(path: &Path) -> Result<Option<String>, AstFormatError> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path).map_err(AstFormatError::Io)?;
    for line in raw.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("module ") {
            let m = rest.trim().trim_matches('"');
            if !m.is_empty() {
                return Ok(Some(m.to_string()));
            }
        }
    }
    Ok(None)
}

fn io_err(msg: String) -> AstFormatError {
    AstFormatError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, msg))
}

// ---- parse -----------------------------------------------------------------

#[derive(Debug, Clone)]
struct GciImport {
    start: usize,
    end: usize,
    name: String,
    path: String,
}

#[derive(Debug)]
struct ParsedImports {
    imports: Vec<GciImport>,
    head_end: usize,
    tail_start: usize,
    c_start: usize,
    c_end: usize,
}

fn parse_imports(src: &[u8], filename: &str) -> Result<Option<ParsedImports>, AstFormatError> {
    let fset = Arc::new(FileSet::new());
    let name = if filename.is_empty() {
        "gci.go"
    } else {
        filename
    };
    let file = parser_interface::parse_file(&fset, name, Some(src), PARSER_MODE)?;
    extract_imports(src, &fset, &file)
}

fn extract_imports(
    src: &[u8],
    fset: &FileSet,
    file: &guff::ast::File,
) -> Result<Option<ParsedImports>, AstFormatError> {
    if file.imports.is_empty() {
        return Ok(None);
    }

    let f = fset
        .file(file.package)
        .ok_or_else(|| io_err("missing file in FileSet".into()))?;

    let mut head_end = 0usize;
    let mut tail_start = 0usize;
    let mut c_start = 0usize;
    let mut c_end = 0usize;
    let mut data: Vec<GciImport> = Vec::new();

    for (index, decl) in file.decls.iter().enumerate() {
        let Decl::GenDecl(gen) = decl else {
            continue;
        };
        if gen.tok != Some(Token::IMPORT) {
            continue;
        }

        if head_end == 0 {
            head_end = pos_to_start_byte(&f, gen.tok_pos);
        }
        // Match gci: int(decl.End()) with base=1 ⇒ offset(End)+1 exclusive? 
        // GenDecl.End is Pos(rparen+1) which is the Pos after `)`.
        // gci uses int(End) as 0-based exclusive when base=1 (= End.0 when base=1).
        tail_start = pos_to_gci_end_byte(&f, decl.end(), src.len());

        for spec in &gen.specs {
            let Spec::ImportSpec(imp) = spec else {
                continue;
            };
            if imp.path.value == C_IMPORT {
                if let Some(doc) = &gen.doc {
                    c_start = pos_to_start_byte(&f, doc.pos());
                    if index == 0 {
                        head_end = c_start;
                    }
                } else {
                    c_start = pos_to_start_byte(&f, gen.tok_pos);
                }
                c_end = pos_to_gci_end_byte(&f, decl.end(), src.len());
                continue;
            }

            let (start, end, name) = import_byte_range(&f, imp, src.len());
            let path = trim_quotes(&imp.path.value);
            data.push(GciImport {
                start,
                end,
                name,
                path,
            });
        }
    }

    data.sort_by(|a, b| match a.path.cmp(&b.path) {
        Ordering::Equal => a.name.cmp(&b.name),
        o => o,
    });

    Ok(Some(ParsedImports {
        imports: data,
        head_end,
        tail_start,
        c_start,
        c_end,
    }))
}

fn trim_quotes(v: &str) -> String {
    v.trim_matches('"').to_string()
}

fn pos_to_start_byte(file: &guff::File, pos: Pos) -> usize {
    file.offset(pos) as usize
}

/// gci uses `int(pos)` as exclusive end when FileSet base is 1, which is
/// `offset(pos) + 1` — intentionally including one byte past the AST end
/// (typically the trailing newline).
fn pos_to_gci_end_byte(file: &guff::File, pos: Pos, src_len: usize) -> usize {
    let end = file.offset(pos) as usize + 1;
    end.min(src_len)
}

fn import_byte_range(
    file: &guff::File,
    imp: &guff::ast::ImportSpec,
    src_len: usize,
) -> (usize, usize, String) {
    let start = if let Some(doc) = &imp.doc {
        pos_to_start_byte(file, doc.pos())
    } else if let Some(name) = &imp.name {
        pos_to_start_byte(file, name.pos())
    } else {
        pos_to_start_byte(file, imp.path.value_pos)
    };

    let name = imp
        .name
        .as_ref()
        .map(|n| n.name.clone())
        .unwrap_or_default();

    let end = if let Some(cg) = &imp.comment {
        pos_to_gci_end_byte(file, cg.end(), src_len)
    } else {
        pos_to_gci_end_byte(file, imp.path.end(), src_len)
    };

    (start, end, name)
}

// ---- assign / reconstruct --------------------------------------------------

fn assign_sections(
    imports: &[GciImport],
    sections: &[Section],
) -> Result<Vec<(String, Vec<(usize, usize)>)>, AstFormatError> {
    // Preserve section order; map key → list of (start,end) blocks.
    let mut result: Vec<(String, Vec<(usize, usize)>)> =
        sections.iter().map(|s| (s.key(), Vec::new())).collect();

    for d in imports {
        let mut best_idx: Option<usize> = None;
        let mut best_spec = Specificity::Mismatch;
        for (i, section) in sections.iter().enumerate() {
            let spec = section.match_specificity(d);
            if spec != Specificity::Mismatch && spec.equal(best_spec) {
                // gci returns nil,nil on tie — treat as error.
                return Err(io_err(format!(
                    "equal specificity match for import {}",
                    d.path
                )));
            }
            if spec.is_more_specific(best_spec) {
                best_spec = spec;
                best_idx = Some(i);
            }
        }
        let Some(idx) = best_idx else {
            return Err(io_err(format!(
                "no matching section for import {}",
                d.path
            )));
        };
        result[idx].1.push((d.start, d.end));
    }
    Ok(result)
}

fn reconstruct(
    src: &[u8],
    parsed: &ParsedImports,
    _sections: &[Section],
    grouped: &[(String, Vec<(usize, usize)>)],
) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    let mut first = true;

    for (_key, blocks) in grouped {
        if blocks.is_empty() {
            continue;
        }
        if !body.is_empty() {
            body.push(b'\n');
        }
        for &(start, end) in blocks {
            if !first {
                body.push(b'\t');
            } else {
                first = false;
            }
            let end = end.min(src.len());
            let start = start.min(end);
            body.extend_from_slice(&src[start..end]);
        }
    }

    let mut head = src[..parsed.head_end.min(src.len())].to_vec();
    if parsed.c_start != 0 {
        let cs = parsed.c_start.min(src.len());
        let ce = parsed.c_end.min(src.len()).max(cs);
        head.extend_from_slice(&src[cs..ce]);
        head.push(b'\n');
    }
    head.extend_from_slice(b"import (");
    head.push(b'\n');

    body.push(b')');
    body.push(b'\n');

    let tail_start = parsed.tail_start.min(src.len());
    let tail = &src[tail_start..];

    let mut dist = Vec::with_capacity(head.len() + body.len() + tail.len());
    dist.extend_from_slice(&head);
    dist.extend_from_slice(&body);
    dist.extend_from_slice(tail);
    dist
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(sections: &[&str]) -> NativeOptions {
        NativeOptions {
            gci_sections: sections.iter().map(|s| s.to_string()).collect(),
            filename: "p.go".into(),
            ..Default::default()
        }
    }

    #[test]
    fn shared_with_gofumpt_matches_separate_paths() {
        let src = br#"package p

import (
	"github.com/foo/bar"
	"fmt"
)

func f( a int ) {
	fmt.Println(a)
	_ = bar.X
}
"#;
        let gci_opts = opts(&["standard", "default"]);
        let fumpt_opts = NativeOptions {
            filename: "p.go".into(),
            extra_rules: false,
            match_golangci: true,
            ..Default::default()
        };
        let gci_alone = format(src, &gci_opts).unwrap();
        let fumpt_alone = super::super::gofumpt::format(src, &fumpt_opts).unwrap();
        let (gci_shared, fumpt_shared) =
            format_shared_with_gofumpt(src, &gci_opts, &fumpt_opts);
        assert_eq!(gci_shared.unwrap(), gci_alone);
        assert_eq!(fumpt_shared.unwrap(), fumpt_alone);
        // Sanity: at least one formatter rewrites this fixture.
        assert!(gci_alone != src || fumpt_alone != src);
    }

    #[test]
    fn sorts_stdlib_before_third_party() {
        let src = br#"package p

import (
	"github.com/foo/bar"
	"fmt"
)

func f() {
	fmt.Println()
	_ = bar.X
}
"#;
        let out = format(src, &opts(&["standard", "default"])).unwrap();
        let s = String::from_utf8(out).unwrap();
        let fmt_pos = s.find("\"fmt\"").unwrap();
        let bar_pos = s.find("\"github.com/foo/bar\"").unwrap();
        assert!(fmt_pos < bar_pos, "got:\n{s}");
        assert!(
            s.contains("\"fmt\"\n\n\t\"github.com/foo/bar\""),
            "got:\n{s}"
        );
    }

    #[test]
    fn prefix_section_groups_local() {
        let src = br#"package p

import (
	"github.com/org/project/pkg"
	"github.com/foo/bar"
	"fmt"
)

func f() {}
"#;
        let out = format(
            src,
            &opts(&[
                "standard",
                "default",
                "prefix(github.com/org/project)",
            ]),
        )
        .unwrap();
        let s = String::from_utf8(out).unwrap();
        let fmt_pos = s.find("\"fmt\"").unwrap();
        let bar_pos = s.find("\"github.com/foo/bar\"").unwrap();
        let pkg_pos = s.find("\"github.com/org/project/pkg\"").unwrap();
        assert!(fmt_pos < bar_pos && bar_pos < pkg_pos, "got:\n{s}");
    }

    #[test]
    fn single_import_unchanged() {
        let src = b"package p\n\nimport \"fmt\"\n\nfunc f() { fmt.Println() }\n";
        let out = format(src, &opts(&["standard", "default"])).unwrap();
        assert_eq!(out, src);
    }
}
