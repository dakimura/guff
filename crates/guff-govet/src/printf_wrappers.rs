//! `findPrintLike` — the half of `go/analysis/passes/printf` that decides
//! *which* functions are printf-like in the first place.
//!
//! Upstream answers that question in three ways, in this order
//! (`printf.callKind`):
//!
//! 1. an allowlist keyed on `types.Func.FullName()` ([`IS_PRINT`]),
//! 2. an object **fact** imported from a dependency,
//! 3. an induction over the package being analysed: a function whose last
//!    parameter is `args ...any` and whose body forwards `args...` to something
//!    already known to be printf-like *is itself* printf-like, and so are its
//!    own callers, transitively.
//!
//! guff had none of (3) and instead guessed from the name: any callee whose
//! base name was one of `Printf`/`Sprintf`/`Fprintf`/`Errorf`/`Fatalf`/`Panicf`
//! was checked as a printf call. The guess is wrong in both directions, and a
//! two-package reproducer measured both halves against golangci-lint 2.12.2:
//!
//! ```text
//! guff-only (false positives — a name that ends in `f` but forwards nothing)
//!   (*sugar).Panicf("a %w", err)   sugar.Panicf calls s.log(0, template, args)
//!   (quiet).Errorf("d %z", 1)      body ignores both parameters
//!   (quiet).Printf("i %z", 1)      body ignores both parameters
//! golangci-only (false negatives — a real wrapper guff could not name)
//!   wrapf("b %w", err)             func wrapf(f string, a ...any) { fmt.Printf(f, a...) }
//!   badForward(...)                "missing ... in args forwarded to printf-like function"
//!   wrapf("f %z", 1)               unknown verb, reached through the wrapper
//!   litf("j %z", 1)                var litf = func(f string, a ...any) { fmt.Printf(f, a...) }
//!   hop1("k %z", 1)                a wrapper whose callee is another wrapper
//! ```
//!
//! The two answers were disjoint: six against three, nothing in common. On the
//! corpus the same defect stood behind velero's only remaining diff
//! (`(logrus.FieldLogger).Errorf` — an imported *interface* method, which has
//! no fact and no body, so upstream never checks it) and behind fifteen of
//! Tekton pipeline's (`(*zap.SugaredLogger).Panicf`, which forwards `args` as a
//! slice, not as `args...`, so upstream does not call it a wrapper).
//!
//! # Where the induction stops
//!
//! Like the no-return induction in `ctrlflow`, this runs *inside* a package:
//! guff type-checks only the packages being linted, so there is no fact to
//! import for a wrapper declared in a dependency, and advertising `fact_types`
//! on `printf` would schedule it across every transitive import for facts that
//! could not be produced anyway. What that costs is one shape in the grid above
//! — `sub.ExportedWrapf("g %z", 1)`, a wrapper in a *sibling* package, reported
//! by upstream and silent here. Silence is the direction guff already had.
//!
//! DEFERRED, both measured rather than guessed:
//! - **interface-method induction.** Upstream models an interface method as an
//!   implicit call to every implementing method found by the `satisfy` pass, so
//!   `Logger.Logf` becomes printf-like when `myLogger.Logf` is. It is gated on
//!   the *file's* language version being go1.26, which the corpus does reach
//!   (Tekton pipeline is `go 1.26.4`). Porting it needs `satisfy.Finder`, which
//!   is its own pass; without it a call through such an interface is not
//!   checked. That is the same silence as before this module existed.
//! - **`-printf.funcs`.** Upstream's second allowlist lookup is on the
//!   lower-cased base name, which is empty unless the flag adds to it. guff
//!   does not read `govet.settings.printf.funcs`, so the lookup would always
//!   miss and is not performed.
//! - **`checkPrint`.** guff checks formatted calls only. [`Kind::Print`] is
//!   still modelled, because it decides whether a forwarding call is
//!   well-formed and what `missing ...` says, but a print-kind *call site* is
//!   not inspected — as before.

