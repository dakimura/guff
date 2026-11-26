//! Port of the operations section of go/constant/value.go:
//! `UnaryOp`, `BinaryOp`, `Shift`, `Compare`.
//!
//! Original Go source:
//!   Copyright 2013 The Go Authors. All rights reserved.
//!   Use of this source code is governed by a BSD-style license.

use std::cmp::Ordering;

use dashu::integer::IBig;
use dashu::rational::RBig;
use guff::token::Token;

use crate::helpers::{i64_to_ibig, make_complex, make_float, make_int, make_rat, match_pair};
use crate::value::{BinFloat, Value};

// ----------------------------------------------------------------------------
// UnaryOp

/// Returns the result of the unary expression `op y`.
///
/// `prec`, if non-zero, specifies the result size in bits for the `XOR`
/// (bit-wise complement) operator, used to limit the result precision for
/// unsigned types. If `y` is [`Value::Unknown`], the result is `Unknown`.
///
/// Equivalent to `constant.UnaryOp`.
///
/// # Panics
/// Panics if `op` and `y` form an undefined unary operation.
pub fn unary_op(op: Token, y: Value, prec: usize) -> Value {
    match op {
        Token::ADD => match y {
            Value::Unknown
            | Value::Int64(_)
            | Value::Int(_)
            | Value::Rat(_)
            | Value::Float(_)
            | Value::Complex { .. } => return y,
            _ => {}
        },
        Token::SUB => match y {
            Value::Unknown => return Value::Unknown,
            Value::Int64(v) => {
                // overflow-safe negation: i64::MIN cannot be negated as i64.
                match v.checked_neg() {
                    Some(neg) => return Value::Int64(neg),
                    None => return make_int(-IBig::from(v)),
                }
            }
            Value::Int(v) => return make_int(-v),
            Value::Rat(v) => return make_rat(-v),
            Value::Float(v) => return make_float(-v),
            Value::Complex { re, im } => {
                let nre = unary_op(Token::SUB, *re, 0);
                let nim = unary_op(Token::SUB, *im, 0);
                return make_complex(nre, nim);
            }
            _ => {}
        },
        Token::XOR => {
            let z: IBig = match y {
                Value::Unknown => return Value::Unknown,
                Value::Int64(v) => !i64_to_ibig(v),
                Value::Int(v) => !v,
                _ => panic!("invalid unary operation {}{:?}", op.as_str(), y),
            };
            // For unsigned types of bit-width `prec`, mask the result.
            if prec > 0 {
                let mask = (IBig::ONE << prec) - IBig::ONE;
                return make_int(z & mask);
            }
            return make_int(z);
        }
        Token::NOT => match y {
            Value::Unknown => return Value::Unknown,
            Value::Bool(b) => return Value::Bool(!b),
            _ => {}
        },
        _ => {}
    }
    panic!("invalid unary operation {}{:?}", op.as_str(), y);
}

// ----------------------------------------------------------------------------
// BinaryOp

/// Returns the result of the binary expression `x op y`.
///
/// The operation must be defined for the operands; otherwise this panics.
/// If either operand is [`Value::Unknown`], the result is `Unknown`.
///
/// `BinaryOp` does **not** handle comparisons or shifts; use [`compare`] /
/// [`shift`] instead.
///
/// To force integer division of `Int` operands, use [`Token::QuoAssign`]
/// instead of [`Token::QUO`]; the result is guaranteed to be `Int` in this
/// case. Division by zero panics.
///
/// Equivalent to `constant.BinaryOp`.
///
/// # Panics
/// Panics if `op` is undefined for the (matched) operands.
pub fn binary_op(x_in: Value, op: Token, y_in: Value) -> Value {
    let (x, y) = match_pair(x_in.clone(), y_in.clone());
    match x {
        Value::Unknown => return Value::Unknown,
        Value::Bool(a) => {
            if let Value::Bool(b) = y {
                match op {
                    Token::LAND => return Value::Bool(a && b),
                    Token::LOR => return Value::Bool(a || b),
                    _ => {}
                }
            }
        }
        Value::Int64(a) => {
            if let Value::Int64(b) = y {
                return binary_op_i64(a, op, b);
            }
        }
        Value::Int(a) => {
            if let Value::Int(b) = y {
                return binary_op_ibig(a, op, b);
            }
        }
        Value::Rat(a) => {
            if let Value::Rat(b) = y {
                return binary_op_rbig(a, op, b);
            }
        }
        Value::Float(a) => {
            if let Value::Float(b) = y {
                return binary_op_fbig(a, op, b);
            }
        }
        Value::Complex { re: ax, im: ay } => {
            if let Value::Complex { re: bx, im: by } = y {
                return binary_op_complex(*ax, *ay, op, *bx, *by);
            }
        }
        Value::String(s) => {
            if let Value::String(t) = y {
                if op == Token::ADD {
                    let mut combined = s.to_vec();
                    combined.extend_from_slice(&t);
                    return crate::value::make_string_bytes(combined);
                }
            }
        }
    }
    panic!(
        "invalid binary operation {:?} {} {:?}",
        x_in,
        op.as_str(),
        y_in
    );
}

