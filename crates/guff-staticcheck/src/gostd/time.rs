//! Port of Go's `time` layout parser (`src/time/format.go`), error paths only.
//!
//! [`parse`] mirrors `time.parse` closely enough to reproduce `*time.ParseError`
//! byte-for-byte; the success path discards the parsed instant, since SA1002
//! only ever looks at the error. Everything downstream of the last error return
//! in `time.parse` (zone lookup, `Date`) is therefore omitted.
//!
//! The port works on bytes, not `char`s, because Go indexes layout and value by
//! byte and `quote` hex-escapes anything outside printable ASCII one byte at a
//! time.

use std::fmt;

/// Go's `*time.ParseError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    layout: Vec<u8>,
    value: Vec<u8>,
    layout_elem: Vec<u8>,
    value_elem: Vec<u8>,
    message: String,
}

impl fmt::Display for ParseError {
    /// Mirrors `(*ParseError).Error`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            write!(
                f,
                "parsing time {} as {}: cannot parse {} as {}",
                quote(&self.value),
                quote(&self.layout),
                quote(&self.value_elem),
                quote(&self.layout_elem),
            )
        } else {
            write!(f, "parsing time {}{}", quote(&self.value), self.message)
        }
    }
}

/// The `errBad` placeholder — never rendered, only tested for.
struct ErrBad;

type BadResult<T> = Result<T, ErrBad>;

/// `time.Parse(layout, value)`, reduced to its error.
///
/// `Ok(())` means Go would have returned a `Time`.
pub fn parse(layout: &str, value: &str) -> Result<(), ParseError> {
    parse_bytes(layout.as_bytes(), value.as_bytes())
}

/// [`parse`] over raw bytes, for layouts that are not valid UTF-8.
pub fn parse_bytes(layout: &[u8], value: &[u8]) -> Result<(), ParseError> {
    parse_impl(layout, value)
}

const HEX: &[u8; 16] = b"0123456789abcdef";
const RUNE_SELF: u32 = 0x80;
const RUNE_ERROR: u32 = 0xFFFD;

/// Mirrors `time.quote`.
fn quote(s: &[u8]) -> String {
    let mut buf = String::with_capacity(s.len() + 2);
    buf.push('"');
    let mut i = 0;
    while i < s.len() {
        let (c, size) = decode_rune(&s[i..]);
        if c >= RUNE_SELF || c < u32::from(b' ') {
            // Unprintable or non-ASCII: hex-escape every byte of the rune.
            let width = if c == RUNE_ERROR {
                if i + 2 < s.len() && &s[i..i + 3] == "\u{FFFD}".as_bytes() {
                    3
                } else {
                    1
                }
            } else {
                size
            };
            for j in 0..width {
                buf.push_str("\\x");
                buf.push(HEX[usize::from(s[i + j] >> 4)] as char);
                buf.push(HEX[usize::from(s[i + j] & 0xF)] as char);
            }
        } else {
            if c == u32::from(b'"') || c == u32::from(b'\\') {
                buf.push('\\');
            }
            buf.push(c as u8 as char);
        }
        i += size;
    }
    buf.push('"');
    buf
}

/// `for i, c := range s` semantics: invalid UTF-8 decodes to `RuneError` with
/// width 1. `s` is never empty.
fn decode_rune(s: &[u8]) -> (u32, usize) {
    // A rune is at most four bytes, so a four-byte window always contains the
    // first one whole unless it is itself malformed.
    let head = &s[..s.len().min(4)];
    let prefix = match std::str::from_utf8(head) {
        Ok(valid) => valid,
        Err(err) if err.valid_up_to() > 0 => {
            // Safe: `valid_up_to` is by definition a valid UTF-8 boundary.
            std::str::from_utf8(&head[..err.valid_up_to()]).unwrap_or("")
        }
        Err(_) => "",
    };
    match prefix.chars().next() {
        Some(c) => (c as u32, c.len_utf8()),
        None => (RUNE_ERROR, 1),
    }
}

/// A layout element. Mirrors the `std*` constants; the fractional-second digit
/// count that Go packs into the high bits of `std` is a field instead.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Std {
    LongMonth,
    Month,
    NumMonth,
    ZeroMonth,
    LongWeekDay,
    WeekDay,
    Day,
    UnderDay,
    ZeroDay,
    UnderYearDay,
    ZeroYearDay,
    Hour,
    Hour12,
    ZeroHour12,
    Minute,
    ZeroMinute,
    Second,
    ZeroSecond,
    LongYear,
    Year,
    Pm,
    LowerPm,
    Tz,
    Iso8601Tz,
    Iso8601SecondsTz,
    Iso8601ShortTz,
    Iso8601ColonTz,
    Iso8601ColonSecondsTz,
    NumTz,
    NumSecondsTz,
    NumShortTz,
    NumColonTz,
    NumColonSecondsTz,
    FracSecond0 { digits: usize },
    FracSecond9 { digits: usize },
}

