//! Serializable analysis facts (wire format + codec registry).
//!
//! Complements the in-memory [`super::FactStore`] with a stable encoding used
//! by the runner's persistent facts cache (golangci `runner_action_cache.go`).
//! Object identity is `(pkg_path, objectpath)`; package facts use an empty
//! objectpath.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use guff_packages::TypecheckArtifacts;
use guff_types::arena::{ObjectId, PackageId};
use guff_types::{objectpath_for, objectpath_object};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Fact, FactStore};

/// One fact on disk / across arena boundaries.
///
/// Equivalent to golangci-lint's `goanalysis.Fact`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EncodedFact {
    /// Import path of the owning package. Empty means "the package that
    /// produced this cache entry" (golangci backward-compat convention).
    #[serde(default)]
    pub pkg_path: String,
    /// Non-empty only for object facts ([`objectpath_for`] relative to the
    /// owning package). Empty = package fact.
    #[serde(default)]
    pub object_path: String,
    /// Stable fact type name (see [`Fact::type_name`]).
    pub fact_type: String,
    /// JSON payload produced by [`Fact::encode_payload`].
    pub payload: Value,
}

type FactDecoder = fn(Value) -> Option<Box<dyn Fact>>;

fn decoder_registry() -> &'static Mutex<HashMap<&'static str, FactDecoder>> {
    static REG: OnceLock<Mutex<HashMap<&'static str, FactDecoder>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a decoder for `type_name`. Safe to call multiple times (last wins).
///
/// Analyzers that define private fact types should call this from their
/// `analyzer()` singleton initializer.
pub fn register_fact_decoder(type_name: &'static str, decode: FactDecoder) {
    decoder_registry()
        .lock()
        .unwrap()
        .insert(type_name, decode);
}

/// Decode a fact payload previously produced by [`Fact::encode_payload`].
pub fn decode_fact(type_name: &str, payload: Value) -> Option<Box<dyn Fact>> {
    let guard = decoder_registry().lock().unwrap();
    let decode = guard.get(type_name)?;
    decode(payload)
}

/// Encode every fact in `store` using `artifacts` for objectpath resolution.
///
/// Facts that cannot be addressed from a package scope are skipped (same as
/// golangci dropping non-`objectpath.For` objects).
pub fn encode_fact_store(
    store: &FactStore,
    artifacts: &TypecheckArtifacts,
    current_pkg_path: &str,
) -> Vec<EncodedFact> {
    let mut out = Vec::new();

    for of in store.all_object_facts() {
        let Some(owner_pkg) = of.object.pkg(&artifacts.objects) else {
            continue;
        };
        let owner_path = artifacts.packages.get(owner_pkg).path().to_string();
        let Ok(object_path) = objectpath_for(
            &artifacts.packages,
            &artifacts.objects,
            &artifacts.scopes,
            of.object,
        ) else {
            continue;
        };
        let pkg_path = if owner_path == current_pkg_path {
            String::new()
        } else {
            owner_path
        };
        out.push(EncodedFact {
            pkg_path,
            object_path,
            fact_type: of.fact.type_name().to_string(),
            payload: of.fact.encode_payload(),
        });
    }

    for pf in store.all_package_facts() {
        let owner_path = artifacts.packages.get(pf.package).path().to_string();
        let pkg_path = if owner_path == current_pkg_path {
            String::new()
        } else {
            owner_path
        };
        out.push(EncodedFact {
            pkg_path,
            object_path: String::new(),
            fact_type: pf.fact.type_name().to_string(),
            payload: pf.fact.encode_payload(),
        });
    }

    out
}

/// Decode `encoded` facts into `store`, resolving identities in `artifacts`.
///
/// `current_pkg_path` fills in empty [`EncodedFact::pkg_path`] entries.
/// Unresolvable facts are skipped (lenient, matching golangci).
pub fn decode_facts_into(
    encoded: &[EncodedFact],
    artifacts: &TypecheckArtifacts,
    current_pkg_path: &str,
    store: &mut FactStore,
) {
    for ef in encoded {
        let owner_path = if ef.pkg_path.is_empty() {
            current_pkg_path
        } else {
            ef.pkg_path.as_str()
        };
        let Some(pkg) = artifacts.packages.find_by_path(owner_path) else {
            continue;
        };
        let Some(fact) = decode_fact(&ef.fact_type, ef.payload.clone()) else {
            continue;
        };
        if ef.object_path.is_empty() {
            store.export_package_fact(pkg, fact);
            continue;
        }
        let Ok(obj) =
            objectpath_object(&artifacts.packages, &artifacts.scopes, pkg, &ef.object_path)
        else {
            continue;
        };
        let _ = obj; // silence when used below
        store.export_object_fact(obj, fact);
    }
}

/// Remap facts from `src_artifacts` into `dst` using objectpath identity.
///
/// Used when inheriting facts from a dependency Action that ran in a different
/// per-package Checker arena.
pub fn remap_facts(
    src: &FactStore,
    src_artifacts: &TypecheckArtifacts,
    src_pkg_path: &str,
    dst_artifacts: &TypecheckArtifacts,
    dst: &mut FactStore,
) {
    let encoded = encode_fact_store(src, src_artifacts, src_pkg_path);
    decode_facts_into(&encoded, dst_artifacts, src_pkg_path, dst);
}

#[cfg(test)]
mod tests {
    use guff::parser::{parse_file, Mode};
    use guff::position::FileSet;
    use guff_types::scope::lookup as scope_lookup;
    use guff_types::{Checker, Config, ObjectId};

    use super::*;
    use crate::facts::StringFact;

    fn artifacts(src: &str) -> (TypecheckArtifacts, ObjectId) {
        let fset = FileSet::new();
        let file = parse_file(&fset, "t.go", src.as_bytes(), Mode::NONE).expect("parse");
        let mut check = Checker::new(Config::default());
        check.check_files(vec![file]);
        let scope = check.packages.get(check.pkg).scope();
        let obj = scope_lookup(&check.scopes, scope, "V").expect("V");
        let arts = TypecheckArtifacts {
            type_pkg: check.pkg,
            types: check.types,
            objects: check.objects,
            scopes: check.scopes,
            packages: check.packages,
            info: std::sync::Arc::new(check.info.clone()),
        };
        (arts, obj)
    }

    #[test]
    fn encode_decode_object_fact_roundtrip() {
        crate::facts::ensure_builtin_fact_decoders();
        let (arts, obj) = artifacts("package p\nvar V int\n");
        let pkg_path = arts.packages.get(arts.type_pkg).path().to_string();
        let mut store = FactStore::default();
        store.export_object_fact(obj, Box::new(StringFact("x".into())));
        let encoded = encode_fact_store(&store, &arts, &pkg_path);
        assert_eq!(encoded.len(), 1);
        assert_eq!(encoded[0].object_path, "V");
        assert!(encoded[0].pkg_path.is_empty());

        let mut restored = FactStore::default();
        decode_facts_into(&encoded, &arts, &pkg_path, &mut restored);
        let mut fact = StringFact(String::new());
        assert!(restored.import_object_fact(obj, &mut fact));
        assert_eq!(fact.0, "x");
    }

    #[test]
    fn encode_decode_package_fact_roundtrip() {
        crate::facts::ensure_builtin_fact_decoders();
        let (arts, _) = artifacts("package p\nvar V int\n");
        let pkg_path = arts.packages.get(arts.type_pkg).path().to_string();
        let mut store = FactStore::default();
        store.export_package_fact(arts.type_pkg, Box::new(StringFact("pkg".into())));
        let encoded = encode_fact_store(&store, &arts, &pkg_path);
        assert_eq!(encoded.len(), 1);
        assert!(encoded[0].object_path.is_empty());

        let mut restored = FactStore::default();
        decode_facts_into(&encoded, &arts, &pkg_path, &mut restored);
        let mut fact = StringFact(String::new());
        assert!(restored.import_package_fact(arts.type_pkg, &mut fact));
        assert_eq!(fact.0, "pkg");
    }
}