fn binary_op_i64(a: i64, op: Token, b: i64) -> Value {
    fn is_63bit(x: i64) -> bool {
        // -2^62 <= x <= 2^62 - 1 — i.e. fits in 63 signed bits.
        const LIM: i64 = 1 << 62;
        -LIM <= x && x < LIM
    }
    fn is_32bit(x: i64) -> bool {
        i32::try_from(x).is_ok()
    }
    let result: i64 = match op {
        Token::ADD => {
            if !is_63bit(a) || !is_63bit(b) {
                return make_int(IBig::from(a) + IBig::from(b));
            }
            a + b
        }
        Token::SUB => {
            if !is_63bit(a) || !is_63bit(b) {
                return make_int(IBig::from(a) - IBig::from(b));
            }
            a - b
        }
        Token::MUL => {
            if !is_32bit(a) || !is_32bit(b) {
                return make_int(IBig::from(a) * IBig::from(b));
            }
            a * b
        }
        Token::QUO => {
            // True (rational) division. Constant operations preserve full
            // precision — go to RBig directly.
            if b == 0 {
                panic!("division by zero");
            }
            return make_rat(RBig::from_parts_signed(IBig::from(a), IBig::from(b)));
        }
        Token::QuoAssign => {
            // Forced integer division.
            if b == 0 {
                panic!("division by zero");
            }
            a / b
        }
        Token::REM => {
            if b == 0 {
                panic!("division by zero");
            }
            a % b
        }
        Token::AND => a & b,
        Token::OR => a | b,
        Token::XOR => a ^ b,
        Token::AndNot => a & !b,
        _ => panic!("invalid Int binary operation {}", op.as_str()),
    };
    Value::Int64(result)
}

fn binary_op_ibig(a: IBig, op: Token, b: IBig) -> Value {
    let z = match op {
        Token::ADD => a + b,
        Token::SUB => a - b,
        Token::MUL => a * b,
        Token::QUO => {
            return make_rat(RBig::from_parts_signed(a, b));
        }
        Token::QuoAssign => a / b,
        Token::REM => a % b,
        Token::AND => a & b,
        Token::OR => a | b,
        Token::XOR => a ^ b,
        Token::AndNot => a & !b,
        _ => panic!("invalid Int binary operation {}", op.as_str()),
    };
    make_int(z)
}

fn binary_op_rbig(a: RBig, op: Token, b: RBig) -> Value {
    let z = match op {
        Token::ADD => a + b,
        Token::SUB => a - b,
        Token::MUL => a * b,
        Token::QUO => a / b,
        _ => panic!("invalid Float binary operation {}", op.as_str()),
    };
    make_rat(z)
}

fn binary_op_fbig(a: BinFloat, op: Token, b: BinFloat) -> Value {
    let z = match op {
        Token::ADD => a + b,
        Token::SUB => a - b,
        Token::MUL => a * b,
        Token::QUO => a / b,
        _ => panic!("invalid Float binary operation {}", op.as_str()),
    };
    make_float(z.with_precision(crate::value::PREC).value())
}

fn binary_op_complex(ax: Value, ay: Value, op: Token, bx: Value, by: Value) -> Value {
    let add = |x, y| binary_op(x, Token::ADD, y);
    let sub = |x, y| binary_op(x, Token::SUB, y);
    let mul = |x, y| binary_op(x, Token::MUL, y);
    let quo = |x, y| binary_op(x, Token::QUO, y);

    let (re, im) = match op {
        Token::ADD => (add(ax, bx), add(ay, by)),
        Token::SUB => (sub(ax, bx), sub(ay, by)),
        Token::MUL => {
            // (ax + ay*i)(bx + by*i) = (ax*bx - ay*by) + i(ay*bx + ax*by)
            let ac = mul(ax.clone(), bx.clone());
            let bd = mul(ay.clone(), by.clone());
            let bc = mul(ay, bx);
            let ad = mul(ax, by);
            (sub(ac, bd), add(bc, ad))
        }
        Token::QUO => {
            // (ac+bd)/s + i(bc-ad)/s, with s = cc + dd
            let ac = mul(ax.clone(), bx.clone());
            let bd = mul(ay.clone(), by.clone());
            let bc = mul(ay, bx.clone());
            let ad = mul(ax, by.clone());
            let cc = mul(bx.clone(), bx);
            let dd = mul(by.clone(), by);
            let s = add(cc, dd);
            let re = quo(add(ac, bd), s.clone());
            let im = quo(sub(bc, ad), s);
            (re, im)
        }
        _ => panic!("invalid Complex binary operation {}", op.as_str()),
    };
    make_complex(re, im)
}

