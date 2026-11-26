//! A left-associative binary chain is as deep as it is long, and generated Go
//! makes it very long: `cloud.google.com/go/compute/apiv1/computepb` carries
//! its file descriptor as one `const … = "" + "\x0a…" + …` with 24,711 terms.
//! Go type-checks that on a goroutine stack that grows; guff's lint worker has
//! a fixed 8 MiB, and the recursive form — `binary` → `expr` → `raw_expr` →
//! `expr_internal` → `binary`, four frames per term — ran out at roughly
//! 11,000 terms in a release build. What the user saw was the process aborting
//! with "has overflowed its stack": no findings, no diagnostic, nothing to act
//! on. Every elastic/beats package that reaches computepb through the OTel
//! collector did that (measured 2026-09-20; `./x-pack/...` is five of them).
//!
//! `Checker::binary` therefore walks the left spine into a vector and folds it
//! back up, so the stack no longer grows with the chain.
//!
//! The deep cases below build the chain as an AST rather than parsing one.
//! That is not a shortcut — it is what makes the test a gate. Parsing is
//! itself recursive and costs *more* per term than the old checker did
//! (measured in a debug build: parsing overflows 8 MiB at ~2,300 terms), so a
//! source-driven test can never reach a depth where the checker is the thing
//! that breaks. Driving `Checker::expr` directly leaves the spine fold as the
//! only depth-sensitive work, and the whole thing runs on a 8 MiB thread —
//! production size — where the recursive form would have needed ~75 MiB.

use guff::ast::{BasicLit, BinaryExpr, Expr, Ident};
use guff::parser::{parse_file, Mode};
use guff::position::{FileSet, Pos};
use guff::token::Token;

use guff_types::arena::ObjectData;
use guff_types::operand::OperandMode;
use guff_types::scope::lookup as scope_lookup;
use guff_types::{BasicKind, Checker, Config, Operand};

/// The lint worker's stack (`guff-lint`'s `LINT_WORKER_STACK`).
const STACK: usize = 8 * 1024 * 1024;

/// The term count the corpus actually holds (computepb is 24,711).
const DEEP: usize = 25_000;

// ---------------------------------------------------------------- AST chains

/// Node ids are handed out here rather than by `guff::stamp::stamp_expr_ids`,
/// which is itself a recursive walk and overflows 8 MiB at ~25,000 nodes in a
/// debug build. The checker only needs them distinct and non-zero (`0` means
/// "unstamped" and is dropped from `Info`), so a counter does the job and
/// keeps the only deep recursion in the test the one under test.
fn next_id() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn lit(kind: Token, value: &str) -> Expr {
    Expr::BasicLit(BasicLit {
        kind: Some(kind),
        value: value.to_string(),
        id: next_id(),
        ..Default::default()
    })
}

fn ident(name: &str) -> Expr {
    Expr::Ident(Ident {
        name: name.to_string(),
        id: next_id(),
        ..Default::default()
    })
}

fn binary(x: Expr, op: Token, y: Expr) -> Expr {
    Expr::BinaryExpr(BinaryExpr {
        x: Box::new(x),
        op_pos: Pos(1),
        op,
        y: Box::new(y),
        id: next_id(),
    })
}

/// `head op term op term …` with `n` terms, nested to the left the way Go's
/// parser nests `a + b + c`.
fn chain(head: Expr, op: Token, n: usize, term: impl Fn(usize) -> Expr) -> Expr {
    let mut e = head;
    for i in 0..n {
        e = binary(e, op, term(i));
    }
    e
}

/// What one run of `Checker::expr` produced: the pieces that are `Send`, so
/// they can come back out of the worker thread.
struct Checked {
    mode: OperandMode,
    val: Option<guff_constant::Value>,
    typ_is_untyped_bool: bool,
    errors: Vec<String>,
}

/// Type-check `e` (built by the caller inside the thread) against a package
/// whose source is `src`, on a production-sized stack.
fn check_expr_on_worker_stack(
    src: &'static str,
    build: impl FnOnce() -> Expr + Send + 'static,
) -> Checked {
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || {
            let fset = FileSet::new();
            let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");
            let mut check = Checker::new(Config::default());
            check.files = vec![file];
            check.collect_objects();
            let pkg_scope = check.packages.get(check.pkg).scope();
            check.env.scope = Some(pkg_scope);

            let e = build();
            let mut x = Operand::invalid();
            check.expr(&mut x, &e);
            Checked {
                mode: x.mode,
                val: x.val.clone(),
                typ_is_untyped_bool: x.typ == Some(check.basic(BasicKind::UntypedBool)),
                errors: check.errors.iter().map(|err| err.msg.clone()).collect(),
            }
        })
        .expect("spawn")
        .join()
        .unwrap_or_else(|_| panic!("the checker overflowed a {STACK}-byte stack"))
}

// ---------------------------------------------------------------- the checks

#[test]
fn string_chain_folds_at_every_length() {
    // 0 terms is a bare literal (no `binary` at all), 1 is the shallowest
    // chain, 2 the first with an inner node to fold, and DEEP is the corpus
    // shape. The folded value is asserted at every length: a fold that dropped
    // or repeated a term would not crash, it would just be wrong.
    for n in [0usize, 1, 2, 3, 100, DEEP] {
        let got = check_expr_on_worker_stack("package p\n", move || {
            chain(lit(Token::STRING, "\"\""), Token::ADD, n, |_| {
                lit(Token::STRING, "\"x\"")
            })
        });
        assert!(got.errors.is_empty(), "n={n}: errors: {:?}", got.errors);
        assert_eq!(got.mode, OperandMode::Constant, "n={n}");
        let bytes = guff_constant::string_val(got.val.as_ref().expect("a value"));
        assert_eq!(bytes, vec![b'x'; n], "n={n}: folded value");
    }
}

