//! Port of `github.com/rogpeppe/go-internal/diff` (v1.16.0, golangci-lint
//! 2.14.0's pin) — Go's `internal/diff`, the anchored diff.
//!
//! golangci-lint renders every formatter result through it: `fmt --diff`
//! prints the patch, and the formatter linters parse the same patch back into
//! findings (`goformatters/internal/diff.go`). A minimal-edit diff (Myers) can
//! align a repeated line — a blank, a `}` — differently, and a different
//! alignment is a different line in the finding and a different patch.
//!
//! Lines are byte slices, as Go's strings are: a file need not be UTF-8.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pair {
    x: usize,
    y: usize,
}

/// `diff.Diff`: an anchored diff of `old` and `new` in unified format, with
/// three lines of context. Empty when the inputs are equal.
pub fn diff(old_name: &str, old: &[u8], new_name: &str, new: &[u8]) -> Vec<u8> {
    if old == new {
        return Vec::new();
    }
    let x = lines(old);
    let y = lines(new);

    let mut out = Vec::new();
    out.extend_from_slice(format!("diff {old_name} {new_name}\n").as_bytes());
    out.extend_from_slice(format!("--- {old_name}\n").as_bytes());
    out.extend_from_slice(format!("+++ {new_name}\n").as_bytes());

    const C: usize = 3; // number of context lines
    let mut done = Pair { x: 0, y: 0 }; // printed up to x[..done.x] and y[..done.y]
    let mut chunk = Pair { x: 0, y: 0 }; // start lines of current chunk
    let mut count = Pair { x: 0, y: 0 }; // number of lines from each side in current chunk
    let mut ctext: Vec<(u8, &[u8])> = Vec::new(); // lines for current chunk

    for m in tgs(&x, &y) {
        if m.x < done.x {
            // Already handled scanning forward from earlier match.
            continue;
        }

        // Expand matching lines as far as possible.
        let mut start = m;
        while start.x > done.x && start.y > done.y && x[start.x - 1] == y[start.y - 1] {
            start.x -= 1;
            start.y -= 1;
        }
        let mut end = m;
        while end.x < x.len() && end.y < y.len() && x[end.x] == y[end.y] {
            end.x += 1;
            end.y += 1;
        }

        // Emit the mismatched lines before start into this chunk.
        for s in &x[done.x..start.x] {
            ctext.push((b'-', s));
            count.x += 1;
        }
        for s in &y[done.y..start.y] {
            ctext.push((b'+', s));
            count.y += 1;
        }

        // If we're not at EOF and have too few common lines, the chunk
        // includes all the common lines and continues.
        if (end.x < x.len() || end.y < y.len())
            && (end.x - start.x < C || (!ctext.is_empty() && end.x - start.x < 2 * C))
        {
            for s in &x[start.x..end.x] {
                ctext.push((b' ', s));
                count.x += 1;
                count.y += 1;
            }
            done = end;
            continue;
        }

        // End chunk with common lines for context.
        if !ctext.is_empty() {
            let n = (end.x - start.x).min(C);
            for s in &x[start.x..start.x + n] {
                ctext.push((b' ', s));
                count.x += 1;
                count.y += 1;
            }
            done = Pair { x: start.x + n, y: start.y + n };

            // Convert line numbers to 1-indexed. Special case: empty file
            // shows up as 0,0 not 1,0.
            if count.x > 0 {
                chunk.x += 1;
            }
            if count.y > 0 {
                chunk.y += 1;
            }
            out.extend_from_slice(
                format!("@@ -{},{} +{},{} @@\n", chunk.x, count.x, chunk.y, count.y).as_bytes(),
            );
            for (prefix, s) in &ctext {
                out.push(*prefix);
                out.extend_from_slice(s);
            }
            count = Pair { x: 0, y: 0 };
            ctext.clear();
        }

        // If we reached EOF, we're done.
        if end.x >= x.len() && end.y >= y.len() {
            break;
        }

        // Otherwise start a new chunk.
        chunk = Pair { x: end.x - C, y: end.y - C };
        for s in &x[chunk.x..end.x] {
            ctext.push((b' ', s));
            count.x += 1;
            count.y += 1;
        }
        done = end;
    }
    out
}