impl Std {
    fn is_frac_second(self) -> bool {
        matches!(self, Std::FracSecond0 { .. } | Std::FracSecond9 { .. })
    }
}

/// `nextStdChunk`'s result: `layout[..prefix]`, the element, `layout[suffix..]`.
struct Chunk {
    prefix: usize,
    std: Option<Std>,
    suffix: usize,
}

/// `std0x` — the elements for `"01"`..`"06"`.
const STD_0X: [Std; 6] = [
    Std::ZeroMonth,
    Std::ZeroDay,
    Std::ZeroHour12,
    Std::ZeroMinute,
    Std::ZeroSecond,
    Std::Year,
];

fn starts_with_lower_case(s: &[u8]) -> bool {
    matches!(s.first(), Some(c) if c.is_ascii_lowercase())
}

fn is_digit(s: &[u8], i: usize) -> bool {
    matches!(s.get(i), Some(c) if c.is_ascii_digit())
}

/// Mirrors `nextStdChunk`.
fn next_std_chunk(layout: &[u8]) -> Chunk {
    let n = layout.len();
    let found = |prefix: usize, std: Std, suffix: usize| Chunk {
        prefix,
        std: Some(std),
        suffix,
    };
    for i in 0..n {
        match layout[i] {
            b'J' => {
                // January, Jan
                if n >= i + 3 && &layout[i..i + 3] == b"Jan" {
                    if n >= i + 7 && &layout[i..i + 7] == b"January" {
                        return found(i, Std::LongMonth, i + 7);
                    }
                    if !starts_with_lower_case(&layout[i + 3..]) {
                        return found(i, Std::Month, i + 3);
                    }
                }
            }
            b'M' => {
                // Monday, Mon, MST
                if n >= i + 3 {
                    if &layout[i..i + 3] == b"Mon" {
                        if n >= i + 6 && &layout[i..i + 6] == b"Monday" {
                            return found(i, Std::LongWeekDay, i + 6);
                        }
                        if !starts_with_lower_case(&layout[i + 3..]) {
                            return found(i, Std::WeekDay, i + 3);
                        }
                    }
                    if &layout[i..i + 3] == b"MST" {
                        return found(i, Std::Tz, i + 3);
                    }
                }
            }
            b'0' => {
                // 01, 02, 03, 04, 05, 06, 002
                if n >= i + 2 && (b'1'..=b'6').contains(&layout[i + 1]) {
                    return found(i, STD_0X[usize::from(layout[i + 1] - b'1')], i + 2);
                }
                if n >= i + 3 && layout[i + 1] == b'0' && layout[i + 2] == b'2' {
                    return found(i, Std::ZeroYearDay, i + 3);
                }
            }
            b'1' => {
                // 15, 1
                if n >= i + 2 && layout[i + 1] == b'5' {
                    return found(i, Std::Hour, i + 2);
                }
                return found(i, Std::NumMonth, i + 1);
            }
            b'2' => {
                // 2006, 2
                if n >= i + 4 && &layout[i..i + 4] == b"2006" {
                    return found(i, Std::LongYear, i + 4);
                }
                return found(i, Std::Day, i + 1);
            }
            b'_' => {
                // _2, _2006, __2
                if n >= i + 2 && layout[i + 1] == b'2' {
                    // `_2006` is a literal `_` followed by stdLongYear.
                    if n >= i + 5 && &layout[i + 1..i + 5] == b"2006" {
                        return found(i + 1, Std::LongYear, i + 5);
                    }
                    return found(i, Std::UnderDay, i + 2);
                }
                if n >= i + 3 && layout[i + 1] == b'_' && layout[i + 2] == b'2' {
                    return found(i, Std::UnderYearDay, i + 3);
                }
            }
            b'3' => return found(i, Std::Hour12, i + 1),
            b'4' => return found(i, Std::Minute, i + 1),
            b'5' => return found(i, Std::Second, i + 1),
            b'P' => {
                if n >= i + 2 && layout[i + 1] == b'M' {
                    return found(i, Std::Pm, i + 2);
                }
            }
            b'p' => {
                if n >= i + 2 && layout[i + 1] == b'm' {
                    return found(i, Std::LowerPm, i + 2);
                }
            }
            b'-' => {
                // -070000, -07:00:00, -0700, -07:00, -07
                if n >= i + 7 && &layout[i..i + 7] == b"-070000" {
                    return found(i, Std::NumSecondsTz, i + 7);
                }
                if n >= i + 9 && &layout[i..i + 9] == b"-07:00:00" {
                    return found(i, Std::NumColonSecondsTz, i + 9);
                }
                if n >= i + 5 && &layout[i..i + 5] == b"-0700" {
                    return found(i, Std::NumTz, i + 5);
                }
                if n >= i + 6 && &layout[i..i + 6] == b"-07:00" {
                    return found(i, Std::NumColonTz, i + 6);
                }
                if n >= i + 3 && &layout[i..i + 3] == b"-07" {
                    return found(i, Std::NumShortTz, i + 3);
                }
            }
            b'Z' => {
                // Z070000, Z07:00:00, Z0700, Z07:00, Z07
                if n >= i + 7 && &layout[i..i + 7] == b"Z070000" {
                    return found(i, Std::Iso8601SecondsTz, i + 7);
                }
                if n >= i + 9 && &layout[i..i + 9] == b"Z07:00:00" {
                    return found(i, Std::Iso8601ColonSecondsTz, i + 9);
                }
                if n >= i + 5 && &layout[i..i + 5] == b"Z0700" {
                    return found(i, Std::Iso8601Tz, i + 5);
                }
                if n >= i + 6 && &layout[i..i + 6] == b"Z07:00" {
                    return found(i, Std::Iso8601ColonTz, i + 6);
                }
                if n >= i + 3 && &layout[i..i + 3] == b"Z07" {
                    return found(i, Std::Iso8601ShortTz, i + 3);
                }
            }
            b'.' | b',' => {
                // ,000 / .000 / ,999 / .999 — repeated digits for fractions.
                if i + 1 < n && (layout[i + 1] == b'0' || layout[i + 1] == b'9') {
                    let ch = layout[i + 1];
                    let mut j = i + 1;
                    while j < n && layout[j] == ch {
                        j += 1;
                    }
                    // The digit run must end here; only a fraction is all digits.
                    if !is_digit(layout, j) {
                        let digits = j - (i + 1);
                        let std = if ch == b'9' {
                            Std::FracSecond9 { digits }
                        } else {
                            Std::FracSecond0 { digits }
                        };
                        // Go packs the `,` vs `.` separator into `std` too; it
                        // only affects formatting, so it is dropped here.
                        return found(i, std, j);
                    }
                }
            }
            _ => {}
        }
    }
    Chunk {
        prefix: n,
        std: None,
        suffix: n,
    }
}

