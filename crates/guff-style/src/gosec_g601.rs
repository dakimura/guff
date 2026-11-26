//! Gosec **G601** — implicit memory aliasing in a `range` loop (pre-Go 1.22).
//!
//! Port of securego/gosec v2.26.1 `rules/implicit_aliasing.go` (the version
//! golangci-lint 2.12.2 pins).
//!
//! Upstream is not a query about a loop; it is a **stateful preorder walk**, and
//! its state is what decides most shapes:
//!
//! - a `RangeStmt` whose value is an identifier adds that variable to a set of
//!   aliased vars and raises `rightBrace` to the loop body's `}` if it is
//!   further along — `rightBrace` only ever grows;
//! - every `UnaryExpr` (any operator: `-v` and `<-ch` too) past `rightBrace`
//!   first empties the set;
//! - a `ReturnStmt` marks each of its top-level `&x` results as acceptable, and
//!   the walk reaches the return before its results, so `return &v` is silent
//!   but `return f(&v)` is not;
//! - `&x` fires when `x` strips (through selectors and nested unaries, but not
//!   parens or index expressions) to an identifier in the set, unless there was
//!   a selector **and** the variable's type is literally a pointer.
//!
//! The consequences, all measured against golangci-lint in a `go 1.18` module:
//! the key of a range is never tracked (so `for v := range ch` is silent — `v`
//! is the key there); `&v` after an inner loop but still inside an outer range
//! body fires even for a variable assigned with `=`; `&(v)` is silent; a named
//! pointer type (`type P *T`) or an alias of one (`type A = *T`, a
//! `*types.Alias` to go/types) fires on `&v.f` because neither is a
//! `*types.Pointer`.
//!
//! The state is per package and carries across its files in order, like
//! gosec's one rule instance per package.
//!
//! **Version gate.** Upstream returns before looking at anything when
//! `gosec.GoVersion()` is 1.22 or later. That is not the file's version and not
//! the toolchain: golangci-lint sets `GOSECGOVERSION` to its `run.go`, which is
//! the configured value or, unset, `detectGoVersion` over the main go.mod —
//! the **`toolchain`** line if there is one, then the `go` line. The value
//! arrives here as [`GosecOptions::go`].

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

use guff::ast::{Expr, File, Ident, UnaryExpr};
use guff::token::Token;
use guff::walk::{preorder, NodeRef};
use guff_analysis::Pass;
use guff_types::arena::{ObjectData, ObjectId, TypeData};

use crate::options::GosecOptions;

const MESSAGE: &str = "G601: Implicit memory aliasing in for loop.";

/// Report G601 into `pending` (`(pos, end, message)` like every gosec rule).
pub(crate) fn check_g601(
    pass: &Pass<'_>,
    enabled: &HashSet<&'static str>,
    opts: &GosecOptions,
    pending: &mut Vec<(u32, u32, String)>,
) {
    if !enabled.contains("G601") {
        return;
    }
    let version = opts
        .go
        .clone()
        .unwrap_or_else(|| guff_analysis::code::module_go_version(pass));
    if at_least_go_1_22(&version) {
        return;
    }

    let mut state = State::default();
    for file in pass.files() {
        state.walk(pass, file, pending);
    }
}

/// `gosec.GoVersion()`'s parse and the rule's `major == 1 && minor >= 22 ||
/// major > 1`. The pattern is `(\d+).(\d+)(?:.(\d+))?.*` with unescaped dots,
/// and a string it does not match is `0.0.0` — which is *below* 1.22, so an
/// unparsable version leaves the rule on.
fn at_least_go_1_22(version: &str) -> bool {
    let (major, minor) = parse_major_minor(version);
    (major == 1 && minor >= 22) || major > 1
}

fn parse_major_minor(version: &str) -> (u64, u64) {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(\d+).(\d+)(?:.(\d+))?.*").expect("gosec version pattern"));
    let Some(c) = re.captures(version) else {
        return (0, 0);
    };
    let num = |i: usize| c.get(i).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
    (num(1), num(2))
}

