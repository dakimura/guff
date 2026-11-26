//! Port of `go/types/exprstring.go` — the (possibly shortened) rendering of an
//! expression that go/types splices into error messages.
//!
//! This is deliberately *not* a source printer: composite-literal elements
//! collapse to `…`, a function literal becomes `(func() literal)`, and struct
//! tags are dropped. Error text has to match go/types byte-for-byte, so the
//! shortenings are part of the contract, not a simplification.
//!
//! Equivalent to `ExprString` / `WriteExpr`.

use std::fmt::Write as _;

use guff::ast::{ChanDir, Expr, Field, FuncType, Ident};

/// The (possibly shortened) string representation of `x`.
///
/// Equivalent to `types.ExprString`.
pub fn expr_string(x: &Expr) -> String {
    let mut buf = String::new();
    write_expr(&mut buf, x);
    buf
}

/// Equivalent to `types.WriteExpr`.
///
/// The AST preserves source-level parentheses, so no parentheses are inserted
/// here to correct for operator precedence.
pub fn write_expr(buf: &mut String, x: &Expr) {
    match x {
        // Go's default arm prints `(ast: %T)` for nil / BadExpr / KeyValueExpr.
        Expr::BadExpr(_) => buf.push_str("(ast: *ast.BadExpr)"),
        Expr::KeyValueExpr(_) => buf.push_str("(ast: *ast.KeyValueExpr)"),

        Expr::Ident(id) => buf.push_str(&id.name),

        Expr::Ellipsis(e) => {
            buf.push_str("...");
            if let Some(elt) = &e.elt {
                write_expr(buf, elt);
            }
        }

        Expr::BasicLit(l) => buf.push_str(&l.value),

        Expr::FuncLit(f) => {
            buf.push('(');
            buf.push_str("func");
            write_sig_expr(buf, &f.ty);
            buf.push_str(" literal)"); // shortened
        }

        Expr::CompositeLit(c) => {
            if let Some(t) = &c.ty {
                write_expr(buf, t);
            }
            buf.push('{');
            if !c.elts.is_empty() {
                buf.push('…');
            }
            buf.push('}');
        }

        Expr::ParenExpr(p) => {
            buf.push('(');
            write_expr(buf, &p.x);
            buf.push(')');
        }

        Expr::SelectorExpr(s) => {
            write_expr(buf, &s.x);
            buf.push('.');
            buf.push_str(&s.sel.name);
        }

        // Go unpacks IndexExpr / IndexListExpr into one indexed form.
        Expr::IndexExpr(i) => {
            write_expr(buf, &i.x);
            buf.push('[');
            write_expr(buf, &i.index);
            buf.push(']');
        }
        Expr::IndexListExpr(i) => {
            write_expr(buf, &i.x);
            buf.push('[');
            write_expr_list(buf, &i.indices);
            buf.push(']');
        }

        Expr::SliceExpr(s) => {
            write_expr(buf, &s.x);
            buf.push('[');
            if let Some(lo) = &s.low {
                write_expr(buf, lo);
            }
            buf.push(':');
            if let Some(hi) = &s.high {
                write_expr(buf, hi);
            }
            if s.slice3 {
                buf.push(':');
                if let Some(m) = &s.max {
                    write_expr(buf, m);
                }
            }
            buf.push(']');
        }

        Expr::TypeAssertExpr(t) => {
            write_expr(buf, &t.x);
            buf.push_str(".(");
            match &t.ty {
                Some(ty) => write_expr(buf, ty),
                // `x.(type)` in a type switch: Go's WriteExpr is called with a
                // nil Type and prints the default arm.
                None => buf.push_str("(ast: <nil>)"),
            }
            buf.push(')');
        }

        Expr::CallExpr(c) => {
            write_expr(buf, &c.fun);
            buf.push('(');
            write_expr_list(buf, &c.args);
            if c.ellipsis.0 != 0 {
                buf.push_str("...");
            }
            buf.push(')');
        }

        Expr::StarExpr(s) => {
            buf.push('*');
            write_expr(buf, &s.x);
        }

        Expr::UnaryExpr(u) => {
            let _ = write!(buf, "{}", u.op);
            write_expr(buf, &u.x);
        }

        Expr::BinaryExpr(b) => {
            write_expr(buf, &b.x);
            let _ = write!(buf, " {} ", b.op);
            write_expr(buf, &b.y);
        }

        Expr::ArrayType(a) => {
            buf.push('[');
            if let Some(len) = &a.len {
                write_expr(buf, len);
            }
            buf.push(']');
            write_expr(buf, &a.elt);
        }

        Expr::StructType(s) => {
            buf.push_str("struct{");
            write_field_list(buf, &s.fields.list, "; ", false);
            buf.push('}');
        }

        Expr::FuncType(f) => {
            buf.push_str("func");
            write_sig_expr(buf, f);
        }

        Expr::InterfaceType(i) => {
            buf.push_str("interface{");
            write_field_list(buf, &i.methods.list, "; ", true);
            buf.push('}');
        }

        Expr::MapType(m) => {
            buf.push_str("map[");
            write_expr(buf, &m.key);
            buf.push(']');
            write_expr(buf, &m.value);
        }

        Expr::ChanType(c) => {
            buf.push_str(if c.dir == ChanDir::SEND {
                "chan<- "
            } else if c.dir == ChanDir::RECV {
                "<-chan "
            } else {
                "chan "
            });
            write_expr(buf, &c.value);
        }
    }
}

/// Equivalent to `writeSigExpr`.
fn write_sig_expr(buf: &mut String, sig: &FuncType) {
    buf.push('(');
    if let Some(params) = &sig.params {
        write_field_list(buf, &params.list, ", ", false);
    }
    buf.push(')');

    let Some(res) = &sig.results else { return };
    // `NumFields` counts names, not entries: `(a, b int)` is two fields.
    let n: usize = res
        .list
        .iter()
        .map(|f| if f.names.is_empty() { 1 } else { f.names.len() })
        .sum();
    if n == 0 {
        return; // no result
    }

    buf.push(' ');
    if n == 1 && res.list.len() == 1 && res.list[0].names.is_empty() {
        // single unnamed result
        if let Some(t) = &res.list[0].ty {
            write_expr(buf, t);
        }
        return;
    }

    buf.push('(');
    write_field_list(buf, &res.list, ", ", false);
    buf.push(')');
}

/// Equivalent to `writeFieldList`.
fn write_field_list(buf: &mut String, list: &[Field], sep: &str, iface: bool) {
    for (i, f) in list.iter().enumerate() {
        if i > 0 {
            buf.push_str(sep);
        }

        write_ident_list(buf, &f.names);

        // Types of interface methods consist of signatures only.
        if iface {
            if let Some(Expr::FuncType(sig)) = &f.ty {
                write_sig_expr(buf, sig);
                continue;
            }
        }

        // Named fields are separated from their type with a blank.
        if !f.names.is_empty() {
            buf.push(' ');
        }

        if let Some(t) = &f.ty {
            write_expr(buf, t);
        }
        // The tag is ignored.
    }
}

/// Equivalent to `writeIdentList`.
fn write_ident_list(buf: &mut String, list: &[Ident]) {
    for (i, x) in list.iter().enumerate() {
        if i > 0 {
            buf.push_str(", ");
        }
        buf.push_str(&x.name);
    }
}

/// Equivalent to `writeExprList`.
fn write_expr_list(buf: &mut String, list: &[Expr]) {
    for (i, x) in list.iter().enumerate() {
        if i > 0 {
            buf.push_str(", ");
        }
        write_expr(buf, x);
    }
}
