//! `printf` — check Printf-like format strings against their arguments.
//!
//! Port of the core of `golang.org/x/tools/go/analysis/passes/printf`. Covers:
//! - unknown verbs and `%w` outside `Errorf`,
//! - argument count (too few / too many), honouring `*` width/precision and
//!   explicit `%[n]` argument indexes,
//! - argument type matching (`%d` wants an integer, `%s` a string, …),
//!   recursing into slices/arrays/maps/pointers and accepting types that
//!   implement `fmt.Formatter` / `fmt.Stringer` / `error`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{call_name, expr_to_bytes};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::{basic_kind, BasicKind};
use guff_types::predicates::{is_boolean, is_complex, is_float, is_integer, is_string, is_valid};
use guff_types::signature::signature_params;
use guff_types::tuple::tuple_len;
use guff_types::TypeId;
use guff_types::api_predicates::api_implements;
use guff_types::alias::unalias_readonly;

use crate::govet_util::expr_type;

const KNOWN_PRINTF: &[&str] = &[
    "fmt.Errorf",
    "fmt.Fprintf",
    "fmt.Printf",
    "fmt.Sprintf",
    "log.Printf",
    "log.Fatalf",
    "log.Panicf",
];

// Argument-type categories (a bitmask). `rune` is folded into `INT`.
const B_BOOL: u32 = 1 << 0;
const B_INT: u32 = 1 << 1;
const B_STRING: u32 = 1 << 2;
const B_FLOAT: u32 = 1 << 3;
const B_COMPLEX: u32 = 1 << 4;
const B_POINTER: u32 = 1 << 5;
const B_SLICE: u32 = 1 << 6;
const B_ERROR: u32 = 1 << 7;
const B_ANY: u32 = u32::MAX;

/// Allowed argument categories for a verb, or `None` if the verb is unknown.
fn verb_arg_type(verb: char) -> Option<u32> {
    Some(match verb {
        'b' => B_INT | B_FLOAT | B_COMPLEX | B_POINTER,
        'c' => B_INT,
        'd' => B_INT | B_POINTER,
        'e' | 'E' | 'f' | 'F' | 'g' | 'G' => B_FLOAT | B_COMPLEX,
        'o' | 'O' => B_INT | B_POINTER,
        'p' => B_POINTER,
        'q' => B_INT | B_STRING,
        's' => B_STRING,
        't' => B_BOOL,
        'T' => B_ANY,
        'U' => B_INT,
        'v' => B_ANY,
        'w' => B_ERROR,
        'x' | 'X' => {
            B_INT | B_STRING | B_FLOAT | B_COMPLEX | B_POINTER | B_SLICE
        }
        _ => return None,
    })
}

fn is_string_ish(verb: char) -> bool {
    matches!(verb, 's' | 'q' | 'v' | 'x' | 'X')
}

fn printf_kind(pass: &Pass<'_>, fun: &Expr) -> Option<()> {
    let name = call_name(pass, fun)?;
    if KNOWN_PRINTF.iter().any(|k| *k == name) {
        return Some(());
    }
    let short = name.rsplit('.').next()?;
    if matches!(
        short,
        "Printf" | "Sprintf" | "Fprintf" | "Errorf" | "Fatalf" | "Panicf"
    ) {
        return Some(());
    }
    None
}

/// Index into `call.args` of the format-string argument.
///
/// Determined from the callee signature: the format string is the parameter
/// immediately before the variadic `...` parameter. Falls back to a name-based
/// guess (`Fprintf` has a leading writer) when the signature is unavailable.
fn format_index(pass: &Pass<'_>, call: &CallExpr) -> usize {
    if let (Some(sig), Some(artifacts)) =
        (expr_type(pass, &call.fun), pass.pkg().type_artifacts.as_ref())
    {
        let u = sig.underlying(&artifacts.types);
        if let TypeData::Signature(s) = artifacts.types.get(u) {
            if s.variadic() {
                let params = signature_params(&artifacts.types, u);
                let n = tuple_len(&artifacts.types, params);
                if n >= 2 {
                    return n - 2;
                }
            }
        }
    }
    // Fallback: only Fprintf (writer, format, ...) has a leading argument.
    match call_name(pass, &call.fun).as_deref().and_then(|n| n.rsplit('.').next().map(str::to_string)) {
        Some(ref s) if s == "Fprintf" => 1,
        _ => 0,
    }
}

