//! Port of [`github.com/Antonboom/nilnil`](https://github.com/Antonboom/nilnil).

use std::sync::OnceLock;

use guff::ast::{BasicLit, Expr, FuncType, ReturnStmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::api_predicates::api_implements;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::BasicKind;
use guff_types::predicates::is_interface;
use guff_types::TypeId;

const NIL_NIL_MSG: &str =
    "return both a `nil` error and an invalid value: use a sentinel error instead";

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// When true, only check functions with exactly two return values (upstream default).
    pub only_two: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { only_two: true }
    }
}

#[derive(Clone, Copy)]
enum ZeroValue {
    Nil,
    Zero,
}

fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn universe_error(pass: &Pass<'_>) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    for oid in artifacts.objects.ids() {
        let ObjectData::TypeName(tn) = artifacts.objects.get(oid) else {
            continue;
        };
        if tn.name() != "error" {
            continue;
        }
        if oid.pkg(&artifacts.objects).is_some() {
            continue;
        }
        return tn.typ();
    }
    None
}

fn implements_error(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    if !is_interface(&artifacts.types, typ) {
        return false;
    }
    if code::type_with_name(pass, typ, "error") {
        return true;
    }
    let Some(err) = universe_error(pass) else {
        return false;
    };
    let mut types = artifacts.types.clone();
    api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        err,
    )
}

fn danger_zero(pass: &Pass<'_>, typ: TypeId) -> Option<ZeroValue> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let u = typ.underlying(&artifacts.types);
    match artifacts.types.get(u) {
        TypeData::Pointer(_) => Some(ZeroValue::Nil),
        TypeData::Signature(_) => Some(ZeroValue::Nil),
        TypeData::Interface(_) => Some(ZeroValue::Nil),
        TypeData::Map(_) => Some(ZeroValue::Nil),
        TypeData::Chan(_) => Some(ZeroValue::Nil),
        TypeData::Basic(b) if b.kind() == BasicKind::Uintptr => Some(ZeroValue::Zero),
        TypeData::Basic(b) if b.kind() == BasicKind::UnsafePointer => Some(ZeroValue::Nil),
        _ => None,
    }
}

/// Parse Go integer literal text the way `strconv.ParseInt(s, 0, 64)` does.
fn parse_go_int(s: &str) -> Option<i64> {
    let s = s.trim_start_matches('+');
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return i64::from_str_radix(rest, 16).ok();
    }
    if let Some(rest) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
        return i64::from_str_radix(rest, 8).ok();
    }
    if let Some(rest) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        return i64::from_str_radix(rest, 2).ok();
    }
    s.parse().ok()
}

fn is_zero_lit(e: &Expr) -> bool {
    let Expr::BasicLit(BasicLit { kind, value, .. }) = e else {
        return false;
    };
    if kind != &Some(Token::INT) {
        return false;
    }
    parse_go_int(value).is_some_and(|v| v == 0)
}

/// One result type per **field** of `ft.Results`, not per return value.
///
/// Upstream indexes `ft.Results.List` directly and compares its length with
/// `len(v.Results)`, so a grouped list is one entry however many names it
/// carries: `func () (a, b error)` has *one* field against *two* returned
/// expressions and is dropped before anything is checked, while
/// `func () (a error, b error)` has two and is checked. Expanding the names —
/// which is what guff did — reports the first form, and golangci-lint does not.
fn result_field_types(pass: &Pass<'_>, ft: &FuncType) -> Option<Vec<TypeId>> {
    let results = ft.results.as_ref()?;
    let mut out = Vec::with_capacity(results.list.len());
    for field in &results.list {
        out.push(type_of(pass, field.ty.as_ref()?)?);
    }
    Some(out)
}

/// What the walk should do with a `return` after looking at it.
///
/// Upstream's callback is an `inspector.Nodes` visitor, so its `bool` is "look
/// inside this node", and the `ReturnStmt` arm answers `false` on each of its
/// rejections **and after reporting** — but falls through to `true` when it
/// checked the return and found nothing. That last path is the one guff was
/// missing: it stopped on every return it declined to report, and a function
/// literal written inside such a return was never visited.
enum Outcome {
    /// Do not descend (upstream's `return false`).
    Stop,
    /// Nothing to report here, but keep walking the subtree.
    Descend,
    Report(u32, String),
}

