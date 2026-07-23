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
            confidence: None,
        });
    }

    let mut year = 0i64;
    let mut month = 0i64;

    for (pos, arg) in call.args[..TIME_DATE_ARITY - 1].iter().enumerate() {
        let field = TIME_DATE_ARGS[pos];
        let Some(bl) = check_arg_sign(arg, field, failures) else {
            continue;
        };
        let Ok(parsed) = parse_decimal_integer(bl) else {
            continue;
        };

        match field {
            TimeDateArg::Year => year = parsed,
            TimeDateArg::Month => {
                month = parsed;
                if parsed == 0 {
                    failures.push(Failure {
                        rule: "time-date",
                        pos: arg.pos().0 as u32,
                        message: "time.Date month argument should not be zero".into(),
            confidence: None,
        });
                } else if !(1..=12).contains(&parsed) {
                    failures.push(Failure {
                        rule: "time-date",
                        pos: arg.pos().0 as u32,
                        message: format!(
                            "time.Date month argument should be between 1 and 12, got {parsed}"
                        ),
            confidence: None,
        });
                }
            }
            TimeDateArg::Day => {
                if parsed == 0 {
                    failures.push(Failure {
                        rule: "time-date",
                        pos: arg.pos().0 as u32,
                        message: "time.Date day argument should not be zero".into(),
            confidence: None,
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
            confidence: None,
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
            message: format!(
                "time.Date {} argument should be between {} and {}, got {}",
                field_name(field),
                min,
                max,
                parsed
            ),
            confidence: None,
        });
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
            confidence: None,
        }),
        Token::ADD => failures.push(Failure {
            rule: "time-date",
            pos: arg.pos().0 as u32,
            message: format!(
                "time.Date {} argument contains a useless plus sign",
                field_name(field)
            ),
            confidence: None,
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

fn parse_decimal_integer(bl: &BasicLit) -> Result<i64, ()> {
    let raw = bl.value.as_str();
    if raw == "0" {
        return Ok(0);
    }
    if bl.kind == Some(Token::FLOAT) {
        return Err(());
    }
    if bl.kind != Some(Token::INT) {
        return Err(());
    }
    if raw.starts_with("0x") || raw.starts_with("0b") || raw.starts_with("0o") {
        return Err(());
    }
    if raw.starts_with('0') && raw.len() > 1 {
        return Err(());
    }
    raw.parse::<i64>().map_err(|_| ())
}
