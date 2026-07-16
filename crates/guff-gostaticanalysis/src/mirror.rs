//! Port of [`github.com/butuzov/mirror`](https://github.com/butuzov/mirror).
//!
//! Suggests mirror `string`/`[]byte` APIs to avoid unnecessary conversions.
//!
//! DEFERRED: full go/printer SuggestedFix parity for multi-line calls;
//! import rewrite when `AltPackage` differs from `Package`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use guff::ast::{
    ArrayType, BasicLit, CallExpr, CompositeLit, Expr, File, Ident, ImportSpec, SelectorExpr,
};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::typestring::type_string;

#[path = "mirror_tables.rs"]
mod tables;

const STRINGS: &str = "string";
const BYTES: &str = "[]byte";
const RUNE: &str = "rune";
const UNTYPED_RUNE: &str = "untyped rune";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViolationKind {
    Function,
    Method,
}

#[derive(Clone, Debug)]
struct Violation {
    kind: ViolationKind,
    args: &'static [usize],
    args_type: Option<&'static str>,
    targets: &'static str,
    package: &'static str,
    alt_package: Option<&'static str>,
    struct_name: Option<&'static str>,
    caller: &'static str,
    alt_caller: &'static str,
}

#[derive(Clone, Debug)]
struct Import {
    pkg: String,
    name: String,
}

type Imports = HashMap<String, Vec<Import>>;

struct Checker {
    violations: Vec<Violation>,
    packages: HashMap<String, Vec<usize>>,
}

impl Checker {
    fn new(violations: Vec<Violation>) -> Self {
        let mut c = Self {
            violations: Vec::new(),
            packages: HashMap::new(),
        };
        for v in violations {
            c.register(v);
        }
        c
    }

    fn register(&mut self, v: Violation) {
        self.violations.push(v);
        let idx = self.violations.len() - 1;
        let (package, struct_name) = {
            let v = &self.violations[idx];
            (v.package, v.struct_name)
        };
        if let Some(st) = struct_name {
            self.register_idx(format!("{package}.{st}"), idx);
        }
        self.register_idx(package.to_string(), idx);
    }

    fn register_idx(&mut self, pkg: String, idx: usize) {
        self.packages.entry(pkg).or_default().push(idx);
    }

    fn match_one(&self, pkg_name: &str, name: &str) -> Option<Violation> {
        self.matches(pkg_name, name).into_iter().next()
    }

    fn matches(&self, pkg_name: &str, name: &str) -> Vec<Violation> {
        let check_struct = pkg_name.contains('.');
        let mut out = Vec::new();
        let Some(idxs) = self.packages.get(pkg_name) else {
            return out;
        };
        for &idx in idxs {
            let v = &self.violations[idx];
            if v.caller != name {
                continue;
            }
            let is_func = v.struct_name.is_none();
            if check_struct == is_func {
                continue;
            }
            out.push(v.clone());
        }
        out
    }
}

#[derive(Default, Clone)]
struct Options {
    /// When false, skip `*_test.go` (upstream flag). golangci-lint forces true;
    /// guff matches that (use `run.tests` / exclude-rules to skip tests).
    with_tests: bool,
}

fn options(_pass: &Pass<'_>) -> Options {
    // golangci-lint always passes `with-tests: true` to mirror.
    Options { with_tests: true }
}

fn load_imports(file: &File, file_key: &str, imports: &mut Imports) {
    for spec in &file.imports {
        let ImportSpec { name, path, .. } = spec;
        let pkg = path.value.trim_matches('"').to_string();
        let alias = if let Some(id) = name {
            id.name.clone()
        } else {
            Path::new(&pkg)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&pkg)
                .to_string()
        };
        imports
            .entry(file_key.to_string())
            .or_default()
            .push(Import {
                pkg,
                name: alias,
            });
    }
}

fn lookup_import<'a>(imports: &'a Imports, file: &str, name: &str) -> Option<&'a str> {
    imports.get(file)?.iter().find(|i| i.name == name).map(|i| i.pkg.as_str())
}

fn normal_type(s: &str) -> &str {
    match s {
        UNTYPED_RUNE => RUNE,
        "untyped string" => STRINGS,
        // byte is an alias of uint8; type strings may use either form.
        "[]uint8" => BYTES,
        "uint8" => "byte",
        other => other,
    }
}



fn expected_arg_type(v: &Violation) -> &str {
    if let Some(t) = v.args_type {
        return t;
    }
    if v.targets == STRINGS {
        BYTES
    } else {
        STRINGS
    }
}

fn type_str(pass: &Pass<'_>, expr: &Expr) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let info = pass.types_info()?;
    let typ = info.types.get(&expr.id())?.typ;
    Some(type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    ))
}

fn is_conversion_call(pass: &Pass<'_>, call: &CallExpr) -> bool {
    // Prefer type info (upstream).
    if let Some(t) = type_str(pass, &call.fun) {
        if t == BYTES || t == STRINGS {
            return true;
        }
    }
    // AST fallback for robust fixture runs.
    match &*call.fun {
        Expr::Ident(id) if id.name == "string" => true,
        Expr::ArrayType(ArrayType { len: None, elt, .. }) => {
            matches!(elt.as_ref(), Expr::Ident(id) if id.name == "byte")
        }
        _ => false,
    }
}

