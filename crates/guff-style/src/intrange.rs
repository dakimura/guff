//! Port of [`github.com/ckaznocha/intrange`](https://github.com/ckaznocha/intrange)
//! (golangci-lint wrapper in `pkg/golinters/intrange`).
//!
//! Finds classic `for i := 0; i < n; i++` loops that can use Go 1.22+ integer
//! range (`for i := range n`), and `for i := range len(s)` that can become
//! `for i := range s` for slices/arrays.
//!
//! No `linters.settings.intrange` keys (upstream has none). Go < 1.22 is
//! skipped (golangci disables the linter similarly).

use std::sync::OnceLock;

use guff::ast::{
    BasicLit, BinaryExpr, CallExpr, Expr, ForStmt, Ident, IncDecStmt, RangeStmt, Stmt,
};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code::{self, object_of};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::{BasicKind, TypeData, TypeId};

const MSG: &str = "for loop can be changed to use an integer range (Go 1.22+)";

struct Pending {
    diag: Diagnostic,
}

fn go_at_least_122(pass: &Pass<'_>, pos: u32) -> bool {
    code::version_compare(&code::stdlib_version(pass, pos), "go1.22") >= 0
}

fn ident_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn compare_number_lit(pass: &Pass<'_>, expr: &Expr, val: i64) -> bool {
    if code::is_integer_literal(pass, expr, val) {
        return true;
    }
    // Fallback for fixtures where typed constants are missing.
    match expr {
        Expr::BasicLit(BasicLit {
            kind: Some(Token::INT),
            value,
            ..
        }) => parse_int_lit(value) == Some(val),
        Expr::CallExpr(call) => {
            let Expr::Ident(fun) = call.fun.as_ref() else {
                return false;
            };
            if !is_int_cast_name(&fun.name) || call.args.len() != 1 {
                return false;
            }
            compare_number_lit(pass, &call.args[0], val)
        }
        _ => false,
    }
}

fn parse_int_lit(s: &str) -> Option<i64> {
    if let Ok(n) = s.parse::<i64>() {
        return Some(n);
    }
    i64::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
}

fn is_number_lit(pass: &Pass<'_>, expr: &Expr) -> bool {
    if code::expr_to_int(pass, expr).is_some() {
        return true;
    }
    match expr {
        Expr::BasicLit(BasicLit {
            kind: Some(Token::INT),
            ..
        }) => true,
        Expr::CallExpr(call) => {
            let Expr::Ident(fun) = call.fun.as_ref() else {
                return false;
            };
            is_int_cast_name(&fun.name)
                && call.args.len() == 1
                && is_number_lit(pass, &call.args[0])
        }
        _ => false,
    }
}

fn is_int_cast_name(name: &str) -> bool {
    matches!(
        name,
        "int" | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
    )
}

fn is_len(call: &CallExpr) -> bool {
    matches!(call.fun.as_ref(), Expr::Ident(id) if id.name == "len") && call.args.len() == 1
}

fn is_function_or_method_call(expr: &Expr) -> bool {
    let Expr::CallExpr(call) = expr else {
        return false;
    };
    match call.fun.as_ref() {
        Expr::Ident(fun) => {
            if is_len(call) || is_int_cast_name(&fun.name) {
                return false;
            }
            true
        }
        _ => true,
    }
}

fn find_n_expr(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::CallExpr(call) if is_len(call) => find_n_expr(&call.args[0]),
        Expr::BasicLit(_) => None,
        Expr::Ident(_) | Expr::SelectorExpr(_) | Expr::IndexExpr(_) => Some(expr),
        _ => None,
    }
}

