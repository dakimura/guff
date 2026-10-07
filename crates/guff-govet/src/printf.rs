//! `printf` — check Printf-like format strings against their arguments.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/printf` (v0.50):
//! `checkPrintf` / `okPrintfArg` for formatted calls — verbs, flags, operand
//! counts with `*` and `%[n]`, operand types ([`crate::printf_types`]), func
//! values and recursive `String` / `Error` calls, and the go1.24 non-constant
//! format string with its fix — and `checkPrint` for unformatted ones.
//! Which functions are print-like is [`crate::printf_wrappers`].
//!
//! Not ported: the go1.27 rules (`%d` of a pointer, `%w` of a pointer to an
//! error type; PR 15), and wrappers declared in other packages (facts).

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{self, call_name, expr_to_bytes};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::arena::{ObjectData, TypeData};
use guff_types::signature::signature_params;
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::alias::unalias_readonly;

use crate::govet_util::expr_type;
use crate::printf_types::{
    self, Scratch, ANY_TYPE, ARG_BOOL, ARG_BYTE, ARG_COMPLEX, ARG_ERROR, ARG_FLOAT, ARG_INT,
    ARG_POINTER, ARG_RUNE, ARG_STRING,
};
use crate::printf_wrappers;

/// One entry of upstream's `printVerbs`: the flags a verb knows and the
/// operand types it accepts.
struct PrintVerb {
    verb: char,
    flags: &'static str,
    typ: u32,
}

// Common flag sets for printf verbs.
const NO_FLAG: &str = "";
const NUM_FLAG: &str = " -+.0";
const SHARP_NUM_FLAG: &str = " -+.0#";
const ALL_FLAGS: &str = " -+.0#";

