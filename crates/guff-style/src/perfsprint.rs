//! Port of [`github.com/catenacyber/perfsprint`](https://github.com/catenacyber/perfsprint).
//!
//! Implements the default-on fmt.Sprint / fmt.Sprintf / fmt.Errorf rewrites
//! (string / bool / integer / hex / errors.New).
//!
//! DEFERRED: concat-loop, fiximports, `err-error` / remaining settings flags.

use std::sync::OnceLock;

use guff::ast::{BasicLit, CallExpr, Expr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::alias::unalias_readonly;
use guff_types::basic::BasicKind;
use guff_types::arena::TypeData;
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

fn expr_string(e: &Expr) -> String {
    match e {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", expr_string(&sel.x), sel.sel.name),
        Expr::CallExpr(c) => {
            let args: Vec<String> = c.args.iter().map(expr_string).collect();
            format!("{}({})", expr_string(&c.fun), args.join(", "))
        }
        Expr::ParenExpr(p) => format!("({})", expr_string(&p.x)),
        Expr::StarExpr(s) => format!("*{}", expr_string(&s.x)),
        Expr::UnaryExpr(u) => format!("{}{}", u.op, expr_string(&u.x)),
        Expr::BasicLit(l) => l.value.clone(),
        Expr::IndexExpr(i) => format!("{}[{}]", expr_string(&i.x), expr_string(&i.index)),
        _ => "<expr>".into(),
    }
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
            "error-format" => options.error_format && options.errorf,
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
        pending.push(Pending {
            pos: call.pos().0 as u32,
            end: call.end().0 as u32,
            message: format!("{checker}: {msg}"),
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
            let text = expr_string(value);
            report(
                "string-format",
                format!("{fn_name} can be replaced with just using the string"),
                vec![replace_whole_call(call, "Just use string value", text)],
            );
        }
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

    if is_basic(
        pass,
        value_type,
        &[BasicKind::Int8, BasicKind::Int16, BasicKind::Int32],
    ) && one_of(verb_ref, &["%v", "%d"])
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

    if is_basic(
        pass,
        value_type,
        &[
            BasicKind::Uint8,
            BasicKind::Uint16,
            BasicKind::Uint32,
            BasicKind::Uint,
        ],
    ) && one_of(verb_ref, &["%v", "%d", "%x"])
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
        let val = expr_string(value);
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

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "perfsprint requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<PerfsprintOptions>("perfsprint")
        .copied()
        .unwrap_or_default();

    let mut pending = Vec::new();
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
