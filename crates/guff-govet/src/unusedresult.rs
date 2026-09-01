//! `unusedresult` — check for unused results of important stdlib calls.

use std::sync::OnceLock;

use guff::ast::{Expr, ExprStmt};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::ObjectId;
use guff_types::signature::{signature_recv, signature_results, signature_params};
use guff_types::tuple::{tuple_at, tuple_len};

use crate::expreq::unparen;

/// `funcs`, upstream's default. Keyed by the callee's **package path** and
/// name, which is what `fn.Pkg().Path()` answers — not by the qualifier
/// written at the call site. guff matched on the identifier before the dot, so
/// `github.com/pkg/errors.New` was read as `errors.New` and reported, while the
/// real `errors.New` imported as `stderrors` was not.
const MUST_USE_FUNCS: &[(&str, &str)] = &[
    ("context", "WithCancel"),
    ("context", "WithDeadline"),
    ("context", "WithTimeout"),
    ("context", "WithValue"),
    ("errors", "New"),
    ("fmt", "Append"),
    ("fmt", "Appendf"),
    ("fmt", "Appendln"),
    ("fmt", "Errorf"),
    ("fmt", "Sprint"),
    ("fmt", "Sprintf"),
    ("fmt", "Sprintln"),
    ("maps", "All"),
    ("maps", "Clone"),
    ("maps", "Collect"),
    ("maps", "Equal"),
    ("maps", "EqualFunc"),
    ("maps", "Keys"),
    ("maps", "Values"),
    ("slices", "All"),
    ("slices", "AppendSeq"),
    ("slices", "Backward"),
    ("slices", "BinarySearch"),
    ("slices", "BinarySearchFunc"),
    ("slices", "Chunk"),
    ("slices", "Clip"),
    ("slices", "Clone"),
    ("slices", "Collect"),
    ("slices", "Compact"),
    ("slices", "CompactFunc"),
    ("slices", "Compare"),
    ("slices", "CompareFunc"),
    ("slices", "Concat"),
    ("slices", "Contains"),
    ("slices", "ContainsFunc"),
    ("slices", "Delete"),
    ("slices", "DeleteFunc"),
    ("slices", "Equal"),
    ("slices", "EqualFunc"),
    ("slices", "Grow"),
    ("slices", "Index"),
    ("slices", "IndexFunc"),
    ("slices", "Insert"),
    ("slices", "IsSorted"),
    ("slices", "IsSortedFunc"),
    ("slices", "Max"),
    ("slices", "MaxFunc"),
    ("slices", "Min"),
    ("slices", "MinFunc"),
    ("slices", "Repeat"),
    ("slices", "Replace"),
    ("slices", "Sorted"),
    ("slices", "SortedFunc"),
    ("slices", "SortedStableFunc"),
    ("slices", "Values"),
    ("sort", "Reverse"),
];

/// `stringMethods`, upstream's default (`stringMethods.Set("Error,String")`).
const STRING_METHODS: &[&str] = &["Error", "String"];

/// Is `sig` identical to `func() string`? (Go: `sigNoArgsStringResult`.)
///
/// The receiver is not part of that comparison — upstream builds the signature
/// with `types.NewSignature(nil, …)` and compares against `fn.Signature()`,
/// whose receiver `types.Identical` ignores.
fn is_no_args_string_result(pass: &Pass<'_>, sig: guff_types::TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let params = signature_params(&artifacts.types, sig);
    if tuple_len(&artifacts.types, params) != 0 {
        return false;
    }
    let results = signature_results(&artifacts.types, sig);
    if tuple_len(&artifacts.types, results) != 1 {
        return false;
    }
    let Some(results) = results else {
        return false;
    };
    let Some(typ) = tuple_at(&artifacts.types, results, 0).typ(&artifacts.objects) else {
        return false;
    };
    guff_types::basic::basic_kind(&artifacts.types, typ) == guff_types::basic::BasicKind::String
}

/// The message upstream writes for `call`, or `None` when the callee is not on
/// either list.
fn must_use_message(pass: &Pass<'_>, obj: ObjectId) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let name = obj.name(&artifacts.objects).to_string();
    let sig = obj.typ(&artifacts.objects)?;
    if !matches!(
        artifacts.types.get(sig),
        guff_types::arena::TypeData::Signature(_)
    ) {
        return None;
    }

    if let Some(recv) = signature_recv(&artifacts.types, sig) {
        // A method: only `func() string` named `Error` or `String`.
        if !STRING_METHODS.contains(&name.as_str()) {
            return None;
        }
        if !is_no_args_string_result(pass, sig) {
            return None;
        }
        let recv_typ = recv.typ(&artifacts.objects)?;
        let recv_str = guff_types::typestring::type_string(
            &artifacts.types,
            &artifacts.objects,
            &artifacts.packages,
            recv_typ,
            None,
        );
        return Some(format!("result of ({recv_str}).{name} call not used"));
    }

    // A package-level function, keyed on the package *path*.
    let pkg = obj.pkg(&artifacts.objects)?;
    let path = artifacts.packages.get(pkg).path().to_string();
    MUST_USE_FUNCS
        .iter()
        .any(|(p, n)| *p == path && *n == name)
        .then(|| format!("result of {path}.{name} call not used"))
}

/// The `types.Func` a call resolves to, method or function. (Go:
/// `typeutil.Callee`.)
fn callee_obj(pass: &Pass<'_>, fun: &Expr) -> Option<ObjectId> {
    let info = pass.types_info()?;
    let id = match unparen(fun) {
        Expr::Ident(id) => id.id,
        Expr::SelectorExpr(sel) => sel.sel.id,
        _ => return None,
    };
    info.uses
        .get(&id)
        .copied()
        .or_else(|| info.defs.get(&id).and_then(|o| *o))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "unusedresult requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(ExprStmt), pass.files(), |n| {
        let NodeRef::ExprStmt(ExprStmt { x, .. }) = n else {
            return;
        };
        let Expr::CallExpr(call) = unparen(x) else {
            return;
        };
        let Some(obj) = callee_obj(pass, &call.fun) else {
            return;
        };
        if let Some(message) = must_use_message(pass, obj) {
            // RangeOf(call.Pos(), call.Lparen): the range starts at the
            // callee, not at the paren.
            pending.push((call.fun.pos().0 as u32, message));
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
        name: "unusedresult",
        doc: "check for unused results of calls to certain functions",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/unusedresult",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
