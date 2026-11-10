//! `unnecessary-format` — warn on formatting functions without format verbs.

use guff::ast::{CallExpr, Expr};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use guff::token::Token;

use crate::failure::Failure;

/// Upstream `formattingFuncs` (revive v1.17.1): the printed callee, the
/// position of the format argument, and the suggested alternative. The
/// `logger` / `t` / `b` / `f` rows match a variable of that name whatever its
/// type, as upstream's do. `fmt.Sprintf`'s alternative carries a stray closing
/// quote upstream, reproduced here.
const FORMATTING_FUNCS: &[(&str, usize, &str)] = &[
    ("fmt.Appendf", 1, r#""fmt.Append""#),
    ("fmt.Errorf", 0, r#""errors.New""#),
    ("fmt.Fprintf", 1, r#""fmt.Fprint""#),
    ("fmt.Fscanf", 1, r#""fmt.Fscan" or "fmt.Fscanln""#),
    ("fmt.Printf", 0, r#""fmt.Print" or "fmt.Println""#),
    ("fmt.Scanf", 0, r#""fmt.Scan""#),
    ("fmt.Sprintf", 0, r#""fmt.Sprint" or just the string itself""#),
    ("fmt.Sscanf", 1, r#""fmt.Sscan""#),
    ("log.Fatalf", 0, r#""log.Fatal""#),
    ("log.Panicf", 0, r#""log.Panic""#),
    ("log.Printf", 0, r#""log.Print""#),
    ("logger.Fatalf", 0, r#""logger.Fatal""#),
    ("logger.Panicf", 0, r#""logger.Panic""#),
    ("logger.Printf", 0, r#""logger.Print""#),
    ("t.Errorf", 0, r#""t.Error""#),
    ("t.Fatalf", 0, r#""t.Fatal""#),
    ("t.Logf", 0, r#""t.Log""#),
    ("t.Skipf", 0, r#""t.Skip""#),
    ("b.Errorf", 0, r#""b.Error""#),
    ("b.Fatalf", 0, r#""b.Fatal""#),
    ("b.Logf", 0, r#""b.Log""#),
    ("b.Skipf", 0, r#""b.Skip""#),
    ("f.Errorf", 0, r#""f.Error""#),
    ("f.Fatalf", 0, r#""f.Fatal""#),
    ("f.Logf", 0, r#""f.Log""#),
    ("f.Skipf", 0, r#""f.Skip""#),
    ("trace.Logf", 2, r#""trace.Log""#),
];

/// `astutils.GoFmt(ce.Fun)` for the only shape a table key can print as:
/// `x.Sel` with `x` a bare identifier. Anything else — a parenthesis anywhere,
/// a longer selector chain, an index — prints as something that is no key.
fn printed_callee(fun: &Expr) -> Option<String> {
    let Expr::SelectorExpr(sel) = fun else {
        return None;
    };
    let Expr::Ident(x) = sel.x.as_ref() else {
        return None;
    };
    Some(format!("{}.{}", x.name, sel.sel.name))
}

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
    if call.args.is_empty() {
        return;
    }
    // Upstream keys the table on `astutils.GoFmt(ce.Fun)` — the *printed*
    // callee — so a parenthesis anywhere in it (`(fmt.Errorf)(…)`,
    // `(fmt).Errorf(…)`) renders to something that is not a key and the rule
    // stays silent. And `astutils.IsStringLiteral` is a bare
    // `e.(*ast.BasicLit)` with no unwrapping, so a parenthesized format string
    // is not a string literal to it either.
    //
    // This is the mirror image of the staticcheck rules in the same session:
    // honnef matches through `pattern`, which strips parentheses at every
    // level, and revive asserts, which never does. The polarity has to be read
    // off the upstream matcher per linter — guff had it the staticcheck way
    // here, and reported `fmt.Errorf(("clean error"))` where upstream does not
    // (`compat/fuzz.py --allow-dirty-seeds --case revive`, COMPAT-HARDENING §4
    // 2026-08-13).
    let Some(func_name) = printed_callee(&call.fun) else {
        return;
    };
    let Some(&(_, format_arg, alternative)) =
        FORMATTING_FUNCS.iter().find(|(name, _, _)| *name == func_name)
    else {
        return;
    };
    if call.args.len() <= format_arg {
        return;
    }
    let Expr::BasicLit(lit) = &call.args[format_arg] else {
        return;
    };
    if lit.kind != Some(Token::STRING) {
        return;
    }
    // revive v1.17.1 inspects the string's value, not its spelling, so a
    // directive written as an escape (`"\x25d"`) counts; a literal that does
    // not unquote is skipped.
    match guff_gostd::strconv::unquote(&lit.value) {
        Ok(format) if !format.contains('%') => {}
        _ => return,
    }
    failures.push(Failure {
        rule: "unnecessary-format",
        pos: call.fun.pos().0 as u32,
        message: format!(
            "unnecessary use of formatting function \"{func_name}\", you can replace it with {}",
            alternative
        ),
        ..Failure::default()
    });
}