/// A single `%…verb` directive parsed out of a format string.
struct Directive {
    verb: char,
    /// Explicit 1-based operand index from `%[n]`, if any.
    index: Option<usize>,
    /// Number of `*` width/precision placeholders (each consumes one operand).
    stars: usize,
    /// Rendered directive text, e.g. `%d` or `%[2]*.3f`, for messages.
    text: String,
}

/// Outcome of scanning one `%` sequence.
enum Scan {
    /// A verb directive.
    Directive(Directive),
    /// `%%` — no operand.
    Literal,
    /// A malformed directive with an error message (verb, message).
    Error(String),
}

/// Outcome of one `parseIndex` attempt.
enum IndexScan {
    /// No `[` here — upstream's `parseIndex` returns nil without consuming.
    Absent,
    /// `[n]` consumed: the index, the text to append, and the new offset.
    Found(usize, String, usize),
    /// Malformed: message and the offset to resume reporting from.
    Bad(String, usize),
}

/// `fmtstr.state.parseIndex` — an explicit `[n]` argument index.
///
/// Upstream calls this at **three** points in one directive: after the flags,
/// again after a `.` inside `parsePrecision`, and once more just before the
/// verb when no index is still pending. `%-36[1]s` (cobra) only has one at the
/// third position, so parsing the index solely after the flags leaves `[` to be
/// read as the verb.
fn parse_arg_index(format: &[u8], i: usize) -> IndexScan {
    if format.get(i) != Some(&b'[') {
        return IndexScan::Absent;
    }
    let open = i;
    let Some(close) = format[i..].iter().position(|&b| b == b']').map(|off| i + off) else {
        return IndexScan::Bad("format has invalid argument index".into(), i + 1);
    };
    let num = std::str::from_utf8(&format[open + 1..close])
        .ok()
        .and_then(|n| n.parse::<usize>().ok());
    match num {
        Some(n) if n >= 1 => IndexScan::Found(
            n,
            guff_constant::decode_lossy(&format[open..=close]),
            close + 1,
        ),
        _ => IndexScan::Bad("format has invalid argument index".into(), close + 1),
    }
}