use std::collections::HashMap;

use guff::ast::{BlockStmt, CallExpr, Expr, File};
use guff::walk::{inspect, NodeRef};
use guff_analysis::code::call_target_object;
use guff_analysis::Pass;
use guff_types::arena::{ObjectData, ObjectId, TypeData};
use guff_types::basic::BasicKind;
use guff_types::signature::{signature_params, signature_variadic};
use guff_types::tuple::tuple_len;

/// Upstream's `printf.Kind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    /// Not print-like.
    None,
    /// Behaves like `fmt.Print`.
    Print,
    /// Behaves like `fmt.Printf`.
    Printf,
    /// Behaves like `fmt.Errorf` — the one kind where `%w` is legal.
    Errorf,
}

impl Kind {
    /// Upstream's `Kind.String()`, which names the kind in the `missing ...`
    /// diagnostic.
    fn as_str(self) -> &'static str {
        match self {
            Kind::Print => "print",
            Kind::Printf => "printf",
            Kind::Errorf => "errorf",
            Kind::None => "(none)",
        }
    }
}

/// Upstream's `isPrint`, verbatim: the standard-library functions whose kind is
/// known without analysing their bodies. Keys are `types.Func.FullName()`, so a
/// method carries its receiver. A key ending in `f` is a formatted print.
///
/// Upstream keeps the unformatted half even though it can deduce most of it,
/// "because go vet does not [analyse std] with gccgo". guff keeps it for a
/// different reason: it is what makes a wrapper of `fmt.Println` a
/// [`Kind::Print`] wrapper rather than an unclassified one, which is what
/// decides whether its forwarding call is reported as `missing ...`.
const IS_PRINT: &[&str] = &[
    "fmt.Appendf",
    "fmt.Append",
    "fmt.Appendln",
    "fmt.Errorf",
    "fmt.Fprint",
    "fmt.Fprintf",
    "fmt.Fprintln",
    "fmt.Print",
    "fmt.Printf",
    "fmt.Println",
    "fmt.Sprint",
    "fmt.Sprintf",
    "fmt.Sprintln",
    "runtime/trace.Logf",
    "log.Print",
    "log.Printf",
    "log.Println",
    "log.Fatal",
    "log.Fatalf",
    "log.Fatalln",
    "log.Panic",
    "log.Panicf",
    "log.Panicln",
    "(*log.Logger).Fatal",
    "(*log.Logger).Fatalf",
    "(*log.Logger).Fatalln",
    "(*log.Logger).Panic",
    "(*log.Logger).Panicf",
    "(*log.Logger).Panicln",
    "(*log.Logger).Print",
    "(*log.Logger).Printf",
    "(*log.Logger).Println",
    "(*testing.common).Error",
    "(*testing.common).Errorf",
    "(*testing.common).Fatal",
    "(*testing.common).Fatalf",
    "(*testing.common).Log",
    "(*testing.common).Logf",
    "(*testing.common).Skip",
    "(*testing.common).Skipf",
    "(testing.TB).Error",
    "(testing.TB).Errorf",
    "(testing.TB).Fatal",
    "(testing.TB).Fatalf",
    "(testing.TB).Log",
    "(testing.TB).Logf",
    "(testing.TB).Skip",
    "(testing.TB).Skipf",
];

/// The kind of a callee that the allowlist alone can answer.
///
/// Upstream's `callKind` less the fact lookup and the memo, which
/// [`Wrappers::kind_of`] owns.
fn allowlist_kind(full_name: &str, base_name: &str) -> Kind {
    if !IS_PRINT.contains(&full_name) {
        return Kind::None;
    }
    if full_name == "fmt.Errorf" {
        Kind::Errorf
    } else if base_name.ends_with('f') {
        Kind::Printf
    } else {
        Kind::Print
    }
}