const LONG_DAY_NAMES: [&[u8]; 7] = [
    b"Sunday",
    b"Monday",
    b"Tuesday",
    b"Wednesday",
    b"Thursday",
    b"Friday",
    b"Saturday",
];

const SHORT_DAY_NAMES: [&[u8]; 7] = [b"Sun", b"Mon", b"Tue", b"Wed", b"Thu", b"Fri", b"Sat"];

const SHORT_MONTH_NAMES: [&[u8]; 12] = [
    b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov", b"Dec",
];

const LONG_MONTH_NAMES: [&[u8]; 12] = [
    b"January",
    b"February",
    b"March",
    b"April",
    b"May",
    b"June",
    b"July",
    b"August",
    b"September",
    b"October",
    b"November",
    b"December",
];

/// Case-insensitive compare of equal-length byte strings (`time.match`).
fn match_fold(s1: &[u8], s2: &[u8]) -> bool {
    for (&a, &b) in s1.iter().zip(s2) {
        if a != b {
            let c1 = a | (b'a' - b'A');
            let c2 = b | (b'a' - b'A');
            if c1 != c2 || !(b'a'..=b'z').contains(&c1) {
                return false;
            }
        }
    }
    true
}

/// Mirrors `time.lookup`: index into `tab`, plus the unconsumed remainder.
fn lookup<'v>(tab: &[&[u8]], val: &'v [u8]) -> BadResult<(i64, &'v [u8])> {
    for (i, v) in tab.iter().enumerate() {
        if val.len() >= v.len() && match_fold(&val[..v.len()], v) {
            return Ok((i as i64, &val[v.len()..]));
        }
    }
    Err(ErrBad)
}

/// Mirrors `time.leadingInt`.
fn leading_int(s: &[u8]) -> Result<(u64, &[u8]), ()> {
    let mut x: u64 = 0;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if !c.is_ascii_digit() {
            break;
        }
        if x > (1u64 << 63) / 10 {
            return Err(());
        }
        x = x * 10 + u64::from(c - b'0');
        if x > 1u64 << 63 {
            return Err(());
        }
        i += 1;
    }
    Ok((x, &s[i..]))
}

