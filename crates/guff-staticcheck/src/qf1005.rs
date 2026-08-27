//! QF1005 — expand call to `math.Pow`.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1005`.
//!
use std::sync::OnceLock;

use guff::ast::Expr;
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to, unparen};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_constant::{int64_val, to_int, Kind};
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::BasicKind;
use guff_types::TypeId;

use crate::render::render_expr;

fn may_have_side_effects(expr: &Expr) -> bool {
    match expr {
        Expr::CallExpr(_) => true,
        Expr::UnaryExpr(u) => may_have_side_effects(&u.x),
        Expr::BinaryExpr(b) => may_have_side_effects(&b.x) || may_have_side_effects(&b.y),
        Expr::IndexExpr(i) => may_have_side_effects(&i.x) || may_have_side_effects(&i.index),
        Expr::SelectorExpr(s) => may_have_side_effects(&s.x),
        Expr::StarExpr(s) => may_have_side_effects(&s.x),
        Expr::ParenExpr(p) => may_have_side_effects(&p.x),
        Expr::SliceExpr(s) => {
            may_have_side_effects(&s.x)
                || s.low.as_ref().is_some_and(|e| may_have_side_effects(e))
                || s.high.as_ref().is_some_and(|e| may_have_side_effects(e))
                || s.max.as_ref().is_some_and(|e| may_have_side_effects(e))
        }
        _ => false,
    }
}

fn is_constant(pass: &Pass<'_>, expr: &Expr) -> bool {
    pass.types_info()
        .and_then(|info| info.types.get(&expr.id()))
        .is_some_and(|tv| tv.val.is_some())
}

/// The basic kind `expr` would have if it were type-checked **on its own**.
///
/// This is the question upstream asks with `types.CheckExpr`, and it is not the
/// same as the type recorded for the expression. A `math.Pow` argument has
/// already been converted to `float64` by the time the checker records it, and
/// so has its constant value (`representable` runs `constant.ToFloat` and
/// writes the result back), so both fields say `float64` for `2` and for `2.0`.
/// That is why the predicate this replaces — "is the recorded type assignable
/// to float64" — answered yes for every argument QF1005 can ever see, and the
/// conversion was never emitted once (COMPAT-HARDENING 続き 73).
///
/// The kind is recomputed from the syntax instead, which for a constant
/// expression is the same walk the checker does: a literal carries its kind in
/// its token, a named constant in its declared type, a shift takes its left
/// operand's, and everything else the wider of its operands'.
fn standalone_kind(pass: &Pass<'_>, expr: &Expr) -> Option<BasicKind> {
    match expr {
        Expr::ParenExpr(p) => standalone_kind(pass, &p.x),
        Expr::UnaryExpr(u) => standalone_kind(pass, &u.x),
        Expr::BasicLit(lit) => match lit.kind? {
            guff::token::Token::INT => Some(BasicKind::UntypedInt),
            guff::token::Token::FLOAT => Some(BasicKind::UntypedFloat),
            guff::token::Token::CHAR => Some(BasicKind::UntypedRune),
            guff::token::Token::IMAG => Some(BasicKind::UntypedComplex),
            _ => None,
        },
        Expr::BinaryExpr(b) => {
            let left = standalone_kind(pass, &b.x)?;
            // A shift takes its left operand's kind alone. That is what makes
            // `1 << 3` untyped int, and so wrapped, while `2.0 * 2` is untyped
            // float and so is not.
            if matches!(b.op, guff::token::Token::SHL | guff::token::Token::SHR) {
                return Some(left);
            }
            combine(left, standalone_kind(pass, &b.y)?)
        }
        Expr::Ident(ident) => const_kind(pass, ident.id),
        Expr::SelectorExpr(sel) => const_kind(pass, sel.sel.id),
        _ => None,
    }
}

