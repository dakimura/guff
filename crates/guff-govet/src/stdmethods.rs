//! `stdmethods` — check signatures of common interface methods.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{Expr, FuncDecl, FuncType, Ident, InterfaceType};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::api_predicates::api_implements;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::signature::{signature_params, signature_results};
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::{new_pointer, TypeId};

struct CanonSig {
    args: &'static [&'static str],
    results: &'static [&'static str],
}

fn canonical_methods() -> &'static HashMap<&'static str, CanonSig> {
    static TABLE: OnceLock<HashMap<&'static str, CanonSig>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut m = HashMap::new();
        let mut add = |name, args, results| {
            m.insert(
                name,
                CanonSig {
                    args,
                    results,
                },
            );
        };
        add("As", &["any"], &["bool"]);
        add("Format", &["=fmt.State", "rune"], &[]);
        add("GobDecode", &["[]byte"], &["error"]);
        add("GobEncode", &[], &["[]byte", "error"]);
        add("Is", &["error"], &["bool"]);
        add("MarshalJSON", &[], &["[]byte", "error"]);
        add("ReadByte", &[], &["byte", "error"]);
        add("ReadRune", &[], &["rune", "int", "error"]);
        add("Scan", &["=fmt.ScanState", "rune"], &["error"]);
        add("Seek", &["=int64", "int"], &["int64", "error"]);
        add("UnmarshalJSON", &["[]byte"], &["error"]);
        add("UnreadByte", &[], &["error"]);
        add("UnreadRune", &[], &["error"]);
        add("Unwrap", &[], &["error"]);
        add("WriteByte", &["byte"], &["error"]);
        add("WriteTo", &["=io.Writer"], &["int64", "error"]);
        m
    })
}

fn type_string(pass: &Pass<'_>, typ: guff_types::TypeId) -> String {
    let artifacts = pass.pkg().type_artifacts.as_ref().expect("artifacts");
    guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    )
}

/// Match go/types `TypeString` with a package-name qualifier: `byte` stays
/// `byte` (not `uint8`), so `[]byte` matches the stdmethods canon table.
fn normalize_std_type_name(s: &str) -> String {
    s.replace("uint8", "byte")
}

fn types_equiv(got: &str, want: &str) -> bool {
    if got == want {
        return true;
    }
    if (got == "any" || got == "interface{}") && (want == "any" || want == "interface{}") {
        return true;
    }
    // Predeclared aliases: rune≡int32, byte≡uint8 (go vet stdmethods).
    if is_rune_alias(got) && is_rune_alias(want) {
        return true;
    }
    if is_byte_alias(got) && is_byte_alias(want) {
        return true;
    }
    normalize_std_type_name(got) == normalize_std_type_name(want)
}

fn is_rune_alias(s: &str) -> bool {
    s == "rune" || s == "int32"
}

fn is_byte_alias(s: &str) -> bool {
    s == "byte" || s == "uint8"
}

fn match_params(pass: &Pass<'_>, expect: &[&str], params: Option<guff_types::TypeId>, prefix: &str) -> bool {
    let artifacts = pass.pkg().type_artifacts.as_ref().expect("artifacts");
    for (i, x) in expect.iter().enumerate() {
        if !x.starts_with(prefix) {
            continue;
        }
        if i >= tuple_len(&artifacts.types, params) {
            return false;
        }
        let param = tuple_at(&artifacts.types, params.unwrap(), i);
        let Some(t) = param.typ(&artifacts.objects) else {
            return false;
        };
        let got = type_string(pass, t);
        let want = x.strip_prefix('=').unwrap_or(x);
        if !types_equiv(&got, want) {
            return false;
        }
    }
    if prefix.is_empty() && tuple_len(&artifacts.types, params) > expect.len() {
        return false;
    }
    true
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

fn implements_error(pass: &Pass<'_>, recv: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(err) = universe_error(pass) else {
        return false;
    };
    let mut types = artifacts.types.clone();
    let typ = unalias_readonly(&types, recv);
    if api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        err,
    ) {
        return true;
    }
    // Pointer receiver Error() methods: T may not implement error but *T does.
    if let TypeData::Pointer(_) = types.get(typ) {
        return false;
    }
    let ptr = new_pointer(&mut types, typ);
    api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        ptr,
        err,
    )
}