/// `printVerbs`, in upstream's order — the order matters: an unknown verb
/// leaves upstream's loop variable on the last entry (`X`).
const PRINT_VERBS: &[PrintVerb] = &[
    PrintVerb { verb: '%', flags: NO_FLAG, typ: 0 },
    PrintVerb { verb: 'b', flags: SHARP_NUM_FLAG, typ: ARG_INT | ARG_FLOAT | ARG_COMPLEX | ARG_POINTER },
    PrintVerb { verb: 'c', flags: "-", typ: ARG_RUNE | ARG_INT },
    // When analyzing go1.27+ code argPointer is disallowed (PR 15).
    PrintVerb { verb: 'd', flags: NUM_FLAG, typ: ARG_INT | ARG_POINTER },
    PrintVerb { verb: 'e', flags: SHARP_NUM_FLAG, typ: ARG_FLOAT | ARG_COMPLEX },
    PrintVerb { verb: 'E', flags: SHARP_NUM_FLAG, typ: ARG_FLOAT | ARG_COMPLEX },
    PrintVerb { verb: 'f', flags: SHARP_NUM_FLAG, typ: ARG_FLOAT | ARG_COMPLEX },
    PrintVerb { verb: 'F', flags: SHARP_NUM_FLAG, typ: ARG_FLOAT | ARG_COMPLEX },
    PrintVerb { verb: 'g', flags: SHARP_NUM_FLAG, typ: ARG_FLOAT | ARG_COMPLEX },
    PrintVerb { verb: 'G', flags: SHARP_NUM_FLAG, typ: ARG_FLOAT | ARG_COMPLEX },
    PrintVerb { verb: 'o', flags: SHARP_NUM_FLAG, typ: ARG_INT | ARG_POINTER },
    PrintVerb { verb: 'O', flags: SHARP_NUM_FLAG, typ: ARG_INT | ARG_POINTER },
    PrintVerb { verb: 'p', flags: "-#", typ: ARG_POINTER },
    // When analyzing go1.26 code, argInt => argByte (see `ok_printf_arg`).
    PrintVerb { verb: 'q', flags: " -+.0#", typ: ARG_RUNE | ARG_INT | ARG_STRING },
    PrintVerb { verb: 's', flags: " -+.0", typ: ARG_STRING },
    PrintVerb { verb: 't', flags: "-", typ: ARG_BOOL },
    PrintVerb { verb: 'T', flags: "-", typ: ANY_TYPE },
    PrintVerb { verb: 'U', flags: "-#", typ: ARG_RUNE | ARG_INT },
    PrintVerb { verb: 'v', flags: ALL_FLAGS, typ: ANY_TYPE },
    PrintVerb { verb: 'w', flags: ALL_FLAGS, typ: ARG_ERROR },
    PrintVerb {
        verb: 'x',
        flags: SHARP_NUM_FLAG,
        typ: ARG_RUNE | ARG_INT | ARG_STRING | ARG_POINTER | ARG_FLOAT | ARG_COMPLEX,
    },
    PrintVerb {
        verb: 'X',
        flags: SHARP_NUM_FLAG,
        typ: ARG_RUNE | ARG_INT | ARG_STRING | ARG_POINTER | ARG_FLOAT | ARG_COMPLEX,
    },
];

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
///
/// An index binds to whichever position absorbs it, not to the directive as a
/// whole: in `%[2]*[1]s` the width `*` takes argument 2 and the verb takes
/// argument 1. Recording one index per directive read that as "argument 1,
/// then the next one" and reported rclone's
/// `fmt.Sprintf("%[2]*[1]s", str, rawWidth)` as an `int` printed with `%s`.
struct Directive {
    verb: char,
    /// The flags as written, a subset of `#0+- ` (`fmtstr`'s `Flags`).
    flags: String,
    /// `[n]` absorbed by the width `*`, e.g. `2` in `%[2]*d`.
    width_index: Option<usize>,
    has_width_star: bool,
    /// `[n]` absorbed by the precision `*`, e.g. `2` in `%.[2]*d`.
    prec_index: Option<usize>,
    has_prec_star: bool,
    /// `[n]` left for the verb — either written just before it, or written
    /// earlier and never absorbed by a `*`.
    verb_index: Option<usize>,
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
///
/// `op_start` is the offset of the `%`, which the two error messages quote:
/// upstream builds them from `s.operation.Text`, still the whole rest of the
/// format string at this point rather than this one directive.
fn parse_arg_index(format: &[u8], op_start: usize, i: usize) -> IndexScan {
    if format.get(i) != Some(&b'[') {
        return IndexScan::Absent;
    }
    let open = i;
    let Some(close) = format[i..].iter().position(|&b| b == b']').map(|off| i + off) else {
        return IndexScan::Bad(
            format!(
                "format {} is missing closing ]",
                guff_constant::decode_lossy(&format[op_start..])
            ),
            format.len(),
        );
    };
    let body = &format[open + 1..close];
    // `scanNum` reads digits only, and `ParseInt(…, 10, 32)` rejects what
    // overflows an int32, so `[-1]`, `[1x]` and `[999999999999]` are all
    // spelled back to the user verbatim.
    let num = std::str::from_utf8(body)
        .ok()
        .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|n| n.parse::<i32>().ok());
    match num {
        Some(n) if n >= 1 => IndexScan::Found(
            n as usize,
            guff_constant::decode_lossy(&format[open..=close]),
            close + 1,
        ),
        _ => IndexScan::Bad(
            format!(
                "format has invalid argument index [{}]",
                guff_constant::decode_lossy(body)
            ),
            close + 1,
        ),
    }
}

