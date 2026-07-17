//! Analysis facts — serializable predicates attached to objects or packages.
//!
//! Port of `go/analysis/analysis.go` (`Fact`, `ObjectFact`, `PackageFact`).
//! On-disk encoding lives in [`crate::fact_codec`].

use std::any::{Any, TypeId};
use std::collections::HashMap;

use guff_types::arena::{ObjectId, PackageId};
use serde_json::{json, Value};

use crate::fact_codec::register_fact_decoder;

/// Identifies a concrete fact type (Go's `reflect.TypeOf(fact)`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FactTypeId(TypeId);

impl FactTypeId {
    pub fn of<T: 'static>() -> Self {
        Self(TypeId::of::<T>())
    }

    pub fn type_id(self) -> TypeId {
        self.0
    }
}

/// Intermediate fact produced during analysis.
///
/// Equivalent to `analysis.Fact`.
pub trait Fact: Any + Send + Sync {
    fn fact_type_id(&self) -> FactTypeId;
    fn as_any(&self) -> &dyn Any;
    fn clone_fact(&self) -> Box<dyn Fact>;

    /// Stable name used in the persistent facts cache (golangci gob type key).
    fn type_name(&self) -> &'static str;

    /// JSON payload for [`crate::fact_codec::EncodedFact`].
    fn encode_payload(&self) -> Value;
}

/// A package together with an associated fact.
pub struct PackageFact {
    pub package: PackageId,
    pub fact: Box<dyn Fact>,
}

/// An object together with an associated fact.
pub struct ObjectFact {
    pub object: ObjectId,
    pub fact: Box<dyn Fact>,
}

/// In-memory fact store for a single analysis pass.
#[derive(Default)]
pub struct FactStore {
    object_facts: HashMap<(FactTypeId, ObjectId), Box<dyn Fact>>,
    package_facts: HashMap<(FactTypeId, PackageId), Box<dyn Fact>>,
}

impl FactStore {
    pub fn import_object_fact<F: Fact + Clone>(
        &self,
        object: ObjectId,
        fact: &mut F,
    ) -> bool {
        let id = FactTypeId::of::<F>();
        let Some(stored) = self.object_facts.get(&(id, object)) else {
            return false;
        };
        let Some(val) = stored.as_any().downcast_ref::<F>() else {
            return false;
        };
        *fact = val.clone();
        true
    }

    pub fn export_object_fact(&mut self, object: ObjectId, fact: Box<dyn Fact>) {
        let id = fact.fact_type_id();
        self.object_facts.insert((id, object), fact);
    }

    pub fn import_package_fact<F: Fact + Clone>(
        &self,
        package: PackageId,
        fact: &mut F,
    ) -> bool {
        let id = FactTypeId::of::<F>();
        let Some(stored) = self.package_facts.get(&(id, package)) else {
            return false;
        };
        let Some(val) = stored.as_any().downcast_ref::<F>() else {
            return false;
        };
        *fact = val.clone();
        true
    }

    pub fn export_package_fact(&mut self, package: PackageId, fact: Box<dyn Fact>) {
        let id = fact.fact_type_id();
        self.package_facts.insert((id, package), fact);
    }

    pub fn all_object_facts(&self) -> Vec<ObjectFact> {
        self.object_facts
            .iter()
            .map(|((_, object), fact)| ObjectFact {
                object: *object,
                fact: fact.clone_fact(),
            })
            .collect()
    }

    pub fn all_package_facts(&self) -> Vec<PackageFact> {
        self.package_facts
            .iter()
            .map(|((_, package), fact)| PackageFact {
                package: *package,
                fact: fact.clone_fact(),
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.object_facts.is_empty() && self.package_facts.is_empty()
    }
}

/// Trivial fact type for unit tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringFact(pub String);

impl Fact for StringFact {
    fn fact_type_id(&self) -> FactTypeId {
        FactTypeId::of::<Self>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clone_fact(&self) -> Box<dyn Fact> {
        Box::new(self.clone())
    }

    fn type_name(&self) -> &'static str {
        "StringFact"
    }

    fn encode_payload(&self) -> Value {
        json!({ "s": self.0 })
    }
}

fn decode_string_fact(payload: Value) -> Option<Box<dyn Fact>> {
    let s = payload.get("s")?.as_str()?.to_string();
    Some(Box::new(StringFact(s)))
}

/// Register decoders for facts defined in this crate. Idempotent.
pub fn ensure_builtin_fact_decoders() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        register_fact_decoder("StringFact", decode_string_fact);
        crate::passes::facts::deprecated::register_deprecated_fact_decoder();
    });
}

#[cfg(test)]
mod tests {
    use guff::parser::{parse_file, Mode};
    use guff::position::FileSet;
    use guff_types::scope::lookup as scope_lookup;
    use guff_types::{Checker, Config};

    use super::*;

    #[test]
    fn export_and_import_object_fact() {
        let fset = FileSet::new();
        let file = parse_file(
            &fset,
            "t.go",
            b"package p\nvar V int\n",
            Mode::NONE,
        )
        .expect("parse");
        let mut check = Checker::new(Config::default());
        check.check_files(vec![file]);
        let obj = scope_lookup(&check.scopes, check.packages.get(check.pkg).scope(), "V")
            .expect("V");

        let mut store = FactStore::default();
        store.export_object_fact(obj, Box::new(StringFact("never returns".into())));

        let mut fact = StringFact(String::new());
        assert!(store.import_object_fact(obj, &mut fact));
        assert_eq!(fact.0, "never returns");
    }

    #[test]
    fn export_and_import_package_fact() {
        let fset = FileSet::new();
        let file = parse_file(&fset, "t.go", b"package p\n", Mode::NONE).expect("parse");
        let mut check = Checker::new(Config::default());
        check.check_files(vec![file]);
        let pkg = check.pkg;

        let mut store = FactStore::default();
        store.export_package_fact(pkg, Box::new(StringFact("checked".into())));

        let mut fact = StringFact(String::new());
        assert!(store.import_package_fact(pkg, &mut fact));
        assert_eq!(fact.0, "checked");
    }
}