#[derive(Default)]
struct State {
    aliases: HashSet<ObjectId>,
    right_brace: u32,
    /// `&x` results of a return statement, by their operator position.
    acceptable: HashSet<u32>,
}

impl State {
    fn walk(&mut self, pass: &Pass<'_>, file: &File, pending: &mut Vec<(u32, u32, String)>) {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::RangeStmt(rs) => {
                    if let Some(Expr::Ident(value)) = &rs.value {
                        if let Some(obj) = var_of(pass, value) {
                            self.aliases.insert(obj);
                            let rbrace = rs.body.rbrace.0 as u32;
                            if self.right_brace < rbrace {
                                self.right_brace = rbrace;
                            }
                        }
                    }
                }
                NodeRef::UnaryExpr(u) => self.unary(pass, u, pending),
                NodeRef::ReturnStmt(ret) => {
                    for res in &ret.results {
                        if let Expr::UnaryExpr(u) = res {
                            if u.op == Token::AND {
                                self.acceptable.insert(u.op_pos.0 as u32);
                            }
                        }
                    }
                }
                _ => {}
            }
            true
        });
    }

    fn unary(&mut self, pass: &Pass<'_>, u: &UnaryExpr, pending: &mut Vec<(u32, u32, String)>) {
        let pos = u.op_pos.0 as u32;
        if pos > self.right_brace {
            self.aliases.clear();
            self.acceptable.clear();
        }
        if self.aliases.is_empty() || self.acceptable.contains(&pos) || u.op != Token::AND {
            return;
        }
        let Some((ident, has_selector)) = ident_expr(&u.x, false) else {
            return;
        };
        let Some(obj) = var_of(pass, ident) else {
            return;
        };
        if !self.aliases.contains(&obj) {
            return;
        }
        if has_selector && type_is_pointer(pass, ident, obj) {
            return;
        }
        pending.push((pos, u.x.end().0 as u32, MESSAGE.to_string()));
    }
}

/// `doGetIdentExpr`: an identifier, through selectors (which set
/// `has_selector`) and nested unary expressions. Anything else — a paren, an
/// index, a composite literal — is no identifier.
fn ident_expr(expr: &Expr, has_selector: bool) -> Option<(&Ident, bool)> {
    match expr {
        Expr::Ident(id) => Some((id, has_selector)),
        Expr::SelectorExpr(sel) => ident_expr(&sel.x, true),
        Expr::UnaryExpr(u) => ident_expr(&u.x, has_selector),
        _ => None,
    }
}

/// `c.Info.ObjectOf(ident)` when it is a `*types.Var`.
fn var_of(pass: &Pass<'_>, ident: &Ident) -> Option<ObjectId> {
    let info = pass.types_info()?;
    let obj = info
        .defs
        .get(&ident.id)
        .copied()
        .flatten()
        .or_else(|| info.uses.get(&ident.id).copied())?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    matches!(artifacts.objects.get(obj), ObjectData::Var(_)).then_some(obj)
}

/// `_, isPointer := c.Info.TypeOf(identExpr).(*types.Pointer)` — the type as
/// recorded, not its underlying and not unaliased: a named pointer type and an
/// alias of a pointer are both "not a pointer" here.
fn type_is_pointer(pass: &Pass<'_>, ident: &Ident, obj: ObjectId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let recorded = pass
        .types_info()
        .and_then(|info| info.types.get(&ident.id).map(|tv| tv.typ));
    let Some(typ) = recorded.or_else(|| obj.typ(&artifacts.objects)) else {
        return false;
    };
    matches!(artifacts.types.get(typ), TypeData::Pointer(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_gate_matches_gosec_go_version() {
        for (v, want) in [
            ("1.18", false),
            ("1.21", false),
            ("1.21.5", false),
            ("go1.21", false),
            ("1.22", true),
            ("1.22.0", true),
            ("go1.23rc1", true),
            ("1.26", true),
            ("2.0", true),
            // No match is 0.0.0: below 1.22, the rule stays on.
            ("", false),
            ("1", false),
        ] {
            assert_eq!(at_least_go_1_22(v), want, "{v:?}");
        }
    }
}
