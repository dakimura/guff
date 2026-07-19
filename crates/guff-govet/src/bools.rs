//! `bools` — check for common mistakes involving boolean operators.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{BinaryExpr, Expr, Ident};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

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

fn expr_key(e: &Expr) -> String {
    match unparen(e) {
        Expr::Ident(Ident { name, .. }) => name.clone(),
        Expr::BasicLit(l) => l.value.clone(),
        Expr::BinaryExpr(b) => format!("({} {:?} {})", expr_key(&b.x), b.op, expr_key(&b.y)),
        // Structural keys for compound operands so that distinct expressions
        // produce distinct keys. Collapsing every non-trivial operand to "_"
        // makes e.g. `a.x == 0 && a.y == 0` look redundant (both key to
        // `(_ EQL 0)`) — a false positive. Mirror `expreq::expr_equal`.
        Expr::SelectorExpr(s) => format!("{}.{}", expr_key(&s.x), s.sel.name),
        Expr::StarExpr(s) => format!("(*{})", expr_key(&s.x)),
        Expr::UnaryExpr(u) => format!("({:?}{})", u.op, expr_key(&u.x)),
        Expr::IndexExpr(i) => format!("{}[{}]", expr_key(&i.x), expr_key(&i.index)),
        Expr::CallExpr(c) => {
            let args = c.args.iter().map(expr_key).collect::<Vec<_>>().join(", ");
            format!("{}({args})", expr_key(&c.fun))
        }
        _ => "_".to_string(),
    }
}

fn is_const_operand(pass: &Pass<'_>, e: &Expr) -> bool {
    let info = match pass.types_info() {
        Some(i) => i,
        None => return false,
    };
    info.types
        .get(&e.id())
        .and_then(|tv| tv.val.as_ref())
        .is_some()
}

fn no_effects(pass: &Pass<'_>, e: &Expr) -> bool {
    match unparen(e) {
        Expr::Ident(_) | Expr::BasicLit(_) | Expr::SelectorExpr(_) => true,
        Expr::BinaryExpr(b) if matches!(b.op, Token::EQL | Token::NEQ) => {
            is_const_operand(pass, &b.x) || is_const_operand(pass, &b.y)
        }
        _ => false,
    }
}

fn split_op<'a>(e: &'a Expr, tok: Token, out: &mut Vec<&'a Expr>) {
    match unparen(e) {
        Expr::BinaryExpr(BinaryExpr { op, x, y, .. }) if *op == tok => {
            split_op(x, tok, out);
            split_op(y, tok, out);
        }
        other => out.push(other),
    }
}

fn check_redundant(op: BoolOp, exprs: &[&Expr], pending: &mut Vec<(u32, String)>) {
    let mut seen = HashMap::new();
    for e in exprs {
        let key = expr_key(e);
        if seen.contains_key(&key) {
            pending.push((
                e.pos().0 as u32,
                format!("redundant {}: {key} {:?} {key}", op.name, op.tok),
            ));
        } else {
            seen.insert(key, ());
        }
    }
}

fn check_suspect(pass: &Pass<'_>, op: BoolOp, exprs: &[&Expr], pending: &mut Vec<(u32, String)>) {
    let mut seen: HashMap<String, String> = HashMap::new();
    for e in exprs {
        let Expr::BinaryExpr(b) = unparen(e) else {
            continue;
        };
        if b.op != op.bad_eq {
            continue;
        }
        let x = if is_const_operand(pass, &b.y) {
            &b.x
        } else if is_const_operand(pass, &b.x) {
            &b.y
        } else {
            continue;
        };
        let xkey = expr_key(x);
        let ekey = expr_key(e);
        if let Some(prev) = seen.get(&xkey) {
            if prev != &ekey {
                pending.push((
                    e.pos().0 as u32,
                    format!("suspect {}: {ekey} {:?} {prev}", op.name, op.tok),
                ));
            }
        } else {
            seen.insert(xkey, ekey);
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "bools requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::BinaryExpr(b) = n else {
            return;
        };
        let op = match b.op {
            Token::LOR => OR,
            Token::LAND => AND,
            _ => return,
        };
        let mut flat = Vec::new();
        let root = Expr::BinaryExpr(b.clone());
        split_op(&root, op.tok, &mut flat);
        let mut i = 0;
        while i < flat.len() {
            let mut j = i;
            while j < flat.len() && no_effects(pass, flat[j]) {
                j += 1;
            }
            if j > i {
                let set = &flat[i..j];
                check_redundant(op, set, &mut pending);
                check_suspect(pass, op, set, &mut pending);
            }
            i = j + 1;
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