// ----------------------------------------------------------------------------
// Shift

/// Returns the result of `x op s` for shift operators `op` ([`Token::SHL`] or
/// [`Token::SHR`]).
///
/// `x` must be an [`Value::Int*`][Value::Int] or [`Value::Unknown`]. If `x` is
/// `Unknown`, the result is `x`.
///
/// Equivalent to `constant.Shift`.
///
/// # Panics
/// Panics if `op` is not a shift operator, or `x` is not an Int / Unknown.
pub fn shift(x: Value, op: Token, s: u32) -> Value {
    match x {
        Value::Unknown => Value::Unknown,
        Value::Int64(v) => {
            if s == 0 {
                return Value::Int64(v);
            }
            match op {
                Token::SHL => {
                    let z = i64_to_ibig(v) << (s as usize);
                    make_int(z)
                }
                Token::SHR => Value::Int64(v >> s),
                _ => panic!("invalid shift {:?} {} {}", v, op.as_str(), s),
            }
        }
        Value::Int(v) => {
            if s == 0 {
                return Value::Int(v);
            }
            match op {
                Token::SHL => make_int(v << (s as usize)),
                Token::SHR => make_int(v >> (s as usize)),
                _ => panic!("invalid shift {:?} {} {}", v, op.as_str(), s),
            }
        }
        other => panic!("invalid shift {:?} {} {}", other, op.as_str(), s),
    }
}

// ----------------------------------------------------------------------------
// Compare

/// Returns the result of the comparison `x op y` for relational operators
/// (`==`, `!=`, `<`, `<=`, `>`, `>=`). If either operand is [`Value::Unknown`],
/// the result is `false`.
///
/// Equivalent to `constant.Compare`.
///
/// # Panics
/// Panics if `op` is undefined for the (matched) operands.
pub fn compare(x_in: Value, op: Token, y_in: Value) -> bool {
    let (x, y) = match_pair(x_in.clone(), y_in.clone());
    match (x, y) {
        (Value::Unknown, _) => false,
        (Value::Bool(a), Value::Bool(b)) => match op {
            Token::EQL => a == b,
            Token::NEQ => a != b,
            _ => panic!(
                "invalid comparison {:?} {} {:?}",
                x_in,
                op.as_str(),
                y_in
            ),
        },
        (Value::Int64(a), Value::Int64(b)) => cmp_total(a.cmp(&b), op, &x_in, &y_in),
        (Value::Int(a), Value::Int(b)) => cmp_total(a.cmp(&b), op, &x_in, &y_in),
        (Value::Rat(a), Value::Rat(b)) => cmp_total(a.cmp(&b), op, &x_in, &y_in),
        (Value::Float(a), Value::Float(b)) => cmp_total(a.cmp(&b), op, &x_in, &y_in),
        (Value::Complex { re: ax, im: ay }, Value::Complex { re: bx, im: by }) => {
            let re = compare(*ax, Token::EQL, *bx);
            let im = compare(*ay, Token::EQL, *by);
            match op {
                Token::EQL => re && im,
                Token::NEQ => !re || !im,
                _ => panic!(
                    "invalid comparison {:?} {} {:?}",
                    x_in,
                    op.as_str(),
                    y_in
                ),
            }
        }
        (Value::String(s), Value::String(t)) => {
            // Go orders strings bytewise, which is what `[u8]` does too.
            let xs: &[u8] = s.as_ref();
            let ys: &[u8] = t.as_ref();
            match op {
                Token::EQL => xs == ys,
                Token::NEQ => xs != ys,
                Token::LSS => xs < ys,
                Token::LEQ => xs <= ys,
                Token::GTR => xs > ys,
                Token::GEQ => xs >= ys,
                _ => panic!(
                    "invalid comparison {:?} {} {:?}",
                    x_in,
                    op.as_str(),
                    y_in
                ),
            }
        }
        _ => panic!(
            "invalid comparison {:?} {} {:?}",
            x_in,
            op.as_str(),
            y_in
        ),
    }
}

fn cmp_total(ord: Ordering, op: Token, x: &Value, y: &Value) -> bool {
    let signum = match ord {
        Ordering::Less => -1i32,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    };
    match op {
        Token::EQL => signum == 0,
        Token::NEQ => signum != 0,
        Token::LSS => signum < 0,
        Token::LEQ => signum <= 0,
        Token::GTR => signum > 0,
        Token::GEQ => signum >= 0,
        _ => panic!("invalid comparison {:?} {} {:?}", x, op.as_str(), y),
    }
}
