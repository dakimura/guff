//! SA4032 — comparing runtime.GOOS/GOARCH against impossible value.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4032` (simplified build-tag check).

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{expr_to_string, selector_name};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

const KNOWN_GOOS: &[&str] = &[
    "aix", "android", "darwin", "dragonfly", "freebsd", "hurd", "illumos", "ios", "js", "linux",
    "nacl", "netbsd", "openbsd", "plan9", "solaris", "wasip1", "windows", "zos",
];

const KNOWN_GOARCH: &[&str] = &[
    "386", "amd64", "amd64p32", "arm", "armbe", "arm64", "arm64be", "loong64", "mips", "mipsle",
    "mips64", "mips64le", "mips64p32", "mips64p32le", "ppc", "ppc64", "ppc64le", "riscv", "riscv64",
    "s390", "s390x", "sparc", "sparc64", "wasm",
];

fn file_build_tags(pass: &Pass<'_>, file_idx: usize, file: &guff::ast::File) -> Vec<String> {
    let mut tags = file_build_tags_from_ast(file);
    if !tags.is_empty() {
        return tags;
    }
    let Some(path) = pass.pkg().compiled_go_files.get(file_idx) else {
        return tags;
    };
    let Ok(src) = std::fs::read_to_string(path) else {
        return tags;
    };
    for line in src.lines() {
        let text = line.trim();
        if let Some(rest) = text.strip_prefix("//go:build ") {
            tags.push(rest.to_string());
        } else if let Some(rest) = text.strip_prefix("// +build ") {
            tags.push(rest.to_string());
        }
    }
    tags
}

fn file_build_tags_from_ast(file: &guff::ast::File) -> Vec<String> {
    let mut tags = Vec::new();
    for cg in &file.comments {
        for c in &cg.list {
            let text = c.text.trim();
            if let Some(rest) = text.strip_prefix("//go:build ") {
                tags.push(rest.to_string());
            } else if let Some(rest) = text.strip_prefix("// +build ") {
                tags.push(rest.to_string());
            }
        }
    }
    tags
}

/// Three-valued build-tag evaluation: custom tags are [`Tri::Unknown`] so
/// `!noselfupdate` does not make every GOOS comparison look impossible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tri {
    True,
    False,
    Unknown,
}

impl Tri {
    fn and(self, other: Self) -> Self {
        match (self, other) {
            (Tri::False, _) | (_, Tri::False) => Tri::False,
            (Tri::True, Tri::True) => Tri::True,
            _ => Tri::Unknown,
        }
    }

    fn or(self, other: Self) -> Self {
        match (self, other) {
            (Tri::True, _) | (_, Tri::True) => Tri::True,
            (Tri::False, Tri::False) => Tri::False,
            _ => Tri::Unknown,
        }
    }

    fn not(self) -> Self {
        match self {
            Tri::True => Tri::False,
            Tri::False => Tri::True,
            Tri::Unknown => Tri::Unknown,
        }
    }
}

/// True when no (GOOS, GOARCH) pair that builds this file has `runtime.GOOS == goos`.
fn goos_impossible(tags: &[String], goos: &str) -> bool {
    if tags.is_empty() {
        return false;
    }
    // Multiple `//go:build` / `// +build` lines are AND-combined.
    let expr = tags.join(" && ");
    // Impossible only if the constraint is definitely false for every arch.
    !KNOWN_GOARCH
        .iter()
        .any(|arch| eval_constraint(&expr, Some(goos), Some(arch)) != Tri::False)
}

/// True when no (GOOS, GOARCH) pair that builds this file has `runtime.GOARCH == goarch`.
fn goarch_impossible(tags: &[String], goarch: &str) -> bool {
    if tags.is_empty() {
        return false;
    }
    let expr = tags.join(" && ");
    !KNOWN_GOOS
        .iter()
        .any(|os| eval_constraint(&expr, Some(os), Some(goarch)) != Tri::False)
}