/// Parse the directive that starts at `chars` (just after a `%`).
///
/// The format is bytes, as it is in Go: upstream compares `s.format[s.i]`
/// byte-wise for every flag, index and digit, and decodes a rune only for the
/// verb itself.
fn scan_directive(format: &[u8], start: usize) -> (Scan, usize) {
    let bytes = format;
    let op_start = start - 1; // the '%' itself, quoted by the error messages
    let mut i = start; // index just after '%'
    let mut text = String::from("%");
    let mut width_index: Option<usize> = None;
    let mut prec_index: Option<usize> = None;
    let mut verb_index: Option<usize> = None;
    let mut has_width_star = false;
    let mut has_prec_star = false;

    // ASCII-only view, for the parts upstream reads as single bytes. A
    // non-ASCII byte here never matches any of them.
    let at = |i: usize| -> Option<char> { bytes.get(i).filter(|b| b.is_ascii()).map(|b| *b as char) };

    // %%
    if at(i) == Some('%') {
        return (Scan::Literal, i + 1);
    }

    // Flags.
    let mut flags = String::new();
    while let Some(c) = at(i) {
        if matches!(c, '#' | '0' | '+' | '-' | ' ') {
            flags.push(c);
            text.push(c);
            i += 1;
        } else {
            break;
        }
    }

    // `indexPending` upstream: an index that no `*` has absorbed yet, which
    // therefore belongs to the verb. It is what stops the pre-verb
    // `parseIndex` from running a second time.
    let mut pending: Option<usize> = None;

    // Explicit argument index: %[n], first of three positions.
    match parse_arg_index(format, op_start, i) {
        IndexScan::Absent => {}
        IndexScan::Found(n, t, next) => {
            pending = Some(n);
            text.push_str(&t);
            i = next;
        }
        IndexScan::Bad(msg, next) => return (Scan::Error(msg), next),
    }

    // Width: digits or '*'.
    if at(i) == Some('*') {
        has_width_star = true;
        // `parseSize` absorbs a pending index into the `*` operand.
        width_index = pending.take();
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
        match parse_arg_index(format, op_start, i) {
            IndexScan::Absent => {}
            IndexScan::Found(n, t, next) => {
                pending = Some(n);
                text.push_str(&t);
                i = next;
            }
            IndexScan::Bad(msg, next) => return (Scan::Error(msg), next),
        }
        if at(i) == Some('*') {
            has_prec_star = true;
            prec_index = pending.take();
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
    if pending.is_none() {
        match parse_arg_index(format, op_start, i) {
            IndexScan::Absent => {}
            IndexScan::Found(n, t, next) => {
                verb_index = Some(n);
                text.push_str(&t);
                i = next;
            }
            IndexScan::Bad(msg, next) => return (Scan::Error(msg), next),
        }
    } else {
        // An index no `*` absorbed belongs to the verb.
        verb_index = pending.take();
    }

    // The verb — `verb, w := utf8.DecodeRuneInString(s.format[s.i:])`. This is
    // the one place upstream decodes a rune rather than reading a byte, so
    // `%é` is one unknown verb and not the first byte of one.
    if i >= bytes.len() {
        return (
            Scan::Error(format!(
                "format {} is missing verb at end of string",
                guff_constant::decode_lossy(&format[op_start..])
            )),
            i,
        );
    }
    let (decoded, width) = guff_constant::utf8::decode_rune(&bytes[i..]);
    let verb = decoded.unwrap_or(char::REPLACEMENT_CHARACTER);
    text.push(verb);
    i += width;

    (
        Scan::Directive(Directive {
            verb,
            flags,
            width_index,
            has_width_star,
            prec_index,
            has_prec_star,
            verb_index,
            text,
        }),
        i,
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

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "printf requires inspect analyzer".to_string())?
        .clone();

    // Upstream's `run` is `findPrintLike` then `checkCalls`: which functions
    // are print-like has to be settled for the whole package before any call
    // site is judged, because a wrapper's kind can be learned from a call that
    // appears later in the file than a call to the wrapper itself.
    let (mut wrappers, mut pending) = printf_wrappers::find_print_like(pass);
    // The one diagnostic here that carries a fix.
    let mut non_constant: Vec<Diagnostic> = Vec::new();
    let mut scratch = Scratch::default();

    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        let Some(callee) = guff_analysis::code::call_target_object(pass, &call.fun) else {
            return;
        };
        // Upstream's `fullname`: `types.Func.FullName()` for a function, so a
        // method is `(*github.com/sirupsen/logrus.Entry).Errorf` — receiver and
        // all — and the bare object name for anything else, which is how a
        // function literal held in a variable gets named (`litf`, not the name
        // of whatever it forwards to).
        let Some((name, base)) = printf_wrappers::names_of(pass, callee) else {
            return;
        };
        let kind = wrappers.kind_of(&name, &base, callee);
        let is_errorf = match kind {
            printf_wrappers::Kind::Printf => false,
            printf_wrappers::Kind::Errorf => true,
            printf_wrappers::Kind::Print => {
                check_print(pass, &mut scratch, call, &name, &mut pending);
                return;
            }
            printf_wrappers::Kind::None => return,
        };
        let fmt_idx = format_index(pass, call);
        let Some(format_arg) = call.args.get(fmt_idx) else {
            return;
        };
        // `versions.Lang(versions.FileVersion(info, file))`, "" when unknown.
        let file_version = code::effective_file_go_version(pass, call.pos().0 as u32);
        let Some(format) = expr_to_bytes(pass, format_arg) else {
            // It is a common mistake to call fmt.Printf(msg) with a
            // non-constant format string and no arguments: if msg contains
            // "%", misformatting occurs. Gated on go1.24 so as not to break
            // existing code (golang/go#71485).
            if fmt_idx + 1 == call.args.len()
                && !file_version.is_empty() // fail open
                && code::version_compare(&file_version, "go1.24") >= 0
            {
                let at = format_arg.pos().0 as u32;
                non_constant.push(Diagnostic {
                    pos: at,
                    message: format!("non-constant format string in call to {name}"),
                    suggested_fixes: vec![SuggestedFix {
                        message: r#"Insert "%s" format string"#.into(),
                        text_edits: vec![TextEdit {
                            pos: at,
                            end: at,
                            new_text: r#""%s", "#.into(),
                        }],
                    }],
                    ..Diagnostic::default()
                });
            }
            return;
        };

        check_one(
            pass,
            &mut scratch,
            &file_version,
            call,
            &name,
            is_errorf,
            fmt_idx,
            &format,
            &mut pending,
        );
    });

    let mut diags: Vec<Diagnostic> = pending
        .into_iter()
        .map(|(pos, message)| Diagnostic {
            pos,
            message,
            ..Diagnostic::default()
        })
        .chain(non_constant)
        .collect();
    diags.sort_by_key(|d| d.pos);
    for d in diags {
        pass.report(d);
    }
    Ok(None)
}