/// Mirrors `time.atoi`.
fn atoi(s: &[u8]) -> BadResult<i64> {
    let mut s = s;
    let mut neg = false;
    if let Some(&c) = s.first() {
        if c == b'-' || c == b'+' {
            neg = c == b'-';
            s = &s[1..];
        }
    }
    let (q, rem) = leading_int(s).map_err(|()| ErrBad)?;
    if !rem.is_empty() {
        return Err(ErrBad);
    }
    // Go's `int(q)` truncates rather than trapping, and so does `as`.
    let x = q as i64;
    Ok(if neg { x.wrapping_neg() } else { x })
}

/// Mirrors `time.getnum`: one digit, or two when `fixed`.
fn getnum(s: &[u8], fixed: bool) -> BadResult<(i64, &[u8])> {
    if !is_digit(s, 0) {
        return Err(ErrBad);
    }
    if !is_digit(s, 1) {
        if fixed {
            return Err(ErrBad);
        }
        return Ok((i64::from(s[0] - b'0'), &s[1..]));
    }
    Ok((
        i64::from(s[0] - b'0') * 10 + i64::from(s[1] - b'0'),
        &s[2..],
    ))
}

/// Mirrors `time.getnum3`: up to three digits, or exactly three when `fixed`.
fn getnum3(s: &[u8], fixed: bool) -> BadResult<(i64, &[u8])> {
    let mut n = 0i64;
    let mut i = 0;
    while i < 3 && is_digit(s, i) {
        n = n * 10 + i64::from(s[i] - b'0');
        i += 1;
    }
    if i == 0 || (fixed && i != 3) {
        return Err(ErrBad);
    }
    Ok((n, &s[i..]))
}

fn cutspace(s: &[u8]) -> &[u8] {
    let mut s = s;
    while matches!(s.first(), Some(b' ')) {
        s = &s[1..];
    }
    s
}

/// Mirrors `time.skip`: strip `prefix` from `value`, runs of spaces equivalent.
///
/// Returns the remainder in both arms, as Go's multi-assign does.
fn skip<'v>(value: &'v [u8], prefix: &[u8]) -> (&'v [u8], Option<ErrBad>) {
    let mut value = value;
    let mut prefix = prefix;
    while !prefix.is_empty() {
        if prefix[0] == b' ' {
            if !value.is_empty() && value[0] != b' ' {
                return (value, Some(ErrBad));
            }
            prefix = cutspace(prefix);
            value = cutspace(value);
            continue;
        }
        if value.is_empty() || value[0] != prefix[0] {
            return (value, Some(ErrBad));
        }
        prefix = &prefix[1..];
        value = &value[1..];
    }
    (value, None)
}

fn comma_or_period(b: u8) -> bool {
    b == b'.' || b == b','
}

/// Mirrors `time.parseNanoseconds`.
fn parse_nanoseconds(value: &[u8], nbytes: usize) -> (i64, &'static str, Option<ErrBad>) {
    if !comma_or_period(value[0]) {
        return (0, "", Some(ErrBad));
    }
    let (value, nbytes) = if nbytes > 10 {
        (&value[..10], 10)
    } else {
        (value, nbytes)
    };
    let ns = match atoi(&value[1..nbytes]) {
        Ok(ns) => ns,
        Err(e) => return (0, "", Some(e)),
    };
    if ns < 0 {
        return (0, "fractional second", None);
    }
    let mut ns = ns;
    for _ in 0..(10 - nbytes) {
        ns *= 10;
    }
    (ns, "", None)
}

/// Mirrors `time.parseSignedOffset`.
fn parse_signed_offset(value: &[u8]) -> usize {
    let sign = value[0];
    if sign != b'-' && sign != b'+' {
        return 0;
    }
    let Ok((x, rem)) = leading_int(&value[1..]) else {
        return 0;
    };
    if rem.len() == value.len() - 1 {
        // leadingInt consumed nothing.
        return 0;
    }
    if x > 23 {
        return 0;
    }
    value.len() - rem.len()
}

/// Mirrors `time.parseGMT`.
fn parse_gmt(value: &[u8]) -> usize {
    let value = &value[3..];
    if value.is_empty() {
        return 3;
    }
    3 + parse_signed_offset(value)
}

