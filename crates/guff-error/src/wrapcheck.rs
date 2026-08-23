//! Port of [`github.com/tomarrell/wrapcheck`](https://github.com/tomarrell/wrapcheck).
//!
//! Settings (`linters.settings.wrapcheck`): `ignore-sigs`, `extra-ignore-sigs`,
//! `ignore-sig-regexps`, `ignore-package-globs`, `ignore-interface-regexps`,
//! `report-internal-errors`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, Expr, Ident, ReturnStmt, SelectorExpr};
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::{ObjectId, TypeData};
use guff_types::predicates::is_interface;
use guff_types::typestring::{signature_string, type_string};
use regex::Regex;

use crate::util::{is_pure_error, type_of, unparen};

const DEFAULT_IGNORE_SIGS: &[&str] = &[
    ".Errorf(",
    "errors.New(",
    "errors.Unwrap(",
    "errors.Join(",
    ".Wrap(",
    ".Wrapf(",
    ".WithMessage(",
    ".WithMessagef(",
    ".WithStack(",
];

/// Pass-time options from `linters.settings.wrapcheck`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapcheckOptions {
    /// Substrings of call signatures to ignore. `None` → upstream defaults.
    pub ignore_sigs: Option<Vec<String>>,
    /// Additional ignore substrings (appended to ignore_sigs / defaults).
    pub extra_ignore_sigs: Vec<String>,
    /// Regexps matched against call signatures.
    pub ignore_sig_regexps: Vec<String>,
    /// Package-path globs to treat as non-external (skip wrapcheck).
    pub ignore_package_globs: Vec<String>,
    /// Regexps matched against interface type names (package-name qualifier).
    pub ignore_interface_regexps: Vec<String>,
    /// When true, also report package-internal Ident / same-pkg returns.
    pub report_internal_errors: bool,
}

impl Default for WrapcheckOptions {
    fn default() -> Self {
        Self {
            ignore_sigs: None,
            extra_ignore_sigs: Vec::new(),
            ignore_sig_regexps: Vec::new(),
            ignore_package_globs: Vec::new(),
            ignore_interface_regexps: Vec::new(),
            report_internal_errors: false,
        }
    }
}

struct CompiledOptions {
    ignore_sigs: Vec<String>,
    extra_ignore_sigs: Vec<String>,
    ignore_sig_regexps: Vec<Regex>,
    ignore_package_globs: Vec<String>,
    ignore_interface_regexps: Vec<Regex>,
    report_internal_errors: bool,
}

impl CompiledOptions {
    fn from_options(opts: &WrapcheckOptions) -> Self {
        let ignore_sigs = match &opts.ignore_sigs {
            Some(sigs) => sigs.clone(),
            None => DEFAULT_IGNORE_SIGS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        };
        Self {
            ignore_sigs,
            extra_ignore_sigs: opts.extra_ignore_sigs.clone(),
            ignore_sig_regexps: compile_regexps(&opts.ignore_sig_regexps),
            ignore_package_globs: opts.ignore_package_globs.clone(),
            ignore_interface_regexps: compile_regexps(&opts.ignore_interface_regexps),
            report_internal_errors: opts.report_internal_errors,
        }
    }

    fn ignored_sig(&self, sig: &str) -> bool {
        self.ignore_sigs.iter().any(|p| sig.contains(p.as_str()))
            || self
                .extra_ignore_sigs
                .iter()
                .any(|p| sig.contains(p.as_str()))
            || self.ignore_sig_regexps.iter().any(|re| re.is_match(sig))
    }

    fn ignored_package(&self, pkg_path: &str) -> bool {
        self.ignore_package_globs
            .iter()
            .any(|g| package_glob_match(g, pkg_path))
    }

    fn ignored_interface(&self, name: &str) -> bool {
        self.ignore_interface_regexps
            .iter()
            .any(|re| re.is_match(name))
    }
}

fn compile_regexps(patterns: &[String]) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect()
}

/// gobwas/glob-style match for package paths (`encoding/*`, `github.com/pkg/**`).
fn package_glob_match(pattern: &str, path: &str) -> bool {
    if pattern == path {
        return true;
    }
    if !pattern.contains('*') && !pattern.contains('?') {
        return false;
    }
    let mut regex = String::from("^");
    for ch in pattern.chars() {
        match ch {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '.' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' | '\\' => {
                regex.push('\\');
                regex.push(ch);
            }
            c => regex.push(c),
        }
    }
    regex.push('$');
    Regex::new(&regex).is_ok_and(|re| re.is_match(path))
}

fn is_error_typ(pass: &Pass<'_>, typ: guff_types::TypeId) -> bool {
    guff_analysis::code::type_with_name(pass, typ, "error")
}

