//! ST1003 — poorly chosen identifier (underscores, ALL_CAPS, initialisms).
//!
//! Port of `honnef.co/go/tools/stylecheck/st1003`. Non-default in upstream.
//! Uses the upstream default initialisms list; `initialisms` settings are DEFERRED.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{FieldList, FuncDecl};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::is_in_test_at;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

/// Upstream default initialisms (`honnef.co/go/tools/config.DefaultConfig`).
const DEFAULT_INITIALISMS: &[&str] = &[
    "ACL", "API", "ASCII", "CPU", "CSS", "DNS", "EOF", "GUID", "HTML", "HTTP", "HTTPS", "ID",
    "IP", "JSON", "QPS", "RAM", "RPC", "SLA", "SMTP", "SQL", "SSH", "TCP", "TLS", "TTL", "UDP",
    "UI", "GID", "UID", "UUID", "URI", "URL", "UTF8", "VM", "XML", "XMPP", "XSRF", "XSS", "SIP",
    "RTP", "AMQP", "DB", "TS",
];

fn known_name_exception(name: &str) -> bool {
    matches!(name, "LastInsertId" | "kWh")
}

fn all_caps(s: &str) -> bool {
    let mut has_upper = false;
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            has_upper = true;
        } else if !(c.is_ascii_digit() || c == '_') {
            return false;
        }
    }
    has_upper
}

/// Returns a differently-cased name if `name` should be rewritten (golint `lintName`).
fn lint_name(name: &str, initialisms: &HashSet<&str>) -> String {
    if name == "_" {
        return name.to_string();
    }
    if name.chars().all(|c| c.is_lowercase()) {
        return name.to_string();
    }

    let mut runes: Vec<char> = name.chars().collect();
    let mut w = 0usize;
    let mut i = 0usize;
    while i + 1 <= runes.len() {
        let mut eow = false;
        if i + 1 == runes.len() {
            eow = true;
        } else if runes[i + 1] == '_' && i + 1 != runes.len() - 1 {
            eow = true;
            let mut n = 1usize;
            while i + n + 1 < runes.len() && runes[i + n + 1] == '_' {
                n += 1;
            }
            if i + n + 1 < runes.len()
                && runes[i].is_ascii_digit()
                && runes[i + n + 1].is_ascii_digit()
            {
                n -= 1;
            }
            runes.drain(i + 1..i + 1 + n);
        } else if runes[i].is_lowercase() && !runes[i + 1].is_lowercase() {
            eow = true;
        }
        i += 1;
        if !eow {
            continue;
        }

        let word: String = runes[w..i].iter().collect();
        let upper = word.to_ascii_uppercase();
        if initialisms.contains(upper.as_str()) {
            let mut u = upper;
            if w == 0 && runes[w].is_lowercase() {
                u = u.to_ascii_lowercase();
            }
            for (j, ch) in u.chars().enumerate() {
                runes[w + j] = ch;
            }
        } else if w > 0 && word.chars().all(|c| c.is_lowercase()) {
            runes[w] = runes[w].to_ascii_uppercase();
        }
        w = i;
    }
    runes.into_iter().collect()
}

fn check(
    pending: &mut Vec<(u32, String)>,
    id_name: &str,
    id_pos: u32,
    thing: &str,
    initialisms: &HashSet<&str>,
) {
    if id_name == "_" || known_name_exception(id_name) {
        return;
    }
    if id_name.len() >= 5 && all_caps(id_name) && id_name.contains('_') {
        pending.push((
            id_pos,
            "should not use ALL_CAPS in Go names; use CamelCase instead".into(),
        ));
        return;
    }
    let should = lint_name(id_name, initialisms);
    if id_name == should {
        return;
    }
    if id_name.len() > 2 && id_name[1..id_name.len() - 1].contains('_') {
        pending.push((
            id_pos,
            format!("should not use underscores in Go names; {thing} {id_name} should be {should}"),
        ));
        return;
    }
    pending.push((id_pos, format!("{thing} {id_name} should be {should}")));
}

fn check_list(
    pending: &mut Vec<(u32, String)>,
    fl: Option<&FieldList>,
    thing: &str,
    initialisms: &HashSet<&str>,
) {
    let Some(fl) = fl else {
        return;
    };
    for f in &fl.list {
        for id in &f.names {
            check(pending, &id.name, id.pos().0 as u32, thing, initialisms);
        }
    }
}