/// `checkPrint`: a call to an unformatted print routine such as `Println`.
fn check_print(
    pass: &Pass<'_>,
    scratch: &mut Scratch,
    call: &CallExpr,
    name: &str,
    out: &mut Vec<(u32, String)>,
) {
    let Some(art) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    // Skip checking functions with unknown type.
    let Some(typ) = expr_type(pass, &call.fun) else {
        return;
    };
    let mut first_arg = 0;
    if let TypeData::Signature(sig) = art.types.get(typ.underlying(&art.types)) {
        // Skip checking non-variadic functions.
        if !sig.variadic() {
            return;
        }
        let n = tuple_len(&art.types, sig.params());
        if n == 0 {
            return;
        }
        first_arg = n - 1;
        // Skip variadic functions accepting non-interface{} args.
        let ObjectData::Var(last) =
            art.objects.get(tuple_at(&art.types, sig.params().unwrap(), first_arg))
        else {
            return;
        };
        let TypeData::Slice(slice) = art.types.get(last.typ()) else {
            return;
        };
        let elem = unalias_readonly(&art.types, slice.elem());
        // `types.Unalias(typ).(*types.Interface)` and `Empty()`: the literal
        // `interface{}` / `any`, not a named interface type.
        let TypeData::Interface(it) = art.types.get(elem) else {
            return;
        };
        if it.num_explicit_methods() != 0 || it.num_embeddeds() != 0 {
            return;
        }
    }
    // Skip calls without variadic args.
    if call.args.len() <= first_arg {
        return;
    }
    let call_pos = call.fun.pos().0 as u32;
    let args = &call.args[first_arg..];

    if first_arg == 0 {
        if let Expr::SelectorExpr(sel) = &call.args[0] {
            if let Expr::Ident(x) = &*sel.x {
                if x.name == "os" && sel.sel.name.starts_with("Std") {
                    out.push((
                        call_pos,
                        format!(
                            "{name} does not take io.Writer but has first arg {}",
                            describe_arg(pass, &call.args[0])
                        ),
                    ));
                }
            }
        }
    }

    if let Some(s) = expr_to_bytes(pass, &args[0]) {
        // Ignore trailing % character: the % in "abc 0.0%" couldn't be a
        // formatting directive.
        let s = s.strip_suffix(b"%").unwrap_or(&s);
        if s.contains(&b'%') {
            for m in print_format_matches(s) {
                // Allow %XX where XX are hex digits, as this is common in URLs.
                if m.len() >= 3 && m[1].is_ascii_hexdigit() && m[2].is_ascii_hexdigit() {
                    continue;
                }
                out.push((
                    call_pos,
                    format!(
                        "{name} call has possible Printf formatting directive {}",
                        guff_constant::decode_lossy(m)
                    ),
                ));
                break; // report only the first one
            }
        }
    }
    for arg in args {
        if printf_types::is_function_value(pass, arg) {
            out.push((
                call_pos,
                format!("{name} arg {} is a func value, not called", describe_arg(pass, arg)),
            ));
        }
        if let Some(method) = printf_types::recursive_stringer(pass, scratch, arg) {
            out.push((
                call_pos,
                format!(
                    "{name} arg {} causes recursive call to {method} method",
                    describe_arg(pass, arg)
                ),
            ));
        }
    }
}