/// Parse the directive that starts at `chars` (just after a `%`).
///
/// The format is bytes, as it is in Go: upstream compares `s.format[s.i]`
/// byte-wise for every flag, index and digit, and decodes a rune only for the
/// verb itself.
fn scan_directive(format: &[u8], start: usize) -> (Scan, usize) {
    let bytes = format;
    let mut i = start; // index just after '%'
    let mut text = String::from("%");
    let mut index: Option<usize> = None;
    let mut stars = 0usize;

    // ASCII-only view, for the parts upstream reads as single bytes. A
    // non-ASCII byte here never matches any of them.
    let at = |i: usize| -> Option<char> { bytes.get(i).filter(|b| b.is_ascii()).map(|b| *b as char) };

    // %%
    if at(i) == Some('%') {
        return (Scan::Literal, i + 1);
    }

    // Flags.
    while let Some(c) = at(i) {
        if matches!(c, '#' | '0' | '+' | '-' | ' ') {
            text.push(c);
            i += 1;
        } else {
            break;
        }
    }

    // `indexPending` upstream: an index that no `*` has absorbed yet, which
    // therefore belongs to the verb. It is what stops the pre-verb
    // `parseIndex` from running a second time.
    let mut index_pending = false;

    // Explicit argument index: %[n], first of three positions.
    match parse_arg_index(format, i) {
        IndexScan::Absent => {}
        IndexScan::Found(n, t, next) => {
            index = Some(n);
            index_pending = true;
            text.push_str(&t);
            i = next;
        }
        IndexScan::Bad(msg, next) => return (Scan::Error(msg), next),
    }

    // Width: digits or '*'.
    if at(i) == Some('*') {
        stars += 1;
        // `parseSize` absorbs a pending index into the `*` operand.
        index_pending = false;
        text.push('*');
        i += 1;
    } else {
        while at(i).is_some_and(|c| c.is_ascii_digit()) {
            text.push(at(i).unwrap());
            i += 1;
        }
    }

    // Precision: '.' then an optional index, then digits or '*'.
    if at(i) == Some('.') {
        text.push('.');
        i += 1;
        match parse_arg_index(format, i) {
            IndexScan::Absent => {}
            IndexScan::Found(n, t, next) => {
                index = Some(n);
                index_pending = true;
                text.push_str(&t);
                i = next;
            }
            IndexScan::Bad(msg, next) => return (Scan::Error(msg), next),
        }
        if at(i) == Some('*') {
            stars += 1;
            index_pending = false;
            text.push('*');
            i += 1;
        } else {
            while at(i).is_some_and(|c| c.is_ascii_digit()) {
                text.push(at(i).unwrap());
                i += 1;
            }
        }
    }

    // "Now a verb, possibly prefixed by an index (which we may already have)."
    if !index_pending {
        match parse_arg_index(format, i) {
            IndexScan::Absent => {}
            IndexScan::Found(n, t, next) => {
                index = Some(n);
                text.push_str(&t);
                i = next;
            }
            IndexScan::Bad(msg, next) => return (Scan::Error(msg), next),
        }
    }

    // The verb — `verb, w := utf8.DecodeRuneInString(s.format[s.i:])`. This is
    // the one place upstream decodes a rune rather than reading a byte, so
    // `%é` is one unknown verb and not the first byte of one.
    if i >= bytes.len() {
        return (Scan::Error("format string ends with %".into()), i);
    }
    let (decoded, width) = guff_constant::utf8::decode_rune(&bytes[i..]);
    let verb = decoded.unwrap_or(char::REPLACEMENT_CHARACTER);
    text.push(verb);
    i += width;

    (
        Scan::Directive(Directive {
            verb,
            index,
            stars,
            text,
        }),
        i,
    )
}

/// Whether the type has a directly-declared method with the given name.
///
/// Enough to recognise `fmt.Formatter` (`Format`), `fmt.Stringer` (`String`)
/// and `error` (`Error`) for the common case; unwraps a single pointer. This
/// deliberately errs toward accepting (avoiding false positives).
///
/// Aliases are unwrapped on both sides of the pointer: a method set is a
/// property of the aliased type, so `os.FileMode` (= `io/fs.FileMode`) has to
/// find `String` just as `fs.FileMode` does. Missing this reported
/// `%s has arg mode of wrong type os.FileMode`.
fn type_has_method(pass: &Pass<'_>, typ: TypeId, name: &str) -> bool {
    let Some(art) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut t = unalias_readonly(&art.types, typ);
    if let TypeData::Pointer(p) = art.types.get(t) {
        t = unalias_readonly(&art.types, p.elem());
    }
    if let TypeData::Named(n) = art.types.get(t) {
        for i in 0..n.num_methods() {
            if n.method(i).name(&art.objects) == name {
                return true;
            }
        }
    }
    false
}

/// Whether the struct type (given by its underlying id) has an embedded field.
/// Embedded fields can promote `String`/`Error`/`Format` methods that
/// `type_has_method` does not see, so such structs are accepted conservatively.
fn struct_has_embedded(pass: &Pass<'_>, underlying: TypeId) -> bool {
    let Some(art) = pass.pkg().type_artifacts.as_ref() else {
        return true;
    };
    let n = guff_types::r#struct::struct_num_fields(&art.types, underlying);
    for i in 0..n {
        let f = guff_types::r#struct::struct_field(&art.types, underlying, i);
        if let ObjectData::Var(v) = art.objects.get(f) {
            if v.embedded() {
                return true;
            }
        }
    }
    false
}

fn basic_bits(pass: &Pass<'_>, typ: TypeId) -> u32 {
    let art = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return 0,
    };
    if is_boolean(&art.types, typ) {
        B_BOOL
    } else if is_integer(&art.types, typ) {
        B_INT
    } else if is_float(&art.types, typ) {
        B_FLOAT
    } else if is_complex(&art.types, typ) {
        B_COMPLEX
    } else if is_string(&art.types, typ) {
        B_STRING
    } else {
        0
    }
}

