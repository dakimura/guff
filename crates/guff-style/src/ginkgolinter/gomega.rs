//! `internal/gomegahandler`: find the root of a Gomega call chain by what its
//! calls return, not by what they are named.

use guff::ast::{CallExpr, Expr, File};
use guff_types::arena::TypeId;

use super::types::Tc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CallType {
    Other,
    Sync,
    Async,
    Matcher,
}

/// `GomegaBasicInfo`.
pub(super) struct BasicInfo<'o> {
    pub method_name: String,
    pub has_error_method: bool,
    pub root_call: &'o CallExpr,
    pub call_type: CallType,
}

/// The package-level half of `GetGomegaHandler`: whether
/// `github.com/onsi/gomega/types` is in the import graph at all, and its three
/// interfaces.
pub(super) struct Interfaces {
    sync: Option<TypeId>,
    async_: Option<TypeId>,
    matcher: Option<TypeId>,
}

impl Interfaces {
    pub fn find(tc: &Tc<'_>) -> Option<Interfaces> {
        let pkg = tc.find_package("github.com/onsi/gomega/types")?;
        Some(Interfaces {
            sync: tc.lookup_interface(pkg, "Assertion"),
            async_: tc.lookup_interface(pkg, "AsyncAssertion"),
            matcher: tc.lookup_interface(pkg, "GomegaMatcher"),
        })
    }
}

/// `gomegahandler.Handler`.
pub(super) struct Handler {
    /// The local name gomega is imported under; empty for a dot import.
    pub name: String,
    sync: Option<TypeId>,
    async_: Option<TypeId>,
    matcher: Option<TypeId>,
}

impl Handler {
    pub fn new(ifaces: &Interfaces, file: &File) -> Handler {
        let mut name = "gomega".to_string();
        for imp in &file.imports {
            if imp.path.value != "\"github.com/onsi/gomega\"" {
                continue;
            }
            match imp.name.as_ref().map(|n| n.name.as_str()) {
                Some(".") => name = String::new(),
                None => {}
                Some(n) => name = n.to_string(),
            }
        }
        Handler {
            name,
            sync: ifaces.sync,
            async_: ifaces.async_,
            matcher: ifaces.matcher,
        }
    }

    /// `implements`: `pass.TypesInfo.Types[expr]` against one interface.
    fn implements(&self, tc: &Tc<'_>, id: u32, iface: Option<TypeId>) -> bool {
        let Some(iface) = iface else {
            return false;
        };
        let Some(tv) = tc.tv_id(id) else {
            return false;
        };
        tc.implements(tv.typ, iface)
    }

    fn call_type(&self, tc: &Tc<'_>, id: u32) -> CallType {
        if self.implements(tc, id, self.sync) {
            CallType::Sync
        } else if self.implements(tc, id, self.async_) {
            CallType::Async
        } else if self.implements(tc, id, self.matcher) {
            CallType::Matcher
        } else {
            CallType::Other
        }
    }

    /// `GetGomegaBasicInfo`.
    pub fn basic_info<'o>(&self, tc: &Tc<'_>, mut expr: &'o CallExpr) -> Option<BasicInfo<'o>> {
        let mut has_error_method = false;
        loop {
            if let Expr::SelectorExpr(sel) = expr.fun.as_ref() {
                let x = sel.x.id();
                if self.implements(tc, x, self.sync)
                    || self.implements(tc, x, self.async_)
                    || self.implements(tc, x, self.matcher)
                {
                    let Expr::CallExpr(x) = sel.x.as_ref() else {
                        return None;
                    };
                    if sel.sel.name == "Error" {
                        has_error_method = true;
                    }
                    expr = x;
                    continue;
                }
            }
            let call_type = self.call_type(tc, expr.id);
            if call_type == CallType::Other {
                return None;
            }
            let method_name = match expr.fun.as_ref() {
                Expr::Ident(id) => id.name.clone(),
                Expr::SelectorExpr(sel) => sel.sel.name.clone(),
                _ => String::new(),
            };
            return Some(BasicInfo {
                method_name,
                has_error_method,
                root_call: expr,
                call_type,
            });
        }
    }
}