/// Mirrors `time.parseTimeZone`.
fn parse_time_zone(value: &[u8]) -> Option<usize> {
    if value.len() < 3 {
        return None;
    }
    // ChST and MeST are the only zones with a lower-case letter.
    if value.len() >= 4 && (&value[..4] == b"ChST" || &value[..4] == b"MeST") {
        return Some(4);
    }
    // GMT may carry an hour offset.
    if &value[..3] == b"GMT" {
        return Some(parse_gmt(value));
    }
    // Unnamed zones in +/-00 form.
    if value[0] == b'+' || value[0] == b'-' {
        let length = parse_signed_offset(value);
        return if length > 0 { Some(length) } else { None };
    }
    let mut n_upper = 0;
    while n_upper < 6 {
        match value.get(n_upper) {
            Some(c) if c.is_ascii_uppercase() => n_upper += 1,
            _ => break,
        }
    }
    match n_upper {
        5 if value[4] == b'T' => Some(5),
        4 if value[3] == b'T' || &value[..4] == b"WITA" => Some(4),
        3 => Some(3),
        _ => None,
    }
}

fn is_leap(year: i64) -> bool {
    let mask = if year % 25 != 0 { 3 } else { 0xf };
    year & mask == 0
}

/// Mirrors `time.daysBefore` (month is 1-based).
fn days_before(m: i64) -> i64 {
    let adj = if m >= 3 { -2 } else { 0 };
    (214 * m - 211) / 7 + adj
}

/// Mirrors `time.daysIn`.
fn days_in(m: i64, year: i64) -> i64 {
    if m == 2 {
        return if is_leap(year) { 29 } else { 28 };
    }
    30 + ((m + (m >> 3)) & 1)
}

