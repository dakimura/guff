//! `time-date` — report bad usage of `time.Date`.

use guff::ast::{BasicLit, CallExpr, Expr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_ident, is_pkg_dot_name, unparen};

const TIME_DATE_ARITY: usize = 8;

#[derive(Clone, Copy, PartialEq)]
enum TimeDateArg {
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
    Nanosecond,
    Timezone,
}

const TIME_DATE_ARGS: [TimeDateArg; TIME_DATE_ARITY] = [
    TimeDateArg::Year,
    TimeDateArg::Month,
    TimeDateArg::Day,
    TimeDateArg::Hour,
    TimeDateArg::Minute,
    TimeDateArg::Second,
    TimeDateArg::Nanosecond,
    TimeDateArg::Timezone,
];

pub struct Checker {
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
                    let NodeRef::CallExpr(call) = n else { return; };
                    check_call(call, &mut self.failures);
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}


fn check_call(call: &CallExpr, failures: &mut Vec<Failure>) {
    if call.args.len() != TIME_DATE_ARITY || !is_pkg_dot_name(&call.fun, "time", "Date") {
        return;
    }

    let tz = &call.args[TIME_DATE_ARITY - 1];
    if is_ident(tz, "nil") {
        failures.push(Failure {
            rule: "time-date",
            pos: tz.pos().0 as u32,
            message: "time.Date timezone argument cannot be nil, it would panic on runtime".into(),
            ..Failure::default()
        });
    }

    let mut year = 0i64;
    let mut month = 0i64;

    for (pos, arg) in call.args[..TIME_DATE_ARITY - 1].iter().enumerate() {
        let field = TIME_DATE_ARGS[pos];
        let Some(bl) = check_arg_sign(arg, field, failures) else {
            continue;
        };
        let (parsed, notation) = parse_decimal_integer(bl);
        if let Some(notation) = notation {
            // Everything that is not a plain decimal integer is reported here
            // and skips the range checks below. guff used to swallow all of
            // them, so an octal / hex / float / exponential argument produced
            // no finding at all.
            report_notation(arg, bl, field, parsed, notation, failures);
            continue;
        }

        match field {
            TimeDateArg::Year => year = parsed,
            TimeDateArg::Month => {
                month = parsed;
                if parsed == 0 {
                    failures.push(Failure {
                        rule: "time-date",
                        pos: arg.pos().0 as u32,
                        message: "time.Date month argument should not be zero".into(),
                        ..Failure::default()
                    });
                } else if !(1..=12).contains(&parsed) {
                    failures.push(Failure {
                        rule: "time-date",
                        pos: arg.pos().0 as u32,
                        message: format!(
                            "time.Date month argument should be between 1 and 12: {}",
                            gofmt_arg(arg)
                        ),
                        ..Failure::default()
                    });
                }
            }
            TimeDateArg::Day => {
                if parsed == 0 {
                    failures.push(Failure {
                        rule: "time-date",
                        pos: arg.pos().0 as u32,
                        message: "time.Date day argument should not be zero".into(),
                        ..Failure::default()
                    });
                } else {
                    let max = days_in_month(year, month);
                    if parsed > max {
                        failures.push(Failure {
                            rule: "time-date",
                            pos: arg.pos().0 as u32,
                            message: format!(
                                "time.Date day argument {parsed} exceeds days in month ({max})"
                            ),
                            ..Failure::default()
                        });
                    }
                }
            }
            TimeDateArg::Hour => check_bounds(arg, field, parsed, 0, 23, failures),
            TimeDateArg::Minute => check_bounds(arg, field, parsed, 0, 59, failures),
            TimeDateArg::Second => check_bounds(arg, field, parsed, 0, 60, failures),
            TimeDateArg::Nanosecond => check_bounds(arg, field, parsed, 0, 999_999_999, failures),
            TimeDateArg::Timezone => {}
        }
    }
}

fn check_bounds(
    arg: &Expr,
    field: TimeDateArg,
    parsed: i64,
    min: i64,
    max: i64,
    failures: &mut Vec<Failure>,
) {
    if parsed < min || parsed > max {
        failures.push(Failure {
            rule: "time-date",
            pos: arg.pos().0 as u32,
            // Upstream ends with `: %s` filled by `astutils.GoFmt(arg)` — the
            // argument as written, not the parsed value — so a literal spelled
            // `0x19` prints as `0x19`.
            message: format!(
                "time.Date {} argument should be between {} and {}: {}",
                field_name(field),
                min,
                max,
                gofmt_arg(arg)
            ),
            ..Failure::default()
        });
    }
}

/// The argument as written. `checkArgSign` only ever reaches a basic literal or
/// a signed one, so this covers everything upstream can print here.
fn gofmt_arg(arg: &Expr) -> String {
    match arg {
        Expr::BasicLit(lit) => lit.value.clone(),
        Expr::UnaryExpr(u) => format!("{}{}", u.op.as_str(), gofmt_arg(&u.x)),
        _ => String::new(),
    }
}

fn field_name(field: TimeDateArg) -> &'static str {
    match field {
        TimeDateArg::Year => "year",
        TimeDateArg::Month => "month",
        TimeDateArg::Day => "day",
        TimeDateArg::Hour => "hour",
        TimeDateArg::Minute => "minute",
        TimeDateArg::Second => "second",
        TimeDateArg::Nanosecond => "nanosecond",
        TimeDateArg::Timezone => "timezone",
    }
}