/// `lines`: the lines of `x`, each with its newline. A last line without one
/// carries the BSD/GNU diff warning instead.
fn lines(x: &[u8]) -> Vec<std::borrow::Cow<'_, [u8]>> {
    use std::borrow::Cow;
    // strings.SplitAfter(x, "\n")
    let mut l: Vec<Cow<'_, [u8]>> = Vec::new();
    let mut start = 0;
    for (i, &b) in x.iter().enumerate() {
        if b == b'\n' {
            l.push(Cow::Borrowed(&x[start..=i]));
            start = i + 1;
        }
    }
    let rest = &x[start..];
    if rest.is_empty() {
        // The trailing "" element is dropped.
    } else {
        let mut last = rest.to_vec();
        last.extend_from_slice(b"\n\\ No newline at end of file\n");
        l.push(Cow::Owned(last));
    }
    l
}

/// `tgs`: the pairs of indexes of the longest common subsequence of unique
/// lines in `x` and `y` (a line that appears once in each), with sentinels
/// `{0,0}` and `{len(x),len(y)}`. Szymanski's Algorithm A.
fn tgs<T: AsRef<[u8]>>(x: &[T], y: &[T]) -> Vec<Pair> {
    // 0, 1, many counted as 0, -1, -2 for x and 0, -4, -8 for y.
    let mut m: HashMap<&[u8], i64> = HashMap::new();
    for s in x {
        let c = m.entry(s.as_ref()).or_insert(0);
        if *c > -2 {
            *c -= 1;
        }
    }
    for s in y {
        let c = m.entry(s.as_ref()).or_insert(0);
        if *c > -8 {
            *c -= 4;
        }
    }

    let mut xi: Vec<usize> = Vec::new();
    let mut yi: Vec<usize> = Vec::new();
    let mut inv: Vec<usize> = Vec::new();
    for (i, s) in y.iter().enumerate() {
        let c = m.get_mut(s.as_ref()).expect("counted above");
        if *c == -1 + -4 {
            *c = yi.len() as i64;
            yi.push(i);
        }
    }
    for (i, s) in x.iter().enumerate() {
        if let Some(&j) = m.get(s.as_ref()) {
            if j >= 0 {
                xi.push(i);
                inv.push(j as usize);
            }
        }
    }

    let j = &inv;
    let n = xi.len();
    let mut t = vec![n + 1; n];
    let mut l = vec![0usize; n];
    for i in 0..n {
        // sort.Search(n, func(k) bool { return T[k] >= J[i] })
        let k = t.partition_point(|&v| v < j[i]);
        t[k] = j[i];
        l[i] = k + 1;
    }
    let mut k = l.iter().copied().max().unwrap_or(0);
    let mut seq = vec![Pair { x: 0, y: 0 }; 2 + k];
    seq[1 + k] = Pair { x: x.len(), y: y.len() };
    // Upstream never lowers `lastj` inside the loop; kept as written.
    let lastj = n;
    for i in (0..n).rev() {
        if l[i] == k && j[i] < lastj {
            seq[k] = Pair { x: xi[i], y: yi[j[i]] };
            k -= 1;
        }
    }
    seq[0] = Pair { x: 0, y: 0 };
    seq
}

/// One `Change{From, To}` of golangci-lint's `hunkChangesParser`
/// (`pkg/goformatters/internal/diff.go`), 1-based original line numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub from: i64,
    pub to: i64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LineType {
    Added,
    Original,
    Deleted,
}

#[derive(Clone, Copy)]
struct DiffLine {
    original_number: i64,
    typ: LineType,
}

/// One hunk as sourcegraph/go-diff v0.9.0's `ReadHunk` leaves it: the
/// original start line, the body, and `OrigNoNewlineAt`.
struct Hunk {
    orig_start_line: i64,
    body: Vec<u8>,
    orig_no_newline_at: usize,
}

const NO_NEWLINE_MESSAGE: &[u8] = b"\\ No newline at end of file";

/// The hunks of a single-file patch that [`diff`] produced.
fn parse_hunks(patch: &[u8]) -> Vec<Hunk> {
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut last_line_from_orig = true;
    for line in patch.split(|&b| b == b'\n') {
        if let Some(header) = line.strip_prefix(b"@@ -") {
            // "@@ -%d,%d +%d,%d @@"
            let header = String::from_utf8_lossy(header);
            let start = header
                .split(|c| c == ',' || c == ' ')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            hunks.push(Hunk {
                orig_start_line: start,
                body: Vec::new(),
                orig_no_newline_at: 0,
            });
            last_line_from_orig = true;
            continue;
        }
        let Some(h) = hunks.last_mut() else {
            continue; // the file header
        };
        if line == NO_NEWLINE_MESSAGE {
            if last_line_from_orig {
                // Keep the newline; record that the orig had none.
                h.orig_no_newline_at = h.body.len();
            } else if !h.body.is_empty() {
                // Remove the previous line's newline.
                h.body.pop();
            }
            continue;
        }
        if let Some(&c) = line.first() {
            last_line_from_orig = c == b'-';
        } else {
            // The empty tail after the final newline.
            continue;
        }
        h.body.extend_from_slice(line);
        h.body.push(b'\n');
    }
    hunks
}