/// A candidate print/printf wrapper: a function (or a variable holding a
/// function literal) whose last parameter is `args ...any`.
struct Wrapper<'a> {
    obj: ObjectId,
    /// The body to scan, or `None` for a candidate with no body.
    body: Option<&'a BlockStmt>,
    /// The `format string` parameter, if the signature has one. `None` means
    /// the candidate can only ever be print-like.
    format: Option<ObjectId>,
    /// The `args ...any` parameter.
    args: ObjectId,
    /// Candidates that forward to this one: `(wrapper index, forwarding call)`.
    callers: Vec<(usize, Option<usize>)>,
}

/// What [`find_print_like`] learned about the package: the kind of every
/// callee it has an answer for, plus a memo for the allowlist.
pub(crate) struct Wrappers {
    kinds: HashMap<ObjectId, Kind>,
}

impl Wrappers {
    /// Upstream's `callKind`: the memo, then the allowlist. (There is no fact
    /// lookup — see the module comment.)
    pub(crate) fn kind_of(&mut self, full_name: &str, base_name: &str, obj: ObjectId) -> Kind {
        if let Some(&k) = self.kinds.get(&obj) {
            return k;
        }
        let kind = allowlist_kind(full_name, base_name);
        self.kinds.insert(obj, kind);
        kind
    }
}

/// Upstream's `formatArgsParams`: the `format string` and `args ...any`
/// parameters of a potential wrapper, or `None` if the signature has no
/// `...any` tail.
fn format_args_params(pass: &Pass<'_>, sig: guff_types::TypeId) -> Option<(Option<ObjectId>, ObjectId)> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let types = &artifacts.types;
    if !signature_variadic(types, sig) {
        return None;
    }
    let params = signature_params(types, sig)?;
    let n = tuple_len(types, Some(params));
    if n == 0 {
        return None;
    }
    let TypeData::Tuple(tup) = types.get(params) else {
        return None;
    };

    // Is the second-last parameter exactly the predeclared `string`? Upstream
    // compares against `types.Typ[types.String]` by pointer, so a defined type
    // whose underlying type is `string` does not count.
    let mut format = None;
    if n >= 2 {
        let p = tup.at(n - 2);
        if let Some(t) = p.typ(&artifacts.objects) {
            let t = guff_types::alias::unalias_readonly(types, t);
            if matches!(types.get(t), TypeData::Basic(b) if b.kind() == BasicKind::String) {
                format = Some(p);
            }
        }
    }

    // The last parameter must be `...any`: a slice whose element type is the
    // empty interface written inline (or an alias to it, which is what `any`
    // is). A *defined* empty interface is a `Named`, and upstream's type
    // assertion rejects it too.
    let args = tup.at(n - 1);
    let args_type = args.typ(&artifacts.objects)?;
    let TypeData::Slice(s) = types.get(guff_types::alias::unalias_readonly(types, args_type)) else {
        return None;
    };
    let elem = guff_types::alias::unalias_readonly(types, s.elem());
    match types.get(elem) {
        TypeData::Interface(i) if i.num_explicit_methods() == 0 && i.num_embeddeds() == 0 => {}
        _ => return None,
    }

    Some((format, args))
}

/// `match(info, arg, param)`: is `arg` the identifier that denotes `param`?
fn matches_param(pass: &Pass<'_>, arg: &Expr, param: Option<ObjectId>) -> bool {
    let Some(param) = param else {
        return false;
    };
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Expr::Ident(id) = arg else {
        return false;
    };
    info.uses.get(&id.id).copied() == Some(param)
        || info.defs.get(&id.id).copied().flatten() == Some(param)
}

/// `types.Func.FullName()` and the bare object name of a callee object.
pub(crate) fn names_of(pass: &Pass<'_>, obj: ObjectId) -> Option<(String, String)> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let base = obj.name(&artifacts.objects).to_string();
    let full = match artifacts.objects.get(obj) {
        ObjectData::Func(_) => guff_analysis::code::type_func_name(
            &artifacts.types,
            &artifacts.objects,
            &artifacts.packages,
            obj,
        ),
        // Upstream's `fullname` answers `obj.Name()` for a non-Func, which is
        // how a function literal held in a variable is named.
        _ => base.clone(),
    };
    Some((full, base))
}