fn check_return(
    pass: &Pass<'_>,
    ft: &FuncType,
    ret: &ReturnStmt,
    only_two: bool,
) -> Outcome {
    if ret.results.len() < 2 {
        return Outcome::Stop;
    }
    let Some(types) = result_field_types(pass, ft) else {
        return Outcome::Stop;
    };
    if types.len() != ret.results.len() {
        return Outcome::Stop;
    }
    // `only-two` does not skip the function, it pins the error slot to index 1
    // (upstream `lastIdx = 1`), so a third result is simply never looked at.
    let last = if only_two { 1 } else { types.len() - 1 };
    if !implements_error(pass, types[last]) {
        return Outcome::Stop;
    }
    // The error operand is part of the report condition, not a precondition:
    // a `return v, err` with a non-nil `err` reports nothing *and keeps
    // walking*, which is how a literal in `v` is reached.
    let err_is_nil = code::is_nil(pass, &ret.results[last]);
    for i in 0..last {
        let Some(zv) = danger_zero(pass, types[i]) else {
            continue;
        };
        let hit = err_is_nil
            && match zv {
                ZeroValue::Nil => code::is_nil(pass, &ret.results[i]),
                ZeroValue::Zero => is_zero_lit(&ret.results[i]),
            };
        if hit {
            return Outcome::Report(ret.return_.0 as u32, NIL_NIL_MSG.into());
        }
    }
    Outcome::Descend
}

fn enclosing_func_type<'a>(stack: &[NodeRef<'a>]) -> Option<&'a FuncType> {
    stack.iter().rev().find_map(|n| match n {
        NodeRef::FuncDecl(fd) => Some(&fd.ty),
        NodeRef::FuncLit(fl) => Some(&fl.ty),
        _ => None,
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "nilnil requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<Options>("nilnil")
        .copied()
        .unwrap_or_default();

    let mut pending = Vec::new();
    for file in pass.files() {
        let mut stack = Vec::new();
        walk::preorder_stack(NodeRef::File(file), &mut stack, |n, stack| {
            let NodeRef::ReturnStmt(ret) = n else {
                return true;
            };
            // The `bool` is "look inside this return". Upstream says no on each
            // rejection and after reporting, and yes when it checked the return
            // and found nothing — so gitea's
            //
            //     return db.WithTx2(ctx, func(…) (*Comment, error) {
            //         return nil, nil          // never visited: the outer
            //     })                           // return has one result
            //
            // stays quiet (six of those, and golangci-lint reports none), while
            // k6's
            //
            //     return promise(vu, func() (any, error) {
            //         return nil, nil          // visited: the outer return was
            //     }), nil                      // checked and cleared
            //
            // is reported. guff answered no to both.
            let Some(ft) = enclosing_func_type(stack) else {
                return false;
            };
            match check_return(pass, ft, ret, opts.only_two) {
                Outcome::Report(pos, message) => {
                    pending.push((pos, message));
                    false
                }
                Outcome::Stop => false,
                Outcome::Descend => true,
            }
        });
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "nilnil",
        doc: "checks that there is no simultaneous return of nil error and an invalid value",
        url: "https://github.com/Antonboom/nilnil",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff::Pos;

    #[test]
    fn zero_literals() {
        let lit = |v: &str| {
            Expr::BasicLit(BasicLit {
                value_pos: Pos::default(),
                value_end: Pos::default(),
                kind: Some(Token::INT),
                value: v.into(),
                id: 0,
            })
        };
        assert!(is_zero_lit(&lit("0")));
        assert!(is_zero_lit(&lit("0x0")));
        assert!(is_zero_lit(&lit("0b0")));
        assert!(is_zero_lit(&lit("0o0")));
        assert!(!is_zero_lit(&lit("1")));
    }
}