/// Whether an argument of type `typ` is acceptable for `verb` (bitmask `bits`).
fn match_arg_type(pass: &Pass<'_>, verb: char, bits: u32, typ: TypeId, depth: u32) -> bool {
    if depth > 8 {
        return true;
    }
    let Some(art) = pass.pkg().type_artifacts.as_ref() else {
        return true;
    };
    if !is_valid(&art.types, typ) {
        return true;
    }
    // A Formatter takes over all formatting, so any verb is fine.
    if type_has_method(pass, typ, "Format") {
        return true;
    }
    // String-like verbs accept Stringer / error.
    if is_string_ish(verb)
        && (type_has_method(pass, typ, "String") || type_has_method(pass, typ, "Error"))
    {
        return true;
    }
    // `%w` requires a type that implements `error` (e.g. `*os.LinkError`).
    if bits & B_ERROR != 0 && implements_error(pass, typ) {
        return true;
    }
    if bits == B_ANY {
        return true;
    }

    let u = typ.underlying(&art.types);
    match art.types.get(u) {
        TypeData::Basic(b) => {
            if b.kind() == BasicKind::Invalid {
                return true;
            }
            let tb = basic_bits(pass, typ);
            if tb == 0 {
                // e.g. uintptr / unsafe.Pointer — accept rather than misreport.
                return true;
            }
            bits & tb != 0
        }
        TypeData::Pointer(p) => {
            if bits & B_POINTER != 0 {
                return true;
            }
            let elem = p.elem();
            let eu = elem.underlying(&art.types);
            if matches!(
                art.types.get(eu),
                TypeData::Struct(_) | TypeData::Array(_) | TypeData::Slice(_) | TypeData::Map(_)
            ) {
                match_arg_type(pass, verb, bits, elem, depth + 1)
            } else {
                false
            }
        }
        TypeData::Slice(s) => {
            let elem = s.elem();
            // []byte / []rune print like strings for the string-ish verbs.
            if is_string_ish(verb)
                && matches!(
                    basic_kind(&art.types, elem.underlying(&art.types)),
                    BasicKind::Uint8 | BasicKind::Int32
                )
            {
                return true;
            }
            if bits & (B_SLICE | B_POINTER) != 0 {
                return true;
            }
            match_arg_type(pass, verb, bits, elem, depth + 1)
        }
        TypeData::Array(a) => match_arg_type(pass, verb, bits, a.elem(), depth + 1),
        TypeData::Map(m) => {
            if bits & B_POINTER != 0 {
                return true;
            }
            match_arg_type(pass, verb, bits, m.key(), depth + 1)
                && match_arg_type(pass, verb, bits, m.elem(), depth + 1)
        }
        TypeData::Chan(_) | TypeData::Signature(_) => bits & B_POINTER != 0,
        TypeData::Interface(_) | TypeData::TypeParam(_) | TypeData::Union(_) => true,
        TypeData::Struct(_) => {
            // Match x/tools printf: recurse into every field. Also accept
            // structs with embedded fields whose promoted String/Error/Format
            // methods `type_has_method` may miss.
            if struct_has_embedded(pass, u) {
                return true;
            }
            let n = guff_types::r#struct::struct_num_fields(&art.types, u);
            if n == 0 {
                return true;
            }
            for i in 0..n {
                let f = guff_types::r#struct::struct_field(&art.types, u, i);
                let ObjectData::Var(v) = art.objects.get(f) else {
                    continue;
                };
                if !match_arg_type(pass, verb, bits, v.typ(), depth + 1) {
                    return false;
                }
            }
            true
        }
        _ => true,
    }
}

fn universe_error(pass: &Pass<'_>) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    for oid in artifacts.objects.ids() {
        let ObjectData::TypeName(tn) = artifacts.objects.get(oid) else {
            continue;
        };
        if tn.name() != "error" {
            continue;
        }
        if oid.pkg(&artifacts.objects).is_some() {
            continue;
        }
        return tn.typ();
    }
    None
}