/// Upstream's `findPrintLike`.
///
/// Returns the kinds it settled on and the `missing ...` diagnostics it found
/// on the way (position, message) — those belong to the `printf` analyzer, not
/// to a separate pass, because that is where golangci-lint attributes them.
pub(crate) fn find_print_like(pass: &Pass<'_>) -> (Wrappers, Vec<(u32, String)>) {
    let mut state = Wrappers {
        kinds: HashMap::new(),
    };
    let mut diags: Vec<(u32, String)> = Vec::new();
    let Some(_) = pass.types_info() else {
        return (state, diags);
    };

    // Pass 1: gather candidates.
    let mut wrappers: Vec<Wrapper<'_>> = Vec::new();
    let mut by_obj: HashMap<ObjectId, usize> = HashMap::new();
    collect_candidates(pass, pass.files(), &mut wrappers, &mut by_obj);

    // Every forwarding call, kept out of the wrapper structs so that the graph
    // can be walked while the calls stay borrowed from the AST.
    let mut calls: Vec<&CallExpr> = Vec::new();

    // Pass 2: scan each candidate's body for calls that forward `args`.
    for wi in 0..wrappers.len() {
        let Some(body) = wrappers[wi].body else {
            // An interface method has no body. Upstream treats it as an
            // implicit call to each implementing method; see the DEFERRED note.
            continue;
        };
        scan_body(pass, wi, body, &mut wrappers, &by_obj, &mut calls, &mut state, &mut diags);
    }

    (state, diags)
}

fn collect_candidates<'a>(
    pass: &Pass<'_>,
    files: &'a [File],
    wrappers: &mut Vec<Wrapper<'a>>,
    by_obj: &mut HashMap<ObjectId, usize>,
) {
    let Some(info) = pass.types_info() else {
        return;
    };
    let mut add = |obj: ObjectId, sig: guff_types::TypeId, body: Option<&'a BlockStmt>| {
        if by_obj.contains_key(&obj) {
            return;
        }
        if let Some((format, args)) = format_args_params(pass, sig) {
            by_obj.insert(obj, wrappers.len());
            wrappers.push(Wrapper {
                obj,
                body,
                format,
                args,
                callers: Vec::new(),
            });
        }
    };

    for file in files {
        inspect(NodeRef::File(file), |n| {
            let Some(n) = n else { return true };
            match n {
                // A named function or method: `func wrapf(format string, args ...any) {…}`.
                NodeRef::FuncDecl(d) => {
                    if let Some(body) = d.body.as_ref() {
                        if let Some(Some(obj)) = info.defs.get(&d.name.id).copied() {
                            if let Some(art) = pass.pkg().type_artifacts.as_ref() {
                                if let Some(sig) = obj.typ(&art.objects) {
                                    add(obj, sig, Some(body));
                                }
                            }
                        }
                    }
                }
                // A function literal assigned to a variable, a struct field or
                // an imported var — the four spellings upstream accepts.
                NodeRef::ValueSpec(spec) => {
                    for (i, val) in spec.values.iter().enumerate() {
                        let Expr::FuncLit(lit) = val else { continue };
                        let Some(name) = spec.names.get(i) else { continue };
                        let obj = info
                            .defs
                            .get(&name.id)
                            .copied()
                            .flatten()
                            .or_else(|| info.uses.get(&name.id).copied());
                        let (Some(obj), Some(tv)) = (obj, info.types.get(&val.id())) else {
                            continue;
                        };
                        add(obj, tv.typ, Some(&lit.body));
                    }
                }
                NodeRef::AssignStmt(stmt) => {
                    for (i, rhs) in stmt.rhs.iter().enumerate() {
                        let Expr::FuncLit(lit) = rhs else { continue };
                        let Some(lhs) = stmt.lhs.get(i) else { continue };
                        let obj = match lhs {
                            Expr::Ident(id) => info
                                .defs
                                .get(&id.id)
                                .copied()
                                .flatten()
                                .or_else(|| info.uses.get(&id.id).copied()),
                            Expr::SelectorExpr(sel) => info
                                .selections
                                .get(&sel.id)
                                .map(|s| s.obj())
                                .or_else(|| info.uses.get(&sel.sel.id).copied()),
                            _ => None,
                        };
                        let (Some(obj), Some(tv)) = (obj, info.types.get(&rhs.id())) else {
                            continue;
                        };
                        add(obj, tv.typ, Some(&lit.body));
                    }
                }
                _ => {}
            }
            true
        });
    }
}

