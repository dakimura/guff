//! Port of [`github.com/4meepo/tagalign`](https://github.com/4meepo/tagalign)
//! (golangci-lint wrapper in `pkg/golinters/tagalign`).
//!
//! Defaults match golangci-lint: `align=true`, `sort=true`, `strict=false`, empty `order`
//! (alphabetical tag keys).
//!
//! The fix replaces the whole tag literal with the aligned form. Upstream
//! builds it as `fmt.Sprintf("`%s`", unquoteTag)` and puts the same
//! `unquoteTag` in the message (`tagalign.go:298`), so the replacement is the
//! message's tail in backticks — a string golden already validates.
//!
//! DEFERRED: StrictStyle missing-key column padding; the no-op fix upstream
//! emits on a malformed tag (`replaceStr` is `field.Tag.Value`, the tag
//! replaced by itself, which still makes upstream rewrite and gofmt the file —
//! the `extras.go` mechanism of 続き 71). No fixture reaches it.

use std::sync::OnceLock;

/// A finding, and the tag literal's replacement when there is one.
type Pending = Vec<(u32, String, Option<(u32, String)>)>;

use guff::ast::{Field, StructType};
use guff::position::FileSet;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::options::TagalignOptions;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Tag {
    key: String,
    /// Full `key:"value"` string including quotes/options.
    raw: String,
}

fn unquote_tag_lit(value: &str) -> Option<String> {
    let s = value.trim();
    if s.len() < 2 {
        return None;
    }
    let bytes = s.as_bytes();
    if bytes[0] == b'`' && bytes[s.len() - 1] == b'`' {
        return Some(s[1..s.len() - 1].to_string());
    }
    if bytes[0] == b'"' && bytes[s.len() - 1] == b'"' {
        // Best-effort for uncommon double-quoted tags.
        return Some(s[1..s.len() - 1].replace("\\\"", "\""));
    }
    None
}

/// Parse Go struct tag content (already unquoted) into key/value pairs.
fn parse_struct_tags(content: &str) -> Result<Vec<Tag>, String> {
    let mut tags = Vec::new();
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let key_start = i;
        while i < bytes.len() && bytes[i] != b':' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i == key_start || i >= bytes.len() || bytes[i] != b':' {
            return Err("bad syntax for struct tag value".into());
        }
        let key = String::from_utf8_lossy(&bytes[key_start..i]).into_owned();
        i += 1; // ':'
        if i >= bytes.len() || bytes[i] != b'"' {
            return Err("bad syntax for struct tag value".into());
        }
        i += 1; // opening '"'
        let val_start = i;
        while i < bytes.len() && bytes[i] != b'"' {
            if bytes[i] == b'\\' {
                i += 1;
            }
            i += 1;
        }
        if i >= bytes.len() {
            return Err("bad syntax for struct tag value".into());
        }
        let value = String::from_utf8_lossy(&bytes[val_start..i]).into_owned();
        i += 1; // closing '"'
        tags.push(Tag {
            raw: format!("{key}:\"{value}\""),
            key,
        });
    }
    Ok(tags)
}

fn sort_tags(tags: &mut [Tag], order: &[String]) {
    if order.is_empty() {
        tags.sort_by(|a, b| a.key.cmp(&b.key));
        return;
    }
    tags.sort_by(|a, b| {
        let ai = order.iter().position(|k| k == &a.key);
        let bi = order.iter().position(|k| k == &b.key);
        match (ai, bi) {
            (Some(i), Some(j)) => i.cmp(&j),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.key.cmp(&b.key),
        }
    });
}

fn align_format(width: usize, s: &str) -> String {
    format!("{s:<width$}")
}

fn find_consecutive_groups<'a>(
    fset: &FileSet,
    fields: &'a [Field],
) -> (Vec<&'a Field>, Vec<Vec<&'a Field>>) {
    let mut single = Vec::new();
    let mut groups = Vec::new();
    let mut cur: Vec<&Field> = Vec::new();

    for (i, field) in fields.iter().enumerate() {
        if field.tag.is_none() {
            match cur.len() {
                0 => {}
                1 => single.push(cur[0]),
                _ => groups.push(std::mem::take(&mut cur)),
            }
            cur.clear();
            continue;
        }
        if i > 0 {
            if fields[i - 1].tag.is_none() {
                cur.push(field);
                continue;
            }
            let pre = fset.position(fields[i - 1].tag.as_ref().unwrap().pos()).line;
            let cur_line = fset.position(field.tag.as_ref().unwrap().pos()).line;
            if cur_line - pre > 1 {
                match cur.len() {
                    0 => {}
                    1 => single.push(cur[0]),
                    _ => groups.push(std::mem::take(&mut cur)),
                }
                cur.clear();
                if matches!(&field.ty, Some(guff::ast::Expr::StructType(_))) {
                    continue;
                }
            }
        }
        cur.push(field);
    }
    match cur.len() {
        0 => {}
        1 => single.push(cur[0]),
        _ => groups.push(cur),
    }
    (single, groups)
}

