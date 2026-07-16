//! Port of [`github.com/alingse/asasalint`](https://github.com/alingse/asasalint)
//! (golangci-lint wrapper in `pkg/golinters/asasalint`).
//!
//! Reports passing a `[]any` / `[]interface{}` value as a single argument to a
//! variadic `func(...any)` without `...`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::position::NO_POS;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::typestring::type_string;
use guff_types::{TypeData, TypeId};
use regex::Regex;

use crate::options::AsasalintOptions;

/// Upstream / golangci-lint builtin exclusion of common print/log helpers.
const BUILTIN_EXCLUSIONS: &str =
    r"^(fmt|log|logger|t|)\.(Print|Fprint|Sprint|Fatal|Panic|Error|Warn|Warning|Info|Debug|Log)(|f|ln)$";

fn compile_excludes(opts: &AsasalintOptions) -> Vec<Regex> {
    let mut out = Vec::new();
    if opts.use_builtin_exclusions {
        if let Ok(re) = Regex::new(BUILTIN_EXCLUSIONS) {
            out.push(re);
        }
    }
    for pat in &opts.exclude {
        if pat.is_empty() {
            continue;
        }
        if let Ok(re) = Regex::new(pat) {
            out.push(re);
        }
    }
    out
}

fn expr_text(expr: &Expr) -> String {
    match expr {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", expr_text(&sel.x), sel.sel.name),
        Expr::ParenExpr(p) => expr_text(&p.x),
        Expr::StarExpr(s) => format!("*{}", expr_text(&s.x)),
        Expr::IndexExpr(i) => format!("{}[{}]", expr_text(&i.x), expr_text(&i.index)),
        Expr::CallExpr(c) => {
            let mut s = expr_text(&c.fun);
            s.push('(');
            for (i, a) in c.args.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&expr_text(a));
            }
            s.push(')');
            s
        }
        _ => String::new(),
    }
}

fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn is_slice_any_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let TypeData::Slice(slice) = artifacts.types.get(typ) else {
        return false;
    };
    let elem = unalias_readonly(&artifacts.types, slice.elem());
    let u = elem.underlying(&artifacts.types);
    match artifacts.types.get(u) {
        TypeData::Interface(i) => i.num_explicit_methods() == 0 && i.num_embeddeds() == 0,
        _ => false,
    }
}

fn is_slice_any_variadic_func(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let TypeData::Signature(sig) = artifacts.types.get(typ) else {
        return false;
    };
    if !sig.variadic() {
        return false;
    }
    let nparams = tuple_len(&artifacts.types, sig.params());
    if nparams == 0 {
        return false;
    }
    let last = tuple_at(&artifacts.types, sig.params().unwrap(), nparams - 1);
    let Some(last_ty) = last.typ(&artifacts.objects) else {
        return false;
    };
    is_slice_any_type(pass, last_ty)
}

fn signature_param_count(pass: &Pass<'_>, typ: TypeId) -> Option<usize> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = unalias_readonly(&artifacts.types, typ);
    let TypeData::Signature(sig) = artifacts.types.get(typ) else {
        return None;
    };
    Some(tuple_len(&artifacts.types, sig.params()))
}

fn type_str(pass: &Pass<'_>, typ: TypeId) -> String {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return String::new();
    };
    type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    )
}

fn check_call(
    pass: &Pass<'_>,
    call: &CallExpr,
    excludes: &[Regex],
    pending: &mut Vec<(u32, String)>,
) {
    if call.ellipsis != NO_POS || call.args.is_empty() {
        return;
    }

    let fn_name = expr_text(&call.fun);
    if fn_name.is_empty() {
        return;
    }
    for re in excludes {
        if re.is_match(&fn_name) {
            return;
        }
    }

    let Some(fn_type) = type_of(pass, &call.fun) else {
        return;
    };
    if !is_slice_any_variadic_func(pass, fn_type) {
        return;
    }
    let Some(nparams) = signature_param_count(pass, fn_type) else {
        return;
    };
    // Upstream: only when arg count equals param count (last slot filled by a
    // single []any value, not expanded element-wise).
    if call.args.len() != nparams {
        return;
    }

    let last_arg = &call.args[call.args.len() - 1];
    let Some(arg_type) = type_of(pass, last_arg) else {
        return;
    };
    if !is_slice_any_type(pass, arg_type) {
        return;
    }

    pending.push((
        last_arg.pos().0 as u32,
        format!(
            "pass []any as any to func {} {}",
            fn_name,
            type_str(pass, fn_type)
        ),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "asasalint requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<AsasalintOptions>("asasalint")
        .cloned()
        .unwrap_or_default();
    let excludes = compile_excludes(&opts);

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::CallExpr(call) = n {
                check_call(pass, call, &excludes, &mut pending);
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
        name: "asasalint",
        doc: "check for pass []any as any in variadic func(...any)",
        url: "https://github.com/alingse/asasalint",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
