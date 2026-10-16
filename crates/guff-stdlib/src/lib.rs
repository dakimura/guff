//! `golang.org/x/tools/internal/stdlib.PackageSymbols`: every exported symbol of
//! the standard library and the Go release that introduced it.
//!
//! The table is x/tools v0.50.0's checked-in `manifest.go`, transcribed by
//! `scripts/gen-stdlib-manifest.py` — never regenerated from the local
//! toolchain, whose `api/` files would list a different set of releases.

use std::collections::HashMap;
use std::sync::OnceLock;

/// `stdlib.Kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Type,
    Func,
    Var,
    Const,
    /// `"Point.X"`.
    Field,
    /// `"(*Buffer).Grow"` or `"(Reader).Read"`.
    Method,
}

/// `stdlib.Symbol` without `Signature`, which nothing here reads.
#[derive(Clone, Copy, Debug)]
pub struct Symbol {
    pub name: &'static str,
    pub kind: Kind,
    /// N of go1.N: `stdlib.Version`.
    pub minor: u8,
}

impl Symbol {
    /// `Version.String()`.
    pub fn version(&self) -> String {
        format!("go1.{}", self.minor)
    }

    /// `SplitField`: `"Point.X"` → `("Point", "X")`.
    pub fn split_field(&self) -> (&'static str, &'static str) {
        self.name.split_once('.').unwrap_or((self.name, ""))
    }

    /// `SplitMethod`: `"(*Buffer).Grow"` → `(true, "Buffer", "Grow")`.
    pub fn split_method(&self) -> (bool, &'static str, &'static str) {
        let (recv, name) = self.name.split_once('.').unwrap_or((self.name, ""));
        let recv = recv.strip_prefix('(').unwrap_or(recv);
        let recv = recv.strip_suffix(')').unwrap_or(recv);
        match recv.strip_prefix('*') {
            Some(r) => (true, r, name),
            None => (false, recv, name),
        }
    }
}

static TABLE: OnceLock<HashMap<&'static str, Vec<Symbol>>> = OnceLock::new();

const MANIFEST: &str = include_str!("manifest.txt");

/// `stdlib.PackageSymbols[path]`.
///
/// `None` for anything that is not a public standard-library package —
/// `internal/...`, `vendor/...` and every other module.
pub fn package_symbols(path: &str) -> Option<&'static [Symbol]> {
    table().get(path).map(Vec::as_slice)
}

/// Every package in the manifest, in no particular order: the keys of
/// `stdlib.PackageSymbols`.
pub fn packages() -> impl Iterator<Item = &'static str> {
    table().keys().copied()
}

fn table() -> &'static HashMap<&'static str, Vec<Symbol>> {
    TABLE
        .get_or_init(|| {
            let mut table: HashMap<&'static str, Vec<Symbol>> = HashMap::new();
            for line in MANIFEST.lines() {
                if line.starts_with('#') || line.is_empty() {
                    continue;
                }
                let mut cols = line.split('\t');
                let (Some(pkg), Some(name), Some(kind), Some(minor)) =
                    (cols.next(), cols.next(), cols.next(), cols.next())
                else {
                    panic!("malformed stdlib manifest line: {line:?}");
                };
                let kind = match kind {
                    "Type" => Kind::Type,
                    "Func" => Kind::Func,
                    "Var" => Kind::Var,
                    "Const" => Kind::Const,
                    "Field" => Kind::Field,
                    "Method" => Kind::Method,
                    _ => panic!("unknown kind in stdlib manifest line: {line:?}"),
                };
                let minor = minor
                    .parse()
                    .unwrap_or_else(|_| panic!("bad version in stdlib manifest line: {line:?}"));
                table.entry(pkg).or_default().push(Symbol { name, kind, minor });
            }
            table
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_has_every_package_and_go127() {
        assert_eq!(MANIFEST.lines().filter(|l| !l.starts_with('#')).count(), 18247);
        let sync = package_symbols("sync").unwrap();
        let go = sync.iter().find(|s| s.name == "(*WaitGroup).Go").unwrap();
        assert_eq!((go.kind, go.version()), (Kind::Method, "go1.25".to_string()));
        assert!(package_symbols("encoding/json/v2").is_some());
        assert!(package_symbols("example.com/x").is_none());
    }

    #[test]
    fn splits_like_upstream() {
        let m = Symbol { name: "(*Buffer).Grow", kind: Kind::Method, minor: 1 };
        assert_eq!(m.split_method(), (true, "Buffer", "Grow"));
        let m = Symbol { name: "(Reader).Read", kind: Kind::Method, minor: 0 };
        assert_eq!(m.split_method(), (false, "Reader", "Read"));
        let f = Symbol { name: "Point.X", kind: Kind::Field, minor: 0 };
        assert_eq!(f.split_field(), ("Point", "X"));
    }
}
