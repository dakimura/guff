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

/// Expand `ft.Results` fields into one type per return value (matching upstream).
fn result_types(pass: &Pass<'_>, ft: &FuncType) -> Option<Vec<TypeId>> {
    let results = ft.results.as_ref()?;
    let mut out = Vec::new();
    for field in &results.list {
        let ty_expr = field.ty.as_ref()?;
        let typ = type_of(pass, ty_expr)?;
        let n = if field.names.is_empty() {
            1
        } else {
            field.names.len()
        };
        for _ in 0..n {
            out.push(typ);
        }
    }
    Some(out)
}

fn check_return(
    pass: &Pass<'_>,
    ft: &FuncType,
    ret: &ReturnStmt,
    only_two: bool,
) -> Option<(u32, String)> {
    let types = result_types(pass, ft)?;
    let n = types.len();
    if ret.results.len() != n || n < 2 {
        return None;
    }
    if only_two && n != 2 {
        return None;
    }
    let last = n - 1;
    if !implements_error(pass, types[last]) {
        return None;
    }
    if !code::is_nil(pass, &ret.results[last]) {
        return None;
    }
    for i in 0..last {
        let Some(zv) = danger_zero(pass, types[i]) else {
            continue;
        };
        let hit = match zv {
            ZeroValue::Nil => code::is_nil(pass, &ret.results[i]),
            ZeroValue::Zero => is_zero_lit(&ret.results[i]),
        };
        if hit {
            return Some((ret.return_.0 as u32, NIL_NIL_MSG.into()));
        }
    }
    None
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
            // Upstream's callback `return false`s on **every** path out of the
            // ReturnStmt arm — after reporting, and on each of the rejections
            // before it — and `inspector.Nodes` reads that as "do not descend".
            // So a `return` the rule declines to judge takes its whole subtree
            // with it, function literals included:
            //
            //     return db.WithTx2(ctx, func(…) (*Comment, error) {
            //         return nil, nil          // never visited: the outer
            //     })                           // return has one result
            //
            // gitea writes six of those and golangci-lint reports none of them.
            let Some(ft) = enclosing_func_type(stack) else {
                return false;
            };
            if let Some(diag) = check_return(pass, ft, ret, opts.only_two) {
                pending.push(diag);
            }
            false
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
