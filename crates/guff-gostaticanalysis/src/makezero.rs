//! Port of [`github.com/ashanbrown/makezero`](https://github.com/ashanbrown/makezero).

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{AssignStmt, BasicLit, CallExpr, Expr, Ident};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::{ObjectId, TypeData};
use guff_types::TypeId;

fn unparen(e: &Expr) -> &Expr {
    let mut cur = e;
    while let Expr::ParenExpr(p) = cur {
        cur = &p.x;
    }
    cur
}

fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn is_slice_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    matches!(
        artifacts.types.get(typ.underlying(&artifacts.types)),
        TypeData::Slice(_)
    )
}

fn is_explicit_zero_len(e: &Expr) -> bool {
    matches!(
        unparen(e),
        Expr::BasicLit(BasicLit {
            kind: Some(Token::INT),
            value,
            ..
        }) if value == "0"
    )
}

fn is_make_call(e: &Expr) -> Option<&CallExpr> {
    let Expr::CallExpr(call) = unparen(e) else {
        return None;
    };
    let Expr::Ident(Ident { name, .. }) = unparen(&call.fun) else {
        return None;
    };
    if name != "make" {
        return None;
    }
    Some(call)
}

fn is_append_call(e: &CallExpr) -> bool {
    matches!(unparen(&e.fun), Expr::Ident(Ident { name, .. }) if name == "append")
}

fn def_object(pass: &Pass<'_>, ident: &Ident) -> Option<ObjectId> {
    let info = pass.types_info()?;
    info.defs.get(&ident.id).copied().flatten()
}

fn use_object(pass: &Pass<'_>, ident: &Ident) -> Option<ObjectId> {
    let info = pass.types_info()?;
    info.uses.get(&ident.id).copied()
}

fn record_nonzero_make(pass: &Pass<'_>, lhs: &Expr, call: &CallExpr, out: &mut HashSet<ObjectId>) {
    // Only `make([]T, len)` (exactly 2 args). Cap-only form has 3 args.
    if call.args.len() != 2 {
        return;
    }
    let Some(elem_ty) = type_of(pass, &call.args[0]) else {
        return;
    };
    if !is_slice_type(pass, elem_ty) {
        return;
    }
    if is_explicit_zero_len(&call.args[1]) {
        return;
    }
    let Expr::Ident(ident) = unparen(lhs) else {
        return;
    };
    if let Some(obj) = def_object(pass, ident).or_else(|| use_object(pass, ident)) {
        out.insert(obj);
    }
}

fn check_assign(pass: &Pass<'_>, s: &AssignStmt, nonzero: &mut HashSet<ObjectId>) {
    for (i, right) in s.rhs.iter().enumerate() {
        let Some(call) = is_make_call(right) else {
            continue;
        };
        let Some(left) = s.lhs.get(i) else {
            continue;
        };
        record_nonzero_make(pass, left, call, nonzero);
    }
}

fn check_append(
    pass: &Pass<'_>,
    call: &CallExpr,
    nonzero: &HashSet<ObjectId>,
    pending: &mut Vec<(u32, String)>,
) {
    if !is_append_call(call) || call.args.is_empty() {
        return;
    }
    let Expr::Ident(slice) = unparen(&call.args[0]) else {
        return;
    };
    let Some(obj) = use_object(pass, slice).or_else(|| def_object(pass, slice)) else {
        return;
    };
    if !nonzero.contains(&obj) {
        return;
    }
    let Expr::Ident(fun) = unparen(&call.fun) else {
        return;
    };
    pending.push((
        fun.name_pos.0 as u32,
        format!(
            "append to slice `{}` with non-zero initialized length",
            slice.name
        ),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "makezero requires inspect analyzer".to_string())?;

    let mut nonzero: HashSet<ObjectId> = HashSet::new();
    let mut pending = Vec::new();

    // First pass: record non-zero-length slice makes.
    // Second pass: flag appends. Two passes keep order independent of
    // declaration vs use across siblings.
    for file in pass.files() {
        walk::preorder(NodeRef::File(file), |n| {
            if let NodeRef::AssignStmt(s) = n {
                check_assign(pass, s, &mut nonzero);
            }
            true
        });
    }
    for file in pass.files() {
        walk::preorder(NodeRef::File(file), |n| {
            if let NodeRef::CallExpr(call) = n {
                check_append(pass, call, &nonzero, &mut pending);
            }
            true
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
        name: "makezero",
        doc: "finds slice declarations with non-zero initial length and later appends",
        url: "https://github.com/ashanbrown/makezero",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
