//! Port of [`github.com/catenacyber/perfsprint`](https://github.com/catenacyber/perfsprint).
//!
//! Implements the default-on fmt.Sprint / fmt.Sprintf / fmt.Errorf rewrites
//! (string / bool / integer / hex / errors.New) and concat-loop
//! (string concatenation in loops → `strings.Builder`).
//!
//! Settings: `integer-format` / `int-conversion` / `error-format` / `err-error` /
//! `errorf` / `string-format` / `sprintf1` / `strconcat` / `bool-format` /
//! `hex-format` / `concat-loop` / `loop-other-ops`.
//!
//! DEFERRED: fiximports.

use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{AssignStmt, BasicLit, CallExpr, Decl, Expr, Spec, Stmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::alias::unalias_readonly;
use guff_types::api_predicates::api_implements;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::BasicKind;
use guff_types::TypeId;

use crate::options::PerfsprintOptions;

fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn basic_kind(pass: &Pass<'_>, typ: TypeId) -> Option<BasicKind> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let resolved = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(resolved) {
        TypeData::Basic(b) => Some(b.kind()),
        _ => None,
    }
}

fn is_basic(pass: &Pass<'_>, typ: TypeId, kinds: &[BasicKind]) -> bool {
    basic_kind(pass, typ).is_some_and(|k| kinds.contains(&k))
}

fn is_byte_slice_or_array(pass: &Pass<'_>, typ: TypeId) -> Option<bool> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let resolved = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(resolved) {
        TypeData::Slice(s) => {
            Some(matches!(
                basic_kind(pass, s.elem()),
                Some(BasicKind::Uint8)
            ))
        }
        TypeData::Array(a) => {
            Some(matches!(
                basic_kind(pass, a.elem()),
                Some(BasicKind::Uint8)
            ))
        }
        _ => None,
    }
}

fn one_of(v: &str, expected: &[&str]) -> bool {
    expected.iter().any(|e| *e == v)
}

fn unquote_string(lit: &str) -> Option<String> {
    if lit.len() < 2 {
        return None;
    }
    let quote = lit.as_bytes()[0];
    if quote != b'"' && quote != b'`' {
        return None;
    }
    if lit.as_bytes()[lit.len() - 1] != quote {
        return None;
    }
    if quote == b'`' {
        return Some(lit[1..lit.len() - 1].to_string());
    }
    // Minimal double-quote unquote (enough for format verbs in fixtures).
    let mut out = String::with_capacity(lit.len());
    let mut chars = lit[1..lit.len() - 1].chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

fn is_concatable(verb: &str) -> bool {
    let has_prefix = (verb.starts_with("%s") && !verb.contains("%[1]s"))
        || (verb.starts_with("%[1]s") && !verb.contains("%s"));
    let has_suffix = (verb.ends_with("%s") && !verb.contains("%[1]s"))
        || (verb.ends_with("%[1]s") && !verb.contains("%s"));
    if verb.matches("%[1]s").count() > 1 {
        return false;
    }
    (has_prefix || has_suffix) && !(has_prefix && has_suffix)
}

/// perfsprint renders with `formatNode` (`analyzer/analyzer.go`), which is
/// `format.Node` — go/printer with gofmt's config.
///
/// Most of its uses are suggested-fix text, which no tier can see, but
/// `err-error` puts the rendering in the *message*:
/// `fn+" can be replaced with "+errMethodCall`.
fn expr_string(pass: &Pass<'_>, e: &Expr) -> String {
    guff_analysis::code::node_text(pass, e).unwrap_or_default()
}

fn as_string_lit(expr: &Expr) -> Option<&BasicLit> {
    match expr {
        Expr::BasicLit(lit) if lit.kind == Some(Token::STRING) => Some(lit),
        _ => None,
    }
}

fn go_quote(s: &str) -> String {
    format!("{:?}", s) // Rust Debug quotes similarly enough for ASCII format fragments.
}

fn universe_error(pass: &Pass<'_>) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    for oid in artifacts.objects.ids() {
        let ObjectData::TypeName(tn) = artifacts.objects.get(oid) else {
            continue;
        };
        if tn.name() != "error" {
            continue;
        }
        if oid.pkg(&artifacts.objects).is_some() {
            continue;
        }
        return tn.typ();
    }
    None
}