fn process_group(fields: &[&Field], options: &TagalignOptions, pending: &mut Pending) {
    let mut tags_group: Vec<Vec<Tag>> = Vec::new();
    let mut not_sorted_group: Vec<Vec<Tag>> = Vec::new();
    let mut kept: Vec<&Field> = Vec::new();

    for field in fields {
        let tag_lit = field.tag.as_ref().unwrap();
        let Some(content) = unquote_tag_lit(&tag_lit.value) else {
            pending.push((
                tag_lit.pos().0 as u32,
                "bad syntax for struct tag value".into(),
                None,
            ));
            continue;
        };
        match parse_struct_tags(&content) {
            Ok(mut tags) => {
                not_sorted_group.push(tags.clone());
                if options.sort {
                    sort_tags(&mut tags, &options.order);
                }
                tags_group.push(tags);
                kept.push(field);
            }
            Err(e) => {
                pending.push((tag_lit.pos().0 as u32, e, None));
            }
        }
    }

    if tags_group.is_empty() {
        return;
    }

    let max_tag_num = tags_group.iter().map(|t| t.len()).max().unwrap_or(0);
    let mut max_lens = vec![0usize; max_tag_num];
    for tags in &tags_group {
        for (j, tag) in tags.iter().enumerate() {
            max_lens[j] = max_lens[j].max(tag.raw.len());
        }
    }

    for (i, field) in kept.iter().enumerate() {
        let tags = &tags_group[i];
        let new_tag = if options.align {
            let mut parts = Vec::new();
            for (j, tag) in tags.iter().enumerate() {
                parts.push(align_format(max_lens[j] + 1, &tag.raw));
            }
            parts.join("").trim_end().to_string()
        } else {
            // Upstream sort-only: if order unchanged, ignore whitespace diffs.
            if options.sort && not_sorted_group[i] == *tags {
                continue;
            }
            tags.iter()
                .map(|t| t.raw.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        };
        let new_value = format!("`{new_tag}`");
        let tag_lit = field.tag.as_ref().unwrap();
        if tag_lit.value == new_value {
            continue;
        }
        pending.push((
            tag_lit.pos().0 as u32,
            format!("tag is not aligned, should be: {new_tag}"),
            Some((tag_lit.end().0 as u32, new_value)),
        ));
    }
}

fn process_single(field: &Field, options: &TagalignOptions, pending: &mut Pending) {
    let tag_lit = field.tag.as_ref().unwrap();
    let Some(content) = unquote_tag_lit(&tag_lit.value) else {
        pending.push((
            tag_lit.pos().0 as u32,
            "bad syntax for struct tag value".into(),
            None,
        ));
        return;
    };
    let Ok(mut tags) = parse_struct_tags(&content) else {
        pending.push((
            tag_lit.pos().0 as u32,
            "bad syntax for struct tag value".into(),
            None,
        ));
        return;
    };
    let original: Vec<_> = tags.iter().map(|t| t.raw.clone()).collect();
    if options.sort {
        sort_tags(&mut tags, &options.order);
    }
    let joined = tags
        .iter()
        .map(|t| t.raw.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let new_value = format!("`{joined}`");
    let same_order = original
        .iter()
        .zip(tags.iter())
        .all(|(a, b)| a == &b.raw)
        && original.len() == tags.len();
    if same_order && tag_lit.value == new_value {
        return;
    }
    pending.push((
        tag_lit.pos().0 as u32,
        // The space before the comma is upstream's, not a slip here
        // (`tagalign.go:336`), and golden pins both spellings.
        format!("tag is not aligned , should be: {joined}"),
        Some((tag_lit.end().0 as u32, new_value)),
    ));
}

fn check_struct(
    fset: &FileSet,
    st: &StructType,
    options: &TagalignOptions,
    pending: &mut Pending,
) {
    if st.fields.list.is_empty() {
        return;
    }
    let (singles, groups) = find_consecutive_groups(fset, &st.fields.list);
    for g in groups {
        process_group(&g, options, pending);
    }
    for f in singles {
        process_single(f, options, pending);
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "tagalign requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<TagalignOptions>("tagalign")
        .cloned()
        .unwrap_or_default();

    if !options.align && !options.sort {
        return Ok(None);
    }

    let mut pending: Pending = Vec::new();
    let fset = pass.fset().clone();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::StructType(st) = n {
                check_struct(&fset, st, &options, &mut pending);
            }
            true
        });
    }

    for (pos, message, replacement) in pending {
        // `field.Tag.Pos()..field.Tag.End()` — the whole literal, backticks
        // included, which is why the replacement carries its own.
        let Some((end, new_value)) = replacement else {
            pass.reportf(pos, message);
            continue;
        };
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: String::new(),
                text_edits: vec![TextEdit {
                    pos,
                    end,
                    new_text: new_value,
                }],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "tagalign",
        doc: "check that struct tags are well aligned",
        url: "https://github.com/4meepo/tagalign",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
