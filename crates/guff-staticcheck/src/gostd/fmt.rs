//! Go's `fmt.Sscan` into a `complex128`, which is how `text/template` reads a
//! complex literal (`node.go`'s `newNumber` hands the token to `fmt.Sscan`).
//!
//! Only the scan half is ported: the value is discarded, but the errors are
//! not. `complexTokens` decides where the real part ends and the imaginary
//! part begins, and `convertFloat` turns each half over to `strconv`, so a
//! template carrying `{{0x1+2i}}` fails with `strconv.ParseFloat: parsing
//! "0x1": invalid syntax` and `{{0b1+1i}}` with `syntax error scanning complex
//! number` — neither of which a Rust complex parser would word the same way.

use super::strconv;

const DECIMAL_DIGITS: &str = "0123456789";
const HEXADECIMAL_DIGITS: &str = "0123456789aAbBcCdDeEfF";
const SIGN: &str = "+-";
const PERIOD: &str = ".";
const EXPONENT: &str = "eEpP";

const ERR_COMPLEX: &str = "syntax error scanning complex number";

struct Scanner<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Scanner<'a> {
    /// Mirrors `(*ss).consume` with accept=true: take the next rune if it is in
    /// `set`, appending it to `buf`.
    fn accept(&mut self, set: &str, buf: &mut String) -> bool {
        let Some(&c) = self.input.get(self.pos) else {
            return false;
        };
        // Every set here is ASCII, so a non-ASCII lead byte is never a member
        // and the position does not move.
        let ch = char::from(c);
        if c < 0x80 && set.contains(ch) {
            buf.push(ch);
            self.pos += 1;
            return true;
        }
        false
    }

    /// Mirrors `(*ss).floatToken`.
    fn float_token(&mut self) -> String {
        let mut buf = String::new();

        // NaN?
        if self.accept("nN", &mut buf) && self.accept("aA", &mut buf) && self.accept("nN", &mut buf)
        {
            return buf;
        }
        // Leading sign?
        self.accept(SIGN, &mut buf);
        // Inf?
        if self.accept("iI", &mut buf) && self.accept("nN", &mut buf) && self.accept("fF", &mut buf)
        {
            return buf;
        }

        let mut digits = format!("{DECIMAL_DIGITS}_");
        let mut exp = EXPONENT;
        if self.accept("0", &mut buf) && self.accept("xX", &mut buf) {
            digits = format!("{HEXADECIMAL_DIGITS}_");
            exp = "pP";
        }
        while self.accept(&digits, &mut buf) {}
        if self.accept(PERIOD, &mut buf) {
            while self.accept(&digits, &mut buf) {}
        }
        if self.accept(exp, &mut buf) {
            self.accept(SIGN, &mut buf);
            let decimal = format!("{DECIMAL_DIGITS}_");
            while self.accept(&decimal, &mut buf) {}
        }
        buf
    }
}

/// Mirrors `(*ss).convertFloat`: everything the scanner does with one half of
/// the pair once it has been tokenized.
fn convert_float(s: &str) -> Result<(), String> {
    // indexRune looks for a lowercase 'p' only; an uppercase one falls through
    // to ParseFloat, which rejects it outside a hex mantissa.
    if let Some(p) = s.find('p') {
        if !s.contains('x') && !s.contains('X') {
            // Go puts the *full* token into the error, not the slice it parsed.
            if let Err(e) = strconv::parse_float(&s[..p]) {
                return Err(strconv::num_error("ParseFloat", s, e));
            }
            if let Err(e) = strconv::parse_int(&s[p + 1..], 10, 64) {
                return Err(strconv::num_error("Atoi", s, e));
            }
            return Ok(());
        }
    }
    strconv::parse_float(s)
        .map(|_| ())
        .map_err(|e| strconv::num_error("ParseFloat", s, e))
}

/// Mirrors `fmt.Sscan(s, &complex128)`: reports the error Go would return, or
/// `Ok` if the pair scans.
pub fn sscan_complex(s: &str) -> Result<(), String> {
    let mut sc = Scanner {
        input: s.as_bytes(),
        pos: 0,
    };
    let mut parens_buf = String::new();
    let parens = sc.accept("(", &mut parens_buf);
    let real = sc.float_token();

    let mut sign_buf = String::new();
    if !sc.accept(SIGN, &mut sign_buf) {
        return Err(ERR_COMPLEX.to_string());
    }
    let imag = format!("{sign_buf}{}", sc.float_token());
    let mut discard = String::new();
    if !sc.accept("i", &mut discard) {
        return Err(ERR_COMPLEX.to_string());
    }
    if parens && !sc.accept(")", &mut discard) {
        return Err(ERR_COMPLEX.to_string());
    }

    convert_float(&real)?;
    convert_float(&imag)
}
