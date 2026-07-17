//! Port of [`github.com/manuelarte/funcorder`](https://github.com/manuelarte/funcorder)
//! (golangci-lint wrapper in `pkg/golinters/funcorder`).
//!
//! Checks the order of functions, methods, and constructors:
//! - `constructor` (default on): a constructor (`New*` / `Must*` returning the
//!   struct type) must be placed after the struct declaration and before the
//!   struct's methods.
//! - `struct-method` (default on): exported methods must be placed before
//!   unexported methods of the same struct.
//! - `alphabetical` (default off): constructors and methods are sorted
//!   alphabetically within their group.
//! - `function` (default off): exported top-level functions must be placed
//!   before unexported ones (`init` excluded).
//!
//! Processing is per-file, mirroring upstream's `FileProcessor`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{Decl, Expr, File, FuncDecl, Ident, Spec};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::FuncorderOptions;

/// Minimal record of a function / method declaration.
struct FuncInfo {
    name: String,
    pos: i64,
    exported: bool,
}

/// Everything gathered for a single named type within one file.
#[derive(Default)]
struct Holder {
    /// Position of the type declaration (`ast.TypeSpec.Pos()`), if declared in
    /// this file. `None` means the type is not declared here → skip.
    struct_pos: Option<i64>,
    constructors: Vec<FuncInfo>,
    methods: Vec<FuncInfo>,
}

/// Go's `getIdent`: unwrap `*T` to the underlying identifier.
fn get_ident(expr: &Expr) -> Option<&Ident> {
    match expr {
        Expr::Ident(id) => Some(id),
        Expr::StarExpr(s) => get_ident(&s.x),
        _ => None,
    }
}

/// Go's `funcCanBeConstructor` + `NewStructConstructor`: returns the name of the
/// struct type this function constructs, if it qualifies.
fn constructor_return_type(fd: &FuncDecl) -> Option<String> {
    if !fd.name.is_exported() || fd.recv.is_some() {
        return None;
    }
    let results = fd.ty.results.as_ref()?;
    let first = results.list.first()?;
    let lower = fd.name.name.to_lowercase();
    let is_ctor = ["new", "must"]
        .iter()
        .any(|p| lower.starts_with(p) && fd.name.name.len() > p.len());
    if !is_ctor {
        return None;
    }
    let ty = first.ty.as_ref()?;
    get_ident(ty).map(|id| id.name.clone())
}

/// Go's `funcIsMethod`: receiver type name for a single-receiver method.
fn method_receiver_type(fd: &FuncDecl) -> Option<String> {
    let recv = fd.recv.as_ref()?;
    if recv.list.len() != 1 {
        return None;
    }
    let ty = recv.list[0].ty.as_ref()?;
    get_ident(ty).map(|id| id.name.clone())
}

fn func_info(fd: &FuncDecl) -> FuncInfo {
    FuncInfo {
        name: fd.name.name.clone(),
        pos: fd.ty.pos().0,
        exported: fd.name.is_exported(),
    }
}

fn analyze_constructor(
    struct_name: &str,
    struct_pos: i64,
    holder: &Holder,
    opts: &FuncorderOptions,
    pending: &mut Vec<(i64, String)>,
) {
    for (i, c) in holder.constructors.iter().enumerate() {
        if c.pos < struct_pos {
            pending.push((
                c.pos,
                format!(
                    "constructor {:?} for struct {:?} should be placed after the struct declaration",
                    c.name, struct_name
                ),
            ));
        }
        if let Some(first) = holder.methods.first() {
            if c.pos > first.pos {
                pending.push((
                    c.pos,
                    format!(
                        "constructor {:?} for struct {:?} should be placed before struct method {:?}",
                        c.name, struct_name, first.name
                    ),
                ));
            }
        }
        if opts.alphabetical {
            if let Some(next) = holder.constructors.get(i + 1) {
                if c.name > next.name {
                    pending.push((
                        next.pos,
                        format!(
                            "constructor {:?} for struct {:?} should be placed before constructor {:?}",
                            next.name, struct_name, c.name
                        ),
                    ));
                }
            }
        }
    }
}

fn sort_diagnostics(group: &[&FuncInfo], struct_name: &str, pending: &mut Vec<(i64, String)>) {
    for i in 0..group.len() {
        let Some(next) = group.get(i + 1) else {
            continue;
        };
        if group[i].name > next.name {
            pending.push((
                next.pos,
                format!(
                    "method {:?} for struct {:?} should be placed before method {:?}",
                    next.name, struct_name, group[i].name
                ),
            ));
        }
    }
}

