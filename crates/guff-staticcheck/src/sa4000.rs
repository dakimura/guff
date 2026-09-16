//! SA4000 — binary operator has identical expressions on both sides.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4000`.

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{call_name, is_generated_at};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::{TypeData, TypeId};
use guff_types::basic::BasicKind;

use crate::render::render_node;

/// `isFloat` — but over the type's **term set**, and recursing into arrays and
/// structs:
///
/// ```go
/// tset := typeutil.NewTypeSet(T)
/// if len(tset.Terms) == 0 {
///     // no terms, so floats are a possibility
///     return true
/// }
/// return tset.Any(func(term *types.Term) bool {
///     switch typ := term.Type().Underlying().(type) {
///     case *types.Basic:  return kind == Float32 || kind == Float64
///     case *types.Array:  return isFloat(typ.Elem())
///     case *types.Struct: … any field isFloat …
/// ```
///
/// `a == a` on a `[2]float64` or on a `struct{ f float64 }` is legal and
/// meaningful (NaN), and so is any comparison on an unconstrained type
/// parameter, whose set has no terms at all. guff checked only for a basic
/// float and reported all three.
fn is_float_type(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(tav) = info.types.get(&expr.id()) else {
        return false;
    };
    is_float(pass, tav.typ)
}

fn is_float(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let arena = &artifacts.types;
    // A type parameter's terms are its constraint's; every other type is its
    // own single term. "Could not be determined" reads as no terms, which
    // upstream treats as "floats are a possibility".
    //
    // Asked of the type itself, not of its underlying: a type parameter's
    // underlying *is* its constraint interface, so matching on that would
    // never see the parameter at all.
    let t = guff_types::unalias_readonly(arena, typ);
    if let TypeData::TypeParam(tp) = arena.get(t) {
        let Some(bound) = tp.constraint() else {
            return true;
        };
        let bu = bound.underlying(arena);
        let TypeData::Interface(iface) = arena.get(bu) else {
            return is_float_term(pass, bound);
        };
        let Some(tset) = iface.cached_typeset() else {
            return true;
        };
        let mut terms: Vec<TypeId> = Vec::new();
        tset.is(|_tilde, term| {
            if let Some(t) = term {
                terms.push(t);
            }
            true
        });
        if terms.is_empty() {
            return true;
        }
        return terms.iter().any(|t| is_float_term(pass, *t));
    }
    is_float_term(pass, typ)
}

fn is_float_term(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let arena = &artifacts.types;
    let u = typ.underlying(arena);
    match arena.get(u) {
        TypeData::Basic(b) => matches!(b.kind(), BasicKind::Float32 | BasicKind::Float64),
        TypeData::Array(a) => is_float(pass, a.elem()),
        TypeData::Struct(st) => (0..st.num_fields()).any(|i| {
            st.field(i)
                .typ(&artifacts.objects)
                .is_some_and(|ft| is_float(pass, ft))
        }),
        _ => false,
    }
}