fn call_sig(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    // Match upstream wrapcheck: `pass.TypesInfo.ObjectOf(...).String()`
    // e.g. `func encoding/json.Marshal(v any) ([]byte, error)`.
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let obj = match unparen(&call.fun) {
        Expr::SelectorExpr(sel) => sel_func_obj(pass, sel)?,
        Expr::Ident(id) => object_of_ident(pass, id)?,
        _ => return code::call_name(pass, &call.fun).map(|n| format!("{n}(")),
    };
    let typ = obj.typ(&artifacts.objects)?;
    // go/types' `writeFuncName`: a method is `(RecvType).Name` with the
    // receiver written out, and only a bare function is `path.Name`. Building
    // the name from `obj.pkg()` alone gave `os/exec.StdinPipe` for a method,
    // dropping the receiver — a name Go never prints, so wrapcheck's message
    // and every `ignoreSigs` pattern matched against it were both wrong.
    // `code::type_func_name` is that function, including its interface case.
    let qualified_name = code::type_func_name(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        obj,
    );
    let qf = |pkg_id, parena: &guff_types::arena::PackageArena| {
        parena.get(pkg_id).name().to_string()
    };
    let sig = match artifacts.types.get(typ) {
        TypeData::Signature(_) => signature_string(
            &artifacts.types,
            &artifacts.objects,
            &artifacts.packages,
            typ,
            Some(&qf),
        ),
        _ => "()".into(),
    };
    // go/types prints the `byte`/`rune` aliases; guff's Basic table uses uint8/int32.
    let sig = sig.replace("[]uint8", "[]byte").replace("[]int32", "[]rune");
    Some(format!("func {qualified_name}{sig}"))
}

fn object_of_ident(pass: &Pass<'_>, id: &Ident) -> Option<ObjectId> {
    let info = pass.types_info()?;
    info.uses
        .get(&id.id)
        .copied()
        .or_else(|| info.defs.get(&id.id).copied().flatten())
}

fn sel_func_obj(pass: &Pass<'_>, sel: &SelectorExpr) -> Option<ObjectId> {
    let info = pass.types_info()?;
    info.uses.get(&sel.sel.id).copied()
}

fn pkg_path_of_sel(pass: &Pass<'_>, sel: &SelectorExpr) -> Option<String> {
    let obj = sel_func_obj(pass, sel)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let pkg = obj.pkg(&artifacts.objects)?;
    Some(artifacts.packages.get(pkg).path().to_string())
}

fn is_from_other_pkg(pass: &Pass<'_>, sel: &SelectorExpr, opts: &CompiledOptions) -> bool {
    let Some(path) = pkg_path_of_sel(pass, sel) else {
        return false;
    };
    if opts.ignored_package(&path) {
        return false;
    }
    path != pass.pkg().pkg_path && !path.is_empty()
}

fn is_iface_method(pass: &Pass<'_>, sel: &SelectorExpr) -> bool {
    let Some(typ) = type_of(pass, &sel.x) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    is_interface(&artifacts.types, typ)
        && sel.sel.name.chars().next().is_some_and(|c| c.is_uppercase())
}

fn interface_type_name(pass: &Pass<'_>, sel: &SelectorExpr) -> Option<String> {
    let typ = type_of(pass, &sel.x)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let qf = |pkg_id, parena: &guff_types::arena::PackageArena| {
        parena.get(pkg_id).name().to_string()
    };
    Some(type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        Some(&qf),
    ))
}

fn report_unwrapped(
    pass: &Pass<'_>,
    call: &CallExpr,
    pos: u32,
    opts: &CompiledOptions,
    pending: &mut Vec<(u32, String)>,
) {
    if let Expr::Ident(_) = unparen(&call.fun) {
        if !opts.report_internal_errors {
            return;
        }
        let Some(sig) = call_sig(pass, call) else {
            return;
        };
        if opts.ignored_sig(&sig) {
            return;
        }
        pending.push((
            pos,
            format!("package-internal error should be wrapped: sig: {sig}"),
        ));
        return;
    }
    let Expr::SelectorExpr(sel) = unparen(&call.fun) else {
        return;
    };
    let Some(sig) = call_sig(pass, call) else {
        return;
    };
    if opts.ignored_sig(&sig) {
        return;
    }
    // Upstream: exported interface methods are reported unless ignored by
    // interface-regexp or package-glob (then fall through to other checks).
    if is_iface_method(pass, sel) {
        let pkg_ignored = pkg_path_of_sel(pass, sel).is_some_and(|p| opts.ignored_package(&p));
        let iface_ignored =
            interface_type_name(pass, sel).is_some_and(|n| opts.ignored_interface(&n));
        if !pkg_ignored && !iface_ignored {
            pending.push((
                pos,
                format!("error returned from interface method should be wrapped: sig: {sig}"),
            ));
            return;
        }
    }
    if is_from_other_pkg(pass, sel, opts) {
        pending.push((
            pos,
            format!("error returned from external package is unwrapped: sig: {sig}"),
        ));
        return;
    }
    if opts.report_internal_errors {
        pending.push((
            pos,
            format!("package-internal error should be wrapped: sig: {sig}"),
        ));
    }
}