/// `parseDiffLines`.
fn parse_diff_lines(h: &Hunk) -> Vec<DiffLine> {
    let lines: Vec<&[u8]> = h.body.split(|&b| b == b'\n').collect();
    let mut current = h.orig_start_line;
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if i == lines.len() - 1 && line.is_empty() {
            break;
        }
        let typ = match line.first() {
            Some(b'-') => {
                current += 1;
                LineType::Deleted
            }
            Some(b'+') => LineType::Added,
            _ => {
                current += 1;
                LineType::Original
            }
        };
        // The number recorded is the one *before* the increment.
        let original_number = if typ == LineType::Added { current } else { current - 1 };
        out.push(DiffLine { original_number, typ });
    }
    if h.orig_no_newline_at > 0 {
        out.push(DiffLine {
            original_number: current + 1,
            typ: LineType::Added,
        });
    }
    out
}

/// `hunkChangesParser.parse`, for one hunk. Only the line numbers are kept;
/// guff reports no replacement text.
fn hunk_changes(h: &Hunk) -> Vec<Change> {
    let lines = parse_diff_lines(h);
    let mut changes = Vec::new();
    let mut last_original: Option<i64> = None;
    let mut pending_prepend = false; // replacementLinesToPrepend != nil
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.typ == LineType::Original {
            // handleOriginalLine
            i += 1;
            if pending_prepend {
                while i < lines.len() && lines[i].typ == LineType::Added {
                    i += 1;
                }
                changes.push(Change { from: line.original_number, to: line.original_number });
                pending_prepend = false;
            }
            last_original = Some(line.original_number);
            continue;
        }
        let mut deleted: Vec<DiffLine> = Vec::new();
        while i < lines.len() && lines[i].typ == LineType::Deleted {
            deleted.push(lines[i]);
            i += 1;
        }
        let mut added = 0;
        while i < lines.len() && lines[i].typ == LineType::Added {
            added += 1;
            i += 1;
        }
        if let (Some(first), Some(last)) = (deleted.first(), deleted.last()) {
            // handleDeletedLines: either branch consumes the prepend.
            changes.push(Change { from: first.original_number, to: last.original_number });
            if added != 0 || pending_prepend {
                pending_prepend = false;
            }
            continue;
        }
        // handleAddedOnlyLines
        match last_original {
            None => pending_prepend = added != 0 || pending_prepend,
            Some(n) => {
                changes.push(Change { from: n, to: n });
                pending_prepend = false;
            }
        }
    }
    if pending_prepend {
        // "The diff contains only additions: no original or deleted lines".
        return Vec::new();
    }
    changes
}

/// The changes golangci-lint's formatter linters report for `old` → `new`:
/// `diff.Diff`, `diffpkg.ParseMultiFileDiff`, then `hunkChangesParser` per
/// hunk.
pub fn changes(old: &[u8], new: &[u8]) -> Vec<Change> {
    let patch = diff("a.orig", old, "a", new);
    parse_hunks(&patch).iter().flat_map(hunk_changes).collect()
}

#[cfg(test)]
mod tests {
    use super::diff;

    /// 600 random pairs, rich in repeated lines (`}`, blanks) and missing
    /// final newlines, diffed by go-internal v1.16.0 itself
    /// (`tests/testdata/godiff/gen`).
    #[test]
    fn matches_go_internal_byte_for_byte() {
        let raw = include_str!("../tests/testdata/godiff/vectors.json");
        let vectors: Vec<serde_json::Value> = serde_json::from_str(raw).unwrap();
        assert_eq!(vectors.len(), 600);
        for (i, v) in vectors.iter().enumerate() {
            let old = v["old"].as_str().unwrap();
            let new = v["new"].as_str().unwrap();
            let want = v["diff"].as_str().unwrap();
            let got = diff("a.go.orig", old.as_bytes(), "a.go", new.as_bytes());
            assert_eq!(
                String::from_utf8_lossy(&got),
                want,
                "vector {i}\nold:\n{old}\nnew:\n{new}"
            );
        }
    }

    #[test]
    fn equal_inputs_have_no_diff() {
        assert!(diff("a", b"x\n", "b", b"x\n").is_empty());
    }
}