/// The functions SA4000 exempts, verbatim.
///
/// > We special case functions from the math/rand package. Someone ran into
/// > the following false positive: "rand.Intn(2) - rand.Intn(2), which I wrote
/// > to generate values {-1, 0, 1} with {0.25, 0.5, 0.25} probability."
///
/// guff matched the prefix `math/rand.` instead, which is neither the same set
/// nor a superset of it: `math/rand/v2` is a different import path, and
/// vitess's `rand.IntN(100) - rand.IntN(100)` (v2) was a finding upstream does
/// not have — and the `//nolint:staticcheck` over it then read as unused.
const RAND_FUNCS: &[&str] = &[
    "math/rand.Int",
    "math/rand.Int31",
    "math/rand.Int31n",
    "math/rand.Int63",
    "math/rand.Int63n",
    "math/rand.Intn",
    "math/rand.Uint32",
    "math/rand.Uint64",
    "math/rand.ExpFloat64",
    "math/rand.Float32",
    "math/rand.Float64",
    "math/rand.NormFloat64",
    "(*math/rand.Rand).Int",
    "(*math/rand.Rand).Int31",
    "(*math/rand.Rand).Int31n",
    "(*math/rand.Rand).Int63",
    "(*math/rand.Rand).Int63n",
    "(*math/rand.Rand).Intn",
    "(*math/rand.Rand).Uint32",
    "(*math/rand.Rand).Uint64",
    "(*math/rand.Rand).ExpFloat64",
    "(*math/rand.Rand).Float32",
    "(*math/rand.Rand).Float64",
    "(*math/rand.Rand).NormFloat64",
    "math/rand/v2.Int",
    "math/rand/v2.Int32",
    "math/rand/v2.Int32N",
    "math/rand/v2.Int64",
    "math/rand/v2.Int64N",
    "math/rand/v2.IntN",
    "math/rand/v2.N",
    "math/rand/v2.Uint",
    "math/rand/v2.Uint32",
    "math/rand/v2.Uint32N",
    "math/rand/v2.Uint64",
    "math/rand/v2.Uint64N",
    "math/rand/v2.UintN",
    "math/rand/v2.ExpFloat64",
    "math/rand/v2.Float32",
    "math/rand/v2.Float64",
    "math/rand/v2.NormFloat64",
    "(*math/rand/v2.Rand).Int",
    "(*math/rand/v2.Rand).Int32",
    "(*math/rand/v2.Rand).Int32N",
    "(*math/rand/v2.Rand).Int64",
    "(*math/rand/v2.Rand).Int64N",
    "(*math/rand/v2.Rand).IntN",
    "(*math/rand/v2.Rand).N",
    "(*math/rand/v2.Rand).Uint",
    "(*math/rand/v2.Rand).Uint32",
    "(*math/rand/v2.Rand).Uint32N",
    "(*math/rand/v2.Rand).Uint64",
    "(*math/rand/v2.Rand).Uint64N",
    "(*math/rand/v2.Rand).UintN",
    "(*math/rand/v2.Rand).ExpFloat64",
    "(*math/rand/v2.Rand).Float32",
    "(*math/rand/v2.Rand).Float64",
    "(*math/rand/v2.Rand).NormFloat64",
];

fn is_rand_call(name: &str) -> bool {
    RAND_FUNCS.contains(&name)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4000 requires inspect analyzer".to_string())?
        .clone();
    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(BinaryExpr), pass.files(), |node| {
        let NodeRef::BinaryExpr(op) = node else {
            return;
        };
        let flagged = match op.op {
            Token::EQL | Token::NEQ => true,
            Token::SUB
            | Token::QUO
            | Token::AND
            | Token::REM
            | Token::OR
            | Token::XOR
            | Token::LAND
            | Token::LOR
            | Token::LSS
            | Token::GTR
            | Token::LEQ
            | Token::GEQ => true,
            _ => false,
        };
        if !flagged {
            return;
        }
        if is_float_type(pass, &op.x) {
            return;
        }
        if std::mem::discriminant(&*op.x) != std::mem::discriminant(&*op.y) {
            return;
        }
        // Upstream compares `report.Render` of the two operands, i.e. the
        // printed source. A node the printer cannot render is not a node we
        // know to be identical to anything.
        match (render_node(pass, &op.x), render_node(pass, &op.y)) {
            (Some(x), Some(y)) if x == y => {}
            _ => return,
        }
        if let Expr::CallExpr(c) = &*op.x {
            if let Some(n) = call_name(pass, &c.fun) {
                if is_rand_call(&n) {
                    return;
                }
            }
        }
        if let (Expr::BasicLit(l1), Expr::BasicLit(l2)) = (&*op.x, &*op.y) {
            if l1.value == "0" && l2.value == "0" && is_generated_at(pass, l1.value_pos.0 as u32) {
                return;
            }
        }
        let op_str = match op.op {
            Token::EQL => "==",
            Token::NEQ => "!=",
            Token::SUB => "-",
            Token::QUO => "/",
            Token::AND => "&",
            Token::REM => "%",
            Token::OR => "|",
            Token::XOR => "^",
            Token::LAND => "&&",
            Token::LOR => "||",
            Token::LSS => "<",
            Token::GTR => ">",
            Token::LEQ => "<=",
            Token::GEQ => ">=",
            _ => "?",
        };
        // Upstream reports the BinaryExpr node, whose Pos() is the left
        // operand's start — not the operator.
        pending.push((
            op.x.pos().0 as u32,
            format!("identical expressions on the left and right side of the '{op_str}' operator"),
        ));
    });
    for (pos, msg) in pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn sa4000_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4000",
        doc: "binary operator has identical expressions on both sides",
        url: "https://staticcheck.dev/docs/checks/#SA4000",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4000_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4000_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