fn call_returns_error(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Some(typ) = type_of(pass, &Expr::CallExpr(call.clone())) else {
        let info = match pass.types_info() {
            Some(i) => i,
            None => return false,
        };
        return info
            .types
            .get(&call.id)
            .is_some_and(|tav| is_error_typ(pass, tav.typ) || is_tuple_with_error(pass, tav.typ));
    };
    is_error_typ(pass, typ) || is_tuple_with_error(pass, typ)
}

fn is_tuple_with_error(pass: &Pass<'_>, typ: guff_types::TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    match artifacts.types.get(typ) {
        TypeData::Tuple(t) => (0..t.len()).any(|i| {
            t.at(i)
                .typ(&artifacts.objects)
                .is_some_and(|rt| is_error_typ(pass, rt))
        }),
        _ => false,
    }
}

fn prev_err_assign<'a>(
    pass: &Pass<'_>,
    file: &'a guff::ast::File,
    return_ident: &Ident,
) -> Option<&'a AssignStmt> {
    let ret_obj = object_of_ident(pass, return_ident)?;
    let ret_pos = return_ident.name_pos.0;
    let mut most_recent: Option<&AssignStmt> = None;
    walk::preorder(NodeRef::File(file), |n| {
        let NodeRef::AssignStmt(ass) = n else {
            return true;
        };
        if ass.tok_pos.0 as i64 > ret_pos {
            return true;
        }
        for lhs in &ass.lhs {
            let Expr::Ident(id) = unparen(lhs) else {
                continue;
            };
            if object_of_ident(pass, id) == Some(ret_obj) {
                most_recent = Some(ass);
            }
        }
        true
    });
    most_recent
}

fn check_return(
    pass: &Pass<'_>,
    file: &guff::ast::File,
    ret: &ReturnStmt,
    stack: &[NodeRef<'_>],
    opts: &CompiledOptions,
    pending: &mut Vec<(u32, String)>,
) {
    // Skip returns inside FuncLit.
    for n in stack.iter().rev() {
        match n {
            NodeRef::FuncLit(_) => return,
            NodeRef::FuncDecl(_) => break,
            _ => {}
        }
    }

    for expr in &ret.results {
        if let Expr::CallExpr(call) = unparen(expr) {
            if call_returns_error(pass, call) {
                // `call.Pos()`, which for a CallExpr is `Fun.Pos()` — the
                // start of `flags.SetAnnotation(…)`, not its `(`. Upstream
                // reports there, and the two only differ once the callee is a
                // selector, so `return f()` agreed and `return x.M()` did not.
                report_unwrapped(pass, call, call.pos().0 as u32, opts, pending);
            }
            continue;
        }
        if !is_pure_error(pass, expr) {
            continue;
        }
        let Expr::Ident(ident) = unparen(expr) else {
            continue;
        };
        let Some(ass) = prev_err_assign(pass, file, ident) else {
            continue;
        };
        if ass.rhs.len() != 1 {
            continue;
        }
        let Expr::CallExpr(call) = unparen(&ass.rhs[0]) else {
            continue;
        };
        report_unwrapped(pass, call, ident.name_pos.0 as u32, opts, pending);
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "wrapcheck requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<WrapcheckOptions>("wrapcheck")
        .cloned()
        .unwrap_or_default();
    let opts = CompiledOptions::from_options(&options);

    let mut pending = Vec::new();
    for file in pass.files() {
        let mut stack = Vec::new();
        walk::preorder_stack(NodeRef::File(file), &mut stack, |n, stack| {
            if let NodeRef::ReturnStmt(ret) = n {
                check_return(pass, file, ret, stack, &opts, &mut pending);
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
        name: "wrapcheck",
        doc: "Checks that errors returned from external packages are wrapped",
        url: "https://github.com/tomarrell/wrapcheck",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_glob_encoding_star() {
        assert!(package_glob_match("encoding/*", "encoding/json"));
        assert!(package_glob_match("encoding/*", "encoding/xml"));
        assert!(!package_glob_match("encoding/*", "fmt"));
    }

    #[test]
    fn default_options_use_builtin_sigs() {
        let c = CompiledOptions::from_options(&WrapcheckOptions::default());
        assert!(c.ignored_sig("fmt.Errorf("));
        assert!(c.ignored_sig("errors.New("));
        assert!(!c.ignored_sig("encoding/json.Marshal("));
    }

    #[test]
    fn extra_ignore_sigs_append() {
        let opts = WrapcheckOptions {
            extra_ignore_sigs: vec!["encoding/json.Marshal(".into()],
            ..WrapcheckOptions::default()
        };
        let c = CompiledOptions::from_options(&opts);
        assert!(c.ignored_sig("encoding/json.Marshal("));
        assert!(c.ignored_sig("fmt.Errorf("));
    }
}