/// Whether `typ` implements the universe `error` interface.
fn implements_error(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(err) = universe_error(pass) else {
        return false;
    };
    let mut types = artifacts.types.clone();
    api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        err,
    )
}

/// Upstream clears sub-flags when a parent format group is disabled.
fn effective_options(options: PerfsprintOptions) -> PerfsprintOptions {
    let mut o = options;
    if !o.integer_format {
        o.int_conversion = false;
    }
    if !o.error_format {
        o.err_error = false;
        o.errorf = false;
    }
    if !o.string_format {
        o.sprintf1 = false;
        o.strconcat = false;
    }
    o
}

struct Pending {
    pos: u32,
    end: u32,
    message: String,
    fixes: Vec<SuggestedFix>,
}

fn replace_call_prefix(call: &CallExpr, value: &Expr, new_prefix: &str) -> SuggestedFix {
    SuggestedFix {
        message: format!("Use {new_prefix}"),
        text_edits: vec![TextEdit {
            pos: call.pos().0 as u32,
            end: value.pos().0 as u32,
            new_text: format!("{new_prefix}("),
        }],
    }
}

fn replace_whole_call(call: &CallExpr, message: &str, new_text: String) -> SuggestedFix {
    SuggestedFix {
        message: message.into(),
        text_edits: vec![TextEdit {
            pos: call.pos().0 as u32,
            end: call.end().0 as u32,
            new_text,
        }],
    }
}