fn is_technically_exported(f: &FuncDecl) -> bool {
    if f.recv.is_some() {
        return false;
    }
    let Some(doc) = &f.doc else {
        return false;
    };
    let export = "//export ";
    let linkname = "//go:linkname ";
    for c in &doc.list {
        let text = c.text.as_str();
        if text.starts_with(export)
            && text.len() == export.len() + f.name.name.len()
            && &text[export.len()..] == f.name.name
        {
            return true;
        }
        if text.starts_with(linkname) {
            return true;
        }
    }
    false
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ST1003 requires inspect analyzer".to_string())?
        .clone();

    let initialisms: HashSet<&str> = DEFAULT_INITIALISMS.iter().copied().collect();
    // DEFERRED: linters.settings.staticcheck / stylecheck initialisms override.

    let mut pending: Vec<(u32, String)> = Vec::new();

    for file in pass.files() {
        let pkg_name = file.name.name.as_str();
        if !pkg_name.ends_with("_test") && pkg_name.contains('_') {
            pending.push((
                file.name.pos().0 as u32,
                "should not use underscores in package names".into(),
            ));
        }
        if pkg_name.chars().any(|c| c.is_uppercase()) {
            pending.push((
                file.name.pos().0 as u32,
                format!(
                    "should not use MixedCaps in package name; {pkg_name} should be {}",
                    pkg_name.to_lowercase()
                ),
            ));
        }
    }

    inspect.preorder(pass.files(), |node| {
        match node {
            NodeRef::AssignStmt(stmt) if stmt.tok == Some(Token::DEFINE) => {
                for exp in &stmt.lhs {
                    if let guff::ast::Expr::Ident(id) = exp {
                        check(
                            &mut pending,
                            &id.name,
                            id.pos().0 as u32,
                            "var",
                            &initialisms,
                        );
                    }
                }
            }
            NodeRef::FuncDecl(fd) => {
                if fd.body.is_none() {
                    return;
                }
                if is_in_test_at(pass, fd.name.pos().0 as u32) {
                    let n = fd.name.name.as_str();
                    if n.starts_with("Example")
                        || n.starts_with("Test")
                        || n.starts_with("Benchmark")
                        || n.starts_with("Fuzz")
                    {
                        // Still check params/results below? Upstream returns
                        // entirely for these — skip the whole FuncDecl checks.
                        return;
                    }
                }
                let thing = if fd.recv.is_some() { "method" } else { "func" };
                if !is_technically_exported(fd) {
                    check(
                        &mut pending,
                        &fd.name.name,
                        fd.name.pos().0 as u32,
                        thing,
                        &initialisms,
                    );
                }
                check_list(
                    &mut pending,
                    fd.ty.params.as_ref(),
                    &format!("{thing} parameter"),
                    &initialisms,
                );
                check_list(
                    &mut pending,
                    fd.ty.results.as_ref(),
                    &format!("{thing} result"),
                    &initialisms,
                );
            }
            NodeRef::GenDecl(gen) => {
                if gen.tok == Some(Token::IMPORT) {
                    return;
                }
                let thing = match gen.tok {
                    Some(Token::CONST) => "const",
                    Some(Token::TYPE) => "type",
                    Some(Token::VAR) => "var",
                    _ => return,
                };
                for spec in &gen.specs {
                    match spec {
                        guff::ast::Spec::TypeSpec(ts) => {
                            check(
                                &mut pending,
                                &ts.name.name,
                                ts.name.pos().0 as u32,
                                thing,
                                &initialisms,
                            );
                        }
                        guff::ast::Spec::ValueSpec(vs) => {
                            for id in &vs.names {
                                check(
                                    &mut pending,
                                    &id.name,
                                    id.pos().0 as u32,
                                    thing,
                                    &initialisms,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            NodeRef::InterfaceType(iface) => {
                for x in &iface.methods.list {
                    let Some(ty) = &x.ty else {
                        continue;
                    };
                    let guff::ast::Expr::FuncType(ft) = ty else {
                        continue;
                    };
                    check_list(
                        &mut pending,
                        ft.params.as_ref(),
                        "interface method parameter",
                        &initialisms,
                    );
                    check_list(
                        &mut pending,
                        ft.results.as_ref(),
                        "interface method result",
                        &initialisms,
                    );
                }
            }
            NodeRef::RangeStmt(rng) if rng.tok == Some(Token::DEFINE) => {
                if let Some(guff::ast::Expr::Ident(id)) = rng.key.as_ref() {
                    check(
                        &mut pending,
                        &id.name,
                        id.pos().0 as u32,
                        "range var",
                        &initialisms,
                    );
                }
                if let Some(guff::ast::Expr::Ident(id)) = rng.value.as_ref() {
                    check(
                        &mut pending,
                        &id.name,
                        id.pos().0 as u32,
                        "range var",
                        &initialisms,
                    );
                }
            }
            NodeRef::StructType(st) => {
                for f in &st.fields.list {
                    for id in &f.names {
                        check(
                            &mut pending,
                            &id.name,
                            id.pos().0 as u32,
                            "struct field",
                            &initialisms,
                        );
                    }
                }
            }
            _ => {}
        }
    });

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn st1003_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1003",
        doc: "poorly chosen identifier",
        url: "https://staticcheck.dev/docs/checks/#ST1003",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1003_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1003_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn lint_name_initialisms() {
        let init: HashSet<&str> = DEFAULT_INITIALISMS.iter().copied().collect();
        assert_eq!(lint_name("fnId", &init), "fnID");
        assert_eq!(lint_name("fn_Id", &init), "fnID");
        assert_eq!(lint_name("abc_def", &init), "abcDef");
        assert_eq!(lint_name("foo_bar", &init), "fooBar");
    }
}