/// Mirrors `time.parse`, minus everything after the final error return.
fn parse_impl(layout_in: &[u8], value_in: &[u8]) -> Result<(), ParseError> {
    let alayout = layout_in;
    let avalue = value_in;
    let mut layout = layout_in;
    let mut value = value_in;

    let bad = |layout_elem: &[u8], value_elem: &[u8], message: &str| ParseError {
        layout: alayout.to_vec(),
        value: avalue.to_vec(),
        layout_elem: layout_elem.to_vec(),
        value_elem: value_elem.to_vec(),
        message: message.to_string(),
    };

    let mut range_err = "";
    let mut year = 0i64;
    let mut month = -1i64;
    let mut day = -1i64;
    let mut yday = -1i64;
    let mut hour = 0i64;
    let mut pm_set = false;
    let mut am_set = false;

    loop {
        let mut err: Option<ErrBad> = None;
        let chunk = next_std_chunk(layout);
        let prefix = &layout[..chunk.prefix];
        let stdstr = layout[chunk.prefix..chunk.suffix].to_vec();
        let suffix = &layout[chunk.suffix..];
        let (rest, skip_err) = skip(value, prefix);
        value = rest;
        if skip_err.is_some() {
            return Err(bad(prefix, value, ""));
        }
        let Some(std) = chunk.std else {
            if !value.is_empty() {
                let msg = format!(": extra text: {}", quote(value));
                return Err(bad(b"", value, &msg));
            }
            break;
        };
        layout = suffix;
        let hold = value;
        match std {
            Std::Year => {
                if value.len() < 2 {
                    err = Some(ErrBad);
                } else {
                    let (p, rest) = value.split_at(2);
                    value = rest;
                    match atoi(p) {
                        Ok(y) => year = if y >= 69 { y + 1900 } else { y + 2000 },
                        Err(e) => err = Some(e),
                    }
                }
            }
            Std::LongYear => {
                if value.len() < 4 || !is_digit(value, 0) {
                    err = Some(ErrBad);
                } else {
                    let (p, rest) = value.split_at(4);
                    value = rest;
                    match atoi(p) {
                        Ok(y) => year = y,
                        Err(e) => err = Some(e),
                    }
                }
            }
            Std::Month => match lookup(&SHORT_MONTH_NAMES, value) {
                Ok((m, rest)) => {
                    month = m + 1;
                    value = rest;
                }
                Err(e) => {
                    month = 0;
                    err = Some(e);
                }
            },
            Std::LongMonth => match lookup(&LONG_MONTH_NAMES, value) {
                Ok((m, rest)) => {
                    month = m + 1;
                    value = rest;
                }
                Err(e) => {
                    month = 0;
                    err = Some(e);
                }
            },
            Std::NumMonth | Std::ZeroMonth => match getnum(value, std == Std::ZeroMonth) {
                Ok((m, rest)) => {
                    month = m;
                    value = rest;
                    if month <= 0 || 12 < month {
                        range_err = "month";
                    }
                }
                Err(e) => err = Some(e),
            },
            Std::WeekDay => match lookup(&SHORT_DAY_NAMES, value) {
                Ok((_, rest)) => value = rest,
                Err(e) => err = Some(e),
            },
            Std::LongWeekDay => match lookup(&LONG_DAY_NAMES, value) {
                Ok((_, rest)) => value = rest,
                Err(e) => err = Some(e),
            },
            Std::Day | Std::UnderDay | Std::ZeroDay => {
                if std == Std::UnderDay && matches!(value.first(), Some(b' ')) {
                    value = &value[1..];
                }
                // Any one- or two-digit day is accepted here; the month/day/year
                // combination is validated after the loop.
                match getnum(value, std == Std::ZeroDay) {
                    Ok((d, rest)) => {
                        day = d;
                        value = rest;
                    }
                    Err(e) => err = Some(e),
                }
            }
            Std::UnderYearDay | Std::ZeroYearDay => {
                for _ in 0..2 {
                    if std == Std::UnderYearDay && matches!(value.first(), Some(b' ')) {
                        value = &value[1..];
                    }
                }
                match getnum3(value, std == Std::ZeroYearDay) {
                    Ok((d, rest)) => {
                        yday = d;
                        value = rest;
                    }
                    Err(e) => err = Some(e),
                }
            }
            Std::Hour => match getnum(value, false) {
                Ok((h, rest)) => {
                    hour = h;
                    value = rest;
                    if !(0..24).contains(&hour) {
                        range_err = "hour";
                    }
                }
                Err(e) => {
                    hour = 0;
                    err = Some(e);
                }
            },
            Std::Hour12 | Std::ZeroHour12 => match getnum(value, std == Std::ZeroHour12) {
                Ok((h, rest)) => {
                    hour = h;
                    value = rest;
                    if hour < 0 || 12 < hour {
                        range_err = "hour";
                    }
                }
                Err(e) => {
                    hour = 0;
                    err = Some(e);
                }
            },
            Std::Minute | Std::ZeroMinute => match getnum(value, std == Std::ZeroMinute) {
                Ok((m, rest)) => {
                    value = rest;
                    if !(0..60).contains(&m) {
                        range_err = "minute";
                    }
                }
                Err(e) => err = Some(e),
            },
            Std::Second | Std::ZeroSecond => match getnum(value, std == Std::ZeroSecond) {
                Err(e) => err = Some(e),
                Ok((sec, rest)) => {
                    value = rest;
                    if !(0..60).contains(&sec) {
                        range_err = "second";
                    } else if value.len() >= 2
                        && comma_or_period(value[0])
                        && is_digit(value, 1)
                        && !next_std_chunk(layout)
                            .std
                            .is_some_and(Std::is_frac_second)
                    {
                        // A fraction in the input that the layout does not name.
                        let mut n = 2;
                        while n < value.len() && is_digit(value, n) {
                            n += 1;
                        }
                        let (_, range, e) = parse_nanoseconds(value, n);
                        range_err = range;
                        err = e;
                        value = &value[n..];
                    }
                }
            },
            Std::Pm | Std::LowerPm => {
                let (upper, lower) = if std == Std::Pm {
                    (b"PM".as_slice(), b"AM".as_slice())
                } else {
                    (b"pm".as_slice(), b"am".as_slice())
                };
                if value.len() < 2 {
                    err = Some(ErrBad);
                } else {
                    let (p, rest) = value.split_at(2);
                    value = rest;
                    if p == upper {
                        pm_set = true;
                    } else if p == lower {
                        am_set = true;
                    } else {
                        err = Some(ErrBad);
                    }
                }
            }
            Std::Iso8601Tz
            | Std::Iso8601ShortTz
            | Std::Iso8601ColonTz
            | Std::Iso8601SecondsTz
            | Std::Iso8601ColonSecondsTz
            | Std::NumTz
            | Std::NumShortTz
            | Std::NumColonTz
            | Std::NumSecondsTz
            | Std::NumColonSecondsTz => {
                let iso = matches!(
                    std,
                    Std::Iso8601Tz
                        | Std::Iso8601ShortTz
                        | Std::Iso8601ColonTz
                        | Std::Iso8601SecondsTz
                        | Std::Iso8601ColonSecondsTz
                );
                if iso && matches!(value.first(), Some(b'Z')) {
                    // Zulu: the zone is UTC and nothing else is consumed.
                    value = &value[1..];
                } else {
                    err = parse_numeric_zone(std, &mut value, &mut range_err);
                }
            }
            Std::Tz => {
                if value.len() >= 3 && &value[..3] == b"UTC" {
                    value = &value[3..];
                } else if let Some(n) = parse_time_zone(value) {
                    value = &value[n..];
                } else {
                    err = Some(ErrBad);
                }
            }
            Std::FracSecond0 { digits } => {
                // The exact digit count the layout asked for is required.
                let ndigit = 1 + digits;
                if value.len() < ndigit {
                    err = Some(ErrBad);
                } else {
                    let (_, range, e) = parse_nanoseconds(value, ndigit);
                    range_err = range;
                    err = e;
                    value = &value[ndigit..];
                }
            }
            Std::FracSecond9 { .. } => {
                if value.len() >= 2
                    && comma_or_period(value[0])
                    && (b'0'..=b'9').contains(&value[1])
                {
                    // Take any number of digits, even more than asked for, as
                    // the stdSecond case would.
                    let mut i = 0;
                    while i + 1 < value.len() && value[i + 1].is_ascii_digit() {
                        i += 1;
                    }
                    let (_, range, e) = parse_nanoseconds(value, 1 + i);
                    range_err = range;
                    err = e;
                    value = &value[1 + i..];
                }
            }
        }
        if !range_err.is_empty() {
            let msg = format!(": {range_err} out of range");
            return Err(bad(&stdstr, value, &msg));
        }
        if err.is_some() {
            return Err(bad(&stdstr, hold, ""));
        }
    }

    if pm_set && hour < 12 {
        hour += 12;
    } else if am_set && hour == 12 {
        hour = 0;
    }
    let _ = hour;

    if yday >= 0 {
        let mut d = 0i64;
        let mut m = 0i64;
        let mut yday = yday;
        if is_leap(year) {
            if yday == 31 + 29 {
                m = 2;
                d = 29;
            } else if yday > 31 + 29 {
                yday -= 1;
            }
        }
        if !(1..=365).contains(&yday) {
            return Err(bad(b"", value, ": day-of-year out of range"));
        }
        if m == 0 {
            m = (yday - 1) / 31 + 1;
            if days_before(m + 1) < yday {
                m += 1;
            }
            d = yday - days_before(m);
        }
        if month >= 0 && month != m {
            return Err(bad(b"", value, ": day-of-year does not match month"));
        }
        month = m;
        if day >= 0 && day != d {
            return Err(bad(b"", value, ": day-of-year does not match day"));
        }
        day = d;
    } else {
        if month < 0 {
            month = 1;
        }
        if day < 0 {
            day = 1;
        }
    }

    if day < 1 || day > days_in(month, year) {
        return Err(bad(b"", value, ": day out of range"));
    }

    Ok(())
}