/// Upstream's pass-2 `scan` loop over one candidate's body.
#[allow(clippy::too_many_arguments)]
fn scan_body<'a>(
    pass: &Pass<'_>,
    wi: usize,
    body: &'a BlockStmt,
    wrappers: &mut Vec<Wrapper<'a>>,
    by_obj: &HashMap<ObjectId, usize>,
    calls: &mut Vec<&'a CallExpr>,
    state: &mut Wrappers,
    diags: &mut Vec<(u32, String)>,
) {
    let format = wrappers[wi].format;
    let args = wrappers[wi].args;
    let mut stopped = false;
    let mut forwards: Vec<&'a CallExpr> = Vec::new();

    inspect(NodeRef::BlockStmt(body), |n| {
        if stopped {
            return false;
        }
        let Some(n) = n else { return true };
        match n {
            // A wrapper that reassigns `format` or `args` is not a simple
            // wrapper: stop scanning here, as upstream's `break scan` does.
            NodeRef::AssignStmt(s) => {
                for lhs in &s.lhs {
                    if matches_param(pass, lhs, format) || matches_param(pass, lhs, Some(args)) {
                        stopped = true;
                        return false;
                    }
                }
            }
            // …and so is one that takes their address.
            NodeRef::UnaryExpr(u) => {
                if u.op == guff::token::Token::AND
                    && (matches_param(pass, &u.x, format) || matches_param(pass, &u.x, Some(args)))
                {
                    stopped = true;
                    return false;
                }
            }
            NodeRef::CallExpr(call) => {
                if let Some(last) = call.args.last() {
                    if matches_param(pass, last, Some(args)) {
                        forwards.push(call);
                    }
                }
            }
            _ => {}
        }
        true
    });

    for call in forwards {
        let Some(callee) = call_target_object(pass, &call.fun) else {
            continue;
        };
        let ci = calls.len();
        calls.push(call);
        do_call(pass, wi, callee, ci, calls, wrappers, by_obj, state, diags);
    }
}

/// Upstream's `doCall`: record the edge, and if the callee is already known to
/// be print-like, propagate that backwards through the graph.
#[allow(clippy::too_many_arguments)]
fn do_call<'a>(
    pass: &Pass<'_>,
    wi: usize,
    callee: ObjectId,
    call_index: usize,
    calls: &[&'a CallExpr],
    wrappers: &mut Vec<Wrapper<'a>>,
    by_obj: &HashMap<ObjectId, usize>,
    state: &mut Wrappers,
    diags: &mut Vec<(u32, String)>,
) {
    if let Some(&w2) = by_obj.get(&callee) {
        wrappers[w2].callers.push((wi, Some(call_index)));
    }
    let Some((full, base)) = names_of(pass, callee) else {
        return;
    };
    let kind = state.kind_of(&full, &base, callee);
    if kind != Kind::None {
        propagate(pass, wi, Some(call_index), kind, calls, wrappers, state, diags);
    }
}

