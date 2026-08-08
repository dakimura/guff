//! SA5009 — invalid Printf call.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5009`, including its
//! `honnef.co/go/tools/printf` format-string grammar.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check(call: &mut Call<'_>, ctx: &CallContext<'_>, format_idx: usize, args_start: usize) {
    // `Printf(format, args...)` passes the operands on as an opaque slice, so
    // their number is unknown. Upstream bails out of `checkPrintfCall` when
    // `irutil.Vararg` cannot recover the individual arguments.
    if call.common.ellipsis {
        return;
    }
    let Some(fmt_arg) = call.args.get(format_idx) else {
        return;
    };
    let Some(format) =
        callcheck::extract_const_string(ctx.prog, ctx.caller, fmt_arg.value)
    else {
        return;
    };
    let nargs = call.args.len().saturating_sub(args_start);
    match check_format(&format, nargs) {
        Ok(()) => {}
        Err(msg) => call.args[format_idx].invalid(msg),
    }
}

/// One parsed verb of a format string. Port of `printf.Verb`.
struct Verb {
    letter: char,
    width: Argument,
    precision: Argument,
    /// Which value in the argument list the verb uses. `-1` denotes the next
    /// argument, values > 0 denote explicit arguments, and `0` denotes that no
    /// argument is consumed — which is the case for `%%`.
    value: i64,
    raw: String,
}

/// Port of `printf.Argument`. Only `Star` carries information the check uses;
/// `Default` / `Zero` / `Literal` all mean "consumes no argument".
enum Argument {
    Other,
    Star { index: i64 },
}

/// `honnef.co/go/tools/printf`'s grammar, verbatim. Go's regexp and Rust's
/// `regex` both resolve alternations leftmost-first, so the submatch numbering
/// upstream relies on carries over unchanged.
fn verb_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        const FLAGS: &str = r"([+#0 -]*)";
        const VERB: &str = r"([a-zA-Z%])";
        const INDEX: &str = r"(?:\[([0-9]+)\])";
        // star = `((` + index + `)?\*)`
        let star = format!(r"(({INDEX})?\*)");
        // width = `(?:([0-9]+)|` + star + `)`
        let width = format!(r"(?:([0-9]+)|{star})");
        let precision = format!(r"(?:([0-9]+)|{star})");
        let width_and_precision = format!(r"(?:(?:{width})?(?:(\.)(?:{precision})?)?)");
        regex::Regex::new(&format!(
            r"^%{FLAGS}{width_and_precision}?{INDEX}?{VERB}"
        ))
        .expect("printf verb grammar")
    })
}

fn atoi(s: &str) -> i64 {
    s.parse().unwrap_or(0)
}

/// Port of `printf.ParseVerb`. Returns the verb and how many bytes it consumed.
fn parse_verb(f: &str) -> Option<(Verb, usize)> {
    // Submatch numbers from upstream's `ParseVerb` constants.
    const WIDTH: usize = 2;
    const WIDTH_STAR: usize = 3;
    const WIDTH_INDEX: usize = 5;
    const DOT: usize = 6;
    const PREC: usize = 7;
    const PREC_STAR: usize = 8;
    const PREC_INDEX: usize = 10;
    const VERB_INDEX: usize = 11;
    const VERB: usize = 12;

    if f.len() < 2 {
        return None;
    }
    let m = verb_re().captures(f)?;
    let g = |i: usize| m.get(i).map(|x| x.as_str()).unwrap_or("");

    let star = |whole: usize, index: usize| {
        if g(whole).is_empty() {
            Argument::Other
        } else if g(index).is_empty() {
            Argument::Star { index: -1 }
        } else {
            Argument::Star {
                index: atoi(g(index)),
            }
        }
    };

    let width = if !g(WIDTH).is_empty() {
        Argument::Other
    } else {
        star(WIDTH_STAR, WIDTH_INDEX)
    };
    let precision = if g(DOT).is_empty() || !g(PREC).is_empty() {
        Argument::Other
    } else {
        star(PREC_STAR, PREC_INDEX)
    };

    let letter = g(VERB).chars().next()?;
    let value = if g(VERB) == "%" {
        0
    } else if !g(VERB_INDEX).is_empty() {
        atoi(g(VERB_INDEX))
    } else {
        -1
    };

    let raw = m.get(0)?.as_str().to_string();
    let n = raw.len();
    Some((
        Verb {
            letter,
            width,
            precision,
            value,
            raw,
        },
        n,
    ))
}

/// Port of `printf.Parse`, keeping only the verbs (literal runs carry nothing
/// the check needs). `Err(())` is upstream's `ErrInvalid`.
fn parse_format(format: &str) -> Result<Vec<Verb>, ()> {
    let mut out = Vec::new();
    let mut f = format;
    while !f.is_empty() {
        if f.as_bytes()[0] == b'%' {
            let (v, n) = parse_verb(f).ok_or(())?;
            f = &f[n..];
            out.push(v);
        } else {
            match f.find('%') {
                Some(n) => f = &f[n..],
                None => break,
            }
        }
    }
    Ok(out)
}

