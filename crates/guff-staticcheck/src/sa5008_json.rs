//! SA5008's JSON struct-tag validator.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5008/jsonv2.go`, which is itself a
//! modified copy of `encoding/json/v2`'s `field.go`. The check reads a `json:`
//! tag the way v2 parses one and reports every way the tag is malformed,
//! misspelled, or inapplicable to the field's type.
//!
//! Two deliberate notes:
//!
//! - The **option grammar is v2's**, not v1's, so options v1 never had (`case`,
//!   `format`, `inline`, `unknown`, `omitzero`) are accepted and the near-misses
//!   v2 rejects (`omitEmpty`, `omit_empty`) are reported. That is upstream's
//!   choice and it is what golangci-lint 2.12.2 ships.
//! - `invalid UTF-8 in JSON object name` is **unreachable here**. Go's
//!   `strconv.Unquote` can return a string holding an invalid byte (`\xff`);
//!   Rust's `String` cannot, and [`crate::gostd::strconv::unquote`] decodes such
//!   an escape to the corresponding `char`. The branch is kept so the shape
//!   matches upstream, and so it starts working if that port ever grows a
//!   byte-oriented variant.

use guff::ast::Field;
use guff_analysis::Pass;
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;
use guff_types::object::is_exported;
use guff_types::TypeId;

use crate::gostd::strconv;

const STRING_OPTION_MSG: &str = "invalid appearance of `string` tag option; it is only \
     intended for fields of numeric types or pointers to those";

fn report(field: &Field, pending: &mut Vec<(u32, String)>, msg: String) {
    let pos = field
        .tag
        .as_ref()
        .map(|t| t.value_pos.0 as u32)
        .unwrap_or(0);
    pending.push((pos, msg));
}

