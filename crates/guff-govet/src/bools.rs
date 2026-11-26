//! `bools` — check for common mistakes involving boolean operators.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/bools` (v0.44.0, the version
//! golangci-lint 2.12.2 pins). Three details of that implementation are load
//! bearing and were all wrong here before the golden gate compared columns:
//!
//! * `split` returns the operands of a `||`/`&&` chain in **reverse** source
//!   order, so the duplicate that `checkRedundant` reports is the *leftmost*
//!   one, not the rightmost.
//! * `split` records every `BinaryExpr` it flattens in a package-wide `seen`
//!   set, so the preorder walk does not re-report a nested chain.
//! * the operand text in the message is `go/printer` output, not a structural
//!   key, and the operator is the token's source spelling (`||`, not `LOR`).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::Expr;
use guff::node_mask;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::OperandMode;

use crate::expreq::unparen;

#[derive(Clone, Copy)]
struct BoolOp {
    name: &'static str,
    tok: Token,
    bad_eq: Token,
}

const OR: BoolOp = BoolOp {
    name: "or",
    tok: Token::LOR,
    bad_eq: Token::NEQ,
};
const AND: BoolOp = BoolOp {
    name: "and",
    tok: Token::LAND,
    bad_eq: Token::EQL,
};

/// `astutil.Format`: the expression as `go/printer` renders it. Upstream keys
/// its `seen` maps on this string *and* embeds it in the message, so the two
/// can never disagree.
fn format(pass: &Pass<'_>, e: &Expr) -> String {
    let mut buf: Vec<u8> = Vec::new();
    match guff::printer::fprint(&mut buf, pass.fset(), guff::printer::PrintNode::Expr(e)) {
        Ok(()) => String::from_utf8(buf).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

fn mode_of(pass: &Pass<'_>, e: &Expr) -> Option<OperandMode> {
    pass.types_info()?.types.get(&e.id()).map(|tv| tv.mode)
}

fn is_const_operand(pass: &Pass<'_>, e: &Expr) -> bool {
    pass.types_info()
        .and_then(|i| i.types.get(&e.id()))
        .and_then(|tv| tv.val.as_ref())
        .is_some()
}

/// `typesinternal.CallsPureBuiltin`: a builtin that is a pure computation over
/// its operands. Deliberately excludes append/clear/close/copy/delete/panic/
/// print/println/recover.
fn calls_pure_builtin(pass: &Pass<'_>, call: &guff::ast::CallExpr) -> bool {
    let Expr::Ident(id) = unparen(&call.fun) else {
        return false;
    };
    if mode_of(pass, &call.fun) != Some(OperandMode::Builtin) {
        return false;
    }
    matches!(
        id.name.as_str(),
        "len" | "cap" | "complex" | "imag" | "real" | "make" | "new" | "max" | "min"
    )
}

/// `typesinternal.NoEffects`: whether evaluating `e` can be observed.
fn no_effects(pass: &Pass<'_>, e: &Expr) -> bool {
    let mut ok = true;
    walk::inspect(walk::expr_ref(e), |n| {
        let Some(n) = n else { return true };
        match n {
            NodeRef::Ident(_)
            | NodeRef::BasicLit(_)
            | NodeRef::BinaryExpr(_)
            | NodeRef::ParenExpr(_)
            | NodeRef::SelectorExpr(_)
            | NodeRef::IndexExpr(_)
            | NodeRef::SliceExpr(_)
            | NodeRef::TypeAssertExpr(_)
            | NodeRef::StarExpr(_)
            | NodeRef::CompositeLit(_)
            | NodeRef::KeyValueExpr(_)
            | NodeRef::FieldList(_)
            | NodeRef::Field(_)
            | NodeRef::Ellipsis(_)
            | NodeRef::IndexListExpr(_) => {}

            // Type syntax: no effects, recursively. Prune descent.
            NodeRef::ArrayType(_)
            | NodeRef::StructType(_)
            | NodeRef::ChanType(_)
            | NodeRef::FuncType(_)
            | NodeRef::MapType(_)
            | NodeRef::InterfaceType(_) => return false,

            // A channel receive `<-ch` has effects.
            NodeRef::UnaryExpr(u) => {
                if u.op == Token::ARROW {
                    ok = false;
                }
            }

            // A type conversion has no effects; a pure builtin has none of its
            // own (its operands are still visited).
            NodeRef::CallExpr(c) => {
                let is_conversion = mode_of(pass, &c.fun) == Some(OperandMode::TypeExpr);
                if !is_conversion && !calls_pure_builtin(pass, c) {
                    ok = false;
                }
            }

            // A FuncLit has no effects, but do not descend into it.
            NodeRef::FuncLit(_) => return false,

            _ => ok = false,
        }
        ok
    });
    ok
}

/// `split`: every subexpression of `e` connected by `op`, in **reverse** source
/// order (`a || (b || c) || d` yields `[d, c, b, a]`). Records the flattened
/// `BinaryExpr`s in `seen` so the caller's preorder walk skips them.
fn split(e: &Expr, tok: Token, seen: &mut HashSet<u32>, out: &mut Vec<Expr>) {
    let mut cur = e.clone();
    loop {
        let unparened = unparen(&cur).clone();
        match &unparened {
            Expr::BinaryExpr(b) if b.op == tok => {
                seen.insert(b.id);
                split(&b.y, tok, seen, out);
                cur = (*b.x).clone();
            }
            _ => {
                out.push(unparened);
                return;
            }
        }
    }
}

/// `commutativeSets`: partition `split`'s output at every operand that has side
/// effects, dropping the operand itself.
fn commutative_sets(pass: &Pass<'_>, exprs: &[Expr]) -> Vec<Vec<Expr>> {
    let mut sets = Vec::new();
    let mut i = 0;
    for j in 0..=exprs.len() {
        if j == exprs.len() || !no_effects(pass, &exprs[j]) {
            if i < j {
                sets.push(exprs[i..j].to_vec());
            }
            i = j + 1;
        }
    }
    sets
}

fn check_redundant(pass: &Pass<'_>, op: BoolOp, exprs: &[Expr], pending: &mut Vec<(u32, String)>) {
    let mut seen: HashSet<String> = HashSet::new();
    for e in exprs {
        let efmt = format(pass, e);
        if seen.contains(&efmt) {
            pending.push((
                e.pos().0 as u32,
                format!("redundant {}: {efmt} {} {efmt}", op.name, op.tok),
            ));
        } else {
            seen.insert(efmt);
        }
    }
}

fn check_suspect(pass: &Pass<'_>, op: BoolOp, exprs: &[Expr], pending: &mut Vec<(u32, String)>) {
    // seen maps from expressions 'x' to equality expressions 'x != c'.
    let mut seen: HashMap<String, String> = HashMap::new();
    for e in exprs {
        let Expr::BinaryExpr(b) = e else {
            continue;
        };
        if b.op != op.bad_eq {
            continue;
        }
        // Restrict to cases where one operand is constant; the other is `x`.
        let x = if is_const_operand(pass, &b.y) {
            &b.x
        } else if is_const_operand(pass, &b.x) {
            &b.y
        } else {
            continue;
        };
        let xfmt = format(pass, x);
        let efmt = format(pass, e);
        if let Some(prev) = seen.get(&xfmt) {
            // check_redundant handles the case in which efmt == prev.
            if prev != &efmt {
                pending.push((
                    e.pos().0 as u32,
                    format!("suspect {}: {efmt} {} {prev}", op.name, op.tok),
                ));
            }
        } else {
            seen.insert(xfmt, efmt);
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "bools requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    // Shared across the whole walk: a chain flattened from an outer node must
    // not be reported again when the preorder reaches its inner nodes.
    let mut seen: HashSet<u32> = HashSet::new();
    inspect.preorder_typed(node_mask!(BinaryExpr), pass.files(), |n| {
        let NodeRef::BinaryExpr(b) = n else {
            return;
        };
        if seen.contains(&b.id) {
            return;
        }
        let op = match b.op {
            Token::LOR => OR,
            Token::LAND => AND,
            _ => return,
        };
        let root = Expr::BinaryExpr(b.clone());
        let mut flat = Vec::new();
        split(&root, op.tok, &mut seen, &mut flat);
        for set in commutative_sets(pass, &flat) {
            check_redundant(pass, op, &set, &mut pending);
            check_suspect(pass, op, &set, &mut pending);
        }
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "bools",
        doc: "check for common mistakes involving boolean operators",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/bools",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
