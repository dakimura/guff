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

/// Pass-time options from `linters.settings.makezero`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MakezeroOptions {
    /// Upstream `-always` / `initLenMustBeZero`: report every non-empty slice
    /// initialization, not only the ones a later `append` reaches.
    pub always: bool,
}

fn record_nonzero_make(pass: &Pass<'_>, lhs: &Expr, call: &CallExpr, out: &mut HashSet<ObjectId>) -> bool {
    // Only `make([]T, len)` (exactly 2 args). Cap-only form has 3 args.
    if call.args.len() != 2 {
        return false;
    }
    let Some(elem_ty) = type_of(pass, &call.args[0]) else {
        return false;
    };
    if !is_slice_type(pass, elem_ty) {
        return false;
    }
    if is_explicit_zero_len(&call.args[1]) {
        return false;
    }
    let Expr::Ident(ident) = unparen(lhs) else {
        return false;
    };
    if let Some(obj) = def_object(pass, ident).or_else(|| use_object(pass, ident)) {
        out.insert(obj);
    }
    true
}

fn check_assign(
    pass: &Pass<'_>,
    s: &AssignStmt,
    nonzero: &mut HashSet<ObjectId>,
    always: bool,
    pending: &mut Vec<(u32, String)>,
) {
    for (i, right) in s.rhs.iter().enumerate() {
        let Some(call) = is_make_call(right) else {
            continue;
        };
        let Some(left) = s.lhs.get(i) else {
            continue;
        };
        let recorded = record_nonzero_make(pass, left, call, nonzero);
        // `MustHaveNonZeroInitLenIssue` — upstream's second message, raised
        // for the same shape the first one only reaches through a later
        // `append`. Its position is `node.Pos()` where `node` is the whole
        // AssignStmt, so it lands on the name being assigned.
        if recorded && always {
            if let Expr::Ident(ident) = unparen(left) {
                pending.push((
                    s.lhs
                        .first()
                        .map(|e| e.pos().0 as u32)
                        .unwrap_or(s.tok_pos.0 as u32),
                    format!("slice `{}` does not have non-zero initial length", ident.name),
                ));
            }
        }
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

    let opts = pass
        .settings::<MakezeroOptions>("makezero")
        .copied()
        .unwrap_or_default();

    let mut pending = Vec::new();

    // One `ast.Walk` per file, in source order, against a set that is *filled
    // as the walk goes* — upstream builds a fresh `visitor` per file and its
    // `nonZeroLengthSliceDecls` only ever holds the `make`s already seen.
    //
    // guff used to walk twice, collecting every `make` in the file before
    // looking at any `append`. That reports an `append` written *before* the
    // `make` that gives the slice its length, which upstream does not:
    //
    //     seen = append(seen, name)          // not reported
    //     old := seen
    //     seen = make([]string, len(old)+1)  // the length arrives here
    //
    // k6 `internal/dashboard/registry.go:78` is exactly that, inside a closure.
    for file in pass.files() {
        let mut nonzero: HashSet<ObjectId> = HashSet::new();
        walk::preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::AssignStmt(s) => {
                    check_assign(pass, s, &mut nonzero, opts.always, &mut pending);
                }
                NodeRef::CallExpr(call) => {
                    check_append(pass, call, &nonzero, &mut pending);
                }
                _ => {}
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