fn handle(
    pass: &Pass<'_>,
    v: &Violation,
    call: &CallExpr,
) -> Option<HashMap<usize, Expr>> {
    let mut m = HashMap::new();
    let expect = expected_arg_type(v);
    for &i in v.args {
        if i >= call.args.len() {
            continue;
        }
        let Expr::CallExpr(inner) = &call.args[i] else {
            continue;
        };
        if !is_conversion_call(pass, inner) {
            continue;
        }
        if inner.args.is_empty() {
            continue;
        }
        let arg = &inner.args[0];
        let Some(ty) = type_str(pass, arg) else {
            // Fallback: CompositeLit / conversion patterns.
            let ok = match expect {
                BYTES => matches!(
                    arg,
                    Expr::CompositeLit(CompositeLit { ty: Some(t), .. })
                        if matches!(t.as_ref(), Expr::ArrayType(ArrayType { len: None, elt, .. })
                            if matches!(elt.as_ref(), Expr::Ident(id) if id.name == "byte"))
                ) || matches!(
                    arg,
                    Expr::CallExpr(c)
                        if matches!(&*c.fun, Expr::ArrayType(ArrayType { len: None, elt, .. })
                            if matches!(elt.as_ref(), Expr::Ident(id) if id.name == "byte"))
                ),
                STRINGS => matches!(
                    arg,
                    Expr::BasicLit(BasicLit {
                        kind: Some(Token::STRING),
                        ..
                    })
                ) || matches!(
                    arg,
                    Expr::CallExpr(c) if matches!(&*c.fun, Expr::Ident(id) if id.name == "string")
                ) || matches!(arg, Expr::Ident(_)),
                RUNE => matches!(
                    arg,
                    Expr::BasicLit(BasicLit {
                        kind: Some(Token::CHAR),
                        ..
                    })
                ) || matches!(
                    arg,
                    Expr::CallExpr(c) if matches!(&*c.fun, Expr::Ident(id) if id.name == "rune")
                ),
                _ => false,
            };
            if !ok {
                continue;
            }
            m.insert(i, arg.clone());
            continue;
        };
        if normal_type(&ty) != expect {
            continue;
        }
        m.insert(i, arg.clone());
    }
    if m.len() == v.args.len() {
        Some(m)
    } else {
        None
    }
}

fn message(v: &Violation) -> String {
    if v.kind == ViolationKind::Method {
        let st = v.struct_name.unwrap_or("");
        let pkg = Path::new(v.package)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(v.package);
        return format!("avoid allocations with (*{pkg}.{st}).{}", v.alt_caller);
    }
    let pkg = v.alt_package.unwrap_or(v.package);
    let pkg = Path::new(pkg)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(pkg);
    format!("avoid allocations with {pkg}.{}", v.alt_caller)
}

fn print_expr(expr: &Expr) -> String {
    match expr {
        Expr::Ident(Ident { name, .. }) => name.clone(),
        Expr::BasicLit(BasicLit { value, .. }) => value.clone(),
        Expr::SelectorExpr(SelectorExpr { x, sel, .. }) => {
            format!("{}.{}", print_expr(x), sel.name)
        }
        Expr::StarExpr(s) => format!("*{}", print_expr(&s.x)),
        Expr::ParenExpr(p) => format!("({})", print_expr(&p.x)),
        Expr::UnaryExpr(u) => format!("{}{}", u.op, print_expr(&u.x)),
        Expr::CallExpr(c) => {
            let fun = print_expr(&c.fun);
            let args: Vec<_> = c.args.iter().map(print_expr).collect();
            format!("{fun}({})", args.join(", "))
        }
        Expr::IndexExpr(ix) => format!("{}[{}]", print_expr(&ix.x), print_expr(&ix.index)),
        Expr::CompositeLit(cl) => {
            let ty = cl
                .ty
                .as_ref()
                .map(|t| print_expr(t))
                .unwrap_or_default();
            let elts: Vec<_> = cl.elts.iter().map(print_expr).collect();
            format!("{ty}{{{}}}", elts.join(", "))
        }
        Expr::ArrayType(ArrayType { len, elt, .. }) => {
            let n = len.as_ref().map(|e| print_expr(e)).unwrap_or_default();
            format!("[{n}]{}", print_expr(elt))
        }
        _ => "/*…*/".into(),
    }
}

fn suggest(
    v: &Violation,
    base: Option<&str>,
    call: &CallExpr,
    fixed: &HashMap<usize, Expr>,
) -> Option<String> {
    // Skip multi-line / complex reconstructions.
    let mut buf = String::new();
    if let Some(b) = base {
        if b.contains('\n') || b.contains("/*") {
            return None;
        }
        buf.push_str(b);
        buf.push('.');
    }
    buf.push_str(v.alt_caller);
    buf.push('(');
    for (idx, arg) in call.args.iter().enumerate() {
        if idx > 0 {
            buf.push_str(", ");
        }
        if let Some(fixed_arg) = fixed.get(&idx) {
            let s = print_expr(fixed_arg);
            if s.contains("/*") || s.contains('\n') {
                return None;
            }
            buf.push_str(&s);
        } else {
            let s = print_expr(arg);
            if s.contains("/*") || s.contains('\n') {
                return None;
            }
            buf.push_str(&s);
        }
    }
    buf.push(')');
    Some(buf)
}