fn check_method(pass: &Pass<'_>, id: &Ident, ft: &FuncType) -> Option<String> {
    let _ = ft; // signature comes from type-checker defs (FuncType.id is often untyped)
    let canon = canonical_methods().get(id.name.as_str())?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let obj = guff_analysis::code::object_of(pass, id)?;
    let sig = obj.typ(&artifacts.objects)?;
    // `signature_params` returns None for an empty param tuple — that is valid
    // (e.g. Unwrap()), so only reject when `sig` is not a Signature at all.
    if !matches!(
        artifacts.types.get(sig),
        guff_types::arena::TypeData::Signature(_)
    ) {
        return None;
    }
    let params = signature_params(&artifacts.types, sig); // None = empty
    let results = signature_results(&artifacts.types, sig);

    // Special case: WriteTo with more than one argument — not io.WriterTo.
    if id.name == "WriteTo" && tuple_len(&artifacts.types, params) > 1 {
        return None;
    }

    // Special case: Is, As and Unwrap only apply when the receiver implements error.
    // Interface methods have no receiver → skip (matches x/tools stdmethods).
    if matches!(id.name.as_str(), "Is" | "As" | "Unwrap") {
        let Some(recv) = guff_types::signature::signature_recv(&artifacts.types, sig)
            .and_then(|r| r.typ(&artifacts.objects))
        else {
            return None;
        };
        if !implements_error(pass, recv) {
            return None;
        }
    }

    // Unwrap has two valid signatures: Unwrap() error and Unwrap() []error.
    if id.name == "Unwrap" {
        if tuple_len(&artifacts.types, params) == 0 && tuple_len(&artifacts.types, results) == 1 {
            let r = tuple_at(&artifacts.types, results.unwrap(), 0);
            let tname = r.typ(&artifacts.objects).map(|t| type_string(pass, t));
            if matches!(tname.as_deref(), Some("error") | Some("[]error")) {
                return None;
            }
        }
        return Some(
            "method Unwrap() should have signature Unwrap() error or Unwrap() []error".into(),
        );
    }

    if !match_params(pass, canon.args, params, "=")
        || !match_params(pass, canon.results, results, "=")
    {
        return None;
    }
    if !match_params(pass, canon.args, params, "")
        || !match_params(pass, canon.results, results, "")
    {
        let expect_fmt = format_canon(id.name.as_str(), canon);
        let actual = type_string(pass, sig);
        let actual = actual.strip_prefix("func").unwrap_or(&actual);
        return Some(format!(
            "method {}{} should have signature {expect_fmt}",
            id.name, actual
        ));
    }
    None
}

fn format_canon(name: &str, canon: &CanonSig) -> String {
    let args = canon
        .args
        .iter()
        .map(|s| s.strip_prefix('=').unwrap_or(s))
        .collect::<Vec<_>>()
        .join(", ");
    let mut s = format!("{name}({args})");
    if canon.results.len() == 1 {
        s.push(' ');
        s.push_str(canon.results[0]);
    } else if canon.results.len() > 1 {
        s.push_str(" (");
        s.push_str(&canon.results.join(", "));
        s.push(')');
    }
    s
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "stdmethods requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(FuncDecl, InterfaceType), pass.files(), |n| {
        match n {
            NodeRef::FuncDecl(FuncDecl {
                name,
                recv: Some(_),
                ty,
                ..
            }) => {
                if let Some(msg) = check_method(pass, name, ty) {
                    pending.push((name.pos().0 as u32, msg));
                }
            }
            NodeRef::InterfaceType(InterfaceType { methods, .. }) => {
                for field in &methods.list {
                    let Some(ft) = field.ty.as_ref().and_then(|ty| match ty {
                        Expr::FuncType(ft) => Some(ft),
                        _ => None,
                    }) else {
                        continue;
                    };
                    for id in &field.names {
                        if let Some(msg) = check_method(pass, id, ft) {
                            pending.push((id.pos().0 as u32, msg));
                        }
                    }
                }
            }
            _ => {}
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
        name: "stdmethods",
        doc: "check signatures of methods that implement standard interfaces",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/stdmethods",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
