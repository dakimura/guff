//! S1038 — unnecessarily complex way of printing formatted string.
//!
//! Port of `honnef.co/go/tools/simple/s1038`.
//!
//! **Parentheses.** Upstream states this check as a `pattern` query, and
//! `pattern.match` strips `*ast.ParenExpr` at every recursion (before binding),
//! so `f((x))` matches wherever `f(x)` does. This port descends by hand, so
//! every descent has to `unparen` — `compat/fuzz.py`'s `paren` mutation found
//! nine S-checks going quiet on a parenthesized subexpression at once
//! (COMPAT-HARDENING §4, 2026-08-13).
//!
//! **Three arms over the same node.** Upstream runs `fmtPrintf`, `methSprintf`
//! and `pkgSprintf` on every call expression. They are disjoint in practice:
//! the method arm needs a receiver that *has a type*, which a package
//! qualifier does not, and the package arm resolves its `Symbol` to a package
//! function, which a method is not.
//!
//! **Arity is part of the pattern.** `[x]` means exactly one argument and
//! `[_ x]` exactly two — which is also how `fmt.Fprint`/`fmt.Fprintln` are
//! expressed, their `io.Writer` being the `_`. An earlier revision read
//! `args[0]` whatever the length, so it reported
//! `fmt.Print(fmt.Sprintf(...), "b")` (upstream does not) and missed every
//! `fmt.Fprintln(w, fmt.Sprintf(...))` (upstream does).

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{call_name, is_call_to, type_func_name, unparen};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::ObjectData;
use guff_types::exprstring::expr_string;
use guff_types::lookup::{lookup_field_or_method, LookupResult};
use guff_types::OperandMode;

/// `checkSprintfMapping`, method half: `(FuncName, receiver, alternative)`.
///
/// Upstream keeps one map and two patterns read from it. The rows with a
/// receiver are reachable only through `methSprintf`, whose pattern requires a
/// `SelectorExpr`, and the receiver is compared against the `-f` method's own
/// `FuncName` so that a type merely *having* `Error` and `Errorf` is not
/// mistaken for `testing.TB`.
const METHOD_MAPPING: &[(&str, &str, &str)] = &[
    ("(*testing.common).Error", "(*testing.common)", "Errorf"),
    ("(testing.TB).Error", "(testing.TB)", "Errorf"),
    ("(*testing.common).Fatal", "(*testing.common)", "Fatalf"),
    ("(testing.TB).Fatal", "(testing.TB)", "Fatalf"),
    ("(*testing.common).Log", "(*testing.common)", "Logf"),
    ("(testing.TB).Log", "(testing.TB)", "Logf"),
    ("(*testing.common).Skip", "(*testing.common)", "Skipf"),
    ("(testing.TB).Skip", "(testing.TB)", "Skipf"),
    ("(*log.Logger).Fatal", "(*log.Logger)", "Fatalf"),
    ("(*log.Logger).Fatalln", "(*log.Logger)", "Fatalf"),
    ("(*log.Logger).Panic", "(*log.Logger)", "Panicf"),
    ("(*log.Logger).Panicln", "(*log.Logger)", "Panicf"),
    ("(*log.Logger).Print", "(*log.Logger)", "Printf"),
    ("(*log.Logger).Println", "(*log.Logger)", "Printf"),
];

/// The same map's package half, read by `pkgSprintf`.
const PKG_MAPPING: &[(&str, &str)] = &[
    ("log.Fatal", "log.Fatalf"),
    ("log.Fatalln", "log.Fatalf"),
    ("log.Panic", "log.Panicf"),
    ("log.Panicln", "log.Panicf"),
    ("log.Print", "log.Printf"),
    ("log.Println", "log.Printf"),
];

/// The selector names `checkTestingErrorSprintfQ` allows.
const METHOD_NAMES: &[&str] = &[
    "Error", "Fatal", "Fatalln", "Log", "Panic", "Panicln", "Print", "Println", "Skip",
];

/// The `fmt.Sprintf(...)` the pattern binds: argument `index` of a call with
/// exactly `arity` arguments.
fn sprintf_arg<'a>(
    pass: &Pass<'_>,
    call: &'a CallExpr,
    index: usize,
    arity: usize,
) -> Option<&'a CallExpr> {
    if call.args.len() != arity {
        return None;
    }
    let Expr::CallExpr(inner) = unparen(&call.args[index]) else {
        return None;
    };
    is_call_to(pass, inner, "fmt.Sprintf").then_some(inner)
}