/// `printFormatRE.FindAllString(s, -1)`:
///
/// ```text
/// %[+\-#]*([0-9]+|(\[[0-9]+\])?\*)?\.?([0-9]+|(\[[0-9]+\])?\*)?(\[[0-9]+\])?[bcdefgopqstvxEFGTUX]
/// ```
///
/// The space flag is excluded, so that printing a string like "x % y" is not
/// reported as a format. Leftmost-first, non-overlapping, as Go's regexp
/// returns them.
fn print_format_matches(s: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' {
            if let Some(end) = match_print_format(s, i) {
                out.push(&s[i..end]);
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// The end of a `printFormatRE` match starting at `s[start] == '%'`, if any.
///
/// Every part before the verb is optional and greedy, but Go's regexp
/// backtracks: `%5*` before a verb can still match as `%5` + … only if the
/// rest does. The parts are tried longest-first and the first combination
/// that reaches a verb wins — the leftmost-first semantics of RE2.
fn match_print_format(s: &[u8], start: usize) -> Option<usize> {
    let mut i = start + 1;
    while i < s.len() && matches!(s[i], b'+' | b'-' | b'#') {
        i += 1;
    }
    // Backtracking over flags never helps: none of the later parts can start
    // with a flag character.
    for after_width in num_opt_ends(s, i) {
        let mut dots = vec![];
        if after_width < s.len() && s[after_width] == b'.' {
            dots.push(after_width + 1);
        }
        dots.push(after_width);
        for after_dot in dots {
            for after_prec in num_opt_ends(s, after_dot) {
                for after_index in index_opt_ends(s, after_prec) {
                    if after_index < s.len() && b"bcdefgopqstvxEFGTUX".contains(&s[after_index]) {
                        return Some(after_index + 1);
                    }
                }
            }
        }
    }
    None
}

/// Where `([0-9]+|(\[[0-9]+\])?\*)?` can end when started at `i`, in the
/// order RE2's leftmost-first search prefers them.
fn num_opt_ends(s: &[u8], i: usize) -> Vec<usize> {
    let mut ends = Vec::new();
    // [0-9]+, greedy: longest first.
    let mut j = i;
    while j < s.len() && s[j].is_ascii_digit() {
        j += 1;
    }
    let mut k = j;
    while k > i {
        ends.push(k);
        k -= 1;
    }
    // (\[[0-9]+\])?\*
    for idx_end in index_opt_ends(s, i) {
        if idx_end < s.len() && s[idx_end] == b'*' {
            ends.push(idx_end + 1);
        }
    }
    // The empty alternative.
    ends.push(i);
    ends
}

/// Where `(\[[0-9]+\])?` can end when started at `i`: after the index, then
/// without it.
fn index_opt_ends(s: &[u8], i: usize) -> Vec<usize> {
    let mut ends = Vec::new();
    if i < s.len() && s[i] == b'[' {
        let mut j = i + 1;
        while j < s.len() && s[j].is_ascii_digit() {
            j += 1;
        }
        if j > i + 1 && j < s.len() && s[j] == b']' {
            ends.push(j + 1);
        }
    }
    ends.push(i);
    ends
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

#[allow(clippy::too_many_arguments)]
fn check_one(
    pass: &Pass<'_>,
    scratch: &mut Scratch,
    file_version: &str,
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

    // A format string with no `%` at all is its own branch, before any
    // parsing: upstream reports the leftover arguments at the **first argument
    // after the format**, with its own wording, and returns. guff fell through
    // to the arity check, which says `call needs 0 args but has 3 args` at the
    // callee — a different message at a different position, so the two tools
    // disagreed on a shape they both meant to report (velero's
    // `p.log.Errorf("error parsing operation ID's StartedTime", …)`).
    if !format.contains(&b'%') {
        if nargs > first_arg {
            out.push((
                call.args[first_arg].pos().0 as u32,
                format!("{name} call has arguments but no formatting directives"),
            ));
        }
        return;
    }

    // Upstream parses the whole format string before checking a single
    // argument (`fmtstr.Parse`), so a malformed directive is the only thing
    // reported — the directives around it are never looked at.
    let mut ops: Vec<(u32, Directive)> = Vec::new();
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
        match scan {
            Scan::Literal => continue,
            Scan::Error(msg) => {
                // ReportRangef(formatArg, "%s %s", name, err).
                out.push((format_arg.pos().0 as u32, format!("{name} {msg}")));
                return;
            }
            // Position of the "%v" substring inside the literal.
            Scan::Directive(d) => ops.push((op_pos(format_arg, op_start), d)),
        }
    }

    // `fmtstr.Parse` numbers the operands as it goes: a `*` takes the next
    // argument (or the one an index just before it names), the verb the one
    // after, and `%` none.
    let mut arg_num = first_arg;
    // Upstream's `maxArgIndex`: the highest argument index used so far.
    let mut max_arg_index = first_arg - 1;
    let mut any_index = false;

    for (pos, dir) in ops {
        if dir.width_index.is_some() || dir.prec_index.is_some() || dir.verb_index.is_some() {
            any_index = true;
        }
        let mut stars: Vec<usize> = Vec::new();
        for (idx, star) in [
            (dir.width_index, dir.has_width_star),
            (dir.prec_index, dir.has_prec_star),
        ] {
            if let Some(idx) = idx {
                arg_num = first_arg + idx - 1;
            }
            if star {
                stars.push(arg_num);
                arg_num += 1;
            }
        }
        if let Some(idx) = dir.verb_index {
            arg_num = first_arg + idx - 1;
        }
        let verb_arg = arg_num;
        // Do not waste an argument for '%'.
        if dir.verb != '%' {
            arg_num += 1;
        }

        if dir.verb == 'w' && !is_errorf {
            out.push((
                pos,
                format!("{name} does not support error-wrapping directive %w"),
            ));
            return;
        }
        let op = Op {
            pos,
            dir: &dir,
            stars: &stars,
            verb_arg,
        };
        if !ok_printf_arg(pass, scratch, file_version, call, &mut max_arg_index, first_arg, name, &op, out) {
            // One error per format is enough.
            return;
        }
    }

    // Dotdotdot is hard.
    if ellipsis && max_arg_index + 2 >= nargs {
        return;
    }
    // If any formats are indexed, extra arguments are ignored.
    if any_index {
        return;
    }
    // There should be no leftover arguments.
    if max_arg_index + 1 < nargs {
        let expect = max_arg_index + 1 - first_arg;
        let got = nargs - first_arg;
        out.push((
            call_pos,
            format!("{name} call needs {} but has {}", count(expect, "arg"), count(got, "arg")),
        ));
    }
}

/// One parsed operation and the operands `fmtstr` assigned it.
struct Op<'d> {
    /// Where upstream reports: the `%v` inside the literal.
    pos: u32,
    dir: &'d Directive,
    /// `Width.Dynamic`, `Prec.Dynamic`: the operands of the `*`s.
    stars: &'d [usize],
    /// `Verb.ArgIndex`.
    verb_arg: usize,
}

/// `okPrintfArg`: compare one operation with the arguments actually present.
#[allow(clippy::too_many_arguments)]
fn ok_printf_arg(
    pass: &Pass<'_>,
    scratch: &mut Scratch,
    file_version: &str,
    call: &CallExpr,
    max_arg_index: &mut usize,
    first_arg: usize,
    name: &str,
    op: &Op<'_>,
    out: &mut Vec<(u32, String)>,
) -> bool {
    let dir = op.dir;
    let verb = dir.verb;
    // An unknown verb leaves `v` on the last entry, as upstream's loop does.
    let found = PRINT_VERBS.iter().find(|v| v.verb == verb);
    let v = found.unwrap_or(&PRINT_VERBS[PRINT_VERBS.len() - 1]);
    let mut typ = v.typ;

    // When analyzing go1.26 code, rune and byte are the only %q integers
    // (#72850).
    if verb == 'q'
        && !file_version.is_empty() // fail open
        && code::version_compare(file_version, "go1.26") >= 0
    {
        typ = ARG_RUNE | ARG_BYTE | ARG_STRING;
    }

    // Could verb's arg implement fmt.Formatter? Skip check for the %w verb,
    // which requires an error.
    let mut formatter = false;
    if typ != ARG_ERROR && op.verb_arg < call.args.len() {
        if let Some(t) = expr_type(pass, &call.args[op.verb_arg]) {
            formatter = printf_types::is_formatter(pass, scratch, t);
        }
    }

    if !formatter {
        if found.is_none() {
            out.push((
                op.pos,
                format!("{name} format {} has unknown verb {verb}", dir.text),
            ));
            return false;
        }
        for flag in dir.flags.chars() {
            // Disable complaint about '0' (issues 23598 and 23605).
            if flag == '0' {
                continue;
            }
            if !v.flags.contains(flag) {
                out.push((
                    op.pos,
                    format!("{name} format {} has unrecognized flag {flag}", dir.text),
                ));
                return false;
            }
        }
    }

    // If there are stars, we have something like %.*s and every one of
    // their operands must be an integer.
    for &arg_index in op.stars {
        if !arg_can_be_checked(call, op, arg_index, first_arg, name, out) {
            return false;
        }
        let arg = &call.args[arg_index];
        let (reason, ok) = printf_types::match_arg_type(pass, scratch, ARG_INT, arg);
        if !ok {
            out.push((
                op.pos,
                format!(
                    "{name} format {} uses non-int {}{} as argument of *",
                    dir.text,
                    describe_arg(pass, arg),
                    details(reason)
                ),
            ));
            return false;
        }
    }

    // Collect to update maxArgIndex in one go.
    for &i in op.stars {
        *max_arg_index = (*max_arg_index).max(i);
    }
    if verb != '%' {
        *max_arg_index = (*max_arg_index).max(op.verb_arg);
    }

    // `%` takes no operand ("%10.2%%dhello" prints "%4hello"), and a
    // Formatter decides for itself.
    if verb == '%' || formatter {
        return true;
    }

    // Now check verb's type.
    if !arg_can_be_checked(call, op, op.verb_arg, first_arg, name, out) {
        return false;
    }
    let arg = &call.args[op.verb_arg];
    // The go1.27 `%w`-of-a-pointer check is PR 15.
    if printf_types::is_function_value(pass, arg) && verb != 'p' && verb != 'T' {
        out.push((
            op.pos,
            format!(
                "{name} format {} arg {} is a func value, not called",
                dir.text,
                describe_arg(pass, arg)
            ),
        ));
        return false;
    }
    let (reason, ok) = printf_types::match_arg_type(pass, scratch, typ, arg);
    if !ok {
        let type_string = expr_type(pass, arg)
            .map(|t| printf_types::type_string(pass, t))
            .unwrap_or_default();
        out.push((
            op.pos,
            format!(
                "{name} format {} has arg {} of wrong type {type_string}{}",
                dir.text,
                describe_arg(pass, arg),
                details(reason)
            ),
        ));
        return false;
    }
    // Detect recursive formatting via value's String/Error methods. The '#'
    // flag suppresses the methods, except with %x, %X, and %q.
    if typ & ARG_STRING != 0
        && verb != 'T'
        && (!dir.flags.contains('#') || matches!(verb, 'q' | 'x' | 'X'))
    {
        if let Some(method) = printf_types::recursive_stringer(pass, scratch, arg) {
            out.push((
                op.pos,
                format!(
                    "{name} format {} with arg {} causes recursive {method} method call",
                    dir.text,
                    describe_arg(pass, arg)
                ),
            ));
            return false;
        }
    }
    true
}

/// ` (reason)`, or nothing.
fn details(reason: Option<String>) -> String {
    reason.map(|r| format!(" ({r})")).unwrap_or_default()
}

/// `argCanBeChecked`: whether argument `arg_index` is statically present —
/// it may be beyond the list of arguments, or inside a trailing `xs...`.
fn arg_can_be_checked(
    call: &CallExpr,
    op: &Op<'_>,
    arg_index: usize,
    first_arg: usize,
    name: &str,
    out: &mut Vec<(u32, String)>,
) -> bool {
    let nargs = call.args.len();
    if arg_index + 1 < nargs {
        return true; // Always OK.
    }
    if call.ellipsis.is_valid() {
        return false; // We just can't tell; there could be many more arguments.
    }
    if arg_index < nargs {
        return true;
    }
    // There are bad indexes in the format or there are fewer arguments than
    // the format needs. People think of arguments as 1-indexed.
    let arg = arg_index - first_arg + 1;
    out.push((
        op.pos,
        format!(
            "{name} format {} reads arg #{arg}, but call has {}",
            op.dir.text,
            count(nargs - first_arg, "arg")
        ),
    ));
    false
}

/// `count(n, what)`: "1 what" or "N whats".
fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
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

    fn directive(format: &[u8]) -> Directive {
        match scan_directive(format, 1).0 {
            Scan::Directive(d) => d,
            _ => panic!("expected directive"),
        }
    }

    #[test]
    fn scans_flags_width_precision() {
        let (scan, next) = scan_directive(b"%-+#0 12.34d", 1);
        assert_eq!(next, "%-+#0 12.34d".len());
        match scan {
            Scan::Directive(d) => {
                assert_eq!(d.verb, 'd');
                assert!(!d.has_width_star);
                assert!(!d.has_prec_star);
            }
            _ => panic!("expected directive"),
        }
    }

    #[test]
    fn scans_stars_and_index() {
        let d = directive(b"%[2]*.*d");
        assert_eq!(d.verb, 'd');
        // The index belongs to the width `*`; the precision `*` and the verb
        // take the arguments that follow it.
        assert_eq!(d.width_index, Some(2));
        assert!(d.has_width_star);
        assert_eq!(d.prec_index, None);
        assert!(d.has_prec_star);
        assert_eq!(d.verb_index, None);
    }

    /// rclone's `fmt.Sprintf("%[2]*[1]s", str, rawWidth)`: the width is
    /// argument 2 and the verb prints argument 1.
    #[test]
    fn index_binds_to_the_position_that_absorbs_it() {
        let d = directive(b"%[2]*[1]s");
        assert_eq!(d.width_index, Some(2));
        assert!(d.has_width_star);
        assert_eq!(d.verb_index, Some(1));
    }

    /// An index no `*` absorbs belongs to the verb, wherever it was written.
    #[test]
    fn unabsorbed_index_belongs_to_the_verb() {
        let d = directive(b"%[2]3d");
        assert_eq!(d.width_index, None);
        assert!(!d.has_width_star);
        assert_eq!(d.verb_index, Some(2));
    }

    #[test]
    fn indexed_precision_star() {
        let d = directive(b"%.[2]*[1]d");
        assert_eq!(d.prec_index, Some(2));
        assert!(d.has_prec_star);
        assert_eq!(d.verb_index, Some(1));
    }

    #[test]
    fn invalid_index_is_quoted_back() {
        match scan_directive(b"%[x]d", 1).0 {
            Scan::Error(msg) => assert_eq!(msg, "format has invalid argument index [x]"),
            _ => panic!("expected error"),
        }
        // Rejected by `ParseInt(…, 10, 32)`, not by the digit scan.
        match scan_directive(b"%[999999999999]d", 1).0 {
            Scan::Error(msg) => {
                assert_eq!(msg, "format has invalid argument index [999999999999]")
            }
            _ => panic!("expected error"),
        }
    }

    #[test]
    fn missing_closing_bracket_quotes_the_rest_of_the_format() {
        match scan_directive(b"%[3d b %s", 1).0 {
            Scan::Error(msg) => assert_eq!(msg, "format %[3d b %s is missing closing ]"),
            _ => panic!("expected error"),
        }
    }

    #[test]
    fn literal_percent() {
        assert!(matches!(scan_directive(b"%%", 1).0, Scan::Literal));
    }

    #[test]
    fn trailing_percent_is_error() {
        match scan_directive(b"%", 1).0 {
            Scan::Error(msg) => assert_eq!(msg, "format % is missing verb at end of string"),
            _ => panic!("expected error"),
        }
        match scan_directive(b"%[1]", 1).0 {
            Scan::Error(msg) => assert_eq!(msg, "format %[1] is missing verb at end of string"),
            _ => panic!("expected error"),
        }
    }

    fn matches(s: &str) -> Vec<String> {
        print_format_matches(s.as_bytes())
            .into_iter()
            .map(|m| String::from_utf8(m.to_vec()).unwrap())
            .collect()
    }

    #[test]
    fn print_format_re_matches_like_go_regexp() {
        assert_eq!(matches("hello %s"), ["%s"]);
        assert_eq!(matches("%-+#10.3f and %[2]*d"), ["%-+#10.3f", "%[2]*d"]);
        // The space flag is excluded, so "x % y" is not a directive.
        assert!(matches("x % y").is_empty());
        // `%%d`: the first `%` starts no match, the second does.
        assert_eq!(matches("%%d"), ["%d"]);
        // A width of digits, then a `*` precision: the second size takes it.
        assert_eq!(matches("%5*d"), ["%5*d"]);
        // Backtracking out of the index: `[1]` followed by no verb.
        assert_eq!(matches("%[1]z %v"), ["%v"]);
        // `%XX` hex escapes are matched here; the caller skips them.
        assert_eq!(matches("%2F"), ["%2F"]);
        assert!(matches("50% off").is_empty());
    }
}