fn clean_asterisk(s: &str) -> &str {
    s.strip_prefix('*').unwrap_or(s)
}

fn file_key(pass: &Pass<'_>, file: &File) -> String {
    let fset = pass.fset();
    let pos = file.pos();
    if pos.0 != 0 {
        let name = fset.position(pos).filename;
        if !name.is_empty() {
            return name;
        }
    }
    file.name.name.clone()
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "mirror requires inspect analyzer".to_string())?;

    let opts = options(pass);
    let check = Checker::new(tables::all_violations());

    let mut imports = Imports::new();
    for file in pass.files() {
        let key = file_key(pass, file);
        load_imports(file, &key, &mut imports);
    }

    let mut pending: Vec<Diagnostic> = Vec::new();

    for file in pass.files() {
        let key = file_key(pass, file);
        if !opts.with_tests && key.ends_with("_test.go") {
            continue;
        }

        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            let NodeRef::CallExpr(call) = n else {
                return true;
            };

            match &*call.fun {
                Expr::SelectorExpr(sel) => {
                    let Expr::Ident(x) = sel.x.as_ref() else {
                        return true;
                    };
                    let name = sel.sel.name.as_str();

                    // Case 1: package function call.
                    if let Some(pkg) = lookup_import(&imports, &key, &x.name) {
                        if let Some(v) = check.match_one(pkg, name) {
                            if let Some(args) = handle(pass, &v, call) {
                                let base = print_expr(&sel.x);
                                push_diag(&mut pending, &v, Some(&base), call, &args);
                            }
                            return true;
                        }
                    }

                    // Case 2: method call.
                    let Some(recv_ty) = type_str(pass, &sel.x) else {
                        return true;
                    };
                    let pkg_struct = clean_asterisk(&recv_ty);
                    for v in check.matches(pkg_struct, name) {
                        if let Some(args) = handle(pass, &v, call) {
                            let base = print_expr(&sel.x);
                            push_diag(&mut pending, &v, Some(&base), call, &args);
                            break;
                        }
                    }
                }
                Expr::Ident(id) => {
                    // Dot-import functions.
                    if let Some(pkg) = lookup_import(&imports, &key, ".") {
                        if let Some(v) = check.match_one(pkg, &id.name) {
                            if let Some(args) = handle(pass, &v, call) {
                                push_diag(&mut pending, &v, None, call, &args);
                            }
                        }
                    }
                }
                _ => {}
            }
            true
        });
    }

    for diag in pending {
        pass.report(diag);
    }
    Ok(None)
}

fn push_diag(
    pending: &mut Vec<Diagnostic>,
    v: &Violation,
    base: Option<&str>,
    call: &CallExpr,
    args: &HashMap<usize, Expr>,
) {
    let pos = call.pos().0 as u32;
    let end = call.end().0 as u32;
    let mut diag = Diagnostic {
        pos,
        end,
        message: message(v),
        ..Diagnostic::default()
    };

    let alt_pkg = v.alt_package.unwrap_or(v.package);
    let same_pkg = alt_pkg == v.package;
    let can_fix = match v.kind {
        ViolationKind::Method => true,
        ViolationKind::Function => same_pkg,
    };
    if can_fix {
        if let Some(new_text) = suggest(v, base, call, args) {
            if !new_text.contains('\n') {
                diag.suggested_fixes = vec![SuggestedFix {
                    message: "Fix Issue With".into(),
                    text_edits: vec![TextEdit {
                        pos,
                        end,
                        new_text,
                    }],
                }];
            }
        }
    }
    pending.push(diag);
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "mirror",
        doc: "reports wrong mirror patterns of bytes/strings usage",
        url: "https://github.com/butuzov/mirror",
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
    fn tables_nonempty() {
        assert!(tables::all_violations().len() >= 70);
    }

    #[test]
    fn messages() {
        let v = Violation {
            kind: ViolationKind::Function,
            args: &[0, 1],
            args_type: None,
            targets: STRINGS,
            package: "strings",
            alt_package: Some("bytes"),
            struct_name: None,
            caller: "Compare",
            alt_caller: "Compare",
        };
        assert_eq!(message(&v), "avoid allocations with bytes.Compare");

        let m = Violation {
            kind: ViolationKind::Method,
            args: &[0],
            args_type: None,
            targets: BYTES,
            package: "regexp",
            alt_package: None,
            struct_name: Some("Regexp"),
            caller: "Match",
            alt_caller: "MatchString",
        };
        assert_eq!(
            message(&m),
            "avoid allocations with (*regexp.Regexp).MatchString"
        );
    }
}