/// The callee's `typeutil.FuncName`, which is what `code.CallName` hands the
/// mapping.
///
/// Not [`call_name`]: that is package path plus object name, so the method
/// `(*log.Logger).Print` comes back as `log.Print` — the same string as the
/// package function. An earlier revision matched the mapping with it and
/// answered `log.Printf(...) instead of log.Print(...)` for `l.Print(...)`,
/// where upstream says `l.Printf(...) instead of l.Print(...)`.
fn callee_func_name(pass: &Pass<'_>, fun: &Expr) -> Option<String> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let obj = match unparen(fun) {
        Expr::Ident(id) => info.uses.get(&id.id).copied(),
        Expr::SelectorExpr(sel) => info.uses.get(&sel.sel.id).copied(),
        _ => None,
    }?;
    if !matches!(artifacts.objects.get(obj), ObjectData::Func(_)) {
        return None;
    }
    Some(type_func_name(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        obj,
    ))
}

fn fmt_print_message(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    let name = call_name(pass, &call.fun)?;
    let short = name.strip_prefix("fmt.")?;
    // `fmt.Fprint`/`fmt.Fprintln` take the writer first; the pattern spells
    // that `[_ (CallExpr (Symbol "fmt.Sprintf") f:_)]`.
    let (index, arity) = match short {
        "Print" | "Sprint" | "Println" | "Sprintln" => (0, 1),
        "Fprint" | "Fprintln" => (1, 2),
        _ => return None,
    };
    let inner = sprintf_arg(pass, call, index, arity)?;
    let Some(base) = short.strip_suffix("ln") else {
        return Some(format!(
            "should use fmt.{short}f instead of fmt.{short}(fmt.Sprintf(...))"
        ));
    };
    // The `-ln` arms only fire on a literal format string: with an externally
    // provided one the caller cannot promise it ends in a newline, and
    // upstream would be suggesting a silent behaviour change.
    if !matches!(unparen(inner.args.first()?), Expr::BasicLit(_)) {
        return None;
    }
    Some(format!(
        "should use fmt.{base}f instead of fmt.{short}(fmt.Sprintf(...)) (but don't forget the newline)"
    ))
}

fn pkg_log_message(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    let key = callee_func_name(pass, &call.fun)?;
    let (_, alt) = PKG_MAPPING.iter().find(|(k, _)| *k == key)?;
    sprintf_arg(pass, call, 0, 1)?;
    Some(format!(
        "should use {alt}(...) instead of {key}(fmt.Sprintf(...))"
    ))
}

fn method_message(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    let Expr::SelectorExpr(sel) = unparen(&call.fun) else {
        return None;
    };
    if !METHOD_NAMES.contains(&sel.sel.name.as_str()) {
        return None;
    }
    sprintf_arg(pass, call, 0, 1)?;
    let key = callee_func_name(pass, &call.fun)?;
    let (_, recv_name, alt) = METHOD_MAPPING.iter().find(|(k, _, _)| *k == key)?;

    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    // `pass.TypesInfo.Types[recv]`. A package qualifier has no entry there,
    // which is how `log.Print(...)` reaches the package arm instead of this
    // one even though it is spelled as a selector.
    let recv = unparen(&sel.x);
    let tav = info.types.get(&recv.id())?;

    // "Ensure that Errorf/Fatalf refer to the right method": a type that
    // embeds `*testing.T` but defines its own `Errorf` is not one upstream
    // rewrites.
    let mut types = artifacts.types.clone();
    let found = lookup_field_or_method(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        tav.typ,
        tav.mode == OperandMode::Variable,
        None,
        alt,
    );
    let LookupResult::Found { obj, .. } = found else {
        return None;
    };
    if !matches!(artifacts.objects.get(obj), ObjectData::Func(_)) {
        return None;
    }
    let alt_key = type_func_name(&types, &artifacts.objects, &artifacts.packages, obj);
    if alt_key != format!("{recv_name}.{alt}") {
        return None;
    }
    Some(format!(
        "should use {recv_src}.{alt}(...) instead of {sel_src}(fmt.Sprintf(...))",
        recv_src = expr_string(recv),
        sel_src = expr_string(&Expr::SelectorExpr(sel.clone())),
    ))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1038 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        let msg = fmt_print_message(pass, call)
            .or_else(|| method_message(pass, call))
            .or_else(|| pkg_log_message(pass, call));
        if let Some(msg) = msg {
            pending.push((match_pos(node), msg));
        }
    });

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1038_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1038",
        doc: "unnecessarily complex way of printing formatted string",
        url: "https://staticcheck.dev/docs/checks/#S1038",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1038_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1038_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
