//! Native `goimports` — PERF_TASKS Task 1d.
//!
//! Implements `golang.org/x/tools/internal/imports.Process` for the common
//! path: delete unused imports, add from sibling files / stdlib, then
//! merge/group/sort + gofmt.
//!
//! Layout mirrors goimports precisely: specs are grouped into **runs** (blocks
//! the user separated with blank lines — `sortImports`), each run is sorted
//! independently and never merged with its neighbours, blank lines *between*
//! runs are preserved from the source, and blank lines are (re)inserted only at
//! import-group boundaries *within* a run (`astutil.Imports` +
//! `addImportSpaces`). Multiple import decls fold into the first
//! (`mergeImports`); adds are placed with `astutil.AddNamedImport`'s
//! longest-shared-prefix heuristic. Existing lines keep their original source
//! bytes (comments, blank `_` imports, aliases); only added specs are synthetic.
//!
//! Two cases defer to the system `goimports` binary via
//! [`FormatOutcome::NeedsResolver`]: unresolved third-party imports (no
//! module-cache resolver) and cgo files (`import "C"`, whose preamble layout is
//! not reproduced). So native output is always byte-identical to goimports or
//! deferred — it never silently diverges.

mod fix;

use std::cmp::Ordering;
use std::sync::Arc;

use guff::ast::{Decl, Spec};
use guff::format::{self as go_format, FormatError as AstFormatError};
use guff::parser::{Mode as ParserMode, ALL_ERRORS, PARSE_COMMENTS, SKIP_OBJECT_RESOLUTION, SKIP_STAMP_NODE_IDS};
use guff::parser_interface;
use guff::parser_resolver;
use guff::token::Token;
use guff::{FileSet, Pos, NO_POS};

use crate::native::NativeOptions;
use crate::runner::FormatError;

pub use fix::{FormatOutcome, ImportFix, ImportFixType, ImportInfo};

/// Parse with comments, skip full object resolution and node-id stamping.
/// After parse, [`parser_resolver::resolve_file_names_only`] fills `Ident.obj`
/// without cloning declaring AST nodes — enough for
/// [`fix::collect_references`] to skip local `x.y` while still seeing
/// unresolved package selectors (matching `x/tools/imports.Process`).
/// `ALL_ERRORS` disables the parser's Bailout panic after 10 errors (which
/// races under rayon and otherwise aborts format_checks workers).
const PARSER_MODE: ParserMode = ParserMode(
    PARSE_COMMENTS.0 | ALL_ERRORS.0 | SKIP_OBJECT_RESOLUTION.0 | SKIP_STAMP_NODE_IDS.0,
);

const C_IMPORT: &str = "\"C\"";

/// Format `src` like full `goimports -local …`.
///
/// Returns [`FormatError`] when the result is [`FormatOutcome::NeedsResolver`]
/// (CLI / harness path). Prefer [`format_outcome`] when a subprocess fallback
/// is available.
pub fn format(src: &[u8], opts: &NativeOptions) -> Result<Vec<u8>, FormatError> {
    match format_outcome(src, opts)? {
        FormatOutcome::Formatted(out) => Ok(out),
        FormatOutcome::NeedsResolver => Err(FormatError::Message {
            formatter: "native-goimports".into(),
            path: path_label(opts),
            message: "native goimports needs module resolver (unresolved imports)".into(),
        }),
    }
}

/// Like [`format`], but exposes unresolved third-party imports as
/// [`FormatOutcome::NeedsResolver`] instead of an error.
pub fn format_outcome(src: &[u8], opts: &NativeOptions) -> Result<FormatOutcome, FormatError> {
    match format_inner(src, opts) {
        Ok(o) => Ok(o),
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

fn format_inner(src: &[u8], opts: &NativeOptions) -> Result<FormatOutcome, AstFormatError> {
    // Bailout and other parser panics must not kill rayon workers. Degrade to
    // format-only (group/sort, no add/remove) which uses SkipObjectResolution
    // and matches full goimports on already-correct files.
    install_bailout_silencer();
    let _silence = BailoutSilence::new();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| format_inner_impl(src, opts))) {
        Ok(r) => r,
        Err(_) => format_only_inner(src, opts).map(FormatOutcome::Formatted),
    }
}

thread_local! {
    /// While set on the current thread, the installed panic hook suppresses the
    /// default panic output. Set *only* for the duration of our own
    /// `catch_unwind` around the parser (via [`BailoutSilence`]), so the
    /// parser's internally-caught `Bailout` (deep-nesting / >10 errors) does
    /// not spam stderr under rayon — while genuine panics anywhere else in the
    /// process, and on other threads, still report normally.
    static SILENCE_PANIC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// RAII guard: sets the thread-local silence flag on construction and restores
/// the previous value on drop (so nesting is safe).
struct BailoutSilence(bool);

impl BailoutSilence {
    fn new() -> Self {
        BailoutSilence(SILENCE_PANIC.with(|s| s.replace(true)))
    }
}

impl Drop for BailoutSilence {
    fn drop(&mut self) {
        SILENCE_PANIC.with(|s| s.set(self.0));
    }
}

/// Install (once, process-wide) a panic hook that suppresses output *only*
/// while [`SILENCE_PANIC`] is set on the panicking thread; all other panics are
/// forwarded to the previously-installed hook. This is far narrower than
/// matching on the panic's source location: real bugs — including ones in
/// `parser.rs` outside our guarded parse windows — keep printing.
fn install_bailout_silencer() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if SILENCE_PANIC.with(std::cell::Cell::get) {
                return;
            }
            default(info);
        }));
    });
}