fn analyze_struct_method(
    struct_name: &str,
    holder: &Holder,
    opts: &FuncorderOptions,
    pending: &mut Vec<(i64, String)>,
) {
    let mut last_exported: Option<&FuncInfo> = None;
    for m in &holder.methods {
        if !m.exported {
            continue;
        }
        match last_exported {
            None => last_exported = Some(m),
            Some(le) if le.pos < m.pos => last_exported = Some(m),
            _ => {}
        }
    }

    if let Some(le) = last_exported {
        for m in &holder.methods {
            if m.exported || m.pos >= le.pos {
                continue;
            }
            pending.push((
                m.pos,
                format!(
                    "unexported method {:?} for struct {:?} should be placed after the exported method {:?}",
                    m.name, struct_name, le.name
                ),
            ));
        }
    }

    if opts.alphabetical {
        let exported: Vec<&FuncInfo> = holder.methods.iter().filter(|m| m.exported).collect();
        let unexported: Vec<&FuncInfo> = holder.methods.iter().filter(|m| !m.exported).collect();
        sort_diagnostics(&exported, struct_name, pending);
        sort_diagnostics(&unexported, struct_name, pending);
    }
}

fn analyze_functions(top_level: &[FuncInfo], pending: &mut Vec<(i64, String)>) {
    let mut last_exported: Option<&FuncInfo> = None;
    for f in top_level {
        if f.name == "init" || !f.exported {
            continue;
        }
        match last_exported {
            None => last_exported = Some(f),
            Some(le) if f.pos > le.pos => last_exported = Some(f),
            _ => {}
        }
    }

    let Some(le) = last_exported else {
        return;
    };
    for f in top_level {
        if f.name == "init" || f.exported || f.pos >= le.pos {
            continue;
        }
        pending.push((
            f.pos,
            format!(
                "unexported function {:?} should be placed after the exported function {:?}",
                f.name, le.name
            ),
        ));
    }
}

fn check_file(file: &File, opts: &FuncorderOptions, pending: &mut Vec<(i64, String)>) {
    let mut order: Vec<String> = Vec::new();
    let mut holders: HashMap<String, Holder> = HashMap::new();
    let mut top_level: Vec<FuncInfo> = Vec::new();

    for decl in &file.decls {
        match decl {
            Decl::FuncDecl(fd) => {
                if opts.function && fd.recv.is_none() {
                    top_level.push(func_info(fd));
                }
                if let Some(ret) = constructor_return_type(fd) {
                    if !holders.contains_key(&ret) {
                        order.push(ret.clone());
                    }
                    holders.entry(ret).or_default().constructors.push(func_info(fd));
                    continue;
                }
                if let Some(recv) = method_receiver_type(fd) {
                    if !holders.contains_key(&recv) {
                        order.push(recv.clone());
                    }
                    holders.entry(recv).or_default().methods.push(func_info(fd));
                }
            }
            Decl::GenDecl(gd) => {
                for spec in &gd.specs {
                    if let Spec::TypeSpec(ts) = spec {
                        let name = ts.name.name.clone();
                        if !holders.contains_key(&name) {
                            order.push(name.clone());
                        }
                        holders.entry(name).or_default().struct_pos = Some(ts.name.pos().0);
                    }
                }
            }
            Decl::BadDecl(_) => {}
        }
    }

    for name in &order {
        let holder = holders.get_mut(name).expect("holder exists");
        let Some(struct_pos) = holder.struct_pos else {
            continue;
        };
        holder.methods.sort_by_key(|m| m.pos);
        let holder = &*holder;
        if opts.constructor {
            analyze_constructor(name, struct_pos, holder, opts, pending);
        }
        if opts.struct_method {
            analyze_struct_method(name, holder, opts, pending);
        }
    }

    if opts.function {
        analyze_functions(&top_level, pending);
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "funcorder requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<FuncorderOptions>("funcorder")
        .copied()
        .unwrap_or_default();

    let mut pending: Vec<(i64, String)> = Vec::new();
    for file in pass.files() {
        check_file(file, &opts, &mut pending);
    }

    pending.sort_by_key(|(pos, _)| *pos);
    for (pos, message) in pending {
        pass.reportf(pos as u32, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "funcorder",
        doc: "checks the order of functions, methods, and constructors",
        url: "https://github.com/manuelarte/funcorder",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
