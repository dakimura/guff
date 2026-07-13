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
use guff::walk::NodeRef;
use guff_analysis::code::{call_name, expr_to_string};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::{basic_kind, BasicKind};
use guff_types::predicates::{is_boolean, is_complex, is_float, is_integer, is_string, is_valid};
use guff_types::signature::signature_params;
use guff_types::tuple::tuple_len;
use guff_types::TypeId;

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

/// Parse the directive that starts at `chars` (just after a `%`).
fn scan_directive(format: &str, start: usize) -> (Scan, usize) {
    let bytes = format.as_bytes();
    let mut i = start; // index just after '%'
    let mut text = String::from("%");
    let mut index: Option<usize> = None;
    let mut stars = 0usize;

    let at = |i: usize| -> Option<char> { bytes.get(i).map(|b| *b as char) };

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

    // Explicit argument index: %[n]
    if at(i) == Some('[') {
        let close = format[i..].find(']').map(|off| i + off);
        let Some(close) = close else {
            return (Scan::Error("format has invalid argument index".into()), i + 1);
        };
        let num = &format[i + 1..close];
        match num.parse::<usize>() {
            Ok(n) if n >= 1 => index = Some(n),
            _ => {
                return (
                    Scan::Error("format has invalid argument index".into()),
                    close + 1,
                )
            }
        }
        text.push_str(&format[i..=close]);
        i = close + 1;
    }

    // Width: digits or '*'.
    if at(i) == Some('*') {
        stars += 1;
        text.push('*');
        i += 1;
    } else {
        while at(i).is_some_and(|c| c.is_ascii_digit()) {
            text.push(at(i).unwrap());
            i += 1;
        }
    }

    // Precision: '.' then digits or '*'.
    if at(i) == Some('.') {
        text.push('.');
        i += 1;
        if at(i) == Some('*') {
            stars += 1;
            text.push('*');
            i += 1;
        } else {
            while at(i).is_some_and(|c| c.is_ascii_digit()) {
                text.push(at(i).unwrap());
                i += 1;
            }
        }
    }

    // The verb.
    let Some(verb) = at(i) else {
        return (Scan::Error("format string ends with %".into()), i);
    };
    text.push(verb);
    i += 1;

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
fn type_has_method(pass: &Pass<'_>, typ: TypeId, name: &str) -> bool {
    let Some(art) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut t = typ;
    if let TypeData::Pointer(p) = art.types.get(t) {
        t = p.elem();
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
            // Promoted methods from embedded fields may make this printable;
            // accept such structs conservatively.
            struct_has_embedded(pass, u)
        }
        _ => true,
    }
}

/// Best-effort source rendering of an argument for diagnostics.
fn describe_arg(arg: &Expr) -> String {
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
    inspect.preorder(pass.files(), |n| {
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
        let Some(format) = expr_to_string(pass, format_arg) else {
            return; // non-constant format string: skip (no false positives)
        };

        check_one(pass, call, &name, is_errorf, fmt_idx, &format, &mut pending);
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

fn check_one(
    pass: &Pass<'_>,
    call: &CallExpr,
    name: &str,
    is_errorf: bool,
    fmt_idx: usize,
    format: &str,
    out: &mut Vec<(u32, String)>,
) {
    let pos = call.lparen.0 as u32;
    let first_arg = fmt_idx + 1;
    let nargs = call.args.len();

    let mut arg_num = first_arg;
    let mut max_arg_num = first_arg;
    let mut any_index = false;

    let bytes = format.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        let (scan, next) = scan_directive(format, i + 1);
        i = next;
        let dir = match scan {
            Scan::Literal => continue,
            Scan::Error(msg) => {
                out.push((pos, format!("{name} {msg}")));
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
                    arg.pos().0 as u32,
                    format!(
                        "{name} format {} has arg {} of wrong type {}",
                        dir.text,
                        describe_arg(arg),
                        type_name(pass, typ)
                    ),
                ));
            }
        }
    }

    // Too many arguments (only meaningful without explicit indexes).
    if !any_index && max_arg_num < nargs {
        let expect = max_arg_num - first_arg;
        let got = nargs - first_arg;
        out.push((
            pos,
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
        let (scan, next) = scan_directive("%-+#0 12.34d", 1);
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
        let (scan, _) = scan_directive("%[2]*.*d", 1);
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
        assert!(matches!(scan_directive("%%", 1).0, Scan::Literal));
    }

    #[test]
    fn trailing_percent_is_error() {
        assert!(matches!(scan_directive("%", 1).0, Scan::Error(_)));
    }
}