/// Evaluate a `//go:build` expression under a fixed GOOS/GOARCH.
///
/// Non-OS/non-ARCH identifiers (e.g. `cgo`, custom tags) are [`Tri::Unknown`]
/// so negations like `!noselfupdate` stay satisfiable for every GOOS.
fn eval_constraint(expr: &str, goos: Option<&str>, goarch: Option<&str>) -> Tri {
    let mut p = Parser {
        s: expr.as_bytes(),
        i: 0,
        goos,
        goarch,
    };
    let Ok(v) = p.parse_or() else {
        return Tri::True; // malformed → don't flag
    };
    p.skip_ws();
    if p.i != p.s.len() {
        return Tri::True;
    }
    v
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    goos: Option<&'a str>,
    goarch: Option<&'a str>,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn parse_or(&mut self) -> Result<Tri, ()> {
        let mut v = self.parse_and()?;
        loop {
            self.skip_ws();
            if self.s.get(self.i..).is_some_and(|s| s.starts_with(b"||")) {
                self.i += 2;
                v = v.or(self.parse_and()?);
            } else {
                return Ok(v);
            }
        }
    }

    fn parse_and(&mut self) -> Result<Tri, ()> {
        let mut v = self.parse_unary()?;
        loop {
            self.skip_ws();
            if self.s.get(self.i..).is_some_and(|s| s.starts_with(b"&&")) {
                self.i += 2;
                v = v.and(self.parse_unary()?);
            } else {
                return Ok(v);
            }
        }
    }

    fn parse_unary(&mut self) -> Result<Tri, ()> {
        self.skip_ws();
        if self.s.get(self.i) == Some(&b'!') {
            self.i += 1;
            return Ok(self.parse_unary()?.not());
        }
        if self.s.get(self.i) == Some(&b'(') {
            self.i += 1;
            let v = self.parse_or()?;
            self.skip_ws();
            if self.s.get(self.i) != Some(&b')') {
                return Err(());
            }
            self.i += 1;
            return Ok(v);
        }
        self.parse_ident()
    }

    fn parse_ident(&mut self) -> Result<Tri, ()> {
        self.skip_ws();
        let start = self.i;
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_alphanumeric()
                || self.s[self.i] == b'_'
                || self.s[self.i] == b'.')
        {
            self.i += 1;
        }
        if start == self.i {
            return Err(());
        }
        let name = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| ())?;
        Ok(self.eval_tag(name))
    }

    fn eval_tag(&self, name: &str) -> Tri {
        if name == "unix" {
            return if self
                .goos
                .is_some_and(|os| os != "windows" && os != "plan9")
            {
                Tri::True
            } else {
                Tri::False
            };
        }
        if KNOWN_GOOS.contains(&name) {
            return if self.goos == Some(name) {
                Tri::True
            } else {
                Tri::False
            };
        }
        if KNOWN_GOARCH.contains(&name) {
            return if self.goarch == Some(name) {
                Tri::True
            } else {
                Tri::False
            };
        }
        // Unknown / custom tags: neither force true nor false.
        Tri::Unknown
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4032 requires inspect analyzer".to_string())?
        .clone();
    let tagged_files: Vec<_> = pass
        .files()
        .iter()
        .enumerate()
        .map(|(i, file)| (file, file_build_tags(pass, i, file)))
        .filter(|(_, tags)| !tags.is_empty())
        .collect();
    let mut all_pending = Vec::new();
    for (file, tags) in tagged_files {
        inspect.preorder_typed(node_mask!(BinaryExpr), std::slice::from_ref(file), |node| {
            let NodeRef::BinaryExpr(bin) = node else {
                return;
            };
            if !matches!(bin.op, Token::EQL | Token::NEQ) {
                return;
            }
            // Upstream's pattern is
            //   (BinaryExpr (Symbol "runtime.GOOS") op@(Or "==" "!=") lit@(BasicLit "STRING" _))
            // so the operands are not interchangeable: the symbol must be on the
            // left and the value must be a literal. `"linux" == runtime.GOOS`
            // and `runtime.GOOS == someStringConst` are both quiet upstream.
            let (Expr::SelectorExpr(sel), lit @ Expr::BasicLit(_)) =
                (bin.x.as_ref(), bin.y.as_ref())
            else {
                return;
            };
            let sym = selector_name(pass, sel);
            let Some(go_val) = expr_to_string(pass, lit) else {
                return;
            };
            let msg = match sym.as_deref() {
                Some(s) if s == "runtime.GOOS" && goos_impossible(&tags, &go_val) => Some(format!(
                    "due to the file's build constraints, runtime.GOOS will never equal {go_val:?}"
                )),
                Some(s) if s == "runtime.GOARCH" && goarch_impossible(&tags, &go_val) => Some(
                    format!(
                        "due to the file's build constraints, runtime.GOARCH will never equal {go_val:?}"
                    ),
                ),
                _ => None,
            };
            if let Some(msg) = msg {
                // `report.Report(pass, node, ...)` — the node is the whole
                // BinaryExpr, so the caret sits on the comparison's first
                // token, not on the operator.
                all_pending.push((bin.x.pos().0 as u32, msg));
            }
        });
    }
    for (pos, msg) in all_pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4032_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4032",
        doc: "comparing runtime.GOOS or runtime.GOARCH against impossible value",
        url: "https://staticcheck.dev/docs/checks/#SA4032",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4032_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4032_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn not_windows_allows_freebsd() {
        assert!(!goos_impossible(&["!windows".into()], "freebsd"));
        assert!(goos_impossible(&["!windows".into()], "windows"));
    }

    #[test]
    fn linux_only_rejects_windows() {
        assert!(goos_impossible(&["linux".into()], "windows"));
        assert!(!goos_impossible(&["linux".into()], "linux"));
    }
}