/// The declared kind of the constant `id` denotes, if it denotes one.
fn const_kind(pass: &Pass<'_>, id: u32) -> Option<BasicKind> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let obj = info.uses.get(&id).copied()?;
    let ObjectData::Const(c) = artifacts.objects.get(obj) else {
        return None;
    };
    basic_kind(pass, c.typ())
}

fn basic_kind(pass: &Pass<'_>, typ: TypeId) -> Option<BasicKind> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    match artifacts.types.get(typ) {
        TypeData::Basic(b) => Some(b.kind()),
        _ => None,
    }
}

fn is_untyped(kind: BasicKind) -> bool {
    matches!(
        kind,
        BasicKind::UntypedBool
            | BasicKind::UntypedInt
            | BasicKind::UntypedRune
            | BasicKind::UntypedFloat
            | BasicKind::UntypedComplex
            | BasicKind::UntypedString
            | BasicKind::UntypedNil
    )
}

/// Go's `max_type` for a non-shift binary expression: a typed operand wins
/// outright, otherwise the wider untyped kind does.
fn combine(a: BasicKind, b: BasicKind) -> Option<BasicKind> {
    if !is_untyped(a) {
        return Some(a);
    }
    if !is_untyped(b) {
        return Some(b);
    }
    let rank = |k: BasicKind| match k {
        BasicKind::UntypedInt => Some(1),
        BasicKind::UntypedRune => Some(2),
        BasicKind::UntypedFloat => Some(3),
        BasicKind::UntypedComplex => Some(4),
        _ => None,
    };
    Some(if rank(a)? >= rank(b)? { a } else { b })
}

/// Whether the expanded product has to be wrapped in `float64(...)`.
///
/// `None` means the base is a constant whose standalone kind could not be
/// determined. Upstream `continue`s on the equivalent failure and drops the
/// finding; so does the caller here, because guessing writes bytes that either
/// do not compile or silently change the expression's type.
fn needs_conversion(pass: &Pass<'_>, x: &Expr) -> Option<bool> {
    if !is_constant(pass, x) {
        return Some(false);
    }
    let kind = standalone_kind(pass, x)?;
    Some(!matches!(kind, BasicKind::UntypedFloat | BasicKind::Float64))
}

/// Parenthesize one factor the way upstream's printer ends up doing.
///
/// Two rules, and the second one is not a printer rule at all.
///
/// Upstream builds a `BinaryExpr` and prints it with `format.Node`, so most of
/// the parentheses are go/printer's: an operand below `*`'s precedence needs
/// them on either side, an operand *at* that precedence needs them only on the
/// right because `*` is left-associative, and a unary or primary expression
/// never does. Parenthesizing every binary and unary operand instead — which is
/// what this used to do — writes `(-x) * (-x)` and `(4 / 2) * (4 / 2)` where
/// upstream writes `-x * -x` and `4 / 2 * (4 / 2)`.
///
/// The second rule is `astutil.SimplifyParentheses`, which despite its name
/// does not only strip `ParenExpr`: it also **rotates** `a OP (b OP c)` into
/// `(a OP b) OP c` whenever the two operators are the same, and repeats until
/// nothing changes. The product's operator is always `*`, so a base that is
/// itself a `*` flattens into the chain and never needs parentheses on either
/// side — `math.Pow(x*y, 3)` is `x * y * x * y * x * y`, not
/// `x * y * (x * y) * (x * y)`. A base at the same precedence but a *different*
/// operator does not rotate, which is why `x/y` keeps its parens next to it.
fn render_factor(expr: &Expr, right: bool) -> String {
    let mul = guff::token::Token::MUL.precedence();
    let needs_parens = match expr {
        Expr::BinaryExpr(b) if b.op == guff::token::Token::MUL => false,
        Expr::BinaryExpr(b) if right => b.op.precedence() <= mul,
        Expr::BinaryExpr(b) => b.op.precedence() < mul,
        _ => false,
    };
    let rendered = render_expr(expr);
    if needs_parens {
        format!("({rendered})")
    } else {
        rendered
    }
}