fn recursive_operand_to_string(expr: &Expr, increment_int: bool) -> String {
    match expr {
        Expr::CallExpr(call) => {
            let args: Vec<String> = call
                .args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    recursive_operand_to_string(a, increment_int && call.args.len() == 1 && i == 0)
                })
                .collect();
            format!(
                "{}({})",
                recursive_operand_to_string(&call.fun, false),
                args.join(", ")
            )
        }
        Expr::BasicLit(lit) => {
            if increment_int && lit.kind == Some(Token::INT) {
                if let Some(v) = parse_int_lit(&lit.value) {
                    return (v + 1).to_string();
                }
            }
            lit.value.clone()
        }
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => {
            format!(
                "{}.{}",
                recursive_operand_to_string(&sel.x, false),
                sel.sel.name
            )
        }
        Expr::IndexExpr(ix) => {
            format!(
                "{}[{}]",
                recursive_operand_to_string(&ix.x, false),
                recursive_operand_to_string(&ix.index, false)
            )
        }
        Expr::BinaryExpr(bin) => {
            format!(
                "{} {} {}",
                recursive_operand_to_string(&bin.x, false),
                bin.op.as_str(),
                recursive_operand_to_string(&bin.y, false)
            )
        }
        Expr::StarExpr(star) => format!("*{}", recursive_operand_to_string(&star.x, false)),
        _ => String::new(),
    }
}

fn type_of_expr(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    info.types.get(&expr.id()).map(|tv| tv.typ)
}

fn is_basic_int(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    matches!(
        artifacts.types.get(typ),
        TypeData::Basic(b) if b.kind() == BasicKind::Int
    )
}

fn type_string(pass: &Pass<'_>, typ: TypeId) -> String {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return String::new();
    };
    guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    )
}

fn operand_to_string(
    pass: &Pass<'_>,
    init_ident: &Ident,
    operand: &Expr,
    increment: bool,
) -> String {
    let mut s = recursive_operand_to_string(operand, increment);
    let Some(t) = type_of_expr(pass, &Expr::Ident(init_ident.clone()))
        .or_else(|| {
            // Defs may hold the loop var type when Uses does not.
            let info = pass.types_info()?;
            let obj = info.defs.get(&init_ident.id).and_then(|o| *o)?;
            let artifacts = pass.pkg().type_artifacts.as_ref()?;
            obj.typ(&artifacts.objects)
        })
    else {
        return s;
    };

    if is_basic_int(pass, t) {
        if s.len() > 5 && s.starts_with("int(") && s.ends_with(')') {
            s = s[4..s.len() - 1].to_string();
        }
        return s;
    }

    if s.len() > 2 && s.ends_with(')') {
        return s;
    }

    if let Expr::Ident(op_id) = operand {
        if let (Some(op_t), Some(init_t)) = (
            type_of_expr(pass, &Expr::Ident(op_id.clone())),
            Some(t),
        ) {
            if op_t == init_t {
                return s;
            }
        }
    }

    format!("{}({})", type_string(pass, t), s)
}

fn ident_equal(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(x), Expr::Ident(y)) => x.name == y.name,
        (Expr::SelectorExpr(x), Expr::SelectorExpr(y)) => {
            x.sel.name == y.sel.name && ident_equal(&x.x, &y.x)
        }
        (Expr::IndexExpr(x), Expr::IndexExpr(y)) => {
            ident_equal(&x.x, &y.x) && ident_equal(&x.index, &y.index)
        }
        (Expr::IndexExpr(x), other) => ident_equal(&x.x, other),
        (Expr::BasicLit(x), Expr::BasicLit(y)) => x.value == y.value,
        _ => false,
    }
}

fn ident_equal_ident(a: &Expr, name: &str) -> bool {
    matches!(a, Expr::Ident(id) if id.name == name)
}

struct BodyChecker<'a> {
    init_name: &'a str,
    n_expr: Option<&'a Expr>,
    modified: bool,
    accessed: bool,
}

impl BodyChecker<'_> {
    fn check_node(&mut self, n: NodeRef<'_>) {
        match n {
            NodeRef::AssignStmt(stmt) => {
                for lhs in &stmt.lhs {
                    if ident_equal_ident(lhs, self.init_name)
                        || self
                            .n_expr
                            .is_some_and(|n| ident_equal(lhs, n))
                    {
                        self.modified = true;
                    }
                }
            }
            NodeRef::IncDecStmt(stmt) => {
                if ident_equal_ident(&stmt.x, self.init_name)
                    || self
                        .n_expr
                        .is_some_and(|n| ident_equal(&stmt.x, n))
                {
                    self.modified = true;
                }
            }
            NodeRef::Ident(id) => {
                if id.name == self.init_name {
                    self.accessed = true;
                }
            }
            _ => {}
        }
    }
}