fn implements_error(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    if type_has_method(pass, typ, "Error") {
        return true;
    }
    let Some(err) = universe_error(pass) else {
        // Export data may omit methods; accept named / *named for `%w` rather
        // than false-positive on concrete error types like `*os.LinkError`.
        let t = unalias_readonly(&artifacts.types, typ);
        return match artifacts.types.get(t) {
            TypeData::Interface(_) => true,
            TypeData::Named(_) => true,
            TypeData::Pointer(p) => {
                matches!(
                    artifacts.types.get(unalias_readonly(&artifacts.types, p.elem())),
                    TypeData::Named(_)
                )
            }
            _ => false,
        };
    };
    let mut types = artifacts.types.clone();
    api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        err,
    )
}

/// Best-effort source rendering of an argument for diagnostics.
/// Upstream renders the argument with `analysisutil.Format`, i.e. `go/printer`
/// over the real fileset — `has arg Farewell() of wrong type string`. Matching
/// only literals and identifiers and calling everything else "arg" turned every
/// call, selector, index and conversion into the same three letters.
fn describe_arg(pass: &Pass<'_>, arg: &Expr) -> String {
    let mut buf: Vec<u8> = Vec::new();
    if guff::printer::fprint(&mut buf, pass.fset(), guff::printer::PrintNode::Expr(arg)).is_ok() {
        if let Ok(text) = String::from_utf8(buf) {
            return text;
        }
    }
    match arg {
        Expr::BasicLit(lit) => lit.value.clone(),
        Expr::Ident(id) => id.name.clone(),
        _ => "arg".to_string(),
    }
}

fn type_name(pass: &Pass<'_>, typ: TypeId) -> String {
    let Some(art) = pass.pkg().type_artifacts.as_ref() else {
        return "?".into();
    };
    guff_types::typestring::type_string(&art.types, &art.objects, &art.packages, typ, None)
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "printf requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if printf_kind(pass, &call.fun).is_none() {
            return;
        }
        let name = call_name(pass, &call.fun).unwrap_or_else(|| "Printf".into());
        let is_errorf = name.ends_with("Errorf");
        let fmt_idx = format_index(pass, call);
        let Some(format_arg) = call.args.get(fmt_idx) else {
            return;
        };
        let Some(format) = expr_to_bytes(pass, format_arg) else {
            return; // non-constant format string: skip (no false positives)
        };

        check_one(pass, call, &name, is_errorf, fmt_idx, &format, &mut pending);
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

/// `astutil.PosInStringLiteral`: the source position within a string literal
/// that corresponds to `offset` in the *decoded* string it denotes.
///
/// printf reports at the `%v` substring, not at the call, so every diagnostic
/// that names a directive has to walk the raw literal and account for escape
/// sequences (`"\\t%d"` puts `%` at raw offset 3 but decoded offset 1).
fn pos_in_string_literal(lit: &guff::ast::BasicLit, offset: usize) -> Option<u32> {
    let raw = lit.value.as_bytes();
    if raw.len() < 2 {
        return None;
    }
    let quote = raw[0];
    if quote != b'"' && quote != b'`' {
        return None;
    }
    let body = &raw[1..raw.len() - 1];
    // Raw strings have no escapes, so decoded offset == raw offset.
    if quote == b'`' {
        return (offset <= body.len()).then(|| lit.value_pos.0 as u32 + 1 + offset as u32);
    }

    let mut raw_i = 0usize;
    let mut dec_i = 0usize;
    while dec_i < offset && raw_i < body.len() {
        let (raw_len, dec_len) = escape_lengths(&body[raw_i..])?;
        raw_i += raw_len;
        dec_i += dec_len;
    }
    // A directive never starts mid-rune, so landing past the target means the
    // literal did not decode the way the caller believes.
    (dec_i == offset).then(|| lit.value_pos.0 as u32 + 1 + raw_i as u32)
}