/// Group/sort imports + gofmt without add/remove (`goimports -format-only`).
pub fn format_only(src: &[u8], opts: &NativeOptions) -> Result<Vec<u8>, FormatError> {
    install_bailout_silencer();
    let _silence = BailoutSilence::new();
    match format_only_inner(src, opts) {
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

fn format_only_inner(src: &[u8], opts: &NativeOptions) -> Result<Vec<u8>, AstFormatError> {
    let local = opts.local_prefixes.join(",");
    let filename = if opts.filename.is_empty() {
        "goimports.go"
    } else {
        opts.filename.as_str()
    };
    let fset = Arc::new(FileSet::new());
    // format-only: skip object resolution + stamp (no add/remove; layout + gofmt).
    const MODE: ParserMode = ParserMode(
        PARSE_COMMENTS.0 | SKIP_OBJECT_RESOLUTION.0 | SKIP_STAMP_NODE_IDS.0 | ALL_ERRORS.0,
    );
    let file = match parser_interface::parse_file(&fset, filename, Some(src), MODE) {
        Ok(f) => f,
        Err(_) => return go_format::source(src),
    };
    let region = import_region(&fset, &file, src)?;
    let imps = collect_imps(&fset, &file, src)?;
    if imps.is_empty() && !region.saw_import && region.c_chunks.is_empty() {
        // No imports to rewrite. `go/format.Source` would still re-parse; for an
        // already-clean file the identity result is `src` (check mode). When the
        // body needs gofmt, gofumpt/gofmt in the same `guff run` catch it.
        return Ok(src.to_vec());
    }
    let dist = reconstruct(src, &region, &imps, &local);
    let dist = strip_cr(&dist);
    if bytes_eq_strip_cr(dist.as_slice(), src) {
        return Ok(src.to_vec());
    }
    go_format::source(&dist)
}

fn format_inner_impl(src: &[u8], opts: &NativeOptions) -> Result<FormatOutcome, AstFormatError> {
    let local = opts.local_prefixes.join(",");
    let filename = if opts.filename.is_empty() {
        "goimports.go"
    } else {
        opts.filename.as_str()
    };

    let fset = Arc::new(FileSet::new());
    let file = match parser_interface::parse_file(&fset, filename, Some(src), PARSER_MODE) {
        Ok(f) => f,
        // Unparseable / too many errors: try format-only, else leave bytes.
        Err(_) => {
            return format_only_inner(src, opts).map(FormatOutcome::Formatted);
        }
    };
    // Cheap names-only resolve: sets Ident.obj for locals without ObjDecl AST clones.
    parser_resolver::resolve_file_names_only(&file);

    // cgo files (`import "C"`) carry a preamble comment bound to the C import
    // that goimports lays out specially; our textual reconstruction does not
    // reproduce it byte-for-byte. They are rare, so defer to the real binary
    // rather than risk a spurious "needs formatting" finding.
    if file.imports.iter().any(|i| i.path.value == C_IMPORT) {
        return Ok(FormatOutcome::NeedsResolver);
    }

    let fixes = match fix::get_fixes(&fset, &file, filename, src)? {
        fix::FixesResult::Fixes(f) => f,
        // Third-party resolution not ported — signal Formatter to subprocess.
        fix::FixesResult::NeedsResolver => return Ok(FormatOutcome::NeedsResolver),
    };

    let region = import_region(&fset, &file, src)?;
    let imps = arrange(collect_imps(&fset, &file, src)?, &fixes);

    if imps.is_empty() && !region.saw_import && region.c_chunks.is_empty() {
        return Ok(FormatOutcome::Formatted(src.to_vec()));
    }

    // No add/delete/rename: only rewrite when in-run sort order actually
    // changes, or multiple import decls still need merging. Otherwise keep
    // the source bytes (blank lines, orphan `#nosec` comments, etc.) —
    // matching system goimports when the import set is already correct.
    if fixes.is_empty()
        && !import_runs_need_sort(&imps, &local)
        && !import_runs_need_group_blanks(&imps, &local)
        && !needs_import_decl_merge(&file)
    {
        return Ok(FormatOutcome::Formatted(src.to_vec()));
    }

    let dist = reconstruct(src, &region, &imps, &local);
    let dist = strip_cr(&dist);
    if bytes_eq_strip_cr(dist.as_slice(), src) {
        // Import rewrite is a no-op. Skip `go/format.Source` (re-parse+print)
        // and the AST print — check mode only needs output==input, and a body
        // that still needs gofmt is reported by gofumpt/gofmt when enabled.
        // Profile: print/Source was ~half of native goimports CPU on prometheus.
        return Ok(FormatOutcome::Formatted(src.to_vec()));
    }
    Ok(FormatOutcome::Formatted(go_format::source(&dist)?))
}

/// True when any blank-line run's specs are not already in goimports sort order.
fn import_runs_need_sort(imps: &[Imp], local: &str) -> bool {
    for &(s, e) in &detect_runs(imps) {
        let slice = &imps[s..e];
        // Adjacent out-of-order pair ⇒ needs sort (no allocation).
        if slice
            .windows(2)
            .any(|w| cmp_imp(local, &w[0], &w[1]) == Ordering::Greater)
        {
            return true;
        }
    }
    false
}

/// True when a run contains an import-group boundary, which `addImportSpaces`
/// would separate with a blank line.
///
/// A run is by definition a maximal span with no blank line in it, so a group
/// boundary inside one is always missing its separator. Without this the
/// "nothing to do" exit above fires on a block that is correctly *sorted* but
/// not correctly *spaced* — `"fmt"`, `"os"`, `"github.com/x/y"` in one block
/// came back unchanged, where goimports blanks before the third.
///
/// There is no converse to check: a blank line between two imports of the same
/// group is a run boundary the user wrote, and goimports preserves it rather
/// than closing it up — verified against the real binary, and already guarded
/// by `preserves_user_blank_line_within_group`. Writing this predicate
/// symmetrically would break that test.
fn import_runs_need_group_blanks(imps: &[Imp], local: &str) -> bool {
    detect_runs(imps).iter().any(|&(s, e)| {
        imps[s..e]
            .windows(2)
            .any(|w| import_group(local, &w[0].path) != import_group(local, &w[1].path))
    })
}

/// goimports folds every non-C `import` decl into one parenthesized block.
fn needs_import_decl_merge(file: &guff::ast::File) -> bool {
    let mut n = 0usize;
    for decl in &file.decls {
        let Decl::GenDecl(gen) = decl else {
            break;
        };
        if gen.tok != Some(Token::IMPORT) {
            break;
        }
        let is_c = gen
            .specs
            .iter()
            .any(|s| matches!(s, Spec::ImportSpec(i) if i.path.value == C_IMPORT));
        if !is_c {
            n += 1;
            if n > 1 {
                return true;
            }
        }
    }
    false
}

fn strip_cr(s: &[u8]) -> Vec<u8> {
    if !s.contains(&b'\r') {
        return s.to_vec();
    }
    s.iter().copied().filter(|&b| b != b'\r').collect()
}

fn bytes_eq_strip_cr(a: &[u8], b: &[u8]) -> bool {
    if a == b {
        return true;
    }
    if !a.contains(&b'\r') && !b.contains(&b'\r') {
        return false;
    }
    strip_cr(a) == strip_cr(b)
}

#[derive(Debug, Clone)]
struct Imp {
    name: String,
    path: String,
    /// Original source slice `[doc .. trailing-comment]`; `None` for synthetic
    /// adds. Used verbatim on emit so comments/aliases ride with the spec.
    src_range: Option<(usize, usize)>,
    /// Offset of the import path's end; `None` for synthetic adds. Used to
    /// detect blank lines *between* runs in the original source.
    end_off: Option<usize>,
    /// Line of the path start / path end (goimports run detection). For
    /// synthetic adds these are copied from the neighbour so the add joins the
    /// same run (mirrors `astutil.AddNamedImport` reusing the previous pos).
    pos_line: i64,
    end_line: i64,
}

/// Apply the computed fixes to `imps` (source order) and return the arranged
/// list, mirroring `golang.org/x/tools/internal/imports` apply order: deletes
/// first, then adds placed via `astutil.AddNamedImport`. Run detection and
/// per-run sorting happen later in [`reconstruct`].
fn arrange(mut imps: Vec<Imp>, fixes: &[ImportFix]) -> Vec<Imp> {
    for fix in fixes {
        match fix.fix_type {
            ImportFixType::DeleteImport => {
                imps.retain(|imp| {
                    // Blank/dot imports are never in fix lists; match named ones.
                    !(imp.path == fix.stmt.import_path && imp.name == fix.stmt.name)
                });
            }
            ImportFixType::SetImportName => {
                // Not produced without loadRealPackageNames, but handle safely.
                for imp in imps.iter_mut() {
                    if imp.path == fix.stmt.import_path {
                        imp.name = fix.stmt.name.clone();
                        imp.src_range = None;
                        imp.end_off = None;
                    }
                }
            }
            ImportFixType::AddImport => {}
        }
    }
    for fix in fixes {
        if fix.fix_type != ImportFixType::AddImport {
            continue;
        }
        let (name, path) = (fix.stmt.name.clone(), fix.stmt.import_path.clone());
        if imps.iter().any(|i| i.path == path && i.name == name) {
            continue;
        }
        place_import(&mut imps, name, path);
    }
    imps
}

/// Port of `astutil.AddNamedImport`'s insertion heuristic: insert after the
/// import with the longest shared path prefix and adopt its line so the sorter
/// treats the new spec as part of the same run/block.
fn place_import(v: &mut Vec<Imp>, name: String, path: String) {
    let is_third_party = path.contains('.');
    let mut best: i64 = -1;
    let mut imp_index: i64 = -1;
    let mut seen_third_party = false;
    for (j, s) in v.iter().enumerate() {
        let n = match_len(&s.path, &path) as i64;
        if n > best || (best == 0 && !seen_third_party && is_third_party) {
            best = n;
            imp_index = j as i64;
        }
        seen_third_party = seen_third_party || s.path.contains('.');
    }
    let insert_at = if imp_index >= 0 {
        imp_index as usize + 1
    } else {
        0
    };
    let (pos_line, end_line) = if v.is_empty() {
        (1, 1)
    } else {
        let refi = insert_at.saturating_sub(1);
        (v[refi].pos_line, v[refi].pos_line)
    };
    v.insert(
        insert_at,
        Imp {
            name,
            path,
            src_range: None,
            end_off: None,
            pos_line,
            end_line,
        },
    );
}

/// `astutil.matchLen`: number of shared path segments (counted by `/`) in the
/// common byte prefix of `x` and `y`.
fn match_len(x: &str, y: &str) -> usize {
    let (xb, yb) = (x.as_bytes(), y.as_bytes());
    let mut n = 0;
    let mut i = 0;
    while i < xb.len() && i < yb.len() && xb[i] == yb[i] {
        if xb[i] == b'/' {
            n += 1;
        }
        i += 1;
    }
    n
}

/// True if `s` contains a blank line (a newline followed by only whitespace and
/// then another newline). Used to decide whether an inter-run separator in the
/// source was a real blank line (preserved) or just a comment (rides with its
/// spec).
fn has_blank_line(s: &[u8]) -> bool {
    let mut newlines = 0;
    for &b in s {
        match b {
            b'\n' => {
                newlines += 1;
                if newlines >= 2 {
                    return true;
                }
            }
            b' ' | b'\t' | b'\r' => {}
            _ => newlines = 0,
        }
    }
    false
}

#[derive(Debug)]
struct ImportRegion {
    head_end: usize,
    tail_start: usize,
    c_chunks: Vec<(usize, usize)>,
    saw_import: bool,
    /// True if any non-C import GenDecl used parentheses (keep even for 1 spec).
    had_paren: bool,
}

fn import_region(
    fset: &FileSet,
    file: &guff::ast::File,
    src: &[u8],
) -> Result<ImportRegion, AstFormatError> {
    let f = fset
        .file(file.package)
        .ok_or_else(|| io_err("missing file in FileSet".into()))?;

    let mut head_end = 0usize;
    let mut tail_start = 0usize;
    let mut c_chunks = Vec::new();
    let mut saw_import = false;
    let mut had_paren = false;

    for decl in &file.decls {
        let Decl::GenDecl(gen) = decl else {
            break;
        };
        if gen.tok != Some(Token::IMPORT) {
            break;
        }
        saw_import = true;

        let is_c = gen
            .specs
            .iter()
            .any(|s| matches!(s, Spec::ImportSpec(i) if i.path.value == C_IMPORT));

        if gen.lparen != NO_POS {
            // Parentheses around non-C imports (or mixed); track for emit.
            if !is_c || gen.specs.len() > 1 {
                had_paren = true;
            }
        }

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
        // For a non-parenthesized import, `decl.end()` stops at the path and
        // excludes a trailing line comment (`import _ "x" // note`). The paren
        // form's rparen already covers it. Extend past that comment so it is
        // not duplicated between the emitted spec bytes and the copied tail.
        let mut decl_end = decl.end();
        if let Some(Spec::ImportSpec(last)) = gen.specs.last() {
            if let Some(cg) = &last.comment {
                if f.offset(cg.end()) > f.offset(decl_end) {
                    decl_end = cg.end();
                }
            }
        }
        tail_start = pos_gci_end(&f, decl_end, src.len());

        if is_c {
            let start = if let Some(doc) = &gen.doc {
                pos_start(&f, doc.pos())
            } else {
                pos_start(&f, gen.tok_pos)
            };
            let end = pos_gci_end(&f, decl.end(), src.len());
            c_chunks.push((start, end));
        }
    }

    if !saw_import {
        head_end = pos_gci_end(&f, file.name.end(), src.len());
        if head_end < src.len() && src[head_end] == b'\n' {
            head_end += 1;
        }
        tail_start = head_end;
        while tail_start < src.len() && (src[tail_start] == b'\n' || src[tail_start] == b'\r') {
            tail_start += 1;
        }
    }

    Ok(ImportRegion {
        head_end,
        tail_start,
        c_chunks,
        saw_import,
        had_paren,
    })
}

fn collect_imps(
    fset: &FileSet,
    file: &guff::ast::File,
    src: &[u8],
) -> Result<Vec<Imp>, AstFormatError> {
    let f = fset
        .file(file.package)
        .ok_or_else(|| io_err("missing file in FileSet".into()))?;

    // `mergeImports` folds every non-C import decl into the first one,
    // repositioning the moved specs onto the first decl's line. That collapses
    // their original blank-line runs — only the first decl's runs survive. Find
    // that first decl so merged specs adopt its line for run detection.
    let mut first_decl_idx: Option<usize> = None;
    let mut merge_line: i64 = 1;
    for (di, decl) in file.decls.iter().enumerate() {
        let Decl::GenDecl(gen) = decl else { break };
        if gen.tok != Some(Token::IMPORT) {
            break;
        }
        let is_c = gen
            .specs
            .iter()
            .any(|s| matches!(s, Spec::ImportSpec(i) if i.path.value == C_IMPORT));
        if !is_c {
            first_decl_idx = Some(di);
            merge_line = f.line(gen.tok_pos);
            break;
        }
    }

    let mut imps = Vec::new();
    for (di, decl) in file.decls.iter().enumerate() {
        let Decl::GenDecl(gen) = decl else {
            break;
        };
        if gen.tok != Some(Token::IMPORT) {
            break;
        }
        for spec in &gen.specs {
            let Spec::ImportSpec(imp) = spec else {
                continue;
            };
            if imp.path.value == C_IMPORT {
                continue;
            }
            let (start, end, name) = import_range(&f, imp, src.len());
            // Run detection uses the path line (matches both sortImports' Pos/End
            // and astutil.Imports' ValuePos gaps). Specs merged from a later decl
            // adopt the first decl's line so they never open a spurious run.
            let (pos_line, end_line) = if Some(di) == first_decl_idx {
                (f.line(imp.path.value_pos), f.line(imp.path.end()))
            } else {
                (merge_line, merge_line)
            };
            imps.push(Imp {
                name,
                path: trim_quotes(&imp.path.value),
                src_range: Some((start, end)),
                end_off: Some(f.offset(imp.path.end()) as usize),
                pos_line,
                end_line,
            });
        }
    }
    absorb_orphan_line_comments(src, &mut imps);
    Ok(imps)
}

/// Fold free-floating `//` comments that sit on the lines after an import
/// spec (before the next spec / blank-line gap) into that spec's `src_range`.
///
/// go/parser leaves these on `File.comments` rather than `ImportSpec.comment`
/// when they are not same-line. System goimports keeps them; without this,
/// reconstruct drops grafana-style `#nosec` notes inside the import block.
fn absorb_orphan_line_comments(src: &[u8], imps: &mut [Imp]) {
    for i in 0..imps.len() {
        let Some((start, mut end)) = imps[i].src_range else {
            continue;
        };
        let limit = imps
            .get(i + 1)
            .and_then(|n| n.src_range.map(|(s, _)| s))
            .unwrap_or(src.len());
        end = extend_through_line_comments(src, end, limit);
        imps[i].src_range = Some((start, end));
    }
}

fn extend_through_line_comments(src: &[u8], mut end: usize, limit: usize) -> usize {
    let limit = limit.min(src.len());
    while end < limit {
        // Skip spaces/tabs on the current line remainder.
        while end < limit && (src[end] == b' ' || src[end] == b'\t') {
            end += 1;
        }
        if end >= limit {
            break;
        }
        if src[end] == b'\r' {
            end += 1;
            continue;
        }
        if src[end] == b'\n' {
            // Peek whether the next non-empty content is a line comment.
            let mut j = end + 1;
            while j < limit && (src[j] == b' ' || src[j] == b'\t') {
                j += 1;
            }
            if j + 1 < limit && src[j] == b'/' && src[j + 1] == b'/' {
                // Absorb newline + comment through end-of-line.
                end = j + 2;
                while end < limit && src[end] != b'\n' {
                    if src[end] == b'\r' {
                        end += 1;
                        continue;
                    }
                    end += 1;
                }
                continue;
            }
            // Blank line or next spec — stop (keep `end` before this newline
            // so inter-run blank detection still sees the gap).
            break;
        }
        if end + 1 < limit && src[end] == b'/' && src[end + 1] == b'/' {
            end += 2;
            while end < limit && src[end] != b'\n' {
                if src[end] == b'\r' {
                    end += 1;
                    continue;
                }
                end += 1;
            }
            continue;
        }
        break;
    }
    end
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

/// Port of `golang.org/x/tools/internal/imports.importGroup`.
pub(crate) fn import_group(local_prefix: &str, import_path: &str) -> i32 {
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

fn cmp_imp(local: &str, a: &Imp, b: &Imp) -> Ordering {
    // Blank imports sort with their path group; Go keeps them in the block.
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
    a.name.cmp(&b.name)
}

/// Contiguous source-order specs with no blank-line gap between them —
/// goimports' unit of sorting (`sortImports`) and of group-blank insertion
/// (`astutil.Imports` + `addImportSpaces`). Blank lines *between* runs are the
/// user's and are preserved verbatim; blanks *within* a run are removed and
/// re-inserted at import-group boundaries.
fn detect_runs(imps: &[Imp]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    if imps.is_empty() {
        return runs;
    }
    let mut start = 0;
    for j in 1..imps.len() {
        if imps[j].pos_line > 1 + imps[j - 1].end_line {
            runs.push((start, j));
            start = j;
        }
    }
    runs.push((start, imps.len()));
    runs
}

/// Source byte span `[min start .. max path-end]` of a run's *real* specs
/// (synthetic adds contribute nothing). Order-independent, so it survives the
/// in-run sort. Used to inspect the original inter-run separator.
fn run_span(src_ranges: &[Imp], run: (usize, usize)) -> (Option<usize>, Option<usize>) {
    let mut start: Option<usize> = None;
    let mut end: Option<usize> = None;
    for imp in &src_ranges[run.0..run.1] {
        if let Some((s, _)) = imp.src_range {
            start = Some(start.map_or(s, |v: usize| v.min(s)));
        }
        if let Some(e) = imp.end_off {
            end = Some(end.map_or(e, |v: usize| v.max(e)));
        }
    }
    (start, end)
}

fn reconstruct(src: &[u8], region: &ImportRegion, imps: &[Imp], local: &str) -> Vec<u8> {
    // Detect runs on source order, then sort *within* each run only.
    let runs = detect_runs(imps);
    let mut ordered: Vec<Imp> = imps.to_vec();
    for &(s, e) in &runs {
        ordered[s..e].sort_by(|a, b| cmp_imp(local, a, b));
    }

    let mut head = src[..region.head_end.min(src.len())].to_vec();

    for &(cs, ce) in &region.c_chunks {
        let cs = cs.min(src.len());
        let ce = ce.min(src.len()).max(cs);
        if cs >= region.head_end {
            head.extend_from_slice(&src[cs..ce]);
            if !head.ends_with(b"\n") {
                head.push(b'\n');
            }
        }
    }

    let use_paren = ordered.len() > 1 || (ordered.len() == 1 && region.had_paren);

    let mut body: Vec<u8> = Vec::new();
    if !ordered.is_empty() {
        // When imports already existed, `head` ends at the original `import`
        // keyword — keep whatever spacing/comments preceded it. Only when we
        // are inserting a brand-new import block after `package` do we force
        // a blank line.
        if !region.saw_import {
            ensure_blank_line_before_import(&mut head);
        } else if !head.ends_with(b"\n") {
            head.push(b'\n');
        }
        if use_paren {
            head.extend_from_slice(b"import (");
            head.push(b'\n');
            for (ri, &run) in runs.iter().enumerate() {
                if ri > 0 {
                    // Inter-run separator: emit a blank line only if the source
                    // had one (a bare comment between runs rides with its spec
                    // and needs no synthesized blank).
                    let prev = runs[ri - 1];
                    let (_, prev_end) = run_span(&ordered, prev);
                    let (cur_start, _) = run_span(&ordered, run);
                    let blank = match (prev_end, cur_start) {
                        (Some(pe), Some(cs)) if cs >= pe => {
                            has_blank_line(&src[pe..cs.min(src.len())])
                        }
                        // Synthetic edge (no real spec): fall back to group.
                        _ => {
                            import_group(local, &ordered[prev.1 - 1].path)
                                != import_group(local, &ordered[run.0].path)
                        }
                    };
                    if blank {
                        body.push(b'\n');
                    }
                }
                // Within a run: sort order set above; insert a blank at each
                // import-group change (addImportSpaces).
                let mut first = true;
                let mut last_group = -1i32;
                for imp in &ordered[run.0..run.1] {
                    let g = import_group(local, &imp.path);
                    if !first && g != last_group {
                        body.push(b'\n');
                    }
                    body.push(b'\t');
                    body.extend_from_slice(&imp_bytes(src, imp));
                    body.push(b'\n');
                    first = false;
                    last_group = g;
                }
            }
            body.push(b')');
            body.push(b'\n');
        } else {
            head.extend_from_slice(b"import ");
            body.extend_from_slice(&imp_bytes(src, &ordered[0]));
            body.push(b'\n');
        }
    }

    let imps = &ordered;
    let tail_start = region.tail_start.min(src.len());
    let mut dist = Vec::with_capacity(head.len() + body.len() + src.len() - tail_start + 2);
    dist.extend_from_slice(&head);
    dist.extend_from_slice(&body);
    if !imps.is_empty() && !body.is_empty() {
        let tail = &src[tail_start..];
        if !tail.is_empty() && !tail.starts_with(b"\n") && !dist.ends_with(b"\n\n") {
            dist.push(b'\n');
        }
    }
    dist.extend_from_slice(&src[tail_start..]);
    dist
}

fn ensure_blank_line_before_import(head: &mut Vec<u8>) {
    if !head.ends_with(b"\n") {
        head.push(b'\n');
    }
    if head.len() < 2 || head[head.len() - 2] != b'\n' {
        head.push(b'\n');
    }
}

fn imp_bytes(src: &[u8], imp: &Imp) -> Vec<u8> {
    if let Some((start, end)) = imp.src_range {
        let start = start.min(src.len());
        let end = end.min(src.len()).max(start);
        // import_range end often lands past the trailing newline; strip so the
        // caller can emit a single `\n` without creating blank lines.
        //
        // The tab matters as much as the newline. A spec that is not the last
        // one in its block records a range ending `"fmt"\n\t` — the newline
        // plus the *next* line's indentation. Stopping at `\n`/`\r` alone left
        // the loop looking at the tab on its first step, so it never removed
        // the newline either, and the caller's own `\n` then closed a line
        // holding one tab. go/printer trims that to empty, which is why the
        // symptom was a blank line appearing between two imports of the same
        // group (compat/fmt goimports cases).
        let mut slice = &src[start..end];
        while matches!(slice.last(), Some(b'\n' | b'\r' | b'\t' | b' ')) {
            slice = &slice[..slice.len() - 1];
        }
        return slice.to_vec();
    }
    let mut out = Vec::new();
    if !imp.name.is_empty() {
        out.extend_from_slice(imp.name.as_bytes());
        out.push(b' ');
    }
    out.push(b'"');
    out.extend_from_slice(imp.path.as_bytes());
    out.push(b'"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(local: &str, filename: &str) -> NativeOptions {
        NativeOptions {
            local_prefixes: if local.is_empty() {
                vec![]
            } else {
                vec![local.into()]
            },
            filename: filename.into(),
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

func f() {
	fmt.Println()
	_ = bar.X
	_ = pkg.Y
}
"#;
        let out = format(src, &opts("github.com/org/project", "p.go")).unwrap();
        let s = String::from_utf8(out).unwrap();
        let fmt_pos = s.find("\"fmt\"").unwrap();
        let bar_pos = s.find("\"github.com/foo/bar\"").unwrap();
        let pkg_pos = s.find("\"github.com/org/project/pkg\"").unwrap();
        assert!(fmt_pos < bar_pos && bar_pos < pkg_pos, "got:\n{s}");
    }

    /// Helper: the import block of the formatted output, with tabs kept so a
    /// stray blank line is visible.
    fn import_block(src: &[u8], local: &str) -> String {
        let out = format(src, &opts(local, "p.go")).unwrap();
        let s = String::from_utf8(out).unwrap();
        let start = s.find("import (").expect("paren import block");
        let end = s[start..].find("\n)").expect("block close") + start + 2;
        s[start..end].to_string()
    }

    /// `addImportSpaces`: goimports always separates import groups, even when
    /// the specs are already in sort order. guff used to short-circuit on
    /// "already sorted" and skip the rewrite, leaving no blank line at all.
    #[test]
    fn group_blank_is_added_to_an_already_sorted_block() {
        let src = br#"package p

import (
	"fmt"
	"os"
	"github.com/x/y"
)

func f() { fmt.Println(os.Args, y.Z) }
"#;
        assert_eq!(
            import_block(src, ""),
            "import (\n\t\"fmt\"\n\t\"os\"\n\n\t\"github.com/x/y\"\n)"
        );
    }

    /// The same, with only two specs — the smallest shape that needs a
    /// separator but no reordering.
    #[test]
    fn group_blank_is_added_between_two_sorted_specs() {
        let src = br#"package p

import (
	"fmt"
	"github.com/x/y"
)

func f() { fmt.Println(y.Z) }
"#;
        assert_eq!(
            import_block(src, ""),
            "import (\n\t\"fmt\"\n\n\t\"github.com/x/y\"\n)"
        );
    }

    /// Three standard-library imports, one block, out of order. Every one of
    /// them is `import_group` 0, so no separator belongs anywhere — the blank
    /// line guff used to emit here came from a spec's recorded range carrying
    /// its own `\n\t`, not from the group logic.
    #[test]
    fn sorting_three_same_group_specs_adds_no_blank() {
        let src = br#"package p

import (
	"fmt"
	"os"
	"bytes"
)

func f() { fmt.Println(os.Args, bytes.MinRead) }
"#;
        assert_eq!(
            import_block(src, ""),
            "import (\n\t\"bytes\"\n\t\"fmt\"\n\t\"os\"\n)"
        );
    }

    /// The same defect with a reorder that also crosses a group boundary: one
    /// separator, before the third-party import, and nothing between `fmt`
    /// and `os`.
    #[test]
    fn sorting_across_a_group_boundary_adds_exactly_one_blank() {
        let src = br#"package p

import (
	"fmt"
	"github.com/x/y"
	"os"
)

func f() { fmt.Println(os.Args, y.Z) }
"#;
        assert_eq!(
            import_block(src, ""),
            "import (\n\t\"fmt\"\n\t\"os\"\n\n\t\"github.com/x/y\"\n)"
        );
    }

    /// Already correct: sorted and separated. This is the shape every corpus
    /// on disk is in, which is why `regress/fmt_diff.py --formatter goimports`
    /// was green through both defects above.
    #[test]
    fn an_already_grouped_block_is_left_alone() {
        let src = br#"package p

import (
	"fmt"
	"os"

	"github.com/x/y"
)

func f() { fmt.Println(os.Args, y.Z) }
"#;
        assert_eq!(
            import_block(src, ""),
            "import (\n\t\"fmt\"\n\t\"os\"\n\n\t\"github.com/x/y\"\n)"
        );
    }

    #[test]
    fn deletes_unused_import_keeps_paren() {
        let src = br#"package pkg

import (
	"fmt"
	"os"
)

func F() {
	fmt.Println("hi")
}
"#;
        let out = format(src, &opts("", "a.go")).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("import (\n\t\"fmt\"\n)"), "got:\n{s}");
        assert!(!s.contains("\"os\""), "got:\n{s}");
    }

    #[test]
    fn adds_stdlib_import() {
        let src = br#"package pkg

func F() {
	fmt.Println("hi")
}
"#;
        let out = format(src, &opts("", "a.go")).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert_eq!(
            s,
            "package pkg\n\nimport \"fmt\"\n\nfunc F() {\n\tfmt.Println(\"hi\")\n}\n"
        );
    }

    #[test]
    fn keeps_blank_imports() {
        let src = br#"package pkg

import (
	"fmt"
	_ "os"
)

func F() {
	fmt.Println("hi")
}
"#;
        let out = format(src, &opts("", "a.go")).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("_ \"os\""), "got:\n{s}");
        assert!(s.contains("\"fmt\""), "got:\n{s}");
    }

    #[test]
    fn preserves_user_blank_line_within_group() {
        // Two stdlib runs the user split by hand: goimports keeps both blocks
        // (sorts within, never merges). Regression for the flatten-regroup bug.
        let src = b"package p\n\nimport (\n\t\"os\"\n\n\t\"fmt\"\n)\n\nfunc f() { fmt.Println(os.Args) }\n";
        let out = format(src, &opts("", "p.go")).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), String::from_utf8(src.to_vec()).unwrap());
    }

    #[test]
    fn local_selector_not_treated_as_package() {
        // Shadowing `fmt` must not spuriously add/keep a fmt import.
        let src = br#"package pkg

func F() {
	fmt := struct{ Println func(string) }{Println: func(string) {}}
	fmt.Println("hi")
}
"#;
        let out = format(src, &opts("", "a.go")).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(
            !s.contains("import"),
            "local fmt.Println must not add import:\n{s}"
        );
    }

    #[test]
    fn merges_multiple_import_decls_into_one_run() {
        // `import x "fmt"` + a following block: mergeImports collapses them onto
        // one line, so the result is a single group-sorted run (goyacc output).
        let src = b"package p\n\nimport a \"fmt\"\n\nimport (\n\t\"os\"\n)\n\nfunc f() { a.Println(os.Args) }\n";
        let out = format(src, &opts("", "p.go")).unwrap();
        let s = String::from_utf8(out).unwrap();
        // Both fold into a single paren block with no blank between them.
        assert!(s.contains("import (\n\ta \"fmt\"\n\t\"os\"\n)"), "got:\n{s}");
    }

    #[test]
    fn linkname_trailing_comment_not_duplicated() {
        // `import _ "unsafe" // for linkname`: the trailing comment must appear
        // exactly once (tail must not re-copy it).
        let src = b"package p\n\nimport _ \"unsafe\" // for linkname\n\nvar x int\n";
        let out = format(src, &opts("", "p.go")).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s.matches("for linkname").count(), 1, "got:\n{s}");
        assert_eq!(s, String::from_utf8(src.to_vec()).unwrap(), "got:\n{s}");
    }

    #[test]
    fn cgo_defers_to_resolver() {
        // Files importing "C" defer to the subprocess rather than risk a wrong
        // cgo-preamble layout.
        let src = b"package p\n\n// #include <stdlib.h>\nimport \"C\"\n\nfunc f() { C.free(nil) }\n";
        match format_outcome(src, &opts("", "p.go")).unwrap() {
            FormatOutcome::NeedsResolver => {}
            FormatOutcome::Formatted(_) => panic!("cgo file should defer to resolver"),
        }
    }

    #[test]
    fn nosec_comment_in_import_block_stable() {
        // Blank line after a dangling #nosec comment inside the import block
        // (grafana externalservices_test.go). System goimports leaves this
        // layout alone; native must too when imports are all used.
        let src = br#"package database

import (
	"context"
	// #nosec G505 Used only for generating a 160 bit hash, it's not used for security purposes

	"errors"
	"testing"
)

func f(ctx context.Context, t *testing.T) error {
	_ = ctx
	_ = t
	return errors.New("x")
}
"#;
        let out = format(src, &opts("", "externalservices_test.go")).unwrap();
        let got = String::from_utf8(out).unwrap();
        let want = String::from_utf8(src.to_vec()).unwrap();
        assert_eq!(got, want, "native goimports must leave #nosec-in-import layout alone\ngot:\n{got}");
    }
}