/// `validateJSONTag`.
pub(crate) fn validate_json_tag(
    pass: &Pass<'_>,
    field: &Field,
    tag: &str,
    pending: &mut Vec<(u32, String)>,
) {
    let has_tag = !tag.is_empty();
    let tag_orig = tag;
    let mut tag = tag;

    // Explicitly ignored.
    if tag == "-" {
        return;
    }

    // An unexported, non-embedded field cannot be serialized at all, so a tag
    // on one is user error. (An embedded field of an unexported type can still
    // forward exported fields, so `anonymous` is exempt.)
    let anonymous = field.names.is_empty();
    if !anonymous && !is_exported(&field.names[0].name) {
        if has_tag {
            report(
                field,
                pending,
                format!(
                    "unexported struct field cannot have non-ignored `json:{}` tag",
                    strconv::quote(tag)
                ),
            );
        }
        return;
    }

    if !tag.is_empty() && !tag.starts_with(',') {
        // For better v1 compatibility, accept almost any unescaped name.
        let n = tag.len()
            - tag
                .trim_start_matches(|r: char| !",\\'\"`".contains(r))
                .len();
        let mut name = &tag[..n];
        let mut consumed = n;

        // If the next character is not a comma the name is either malformed
        // (n > 0) or a single-quoted name; either way let consume_tag_option
        // deal with it.
        let owned_name;
        if !tag[n..].starts_with(',') && name.len() != tag.len() {
            match consume_tag_option(tag) {
                Ok((opt, used)) => {
                    owned_name = opt;
                    name = &owned_name;
                    consumed = used;
                }
                Err((opt, used, err)) => {
                    report(field, pending, format!("malformed `json` tag: {err}"));
                    owned_name = opt;
                    name = &owned_name;
                    consumed = used;
                }
            }
        }

        if name == "-" && tag_orig.starts_with('-') {
            let quoted = strconv::quote(tag_orig);
            let suffix = quoted.strip_prefix("\"-").unwrap_or(&quoted);
            report(
                field,
                pending,
                format!(
                    "should encoding/json ignore this field or name it \"-\"? Either use \
                     `json:\"-\"` to ignore the field or use `json:\"'-'{suffix}` to specify {} \
                     as the name",
                    strconv::quote(name)
                ),
            );
        }
        tag = &tag[consumed..];
    }

    // Any additional tag options.
    let mut was_format = false;
    let mut seen_opts: Vec<String> = Vec::new();
    while !tag.is_empty() {
        // Consume the comma delimiter.
        if !tag.starts_with(',') {
            let c = tag.chars().next().unwrap_or('\0');
            report(
                field,
                pending,
                format!(
                    "malformed `json` tag: invalid character {} before next option (expecting ',')",
                    quote_rune(c)
                ),
            );
        } else {
            tag = &tag[1..];
            if tag.is_empty() {
                report(
                    field,
                    pending,
                    "malformed `json` tag: invalid trailing ',' character".to_string(),
                );
                break;
            }
        }

        let (opt, n) = match consume_tag_option(tag) {
            Ok(v) => v,
            Err((opt, n, err)) => {
                report(field, pending, format!("malformed `json` tag: {err}"));
                (opt, n)
            }
        };
        let raw_opt = tag[..n].to_string();
        tag = &tag[n..];

        if was_format {
            report(
                field,
                pending,
                "`format` tag option was not specified last".to_string(),
            );
        } else if raw_opt.starts_with('\'')
            && opt.trim_matches(is_letter_or_digit).is_empty()
        {
            report(
                field,
                pending,
                format!(
                    "unnecessarily quoted appearance of `{raw_opt}` tag option; \
                     specify `{opt}` instead"
                ),
            );
        }

        match opt.as_str() {
            "case" => {
                if !tag.starts_with(':') {
                    report(
                        field,
                        pending,
                        "missing value for `case` tag option; specify `case:ignore` or \
                         `case:strict` instead"
                            .to_string(),
                    );
                } else {
                    tag = &tag[1..];
                    match consume_tag_option(tag) {
                        Err((_, _, err)) => {
                            report(
                                field,
                                pending,
                                format!("malformed value for `case` tag option: {err}"),
                            );
                        }
                        Ok((value, vn)) => {
                            let raw_value = tag[..vn].to_string();
                            tag = &tag[vn..];
                            if raw_value.starts_with('\'') {
                                report(
                                    field,
                                    pending,
                                    format!(
                                        "unnecessarily quoted appearance of `case:{raw_value}` \
                                         tag option; specify `case:{value}` instead"
                                    ),
                                );
                            }
                            if value != "ignore" && value != "strict" {
                                report(
                                    field,
                                    pending,
                                    format!(
                                        "invalid appearance of unknown `case:{raw_value}` tag value"
                                    ),
                                );
                            }
                        }
                    }
                }
            }
            "inline" | "unknown" | "omitzero" | "omitempty" => {}
            "string" => {
                if !string_option_applies(pass, field) {
                    report(field, pending, STRING_OPTION_MSG.to_string());
                }
            }
            "format" => {
                if !tag.starts_with(':') {
                    report(
                        field,
                        pending,
                        "missing value for `format` tag option".to_string(),
                    );
                } else {
                    tag = &tag[1..];
                    match consume_tag_option(tag) {
                        Err((_, _, err)) => {
                            report(
                                field,
                                pending,
                                format!("malformed value for `format` tag option: {err}"),
                            );
                        }
                        Ok((_, vn)) => {
                            tag = &tag[vn..];
                            was_format = true;
                        }
                    }
                }
            }
            other => {
                // Reject keys that resemble a supported option, so mutants like
                // `omitEmpty` or `omit_empty` are named rather than lumped in
                // with genuinely unknown options.
                let norm: String = other.to_lowercase().replace('_', "");
                match norm.as_str() {
                    "case" | "inline" | "unknown" | "omitzero" | "omitempty" | "string"
                    | "format" => {
                        report(
                            field,
                            pending,
                            format!(
                                "invalid appearance of `{other}` tag option; specify `{norm}` \
                                 instead"
                            ),
                        );
                    }
                    _ => {
                        report(
                            field,
                            pending,
                            format!("invalid appearance of unknown `{other}` tag option"),
                        );
                    }
                }
            }
        }

        if seen_opts.iter().any(|s| *s == opt) {
            report(
                field,
                pending,
                format!("duplicate appearance of `{raw_opt}` tag option"),
            );
        }
        seen_opts.push(opt);
    }

    if seen_opts.iter().any(|s| s == "inline") && seen_opts.iter().any(|s| s == "unknown") {
        report(
            field,
            pending,
            "field cannot have both `inline` and `unknown` specified".to_string(),
        );
    }
}