/// `(raw bytes consumed, decoded bytes produced)` for the character at the
/// start of `b` inside a double-quoted Go string literal.
///
/// The decoded lengths here must agree with what upstream's own walk counts,
/// since the offset being mapped is compared against it. That is *not* always
/// the true byte length: `walkStringLiteral` advances by
/// `utf8.RuneLen(r)` and drops the `multibyte` flag `strconv.UnquoteChar`
/// returns alongside `r`, so a `\xff` — one byte in the string — counts as the
/// two bytes U+00FF would occupy. Matching golangci-lint means reproducing
/// that, and a directive after an `\x80`-or-above escape is reported one
/// column early by both tools.
///
/// `printf/escapes.go` in the golden case is what holds this in step with the
/// decoder — it is how the decoder's own escape bug was found.
fn escape_lengths(b: &[u8]) -> Option<(usize, usize)> {
    if b[0] != b'\\' {
        // A UTF-8 rune occupies the same number of bytes either way.
        let n = match b[0] {
            0x00..=0x7f => 1,
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf7 => 4,
            _ => return None,
        };
        return (n <= b.len()).then_some((n, n));
    }
    let c = *b.get(1)?;
    Some(match c {
        b'a' | b'b' | b'f' | b'n' | b'r' | b't' | b'v' | b'\\' | b'\'' | b'"' => (2, 1),
        // A byte escape, counted as upstream counts it: `utf8.RuneLen` of the
        // code point with that number, so 1 below 0x80 and 2 at or above it.
        b'x' => (4, rune_len_of_byte_escape(&b[2..4])?),
        b'0'..=b'7' => (4, rune_len_of_octal_escape(&b[1..4])?),
        // \u and \U decode to a rune, whose UTF-8 length is what the decoded
        // string actually holds.
        b'u' | b'U' => {
            let (n, digits) = if c == b'u' { (6, 4) } else { (10, 8) };
            if b.len() < n {
                return None;
            }
            let hex = std::str::from_utf8(&b[2..2 + digits]).ok()?;
            let cp = u32::from_str_radix(hex, 16).ok()?;
            (n, char::from_u32(cp)?.len_utf8())
        }
        _ => return None,
    })
}

/// `utf8.RuneLen` of the byte `\xHH` names — 1 below 0x80, 2 above.
fn rune_len_of_byte_escape(digits: &[u8]) -> Option<usize> {
    let hex = std::str::from_utf8(digits.get(..2)?).ok()?;
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some(if v < 0x80 { 1 } else { 2 })
}

/// `utf8.RuneLen` of the byte `\OOO` names.
fn rune_len_of_octal_escape(digits: &[u8]) -> Option<usize> {
    let oct = std::str::from_utf8(digits.get(..3)?).ok()?;
    let v = u32::from_str_radix(oct, 8).ok()?;
    Some(if v < 0x80 { 1 } else { 2 })
}

/// `opRange`: the position of a directive within the format string, falling
/// back to the whole format argument when it is not a literal (a named
/// constant, say) or when the offset cannot be mapped.
fn op_pos(format_arg: &Expr, offset: usize) -> u32 {
    if let Expr::BasicLit(lit) = format_arg {
        if let Some(pos) = pos_in_string_literal(lit, offset) {
            return pos;
        }
    }
    format_arg.pos().0 as u32
}