fn expand_pow(pass: &Pass<'_>, x: &Expr, n: i64) -> Option<String> {
    // Upstream's `SimplifyParentheses`: the base is unwrapped before the
    // product is built, so `math.Pow((2), 2)` expands to `2 * 2`.
    let x = unparen(x);
    let wrap = needs_conversion(pass, x)?;
    let product = match n {
        // `1.0` is already an untyped float, so the conversion is skipped for
        // n == 0 and only for n == 0.
        0 => return Some("1.0".into()),
        1 => render_factor(x, false),
        2 => format!("{} * {}", render_factor(x, false), render_factor(x, true)),
        3 => format!(
            "{} * {} * {}",
            render_factor(x, false),
            render_factor(x, true),
            render_factor(x, true)
        ),
        _ => return None,
    };
    // The conversion goes around the whole product, never around each factor.
    Some(if wrap {
        format!("float64({product})")
    } else {
        product
    })
}

fn pow_exponent(pass: &Pass<'_>, expr: &Expr) -> Option<i64> {
    // Prefer AST integer literal (avoids float conversion of typed call args).
    if let Expr::BasicLit(lit) = expr {
        if lit.kind == Some(guff::token::Token::INT) {
            return lit.value.parse().ok();
        }
    }
    let info = pass.types_info()?;
    let tav = info.types.get(&expr.id())?;
    let val = tav.val.as_ref()?;
    let as_int = to_int(val.clone());
    if as_int.kind() == Kind::Unknown {
        return None;
    }
    let (n, exact) = int64_val(&as_int);
    exact.then_some(n)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1005 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if !is_call_to(pass, call, "math.Pow") {
            return;
        }
        if call.args.len() != 2 {
            return;
        }
        let x = &call.args[0];
        // Before the exponent, exactly as upstream does it. Gating on `n >= 2`
        // instead — because only n >= 2 repeats the base — reads as harmless
        // and is not: `math.Pow(g(), 0)` becomes `1.0` and the call is gone.
        // Upstream does not even report these (COMPAT-HARDENING 続き 73).
        if may_have_side_effects(x) {
            return;
        }
        let Some(n) = pow_exponent(pass, &call.args[1]) else {
            return;
        };
        let Some(replacement) = expand_pow(pass, x, n) else {
            return;
        };
        pending.push((
            call.pos().0 as u32,
            call.end().0 as u32,
            replacement,
        ));
    });

    for (pos, end, replacement) in pending {
        pass.report(Diagnostic {
            pos,
            end,
            message: "could expand call to math.Pow".into(),
            suggested_fixes: vec![SuggestedFix {
                message: "Expand call to math.Pow".into(),
                text_edits: vec![TextEdit {
                    pos,
                    end,
                    new_text: replacement,
                }],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn qf1005_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "QF1005",
        doc: "expand call to math.Pow",
        url: "https://staticcheck.dev/docs/checks/#QF1005",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(qf1005_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn qf1005_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn render_factor_follows_the_printer_not_the_operand_kind() {
        let ident = Expr::Ident(guff::ast::Ident::new_ident("x"));
        // A primary expression never needs parentheses, on either side.
        assert_eq!(render_factor(&ident, false), "x");
        assert_eq!(render_factor(&ident, true), "x");
    }

    #[test]
    fn mul_precedence_matches_go_token() {
        use guff::token::Token;
        let mul = Token::MUL.precedence();
        // The three facts render_factor rests on. If go/token's table ever
        // moves, the parenthesization moves with it and this says so first.
        assert_eq!(Token::QUO.precedence(), mul, "/ is at *'s precedence");
        assert_eq!(Token::SHL.precedence(), mul, "<< is at *'s precedence");
        assert!(Token::ADD.precedence() < mul, "+ is below *");
    }
}