fn check_call(
    pass: &Pass<'_>,
    call: &CallExpr,
    options: &PerfsprintOptions,
    pending: &mut Vec<Pending>,
) {
    let Some(name) = code::call_name(pass, &call.fun) else {
        return;
    };

    let (fn_name, verb, value): (&str, String, &Expr) = match name.as_str() {
        "fmt.Errorf" if call.args.len() == 1 => ("fmt.Errorf", "%s".into(), &call.args[0]),
        "fmt.Sprint" if call.args.len() == 1 => ("fmt.Sprint", "%v".into(), &call.args[0]),
        "fmt.Sprintf" if call.args.len() == 1 => ("fmt.Sprintf", "%s".into(), &call.args[0]),
        "fmt.Sprintf" if call.args.len() == 2 => {
            let Some(lit) = as_string_lit(&call.args[0]) else {
                return;
            };
            let Some(mut verb) = unquote_string(&lit.value) else {
                return;
            };
            if let Some(rest) = verb.strip_prefix("%[1]") {
                verb = format!("%{rest}");
            }
            ("fmt.Sprintf", verb, &call.args[1])
        }
        _ => return,
    };

    let verb_ref = verb.as_str();
    let concat_ok = fn_name == "fmt.Sprintf" && is_concatable(verb_ref);
    if !one_of(verb_ref, &["%d", "%v", "%x", "%t", "%s"]) && !concat_ok {
        return;
    }

    let Some(value_type) = type_of(pass, value) else {
        return;
    };

    let mut report = |checker: &str, msg: String, fixes: Vec<SuggestedFix>| {
        let enabled = match checker {
            "integer-format" => options.integer_format,
            // errors.New rewrite: parent error-format + errorf sub-flag.
            "error-format" => options.error_format && options.errorf,
            // err.Error() rewrite: err-error (cleared when error-format is off).
            "err-error" => options.err_error,
            "string-format" => {
                options.string_format
                    || (options.sprintf1 && fn_name == "fmt.Sprintf" && call.args.len() == 1)
            }
            "bool-format" => options.bool_format,
            "hex-format" => options.hex_format,
            _ => true,
        };
        if !enabled {
            return;
        }
        // Upstream always labels err.Error() under the error-format checker name.
        let label = if checker == "err-error" {
            "error-format"
        } else {
            checker
        };
        pending.push(Pending {
            pos: call.pos().0 as u32,
            end: call.end().0 as u32,
            message: format!("{label}: {msg}"),
            fixes,
        });
    };

    if is_basic(pass, value_type, &[BasicKind::String]) && one_of(verb_ref, &["%v", "%s"]) {
        if fn_name == "fmt.Errorf" {
            report(
                "error-format",
                format!("{fn_name} can be replaced with errors.New"),
                vec![replace_call_prefix(call, value, "errors.New")],
            );
        } else {
            let text = expr_string(pass, value);
            report(
                "string-format",
                format!("{fn_name} can be replaced with just using the string"),
                vec![replace_whole_call(call, "Just use string value", text)],
            );
        }
        return;
    }

    // Known false positive if err is nil: fmt.Sprint(nil) does not panic like nil.Error().
    if implements_error(pass, value_type) && one_of(verb_ref, &["%v", "%s"]) {
        let err_call = format!("{}.Error()", expr_string(pass, value));
        report(
            "err-error",
            format!("{fn_name} can be replaced with {err_call}"),
            vec![replace_whole_call(
                call,
                &format!("Use {err_call}"),
                err_call,
            )],
        );
        return;
    }

    if is_basic(pass, value_type, &[BasicKind::Bool]) && one_of(verb_ref, &["%v", "%t"]) {
        report(
            "bool-format",
            format!("{fn_name} can be replaced with faster strconv.FormatBool"),
            vec![replace_call_prefix(call, value, "strconv.FormatBool")],
        );
        return;
    }

    if let Some(is_array) = is_byte_slice_or_array(pass, value_type) {
        if one_of(verb_ref, &["%x"]) {
            if is_array && !matches!(value, Expr::Ident(_)) {
                return;
            }
            let mut edits = vec![TextEdit {
                pos: call.pos().0 as u32,
                end: value.pos().0 as u32,
                new_text: "hex.EncodeToString(".into(),
            }];
            if is_array {
                edits.push(TextEdit {
                    pos: value.end().0 as u32,
                    end: value.end().0 as u32,
                    new_text: "[:]".into(),
                });
            }
            report(
                "hex-format",
                format!("{fn_name} can be replaced with faster hex.EncodeToString"),
                vec![SuggestedFix {
                    message: "Use hex.EncodeToString".into(),
                    text_edits: edits,
                }],
            );
            return;
        }
    }

    // int8/16/32 → strconv.Itoa(int(...)) requires a cast (`int-conversion`).
    if options.int_conversion
        && is_basic(
            pass,
            value_type,
            &[BasicKind::Int8, BasicKind::Int16, BasicKind::Int32],
        )
        && one_of(verb_ref, &["%v", "%d"])
    {
        report(
            "integer-format",
            format!("{fn_name} can be replaced with faster strconv.Itoa"),
            vec![SuggestedFix {
                message: "Use strconv.Itoa".into(),
                text_edits: vec![
                    TextEdit {
                        pos: call.pos().0 as u32,
                        end: value.pos().0 as u32,
                        new_text: "strconv.Itoa(int(".into(),
                    },
                    TextEdit {
                        pos: value.end().0 as u32,
                        end: value.end().0 as u32,
                        new_text: ")".into(),
                    },
                ],
            }],
        );
        return;
    }

    if is_basic(pass, value_type, &[BasicKind::Int]) && one_of(verb_ref, &["%v", "%d"]) {
        report(
            "integer-format",
            format!("{fn_name} can be replaced with faster strconv.Itoa"),
            vec![replace_call_prefix(call, value, "strconv.Itoa")],
        );
        return;
    }

    if is_basic(pass, value_type, &[BasicKind::Int64]) && one_of(verb_ref, &["%v", "%d"]) {
        report(
            "integer-format",
            format!("{fn_name} can be replaced with faster strconv.FormatInt"),
            vec![SuggestedFix {
                message: "Use strconv.FormatInt".into(),
                text_edits: vec![
                    TextEdit {
                        pos: call.pos().0 as u32,
                        end: value.pos().0 as u32,
                        new_text: "strconv.FormatInt(".into(),
                    },
                    TextEdit {
                        pos: value.end().0 as u32,
                        end: value.end().0 as u32,
                        new_text: ", 10".into(),
                    },
                ],
            }],
        );
        return;
    }

    // uint/uint8/16/32 → FormatUint(uint64(...)) requires a cast (`int-conversion`).
    if options.int_conversion
        && is_basic(
            pass,
            value_type,
            &[
                BasicKind::Uint8,
                BasicKind::Uint16,
                BasicKind::Uint32,
                BasicKind::Uint,
            ],
        )
        && one_of(verb_ref, &["%v", "%d", "%x"])
    {
        let base = if verb_ref == "%x" { "16" } else { "10" };
        report(
            "integer-format",
            format!("{fn_name} can be replaced with faster strconv.FormatUint"),
            vec![SuggestedFix {
                message: "Use strconv.FormatUint".into(),
                text_edits: vec![
                    TextEdit {
                        pos: call.pos().0 as u32,
                        end: value.pos().0 as u32,
                        new_text: "strconv.FormatUint(uint64(".into(),
                    },
                    TextEdit {
                        pos: value.end().0 as u32,
                        end: value.end().0 as u32,
                        new_text: format!("), {base}"),
                    },
                ],
            }],
        );
        return;
    }

    if is_basic(pass, value_type, &[BasicKind::Uint64])
        && one_of(verb_ref, &["%v", "%d", "%x"])
    {
        let base = if verb_ref == "%x" { "16" } else { "10" };
        report(
            "integer-format",
            format!("{fn_name} can be replaced with faster strconv.FormatUint"),
            vec![SuggestedFix {
                message: "Use strconv.FormatUint".into(),
                text_edits: vec![
                    TextEdit {
                        pos: call.pos().0 as u32,
                        end: value.pos().0 as u32,
                        new_text: "strconv.FormatUint(".into(),
                    },
                    TextEdit {
                        pos: value.end().0 as u32,
                        end: value.end().0 as u32,
                        new_text: format!(", {base}"),
                    },
                ],
            }],
        );
        return;
    }

    if is_basic(pass, value_type, &[BasicKind::String])
        && fn_name == "fmt.Sprintf"
        && is_concatable(verb_ref)
    {
        if !options.strconcat {
            return;
        }
        let val = expr_string(pass, value);
        let fix = if let Some(prefix) = verb_ref.strip_suffix("%s") {
            format!("{}+{}", go_quote(&prefix.replace("%%", "%")), val)
        } else if let Some(prefix) = verb_ref.strip_suffix("%[1]s") {
            format!("{}+{}", go_quote(&prefix.replace("%%", "%")), val)
        } else if let Some(suffix) = verb_ref.strip_prefix("%s") {
            format!("{}+{}", val, go_quote(&suffix.replace("%%", "%")))
        } else if let Some(suffix) = verb_ref.strip_prefix("%[1]s") {
            format!("{}+{}", val, go_quote(&suffix.replace("%%", "%")))
        } else {
            return;
        };
        report(
            "string-format",
            format!("{fn_name} can be replaced with string concatenation"),
            vec![replace_whole_call(call, "Use string concatenation", fix)],
        );
    }
}