fn check_one(
    pass: &Pass<'_>,
    call: &CallExpr,
    name: &str,
    is_errorf: bool,
    fmt_idx: usize,
    format: &[u8],
    out: &mut Vec<(u32, String)>,
) {
    // Upstream reports the leftover-argument case with ReportRangef(call, ...),
    // i.e. at the callee. Everything else is reported at its directive.
    let call_pos = call.fun.pos().0 as u32;
    let format_arg = &call.args[fmt_idx];
    let first_arg = fmt_idx + 1;
    let nargs = call.args.len();
    // `f(format, args...)` — the operands are whatever the slice holds, so the
    // last argument stands in for an unknown number of them. Upstream's
    // `argCanBeChecked` bails out silently on the final argument of such a
    // call, and skips the leftover-argument check as well.
    let ellipsis = call.ellipsis.is_valid();

    let mut arg_num = first_arg;
    let mut max_arg_num = first_arg;
    let mut any_index = false;

    let bytes = format;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        let op_start = i;
        let (scan, next) = scan_directive(format, i + 1);
        i = next;
        // Position of the "%v" substring inside the literal.
        let pos = op_pos(format_arg, op_start);
        let dir = match scan {
            Scan::Literal => continue,
            Scan::Error(msg) => {
                // ReportRangef(formatArg, "%s %s", name, err).
                out.push((format_arg.pos().0 as u32, format!("{name} {msg}")));
                continue;
            }
            Scan::Directive(d) => d,
        };

        // Explicit index resets the operand cursor.
        if let Some(idx) = dir.index {
            any_index = true;
            arg_num = first_arg + idx - 1;
        }

        // `*` width/precision each consume an integer operand.
        for _ in 0..dir.stars {
            if ellipsis && arg_num + 1 >= nargs {
                return;
            }
            if arg_num >= nargs {
                out.push((
                    pos,
                    format!(
                        "{name} format {} reads arg #{}, but call has {} arg{}",
                        dir.text,
                        arg_num - first_arg + 1,
                        nargs - first_arg,
                        plural(nargs - first_arg)
                    ),
                ));
            } else {
                arg_num += 1;
                max_arg_num = max_arg_num.max(arg_num);
            }
        }

        if dir.verb == '%' {
            continue;
        }

        // Every non-`%` verb consumes exactly one operand, even an unknown one
        // (matching go vet), so counts stay consistent.
        if ellipsis && arg_num + 1 >= nargs {
            return;
        }
        if arg_num >= nargs {
            out.push((
                pos,
                format!(
                    "{name} format {} reads arg #{}, but call has {} arg{}",
                    dir.text,
                    arg_num - first_arg + 1,
                    nargs - first_arg,
                    plural(nargs - first_arg)
                ),
            ));
            continue;
        }
        let arg = &call.args[arg_num];
        arg_num += 1;
        max_arg_num = max_arg_num.max(arg_num);

        let Some(bits) = verb_arg_type(dir.verb) else {
            out.push((
                pos,
                format!("{name} format {} has unknown verb {}", dir.text, dir.verb),
            ));
            continue;
        };
        if dir.verb == 'w' && !is_errorf {
            out.push((
                pos,
                format!("{name} does not support error-wrapping directive %w"),
            ));
            continue;
        }

        if let Some(typ) = expr_type(pass, arg) {
            if !match_arg_type(pass, dir.verb, bits, typ, 0) {
                out.push((
                    pos,
                    format!(
                        "{name} format {} has arg {} of wrong type {}",
                        dir.text,
                        describe_arg(pass, arg),
                        type_name(pass, typ)
                    ),
                ));
            }
        }
    }

    // Dotdotdot is hard: the trailing slice may supply the missing operands.
    if ellipsis && max_arg_num + 1 >= nargs {
        return;
    }
    // Too many arguments (only meaningful without explicit indexes).
    if !any_index && max_arg_num < nargs {
        let expect = max_arg_num - first_arg;
        let got = nargs - first_arg;
        out.push((
            call_pos,
            format!(
                "{name} call needs {expect} arg{} but has {got} arg{}",
                plural(expect),
                plural(got)
            ),
        ));
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "printf",
        doc: "check Printf format strings",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/printf",
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
    fn scans_flags_width_precision() {
        let (scan, next) = scan_directive(b"%-+#0 12.34d", 1);
        assert_eq!(next, "%-+#0 12.34d".len());
        match scan {
            Scan::Directive(d) => {
                assert_eq!(d.verb, 'd');
                assert_eq!(d.stars, 0);
            }
            _ => panic!("expected directive"),
        }
    }

    #[test]
    fn scans_stars_and_index() {
        let (scan, _) = scan_directive(b"%[2]*.*d", 1);
        match scan {
            Scan::Directive(d) => {
                assert_eq!(d.verb, 'd');
                assert_eq!(d.index, Some(2));
                assert_eq!(d.stars, 2);
            }
            _ => panic!("expected directive"),
        }
    }

    #[test]
    fn literal_percent() {
        assert!(matches!(scan_directive(b"%%", 1).0, Scan::Literal));
    }

    #[test]
    fn trailing_percent_is_error() {
        assert!(matches!(scan_directive(b"%", 1).0, Scan::Error(_)));
    }
}