#[test]
fn int_chain_folds_to_the_sum() {
    // Arithmetic rather than concatenation, so the fold goes through
    // `binary_op` on untyped ints and `overflow` on the way out.
    for n in [1usize, 2, 100, DEEP] {
        let got = check_expr_on_worker_stack("package p\n", move || {
            chain(lit(Token::INT, "0"), Token::ADD, n, |_| lit(Token::INT, "1"))
        });
        assert!(got.errors.is_empty(), "n={n}: errors: {:?}", got.errors);
        let (v, exact) = guff_constant::int64_val(got.val.as_ref().expect("a value"));
        assert!(exact, "n={n}: the sum should be an exact int64");
        assert_eq!(v, n as i64, "n={n}: folded sum");
    }
}

#[test]
fn deep_chain_of_values_is_not_constant() {
    // The non-constant path: the deepest operand is a variable, so every node
    // yields a `Value` and the fold runs `match_types` + `binary_op_ok` DEEP
    // times without ever folding a constant.
    let got = check_expr_on_worker_stack("package p\nvar v string\n", || {
        chain(ident("v"), Token::ADD, DEEP, |_| lit(Token::STRING, "\"x\""))
    });
    assert!(got.errors.is_empty(), "errors: {:?}", got.errors);
    assert_eq!(got.mode, OperandMode::Value);
    assert!(got.val.is_none(), "a variable chain has no constant value");
}

#[test]
fn comparison_on_top_of_a_deep_chain() {
    // The outermost node takes the `is_comparison_op` branch while everything
    // below it folds — the flattened spine has to hand the comparison an
    // already-folded left operand.
    let got = check_expr_on_worker_stack("package p\n", || {
        let sum = chain(lit(Token::INT, "0"), Token::ADD, DEEP, |_| {
            lit(Token::INT, "1")
        });
        binary(sum, Token::GTR, lit(Token::INT, "5"))
    });
    assert!(got.errors.is_empty(), "errors: {:?}", got.errors);
    assert_eq!(got.mode, OperandMode::Constant);
    assert!(guff_constant::bool_val(got.val.as_ref().expect("a value")));
    assert!(
        got.typ_is_untyped_bool,
        "a constant comparison is untyped bool"
    );
}

#[test]
fn one_undefined_term_deep_in_the_chain_reports_once() {
    // An error inside the spine must surface exactly once: the recursive form
    // reported it from the node that owned it and propagated `Invalid`
    // outward, and the folded form has to do the same. Reporting it per
    // enclosing node would give DEEP/2 copies.
    let got = check_expr_on_worker_stack("package p\n", || {
        chain(lit(Token::STRING, "\"\""), Token::ADD, DEEP, |i| {
            if i == DEEP / 2 {
                ident("missingIdent")
            } else {
                lit(Token::STRING, "\"x\"")
            }
        })
    });
    assert_eq!(
        got.errors.len(),
        1,
        "expected one error, got {:?}",
        got.errors
    );
    assert!(
        got.errors[0].contains("missingIdent"),
        "the error should name the term: {:?}",
        got.errors
    );
    assert_eq!(got.mode, OperandMode::Invalid);
}

#[test]
fn one_mismatched_term_deep_in_the_chain_reports_once() {
    // Same, for the arm that is reached only after both operands checked —
    // i.e. from inside the fold rather than from the operand itself.
    let got = check_expr_on_worker_stack("package p\nvar s string\n", || {
        chain(ident("s"), Token::ADD, DEEP, |i| {
            if i == DEEP / 2 {
                lit(Token::INT, "1")
            } else {
                lit(Token::STRING, "\"x\"")
            }
        })
    });
    assert_eq!(
        got.errors.len(),
        1,
        "expected one error, got {:?}",
        got.errors
    );
    assert_eq!(got.mode, OperandMode::Invalid);
}

#[test]
fn a_parsed_const_decl_folds_the_same_way() {
    // The end-to-end path — source → `const_decl` → `expr` — at a depth a
    // debug build can parse (see the module comment). This is the shape
    // computepb has; the AST tests above are the same shape at corpus depth.
    const N: usize = 1_000;
    let mut src = String::from("package p\n\nconst S = \"\"");
    for _ in 0..N {
        src.push_str(" +\n\t\"x\"");
    }
    src.push('\n');
    let (errors, bytes) = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || {
            let fset = FileSet::new();
            let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");
            let mut check = Checker::new(Config::default());
            check.check_files(vec![file]);
            let pkg_scope = check.packages.get(check.pkg).scope();
            let obj = scope_lookup(&check.scopes, pkg_scope, "S").expect("S");
            let val = match check.objects.get(obj) {
                ObjectData::Const(c) => c.val().clone(),
                other => panic!("S is not a constant: {other:?}"),
            };
            (
                check
                    .errors
                    .iter()
                    .map(|e| e.msg.clone())
                    .collect::<Vec<_>>(),
                guff_constant::string_val(&val),
            )
        })
        .expect("spawn")
        .join()
        .expect("the const decl overflowed the stack");
    assert!(errors.is_empty(), "errors: {errors:?}");
    assert_eq!(bytes, vec![b'x'; N]);
}