/// The `string` option is only meaningful for numeric fields (and pointers to
/// them); bools and strings are accepted because v1 supported them by accident.
///
/// Upstream walks the field type's *type set*, so a type parameter is judged by
/// its terms. guff answers the concrete case exactly and treats an
/// interface-typed field as upstream treats an unconstrained one — no terms,
/// hence reported.
fn string_option_applies(pass: &Pass<'_>, field: &Field) -> bool {
    let Some(info) = pass.types_info() else {
        return true; // no type information: stay quiet
    };
    let Some(ty) = field.ty.as_ref() else {
        return true;
    };
    let Some(tv) = info.types.get(&ty.id()) else {
        return true;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return true;
    };
    let arena = &artifacts.types;
    let t = dereference(arena, tv.typ.underlying(arena));
    match arena.get(t.underlying(arena)) {
        TypeData::Basic(b) => matches!(
            b.kind(),
            BasicKind::Bool
                | BasicKind::String
                | BasicKind::Int
                | BasicKind::Int8
                | BasicKind::Int16
                | BasicKind::Int32
                | BasicKind::Int64
                | BasicKind::Uint
                | BasicKind::Uint8
                | BasicKind::Uint16
                | BasicKind::Uint32
                | BasicKind::Uint64
                | BasicKind::Uintptr
                | BasicKind::Float32
                | BasicKind::Float64
                | BasicKind::UntypedBool
                | BasicKind::UntypedInt
                | BasicKind::UntypedRune
                | BasicKind::UntypedFloat
                | BasicKind::UntypedString
        ),
        _ => false,
    }
}

/// `typeutil.Dereference`: `*T` becomes `T`, everything else is itself.
fn dereference(arena: &guff_types::arena::TypeArena, t: TypeId) -> TypeId {
    match arena.get(t) {
        TypeData::Pointer(p) => p.elem(),
        _ => t,
    }
}

fn is_letter_or_digit(r: char) -> bool {
    r == '_' || r.is_alphabetic() || r.is_numeric()
}

/// Go's `%q` for a rune.
fn quote_rune(r: char) -> String {
    let quoted = strconv::quote(&r.to_string());
    // `strconv.Quote` gives `"x"`; `%q` on a rune gives `'x'`.
    let inner = &quoted[1..quoted.len() - 1];
    format!("'{}'", inner.replace("\\\"", "\"").replace('\'', "\\'"))
}

/// `consumeTagOption`: the next option, either a Go identifier or a
/// single-quoted string.
///
/// `Err` carries the same `(value, consumed)` upstream returns alongside its
/// error, because upstream *reports and keeps going* with them.
type ConsumeErr = (String, usize, String);

fn consume_tag_option(input: &str) -> Result<(String, usize), ConsumeErr> {
    // For legacy v1 compatibility, options are comma-separated.
    let i = input.find(',').unwrap_or(input.len());

    let Some(r) = input.chars().next() else {
        return Err((input[..i].to_string(), i, "unexpected EOF".to_string()));
    };

    if r == '_' || r.is_alphabetic() {
        let n = input.len() - input.trim_start_matches(is_letter_or_digit).len();
        return Ok((input[..n].to_string(), n));
    }

    if r == '\'' {
        // The grammar matches a double-quoted Go string but with single quotes,
        // because neither backticks nor double quotes survive a struct tag.
        // Convert to a double-quoted string and let `strconv::unquote` finish.
        let mut in_escape = false;
        let mut b = String::from('"');
        let mut n = 1;
        let bytes = input.as_bytes();
        while input.len() > n {
            let rest = &input[n..];
            let ch = rest.chars().next().unwrap();
            let rn = ch.len_utf8();
            if in_escape {
                if ch == '\'' {
                    b.pop(); // `\'` => `'`
                }
                in_escape = false;
            } else if ch == '\\' {
                in_escape = true;
            } else if ch == '"' {
                b.push('\\'); // `"` => `\"`
            } else if ch == '\'' {
                b.push('"');
                n += 1;
                return match strconv::unquote(&b) {
                    Ok(out) => Ok((out, n)),
                    Err(_) => Err((
                        input[..i].to_string(),
                        i,
                        format!("invalid single-quoted string: {}", &input[..n]),
                    )),
                };
            }
            b.push_str(&input[n..n + rn]);
            n += rn;
        }
        let _ = bytes;
        let mut cut = n;
        if cut > 10 {
            cut = 10; // limit the context printed in the error
        }
        return Err((
            input[..i].to_string(),
            i,
            format!("single-quoted string not terminated: {}...", &input[..cut]),
        ));
    }

    Err((
        input[..i].to_string(),
        i,
        format!(
            "invalid character {} at start of option (expecting Unicode letter or single quote)",
            quote_rune(r)
        ),
    ))
}