fn check_post(pass: &Pass<'_>, post: &Stmt, init_name: &str) -> bool {
    match post {
        Stmt::IncDecStmt(IncDecStmt { x, tok, .. }) => {
            *tok == Token::INC && ident_name(x) == Some(init_name)
        }
        Stmt::AssignStmt(assign) => match assign.tok {
            Some(Token::AddAssign) => {
                assign.lhs.len() == 1
                    && assign.rhs.len() == 1
                    && ident_name(&assign.lhs[0]) == Some(init_name)
                    && compare_number_lit(pass, &assign.rhs[0], 1)
            }
            Some(Token::ASSIGN) => {
                if assign.lhs.len() != 1
                    || assign.rhs.len() != 1
                    || ident_name(&assign.lhs[0]) != Some(init_name)
                {
                    return false;
                }
                let Expr::BinaryExpr(BinaryExpr { x, op, y, .. }) = &assign.rhs[0] else {
                    return false;
                };
                if *op != Token::ADD {
                    return false;
                }
                match x.as_ref() {
                    Expr::Ident(id) if id.name == init_name => compare_number_lit(pass, y, 1),
                    Expr::BasicLit(_) | Expr::CallExpr(_) => {
                        compare_number_lit(pass, x, 1) && ident_name(y) == Some(init_name)
                    }
                    _ => false,
                }
            }
            _ => false,
        },
        _ => false,
    }
}

fn check_for_stmt(pass: &Pass<'_>, for_stmt: &ForStmt, pending: &mut Vec<Pending>) {
    let pos = for_stmt.for_.0 as u32;
    if !go_at_least_122(pass, pos) {
        return;
    }
    let Some(Stmt::AssignStmt(init)) = for_stmt.init.as_deref() else {
        return;
    };
    let Some(cond) = for_stmt.cond.as_ref() else {
        return;
    };
    let Some(post) = for_stmt.post.as_deref() else {
        return;
    };

    let init_assign = init.tok == Some(Token::ASSIGN);
    if init.tok != Some(Token::DEFINE) && !init_assign {
        return;
    }
    if init.lhs.len() != 1 || init.rhs.len() != 1 {
        return;
    }
    let Expr::Ident(init_ident) = &init.lhs[0] else {
        return;
    };
    if !compare_number_lit(pass, &init.rhs[0], 0) {
        return;
    }

    let Expr::BinaryExpr(BinaryExpr { x, op, y, .. }) = cond else {
        return;
    };

    let (has_equivalent_operator, operand) = match *op {
        Token::LSS | Token::LEQ => {
            if ident_name(x) != Some(init_ident.name.as_str()) {
                return;
            }
            (*op == Token::LEQ, y.as_ref())
        }
        Token::GTR | Token::GEQ => {
            if ident_name(y) != Some(init_ident.name.as_str()) {
                return;
            }
            (*op == Token::GEQ, x.as_ref())
        }
        _ => return,
    };

    if !check_post(pass, post, &init_ident.name) {
        return;
    }

    let mut bc = BodyChecker {
        init_name: &init_ident.name,
        n_expr: find_n_expr(operand),
        modified: false,
        accessed: false,
    };
    walk::preorder(NodeRef::BlockStmt(&for_stmt.body), |n| {
        bc.check_node(n);
        true
    });
    if bc.modified {
        return;
    }

    if init_assign {
        pending.push(Pending {
            diag: Diagnostic {
                pos,
                message: format!(
                    "{MSG}\nBecause the key is not part of the loop's scope, take care to consider side effects."
                ),
                ..Diagnostic::default()
            },
        });
        return;
    }

    let operand_is_number_lit = is_number_lit(pass, operand);
    if has_equivalent_operator && !operand_is_number_lit {
        return;
    }

    let range_x = operand_to_string(
        pass,
        init_ident,
        operand,
        has_equivalent_operator && operand_is_number_lit,
    );

    let replacement = if bc.accessed {
        format!("{} := range {range_x}", init_ident.name)
    } else {
        format!("range {range_x}")
    };

    if is_function_or_method_call(operand) {
        pending.push(Pending {
            diag: Diagnostic {
                pos,
                message: format!(
                    "{MSG}\nBecause the key is returned by a function or method, take care to consider side effects."
                ),
                ..Diagnostic::default()
            },
        });
        return;
    }

    let end = for_stmt
        .post
        .as_ref()
        .map(|p| p.end().0 as u32)
        .unwrap_or(pos);

    pending.push(Pending {
        diag: Diagnostic {
            pos,
            end,
            message: MSG.into(),
            suggested_fixes: vec![SuggestedFix {
                message: format!("Replace loop with `{replacement}`"),
                text_edits: vec![TextEdit {
                    pos: init.tok_pos.0 as u32,
                    end,
                    new_text: replacement,
                }],
            }],
            ..Diagnostic::default()
        },
    });
}