/// Port of the argument-counting half of `sa5009.checkImpl`.
///
/// The other half — `checkType`, which reports `has arg #N of wrong type T` —
/// needs upstream's verb/type compatibility table and is not ported; guff stays
/// silent where upstream would name a type mismatch (docs/COMPAT-HARDENING.md).
fn check_format(format: &str, nargs: usize) -> Result<(), String> {
    let Ok(verbs) = parse_format(format) else {
        return Err("couldn't parse format string".into());
    };
    let nargs = nargs as i64;
    let mut ptr: i64 = 1;
    let mut has_explicit = false;

    // Upstream reports at most one problem per format string: getting an index
    // wrong invalidates every implicit index after it.
    for verb in &verbs {
        for star in [&verb.width, &verb.precision] {
            let Argument::Star { index } = *star else {
                continue;
            };
            let idx = if index == -1 {
                let idx = ptr;
                ptr += 1;
                idx
            } else {
                has_explicit = true;
                ptr = index + 1;
                index
            };
            if idx == 0 {
                return Err(format!(
                    "Printf format {} reads invalid arg 0; indices are 1-based",
                    verb.raw
                ));
            }
            if idx > nargs {
                return Err(format!(
                    "Printf format {} reads arg #{idx}, but call has only {nargs} args",
                    verb.raw
                ));
            }
        }

        let mut off = ptr;
        if verb.value != -1 {
            // Note that `%%` parses as value 0, so a format containing one
            // suppresses the trailing too-many-arguments check below. That is
            // upstream's behaviour, verified against golangci-lint 2.12.2.
            has_explicit = true;
            off = verb.value;
        }
        if off > nargs {
            return Err(format!(
                "Printf format {} reads arg #{off}, but call has only {nargs} args",
                verb.raw
            ));
        } else if verb.value == 0 && verb.letter != '%' {
            return Err(format!(
                "Printf format {} reads invalid arg 0; indices are 1-based",
                verb.raw
            ));
        }

        match verb.value {
            -1 => ptr += 1,
            0 => {}
            v => ptr = v + 1,
        }
    }

    if !has_explicit && ptr <= nargs {
        return Err(format!(
            "Printf call needs {} args but has {nargs} args",
            ptr - 1
        ));
    }
    Ok(())
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([
            ("fmt.Errorf", check0 as callcheck::CheckFn),
            ("fmt.Printf", check0 as callcheck::CheckFn),
            ("fmt.Sprintf", check0 as callcheck::CheckFn),
            ("fmt.Fprintf", check1 as callcheck::CheckFn),
        ])
    })
}

fn check0(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check(call, ctx, 0, 1);
}

fn check1(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check(call, ctx, 1, 2);
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA5009 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa5009_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA5009",
        doc: "invalid Printf call",
        url: "https://staticcheck.dev/docs/checks/#SA5009",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa5009_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa5009_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn format_arg_count() {
        assert!(check_format("%s %d", 2).is_ok());
        assert!(check_format("%s", 2).is_err());
        // Explicit indices reuse args (`%[1]q … %[1]q … %q` needs 2).
        assert!(check_format(
            r#"/explore?left={"datasource":%[1]q,"queries":[{"datasource":%[1]q,"expr":%q}],"range":{}}"#,
            2
        )
        .is_ok());
        assert!(check_format("%[2]s %[1]s", 2).is_ok());
        assert!(check_format("%[2]s", 1).is_err());
        // Width `*` after flags (`%-*s`) consumes an extra arg.
        assert!(check_format("%-*s %-*s %s\n", 5).is_ok());
        assert!(check_format("%-*s", 1).is_err());
    }

    /// Every string here was read off golangci-lint 2.12.2 on a scratch module,
    /// not derived from upstream's source.
    #[test]
    fn format_messages_match_upstream() {
        let err = |f: &str, n: usize| check_format(f, n).unwrap_err();
        // Too few arguments names the verb and the index it wanted.
        assert_eq!(
            err("%s %d", 0),
            "Printf format %s reads arg #1, but call has only 0 args"
        );
        assert_eq!(
            err("%[2]s", 1),
            "Printf format %[2]s reads arg #2, but call has only 1 args"
        );
        // A `*` width consumes an argument before the verb does.
        assert_eq!(
            err("%*d", 1),
            "Printf format %*d reads arg #2, but call has only 1 args"
        );
        assert_eq!(
            err("%[0]d", 1),
            "Printf format %[0]d reads invalid arg 0; indices are 1-based"
        );
        // Too many arguments is the only case that uses the "needs" wording.
        assert_eq!(err("hello", 1), "Printf call needs 0 args but has 1 args");
        assert_eq!(
            err("%-*s %s", 4),
            "Printf call needs 3 args but has 4 args"
        );
        // A lone `%`, or one followed by a non-verb, fails the grammar.
        assert_eq!(err("%", 0), "couldn't parse format string");
        assert_eq!(err("%!", 0), "couldn't parse format string");
        // `%%` parses with Value == 0, which trips upstream's `hasExplicit`
        // flag and so suppresses the trailing too-many-arguments check for the
        // whole format string. Verified against golangci-lint, which reports
        // nothing for either of these.
        assert!(check_format("%%", 1).is_ok());
        assert!(check_format("%v %%", 2).is_ok());
    }
}