fn check_arg_sign<'a>(
    arg: &'a Expr,
    field: TimeDateArg,
    failures: &mut Vec<Failure>,
) -> Option<&'a BasicLit> {
    if let Expr::BasicLit(bl) = arg {
        return Some(bl);
    }
    let Expr::UnaryExpr(unary) = arg else {
        return None;
    };
    let Expr::BasicLit(bl) = unparen(&unary.x) else {
        return None;
    };
    if field == TimeDateArg::Year && unary.op == Token::SUB {
        return Some(bl);
    }
    match unary.op {
        Token::SUB => failures.push(Failure {
            rule: "time-date",
            pos: arg.pos().0 as u32,
            message: format!(
                "time.Date {} argument is negative",
                field_name(field)
            ),
            ..Failure::default()
        }),
        Token::ADD => failures.push(Failure {
            rule: "time-date",
            pos: arg.pos().0 as u32,
            message: format!(
                "time.Date {} argument contains a useless plus sign",
                field_name(field)
            ),
            ..Failure::default()
        }),
        _ => {}
    }
    None
}

fn is_leap_year(year: i64) -> bool {
    // Match Go's time.Date normalization for Feb 29.
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 31,
    }
}

/// Why a `time.Date` argument is not a plain decimal integer. The text is the
/// `error` upstream constructs, which lands verbatim in the message.
#[derive(Clone, Copy, PartialEq)]
enum Notation {
    Octal,
    OctalWithZero,
    OctalWithPaddingZeroes,
    Hexadecimal,
    Binary,
    Float,
    Exponential,
    Alternative,
    /// Upstream only logs these and reports nothing.
    Invalid,
}

impl Notation {
    fn text(self) -> &'static str {
        match self {
            Notation::Octal => "octal notation",
            Notation::OctalWithZero => "octal notation with leading zero",
            Notation::OctalWithPaddingZeroes => "octal notation with padding zeroes",
            Notation::Hexadecimal => "hexadecimal notation",
            Notation::Binary => "binary notation",
            Notation::Float => "float literal",
            Notation::Exponential => "exponential notation",
            Notation::Alternative => "alternative notation",
            Notation::Invalid => "invalid notation",
        }
    }
}

fn report_notation(
    arg: &Expr,
    bl: &BasicLit,
    field: TimeDateArg,
    parsed: i64,
    notation: Notation,
    failures: &mut Vec<Failure>,
) {
    if notation == Notation::Invalid {
        return;
    }
    let replaced = parsed.to_string();
    let mut instructions = format!("use {replaced} instead of {}", gofmt_arg(arg));
    let confidence = match notation {
        // People may well write 00..07 on purpose.
        Notation::OctalWithZero => 0.5,
        // 000123456 — is that 123456 or 42798? A clear mistake either way.
        Notation::OctalWithPaddingZeroes => {
            let stripped = match bl.value.trim_start_matches('0') {
                "" => "0",
                s => s,
            };
            if stripped != replaced {
                instructions = format!(
                    "choose between {stripped} and {replaced} (decimal value of {stripped} octal value)"
                );
            }
            1.0
        }
        _ => 0.8,
    };
    failures.push(Failure::with_confidence(
        "time-date",
        arg.pos().0 as u32,
        format!(
            "use decimal digits for time.Date {} argument: {} found: {instructions}",
            field_name(field),
            notation.text(),
        ),
        confidence,
    ));
}

/// Port of upstream `parseDecimalInteger`: the value plus, when the literal is
/// not written as a plain decimal, which notation it used instead.
fn parse_decimal_integer(bl: &BasicLit) -> (i64, Option<Notation>) {
    let raw = bl.value.to_ascii_lowercase();
    if raw == "0" {
        return (0, None);
    }
    match bl.kind {
        Some(Token::FLOAT) => {
            let Ok(value) = raw.parse::<f64>() else {
                return (0, Some(Notation::Invalid));
            };
            let notation = if raw.contains('e') {
                Notation::Exponential
            } else {
                Notation::Float
            };
            return (value as i64, Some(notation));
        }
        Some(Token::INT) => {}
        _ => return (0, Some(Notation::Invalid)),
    }

    // Upstream parses with base 0, which accepts every Go integer form.
    let Some(value) = parse_int_base0(&raw) else {
        return (0, Some(Notation::Invalid));
    };
    if raw.starts_with("0b") {
        return (value, Some(Notation::Binary));
    }
    if raw.starts_with("0x") {
        return (value, Some(Notation::Hexadecimal));
    }
    if raw.starts_with('0') {
        // Catches "0o" octal as well as the bare leading zero.
        if matches!(raw.as_str(), "00" | "01" | "02" | "03" | "04" | "05" | "06" | "07") {
            return (value, Some(Notation::OctalWithZero));
        }
        if raw.starts_with("00") {
            return (value, Some(Notation::OctalWithPaddingZeroes));
        }
        return (value, Some(Notation::Octal));
    }
    // Round-trips through decimal? If not it was written some other way (1_0).
    if value.to_string() != raw {
        return (value, Some(Notation::Alternative));
    }
    (value, None)
}

/// `strconv.ParseInt(s, 0, 64)` — base inferred from the prefix, `_` allowed.
fn parse_int_base0(s: &str) -> Option<i64> {
    let t: String = s.chars().filter(|c| *c != '_').collect();
    if let Some(rest) = t.strip_prefix("0x") {
        i64::from_str_radix(rest, 16).ok()
    } else if let Some(rest) = t.strip_prefix("0b") {
        i64::from_str_radix(rest, 2).ok()
    } else if let Some(rest) = t.strip_prefix("0o") {
        i64::from_str_radix(rest, 8).ok()
    } else if t.len() > 1 && t.starts_with('0') {
        i64::from_str_radix(&t[1..], 8).ok()
    } else {
        t.parse::<i64>().ok()
    }
}