fn is_slice_or_array(pass: &Pass<'_>, ident: &Ident) -> bool {
    let Some(obj) = object_of(pass, ident) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(typ) = obj.typ(&artifacts.objects) else {
        return false;
    };
    let under = typ.underlying(&artifacts.types);
    matches!(
        artifacts.types.get(under),
        TypeData::Slice(_) | TypeData::Array(_)
    )
}

fn check_range_stmt(pass: &Pass<'_>, range_stmt: &RangeStmt, pending: &mut Vec<Pending>) {
    let pos = range_stmt.for_.0 as u32;
    if !go_at_least_122(pass, pos) {
        return;
    }
    if range_stmt.value.is_some() {
        return;
    }

    let mut start_pos = range_stmt.range_.0 as u32;
    let mut uses_key = range_stmt.key.is_some();
    let mut ident_name = String::new();

    if let Some(key) = range_stmt.key.as_ref() {
        let Expr::Ident(ident) = key else {
            return;
        };
        if ident.name == "_" {
            uses_key = false;
        }
        ident_name = ident.name.clone();
        start_pos = ident.pos().0 as u32;
    }

    let Expr::CallExpr(x) = &range_stmt.x else {
        return;
    };
    if !matches!(x.fun.as_ref(), Expr::Ident(_)) {
        return;
    }
    if !is_len(x) {
        return;
    }
    let Expr::Ident(arg) = &x.args[0] else {
        return;
    };
    if !is_slice_or_array(pass, arg) {
        return;
    }

    let x_end = range_stmt.x.end().0 as u32;

    if uses_key {
        pending.push(Pending {
            diag: Diagnostic {
                pos: start_pos,
                end: x_end,
                message: format!(
                    "for loop can be changed to `{ident_name} := range {}`",
                    arg.name
                ),
                suggested_fixes: vec![SuggestedFix {
                    message: format!("Replace `len({})` with `{}`", arg.name, arg.name),
                    text_edits: vec![TextEdit {
                        pos: x.pos().0 as u32,
                        end: x_end,
                        new_text: arg.name.clone(),
                    }],
                }],
                ..Diagnostic::default()
            },
        });
        return;
    }

    pending.push(Pending {
        diag: Diagnostic {
            pos: start_pos,
            end: x_end,
            message: format!("for loop can be changed to `range {}`", arg.name),
            suggested_fixes: vec![SuggestedFix {
                message: format!("Replace `len({})` with `{}`", arg.name, arg.name),
                text_edits: vec![TextEdit {
                    pos: start_pos,
                    end: x_end,
                    new_text: format!("range {}", arg.name),
                }],
            }],
            ..Diagnostic::default()
        },
    });
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "intrange requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::ForStmt(for_stmt) => check_for_stmt(pass, for_stmt, &mut pending),
                NodeRef::RangeStmt(range_stmt) => check_range_stmt(pass, range_stmt, &mut pending),
                _ => {}
            }
            true
        });
    }

    for p in pending {
        pass.report(p.diag);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "intrange",
        doc: "intrange is a linter to find places where for loops could make use of an integer range.",
        url: "https://github.com/ckaznocha/intrange",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn analyzer_graph_ok() {
        validate(&[analyzer()]).expect("intrange analyzer graph");
    }
}
