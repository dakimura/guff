//! Port of [`github.com/leonklingele/grouper`](https://github.com/leonklingele/grouper)
//! (golangci-lint wrapper in `pkg/golinters/grouper`).
//!
//! Requires grouped and/or single global `import` / `const` / `var` / `type`
//! declarations. All flags default to **false** (golangci / upstream); enable
//! via `linters.settings.grouper`.

use std::sync::OnceLock;

use guff::ast::GenDecl;
use guff::token::Token;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::GrouperOptions;

struct DeclInfo<'a> {
    decl: &'a GenDecl,
    is_group: bool,
}

fn is_group(gd: &GenDecl) -> bool {
    gd.lparen.is_valid()
}

fn collect_token<'a>(decls: &'a [guff::ast::Decl], tok: Token) -> Vec<DeclInfo<'a>> {
    let mut out = Vec::new();
    for decl in decls {
        let guff::ast::Decl::GenDecl(gd) = decl else {
            continue;
        };
        if gd.tok == Some(tok) {
            out.push(DeclInfo {
                decl: gd,
                is_group: is_group(gd),
            });
        }
    }
    out
}

fn check_globals(
    kind: &str,
    decls: &[DeclInfo<'_>],
    require_single: bool,
    require_grouping: bool,
    pending: &mut Vec<(u32, String)>,
) {
    let n = decls.len();
    if n == 0 {
        return;
    }

    if require_single && n > 1 {
        let second = &decls[1];
        pending.push((
            second.decl.tok_pos.0 as u32,
            format!("should only use a single global '{kind}' declaration, {n} found"),
        ));
    }

    if require_grouping {
        let ungrouped: Vec<&DeclInfo<'_>> = decls.iter().filter(|d| !d.is_group).collect();
        if let Some(first) = ungrouped.first() {
            pending.push((
                first.decl.tok_pos.0 as u32,
                format!("should only use grouped global '{kind}' declarations"),
            ));
        }
    }
}

fn check_imports(
    decls: &[DeclInfo<'_>],
    require_single: bool,
    require_grouping: bool,
    pending: &mut Vec<(u32, String)>,
) {
    let n = decls.len();
    if n == 0 {
        return;
    }

    if require_single && n > 1 {
        let second = &decls[1];
        pending.push((
            second.decl.tok_pos.0 as u32,
            format!("should only use a single 'import' declaration, {n} found"),
        ));
    }

    if require_grouping {
        let ungrouped: Vec<&DeclInfo<'_>> = decls.iter().filter(|d| !d.is_group).collect();
        if let Some(first) = ungrouped.first() {
            pending.push((
                first.decl.tok_pos.0 as u32,
                "should only use grouped 'import' declarations".to_string(),
            ));
        }
    }
}

fn check_file(decls: &[guff::ast::Decl], opts: &GrouperOptions, pending: &mut Vec<(u32, String)>) {
    check_imports(
        &collect_token(decls, Token::IMPORT),
        opts.import_require_single_import,
        opts.import_require_grouping,
        pending,
    );
    check_globals(
        "const",
        &collect_token(decls, Token::CONST),
        opts.const_require_single_const,
        opts.const_require_grouping,
        pending,
    );
    check_globals(
        "var",
        &collect_token(decls, Token::VAR),
        opts.var_require_single_var,
        opts.var_require_grouping,
        pending,
    );
    check_globals(
        "type",
        &collect_token(decls, Token::TYPE),
        opts.type_require_single_type,
        opts.type_require_grouping,
        pending,
    );
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "grouper requires inspect analyzer".to_string())?;

    // Upstream / golangci defaults: all flags false (noop).
    let opts = pass
        .settings::<GrouperOptions>("grouper")
        .copied()
        .unwrap_or_default();

    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        check_file(&file.decls, &opts, &mut pending);
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "grouper",
        doc: "analyze expression groups; require grouped/single import/const/var/type decls",
        url: "https://github.com/leonklingele/grouper",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn graph_ok() {
        validate(&[analyzer()]).expect("grouper analyzer graph");
    }

    #[test]
    fn default_is_noop() {
        let opts = GrouperOptions::default();
        assert!(!opts.const_require_single_const);
        assert!(!opts.const_require_grouping);
        assert!(!opts.import_require_single_import);
        assert!(!opts.import_require_grouping);
        assert!(!opts.type_require_single_type);
        assert!(!opts.type_require_grouping);
        assert!(!opts.var_require_single_var);
        assert!(!opts.var_require_grouping);
    }

    #[test]
    fn enabled_turns_checks_on() {
        let opts = GrouperOptions::enabled();
        assert!(opts.const_require_single_const);
        assert!(opts.const_require_grouping);
        assert!(opts.import_require_single_import);
        assert!(opts.import_require_grouping);
        assert!(opts.type_require_single_type);
        assert!(opts.type_require_grouping);
        assert!(opts.var_require_single_var);
        assert!(opts.var_require_grouping);
    }
}