/// One string-concatenation assign inside a loop (`s += x` or `s = s + x`).
struct ConcatAssign {
    /// AssignStmt.tok_pos — unique id for nested-loop dedup.
    tok_pos: u32,
    stmt_pos: u32,
    stmt_end: u32,
    added_pos: u32,
    added_end: u32,
}

/// If `st` is `idname = idname + Y`, return `Y`.
fn is_string_add<'a>(st: &'a AssignStmt, idname: &str) -> Option<&'a Expr> {
    if st.rhs.len() != 1 {
        return None;
    }
    let Expr::BinaryExpr(add) = &st.rhs[0] else {
        return None;
    };
    if add.op != Token::ADD {
        return None;
    }
    let Expr::Ident(x) = &*add.x else {
        return None;
    };
    if x.name != idname {
        return None;
    }
    Some(&add.y)
}

fn assign_stmt_pos(st: &AssignStmt) -> u32 {
    st.lhs
        .first()
        .map(|e| e.pos().0 as u32)
        .unwrap_or(st.tok_pos.0 as u32)
}

fn assign_stmt_end(st: &AssignStmt) -> u32 {
    st.rhs
        .last()
        .map(|e| e.end().0 as u32)
        .unwrap_or(st.tok_pos.0 as u32)
}

/// Collect loop-external string concatenations (BFS into nested blocks).
fn process_loop(
    pass: &Pass<'_>,
    body: &[Stmt],
    already: &HashSet<u32>,
) -> BTreeMap<String, Vec<ConcatAssign>> {
    let mut decl_in_loop: HashSet<String> = HashSet::new();
    let mut adds: BTreeMap<String, Vec<ConcatAssign>> = BTreeMap::new();
    let mut queue: Vec<&Stmt> = body.iter().collect();
    let mut i = 0;
    while i < queue.len() {
        let st = queue[i];
        i += 1;
        match st {
            Stmt::RangeStmt(r) => queue.extend(r.body.list.iter()),
            Stmt::ForStmt(f) => queue.extend(f.body.list.iter()),
            Stmt::SwitchStmt(sw) => queue.extend(sw.body.list.iter()),
            Stmt::CaseClause(c) => queue.extend(c.body.iter()),
            Stmt::IfStmt(ifs) => {
                queue.extend(ifs.body.list.iter());
                if let Some(Stmt::BlockStmt(el)) = ifs.else_.as_deref() {
                    queue.extend(el.list.iter());
                }
            }
            Stmt::DeclStmt(ds) => {
                let Decl::GenDecl(de) = &ds.decl else {
                    continue;
                };
                if de.specs.len() != 1 {
                    continue;
                }
                let Spec::ValueSpec(vs) = &de.specs[0] else {
                    continue;
                };
                for n in &vs.names {
                    decl_in_loop.insert(n.name.clone());
                }
            }
            Stmt::AssignStmt(asgn) => {
                for (idx, lhs) in asgn.lhs.iter().enumerate() {
                    let Expr::Ident(id) = lhs else {
                        break;
                    };
                    match asgn.tok {
                        Some(Token::DEFINE) => {
                            decl_in_loop.insert(id.name.clone());
                        }
                        Some(Token::ASSIGN) | Some(Token::AddAssign) => {
                            if idx > 0 {
                                break;
                            }
                            if decl_in_loop.contains(&id.name) {
                                break;
                            }
                            let Some(typ) = type_of(pass, lhs) else {
                                break;
                            };
                            if !is_basic(pass, typ, &[BasicKind::String]) {
                                break;
                            }
                            let added = match asgn.tok {
                                Some(Token::ASSIGN) => {
                                    let Some(y) = is_string_add(asgn, &id.name) else {
                                        break;
                                    };
                                    y
                                }
                                Some(Token::AddAssign) => {
                                    if asgn.rhs.len() != 1 {
                                        break;
                                    }
                                    &asgn.rhs[0]
                                }
                                _ => break,
                            };
                            let tok_pos = asgn.tok_pos.0 as u32;
                            if already.contains(&tok_pos) {
                                break;
                            }
                            adds.entry(id.name.clone()).or_default().push(ConcatAssign {
                                tok_pos,
                                stmt_pos: assign_stmt_pos(asgn),
                                stmt_end: assign_stmt_end(asgn),
                                added_pos: added.pos().0 as u32,
                                added_end: added.end().0 as u32,
                            });
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    adds
}

/// Detect other uses of concatenated identifiers in the loop (upstream `addTODO`).
fn other_ops_ident(node: NodeRef<'_>, adds: &BTreeMap<String, Vec<ConcatAssign>>) -> Option<String> {
    let mut found: Option<String> = None;
    walk::inspect(node, |n| {
        if found.is_some() {
            return false;
        }
        let Some(n) = n else {
            return true;
        };
        match n {
            NodeRef::AssignStmt(x)
                if matches!(x.tok, Some(Token::ASSIGN) | Some(Token::AddAssign))
                    && x.lhs.len() == 1 =>
            {
                if let Expr::Ident(id) = &x.lhs[0] {
                    if adds.contains_key(&id.name) {
                        if x.tok == Some(Token::ASSIGN) && is_string_add(x, &id.name).is_none() {
                            found = Some(id.name.clone());
                        }
                        return false;
                    }
                }
            }
            NodeRef::Ident(id) if adds.contains_key(&id.name) => {
                found = Some(id.name.clone());
                return false;
            }
            _ => {}
        }
        true
    });
    found
}

fn report_concat_loop(
    pass: &Pass<'_>,
    loop_pos: u32,
    loop_end: u32,
    loop_node: NodeRef<'_>,
    adds: &BTreeMap<String, Vec<ConcatAssign>>,
    options: &PerfsprintOptions,
    already: &mut HashSet<u32>,
    pending: &mut Vec<Pending>,
) {
    let add_todo = other_ops_ident(loop_node, adds);
    if add_todo.is_some() && !options.loop_other_ops {
        return;
    }

    let loop_start_line = pass.fset().position(guff::position::Pos(loop_pos as i64)).line;

    let mut prefix = String::new();
    if let Some(name) = &add_todo {
        prefix = format!(
            "// FIXME check usages of string identifier {name} (and mayber others) in loop\n"
        );
    }
    let mut suffix = String::new();
    for k in adds.keys() {
        for st in &adds[k] {
            already.insert(st.tok_pos);
        }
        prefix.push_str(&format!("var {k}Sb{loop_start_line} strings.Builder\n"));
        suffix.push_str(&format!("\n{k} += {k}Sb{loop_start_line}.String()"));
    }

    let mut te = vec![TextEdit {
        pos: loop_pos,
        end: loop_pos,
        new_text: prefix,
    }];
    for (k, stmts) in adds {
        for st in stmts {
            te.push(TextEdit {
                pos: st.stmt_pos,
                end: st.added_pos,
                new_text: format!("{k}Sb{loop_start_line}.WriteString("),
            });
            te.push(TextEdit {
                pos: st.added_end,
                end: st.added_end,
                new_text: ")".into(),
            });
        }
    }
    te.push(TextEdit {
        pos: loop_end,
        end: loop_end,
        new_text: suffix,
    });

    let first = adds.values().next().and_then(|v| v.first()).unwrap();
    pending.push(Pending {
        pos: first.stmt_pos,
        end: first.stmt_end,
        message: "concat-loop: string concatenation in a loop".into(),
        fixes: vec![SuggestedFix {
            message: "Use a strings.Builder".into(),
            text_edits: te,
        }],
    });
}

fn check_concat_loops(pass: &Pass<'_>, options: &PerfsprintOptions, pending: &mut Vec<Pending>) {
    if !options.concat_loop {
        return;
    }
    let mut already: HashSet<u32> = HashSet::new();
    for file in pass.files() {
        // Collect (pos, end, body slice pointers via indices) in Preorder so outer
        // loops are handled before nested ones (matches upstream inspect.Preorder).
        let mut loops: Vec<(u32, u32)> = Vec::new();
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::RangeStmt(r) => {
                    loops.push((r.for_.0 as u32, r.body.end().0 as u32));
                }
                NodeRef::ForStmt(f) => {
                    loops.push((f.for_.0 as u32, f.body.end().0 as u32));
                }
                _ => {}
            }
            true
        });

        for &(loop_pos, loop_end) in &loops {
            // Re-resolve the loop node and body for this position.
            let mut handled = false;
            walk::inspect(NodeRef::File(file), |n| {
                if handled {
                    return false;
                }
                let Some(n) = n else {
                    return true;
                };
                let (body, node) = match n {
                    NodeRef::RangeStmt(r) if r.for_.0 as u32 == loop_pos => {
                        (&r.body.list[..], n)
                    }
                    NodeRef::ForStmt(f) if f.for_.0 as u32 == loop_pos => {
                        (&f.body.list[..], n)
                    }
                    _ => return true,
                };
                let adds = process_loop(pass, body, &already);
                if !adds.is_empty() {
                    report_concat_loop(
                        pass,
                        loop_pos,
                        loop_end,
                        node,
                        &adds,
                        options,
                        &mut already,
                        pending,
                    );
                }
                handled = true;
                false
            });
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "perfsprint requires inspect analyzer".to_string())?;

    let options = effective_options(
        pass
            .settings::<PerfsprintOptions>("perfsprint")
            .copied()
            .unwrap_or_default(),
    );

    let mut pending = Vec::new();
    check_concat_loops(pass, &options, &mut pending);

    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::CallExpr(call) = n {
                check_call(pass, call, &options, &mut pending);
            }
            true
        });
    }

    for p in pending {
        pass.report(Diagnostic {
            pos: p.pos,
            end: p.end,
            message: p.message,
            suggested_fixes: p.fixes,
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "perfsprint",
        doc: "Checks that fmt.Sprintf can be replaced with a faster alternative.",
        url: "https://github.com/catenacyber/perfsprint",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