/// Upstream's `propagate`: a well-formed forwarding call makes the caller the
/// same kind as the callee, and that travels back to *its* callers.
#[allow(clippy::too_many_arguments)]
fn propagate<'a>(
    pass: &Pass<'_>,
    wi: usize,
    call_index: Option<usize>,
    kind: Kind,
    calls: &[&'a CallExpr],
    wrappers: &mut Vec<Wrapper<'a>>,
    state: &mut Wrappers,
    diags: &mut Vec<(u32, String)>,
) {
    if let Some(ci) = call_index {
        if !check_forward(pass, wi, calls[ci], kind, wrappers, diags) {
            return;
        }
    }
    let obj = wrappers[wi].obj;
    if state.kinds.get(&obj).copied().unwrap_or(Kind::None) == kind {
        return;
    }
    state.kinds.insert(obj, kind);
    let callers = wrappers[wi].callers.clone();
    for (caller, caller_call) in callers {
        propagate(pass, caller, caller_call, kind, calls, wrappers, state, diags);
    }
}

/// Upstream's `checkForward`. A forwarding call only counts when the format
/// string is delegated too, and when `args` is spread with `...`; the second
/// failure is what the user usually meant to write, so it is reported.
fn check_forward(
    pass: &Pass<'_>,
    wi: usize,
    call: &CallExpr,
    kind: Kind,
    wrappers: &[Wrapper<'_>],
    diags: &mut Vec<(u32, String)>,
) -> bool {
    if matches!(kind, Kind::Printf | Kind::Errorf) {
        let n = call.args.len();
        if n < 2 || !matches_param(pass, &call.args[n - 2], wrappers[wi].format) {
            return false;
        }
    }

    if call.ellipsis.0 == 0 {
        let Some(sig) = crate::govet_util::expr_type(pass, &call.fun) else {
            return false;
        };
        let Some(art) = pass.pkg().type_artifacts.as_ref() else {
            return false;
        };
        let nparams = signature_params(&art.types, sig)
            .map(|p| tuple_len(&art.types, Some(p)))
            .unwrap_or(0);
        if call.args.len() > nparams {
            // Adding `...` here would not compile: the wrapper is passing more
            // arguments than the callee can take.
            return false;
        }
        diags.push((
            call.pos().0 as u32,
            format!(
                "missing ... in args forwarded to {}-like function",
                kind.as_str()
            ),
        ));
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_kind_splits_errorf_from_printf_from_print() {
        // `fmt.Errorf` is the only member of the table that is KindErrorf: it
        // is the only one where `%w` is legal. guff used to answer "errorf" for
        // any name ending in `Errorf`, which silently excused `%w` in
        // `t.Errorf` and in every third-party logger's `Errorf`.
        assert_eq!(allowlist_kind("fmt.Errorf", "Errorf"), Kind::Errorf);
        assert_eq!(
            allowlist_kind("(*testing.common).Errorf", "Errorf"),
            Kind::Printf
        );
        assert_eq!(allowlist_kind("(testing.TB).Errorf", "Errorf"), Kind::Printf);

        // A name that ends in `f` is formatted; one that does not is not.
        assert_eq!(allowlist_kind("log.Panicf", "Panicf"), Kind::Printf);
        assert_eq!(allowlist_kind("log.Panic", "Panic"), Kind::Print);
        assert_eq!(allowlist_kind("fmt.Println", "Println"), Kind::Print);

        // Membership is on the full name, receiver and all: a `Panicf` method
        // on somebody else's logger is not in the table, and nothing about the
        // name may put it there.
        assert_eq!(
            allowlist_kind("(*go.uber.org/zap.SugaredLogger).Panicf", "Panicf"),
            Kind::None
        );
        assert_eq!(
            allowlist_kind("(github.com/sirupsen/logrus.FieldLogger).Errorf", "Errorf"),
            Kind::None
        );
        assert_eq!(allowlist_kind("example.com/pw.wrapf", "wrapf"), Kind::None);
    }

    #[test]
    fn kind_names_match_upstreams_diagnostic() {
        // The `missing ...` message quotes the kind by this name.
        assert_eq!(Kind::Printf.as_str(), "printf");
        assert_eq!(Kind::Print.as_str(), "print");
        assert_eq!(Kind::Errorf.as_str(), "errorf");
    }
}
