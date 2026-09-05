//! SA1026 — cannot marshal channels or functions (JSON/XML).
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1026`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::{ObjectArena, ObjectData, PackageArena, PackageId, TypeArena, TypeData};
use guff_types::pointer::pointer_elem;
use guff_types::typestring::type_string;
use guff_types::lookup::{lookup_field_or_method, LookupResult};
use guff_types::TypeId;

use crate::fakejson;

/// Answers the method-set questions `fakejson` asks, over a private copy of the
/// type arena — `lookup_field_or_method` memoises into it.
struct Lookup<'a> {
    types: RefCell<TypeArena>,
    objects: &'a ObjectArena,
    packages: &'a PackageArena,
}

impl fakejson::MarshalerLookup for Lookup<'_> {
    fn implements(&self, typ: TypeId, method: &str, ptr: bool) -> bool {
        let mut types = self.types.borrow_mut();
        let recv = if ptr {
            guff_types::pointer::new_pointer(&mut types, typ)
        } else {
            typ
        };
        // `addressable` is false: the method set of the receiver as written.
        // Asking for `*T` is what `PtrTo(t)` does, and it is a different
        // question from "would this be addressable here".
        let found = match lookup_field_or_method(
            &mut types,
            self.objects,
            self.packages,
            recv,
            false,
            None,
            method,
        ) {
            LookupResult::Found { obj, .. } => obj,
            _ => return false,
        };
        let ObjectData::Func(f) = self.objects.get(found) else {
            return false;
        };
        // `func() ([]byte, error)` — a `MarshalText` with any other shape does
        // not implement the interface.
        let Some(sig) = f.typ() else {
            return false;
        };
        let TypeData::Signature(sig) = types.get(sig.underlying(&types)) else {
            return false;
        };
        if guff_types::tuple::tuple_len(&types, sig.params()) != 0 {
            return false;
        }
        let Some(results) = sig.results() else {
            return false;
        };
        if guff_types::tuple::tuple_len(&types, Some(results)) != 2 {
            return false;
        }
        let first = guff_types::tuple::tuple_at(&types, results, 0);
        let Some(first) = first.typ(self.objects) else {
            return false;
        };
        let TypeData::Slice(s) = types.get(first.underlying(&types)) else {
            return false;
        };
        matches!(
            types.get(s.elem().underlying(&types)),
            TypeData::Basic(b) if b.kind() == guff_types::basic::BasicKind::Uint8
        )
    }
}

/// `types.RelativeTo(call.Parent.Pkg.Pkg)`: a name from the package being
/// analysed prints bare, every other one keeps its import path.
fn relative_to(pkg: PackageId) -> impl Fn(PackageId, &PackageArena) -> String {
    move |other: PackageId, packages: &PackageArena| {
        if other == pkg {
            String::new()
        } else {
            packages.get(other).path().to_string()
        }
    }
}

fn check_marshal(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let Some(arg) = call.args.first() else {
        return;
    };
    let typ = callcheck::ssa_value_type(ctx.prog, ctx.caller, arg.value);
    let arena = &ctx.prog.type_arena;
    let objects = &ctx.prog.object_arena;
    let packages = &ctx.prog.package_arena;
    let lookup = Lookup {
        types: RefCell::new(arena.clone()),
        objects,
        packages,
    };
    let Some(err) = fakejson::marshal(arena, objects, packages, &lookup, typ) else {
        return;
    };
    // `types.TypeString(err.Type, types.RelativeTo(call.Parent.Pkg.Pkg))` —
    // guff printed the import path for a type of the package under analysis,
    // so even the findings both tools had disagreed on their text.
    let qf = ctx
        .caller
        .pkg
        .map(|p| ctx.prog.packages.get(p).pkg)
        .map(relative_to);
    let typ_str = match &qf {
        Some(q) => type_string(arena, objects, packages, err.typ, Some(q)),
        None => callcheck::render_type(arena, objects, packages, err.typ),
    };
    let msg = if err.path == "x" {
        format!("trying to marshal unsupported type {typ_str}")
    } else {
        format!(
            "trying to marshal unsupported type {typ_str}, via {}",
            err.path
        )
    };
    call.args[0].invalid(msg);
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        // Upstream's rule table is exactly these four. `MarshalIndent` is not
        // on it — checking it too made consul's
        // `json.MarshalIndent(bound, …)` a guff-only finding.
        HashMap::from([
            ("encoding/json.Marshal", check_marshal as callcheck::CheckFn),
            (
                "(*encoding/json.Encoder).Encode",
                check_marshal as callcheck::CheckFn,
            ),
            ("encoding/xml.Marshal", check_marshal as callcheck::CheckFn),
            (
                "(*encoding/xml.Encoder).Encode",
                check_marshal as callcheck::CheckFn,
            ),
        ])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1026 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1026_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1026",
        doc: "cannot marshal channels or functions",
        url: "https://staticcheck.dev/docs/checks/#SA1026",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1026 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1026_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1026_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