/// The `stdNumTZ` family of the `parse` switch, split out for readability.
fn parse_numeric_zone(std: Std, value: &mut &[u8], range_err: &mut &'static str) -> Option<ErrBad> {
    let v = *value;
    let (sign, hour, min, seconds): (&[u8], &[u8], &[u8], &[u8]);
    match std {
        Std::Iso8601ColonTz | Std::NumColonTz => {
            if v.len() < 6 {
                return Some(ErrBad);
            }
            if v[3] != b':' {
                return Some(ErrBad);
            }
            (sign, hour, min, seconds) = (&v[0..1], &v[1..3], &v[4..6], b"00");
            *value = &v[6..];
        }
        Std::NumShortTz | Std::Iso8601ShortTz => {
            if v.len() < 3 {
                return Some(ErrBad);
            }
            (sign, hour, min, seconds) = (&v[0..1], &v[1..3], b"00", b"00");
            *value = &v[3..];
        }
        Std::Iso8601ColonSecondsTz | Std::NumColonSecondsTz => {
            if v.len() < 9 {
                return Some(ErrBad);
            }
            if v[3] != b':' || v[6] != b':' {
                return Some(ErrBad);
            }
            (sign, hour, min, seconds) = (&v[0..1], &v[1..3], &v[4..6], &v[7..9]);
            *value = &v[9..];
        }
        Std::Iso8601SecondsTz | Std::NumSecondsTz => {
            if v.len() < 7 {
                return Some(ErrBad);
            }
            (sign, hour, min, seconds) = (&v[0..1], &v[1..3], &v[3..5], &v[5..7]);
            *value = &v[7..];
        }
        _ => {
            if v.len() < 5 {
                return Some(ErrBad);
            }
            (sign, hour, min, seconds) = (&v[0..1], &v[1..3], &v[3..5], b"00");
            *value = &v[5..];
        }
    }

    let mut err = None;
    let mut hr = 0i64;
    let mut mm = 0i64;
    let mut ss = 0i64;
    match getnum(hour, true) {
        Ok((v, _)) => hr = v,
        Err(e) => err = Some(e),
    }
    if err.is_none() {
        match getnum(min, true) {
            Ok((v, _)) => mm = v,
            Err(e) => err = Some(e),
        }
        if err.is_none() {
            match getnum(seconds, true) {
                Ok((v, _)) => ss = v,
                Err(e) => err = Some(e),
            }
        }
    }

    // The range tests use `>` rather than `>=`: offsets of 24 hours, or 60
    // minutes or seconds, are written in the wild.
    if hr > 24 {
        *range_err = "time zone offset hour";
    }
    if mm > 60 {
        *range_err = "time zone offset minute";
    }
    if ss > 60 {
        *range_err = "time zone offset second";
    }

    match sign[0] {
        b'+' | b'-' => {}
        _ => err = Some(ErrBad),
    }
    err
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every expectation here is `time.Parse`'s actual output, captured by
    /// running the same input through Go — never hand-derived.
    fn err_of(layout: &str, value: &str) -> Option<String> {
        parse(layout, value).err().map(|e| e.to_string())
    }

    fn self_err(layout: &str) -> Option<String> {
        err_of(layout, layout)
    }

    #[test]
    fn reference_layouts_parse_themselves() {
        // `time.Parse(s, s)` succeeds for these — SA1002's whole premise. Note
        // which reference layouts are *absent*: the ones containing `_2` or
        // `Z07:00` cannot parse their own text, which is why SA1002 rewrites
        // both before calling Parse.
        for layout in [
            "2006-01-02",
            "2006",
            "01/02 03:04:05PM '06 -0700",
            "Mon, 02 Jan 2006 15:04:05 MST",
            "Mon, 02 Jan 2006 15:04:05 -0700",
            "2006-01-02 15:04:05",
            "15:04:05",
            "3:04PM",
            "2006-01-02T15:04:05.999999999-07:00",
        ] {
            assert_eq!(self_err(layout), None, "layout {layout:?}");
        }
    }

    #[test]
    fn digits_only_layout_runs_out_of_value() {
        assert_eq!(
            self_err("12345"),
            Some(r#"parsing time "12345" as "12345": cannot parse "" as "4""#.to_string()),
        );
    }

    #[test]
    fn layout_without_std_chunks_is_not_an_error() {
        // No std chunk at all: the layout is a literal that skips itself clean.
        // Upstream stays silent here; the old heuristic reported it.
        assert_eq!(self_err("not-a-layout"), None);
        assert_eq!(self_err(""), None);
        assert_eq!(self_err("hello"), None);
    }

    #[test]
    fn out_of_range_fields_name_the_field() {
        assert_eq!(
            err_of("15:04", "25:04"),
            Some(r#"parsing time "25:04": hour out of range"#.to_string()),
        );
        assert_eq!(
            err_of("2006-01-02", "2006-13-02"),
            Some(r#"parsing time "2006-13-02": month out of range"#.to_string()),
        );
        assert_eq!(
            err_of("2006-01-02", "2021-02-30"),
            Some(r#"parsing time "2021-02-30": day out of range"#.to_string()),
        );
    }

    #[test]
    fn extra_text_is_quoted() {
        assert_eq!(
            err_of("2006", "2006xyz"),
            Some(r#"parsing time "2006xyz": extra text: "xyz""#.to_string()),
        );
    }

    #[test]
    fn quote_hex_escapes_non_ascii() {
        assert_eq!(quote(b"ok"), r#""ok""#);
        assert_eq!(quote("é".as_bytes()), r#""\xc3\xa9""#);
        assert_eq!(quote(b"a\"b\\c"), r#""a\"b\\c""#);
        assert_eq!(quote(b"\x01"), r#""\x01""#);
    }

    #[test]
    fn sa1002_substitutions_are_the_callers_job() {
        // SA1002 rewrites `_`→` ` and `Z`→`-` before calling Parse, because
        // neither element can parse the text it formats. Unsubstituted, both
        // fail — and this port must reproduce that, not paper over it.
        assert_eq!(
            self_err("Z07:00"),
            Some(r#"parsing time "Z07:00": extra text: "07:00""#.to_string()),
        );
        assert_eq!(
            self_err("_2"),
            Some(r#"parsing time "_2" as "_2": cannot parse "_2" as "_2""#.to_string()),
        );
        // After the substitutions they parse clean.
        assert_eq!(self_err("-07:00"), None);
        assert_eq!(self_err(" 2"), None);
    }
}
