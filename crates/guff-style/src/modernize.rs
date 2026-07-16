//! Port of [`golang.org/x/tools/go/analysis/passes/modernize`](https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/modernize)
//! (golangci-lint wrapper: `linters.settings.modernize.disable`).
//!
//! Implemented checkers (default on):
//! - `any` — `interface{}` → `any` (Go 1.18+)
//! - `plusbuild` — obsolete `// +build` beside `//go:build` (Go 1.18+)
//! - `forvar` — redundant `x := x` in range loops (Go 1.22+)
//! - `rangeint` — `for i := 0; i < n; i++` → `for i := range n` (Go 1.22+; simplified)
//! - `minmax` — if/else → `min`/`max` pattern 1 (Go 1.21+)
//! - `fmtappendf` — `[]byte(fmt.Sprint*)` → `fmt.Append*` (Go 1.19+)
//! - `omitzero` — `json:",omitempty"` on struct fields (Go 1.24+)
//! - `slicessort` — `sort.Slice` with natural order → `slices.Sort` (Go 1.21+)
//! - `stringscutprefix` — `HasPrefix`+`TrimPrefix` → `CutPrefix` (Go 1.20+; pattern 1)
//! - `slicescontains` — search loop → `slices.Contains` (Go 1.21+; return true/false)
//! - `stringsseq` — `range strings.Split/Fields` → `SplitSeq`/`FieldsSeq` (Go 1.24+)
//! - `waitgroupgo` — `Add(1)`+`go`+`Done` → `WaitGroup.Go` (Go 1.25+)
//! - `mapsloop` — `for k, v := range x { m[k] = v }` → `maps.Copy` (Go 1.23+; map→map)
//!
//! DEFERRED (recognized in `disable` / documented): atomictypes, embedlit,
//! errorsastype, newexpr, reflecttypefor, slicesbackward, stditerators,
//! stringscut, stringsbuilder, testingcontext, unsafefuncs, mapsloop Insert/
//! Collect (iter.Seq2) / Clone (nil-preserving), HasPrefix/TrimPrefix pattern 2 /
//! bytes variants, slicescontains ContainsFunc / break variants, waitgroupgo
//! trailing-Done / SuggestedFix import edits, and full rangeint/minmax edge-case
//! parity with upstream.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, BinaryExpr, BlockStmt, CallExpr, Expr, Field, File, ForStmt, FuncLit, GoStmt,
    IfStmt, IncDecStmt, InterfaceType, RangeStmt, ReturnStmt, Stmt, StructType,
};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::alias::unalias_readonly;
use guff_types::api_predicates::api_identical;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::BasicKind;
use guff_types::map::{map_elem, map_key};
use guff_types::named::named_obj;
use guff_types::predicates::{is_float, is_string};
use guff_types::signature::signature_recv;
use guff_types::{ObjectId, OperandMode, TypeId};

use crate::options::ModernizeOptions;

fn enabled(opts: &ModernizeOptions, name: &str) -> bool {
    !opts.disable.iter().any(|d| d == name)
}

fn go_at_least(pass: &Pass<'_>, pos: u32, want: &str) -> bool {
    code::version_compare(&code::stdlib_version(pass, pos), want) >= 0
}

fn ident_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn expr_text(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::BasicLit(lit) => Some(lit.value.clone()),
        Expr::SelectorExpr(sel) => {
            let x = expr_text(&sel.x)?;
            Some(format!("{x}.{}", sel.sel.name))
        }
        Expr::CallExpr(call) if call.args.len() == 1 => {
            let fun = expr_text(&call.fun)?;
            let arg = expr_text(&call.args[0])?;
            Some(format!("{fun}({arg})"))
        }
        Expr::IndexExpr(ix) => {
            let x = expr_text(&ix.x)?;
            let index = expr_text(&ix.index)?;
            Some(format!("{x}[{index}]"))
        }
        Expr::ParenExpr(p) => expr_text(&p.x).map(|inner| format!("({inner})")),
        _ => None,
    }
}

fn is_empty_interface(iface: &InterfaceType) -> bool {
    iface.methods.list.is_empty()
}

fn check_any(pass: &Pass<'_>, iface: &InterfaceType, pending: &mut Vec<Diagnostic>) {
    if !is_empty_interface(iface) {
        return;
    }
    let pos = iface.interface_.0 as u32;
    if !go_at_least(pass, pos, "go1.18") {
        return;
    }
    let end = iface.methods.end().0 as u32;
    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: "interface{} can be replaced by any".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace interface{} by any".into(),
            text_edits: vec![TextEdit {
                pos,
                end,
                new_text: "any".into(),
            }],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
}

fn check_plusbuild(file: &File, pending: &mut Vec<Diagnostic>) {
    for group in &file.comments {
        let mut saw_go_build = false;
        for c in &group.list {
            let text = c.text.as_str();
            if saw_go_build && text.starts_with("// +build ") {
                let pos = c.slash.0 as u32;
                let end = c.end().0 as u32;
                pending.push(Diagnostic {
                    pos,
                    end,
                    category: String::new(),
                    message: "+build line is no longer needed".into(),
                    suggested_fixes: vec![SuggestedFix {
                        message: "Remove obsolete +build line".into(),
                        text_edits: vec![TextEdit {
                            pos,
                            end,
                            new_text: String::new(),
                        }],
                    }],
                    related: Vec::new(),
                    url: String::new(),
                    severity: String::new(),
                });
                break;
            }
            if text.starts_with("//go:build ") {
                saw_go_build = true;
            }
        }
    }
}

fn is_loop_var_redecl(assign: &AssignStmt, loop_vars: &HashSet<&str>) -> bool {
    if assign.tok != Some(Token::DEFINE) || assign.lhs.len() != assign.rhs.len() {
        return false;
    }
    for (lhs, rhs) in assign.lhs.iter().zip(assign.rhs.iter()) {
        let Some(l) = ident_name(lhs) else {
            return false;
        };
        let Some(r) = ident_name(rhs) else {
            return false;
        };
        if l != r || !loop_vars.contains(l) {
            return false;
        }
    }
    true
}

fn check_forvar(pass: &Pass<'_>, range_stmt: &RangeStmt, pending: &mut Vec<Diagnostic>) {
    if range_stmt.tok != Some(Token::DEFINE) {
        return;
    }
    let pos = range_stmt.for_.0 as u32;
    if !go_at_least(pass, pos, "go1.22") {
        return;
    }
    let mut loop_vars = HashSet::new();
    if let Some(name) = range_stmt.key.as_ref().and_then(ident_name) {
        if name != "_" {
            loop_vars.insert(name);
        }
    }
    if let Some(name) = range_stmt.value.as_ref().and_then(ident_name) {
        if name != "_" {
            loop_vars.insert(name);
        }
    }
    if loop_vars.is_empty() {
        return;
    }
    for stmt in &range_stmt.body.list {
        let Stmt::AssignStmt(assign) = stmt else {
            break;
        };
        if !is_loop_var_redecl(assign, &loop_vars) {
            break;
        }
        let pos = assign
            .lhs
            .first()
            .map(|e| e.pos().0 as u32)
            .unwrap_or(assign.tok_pos.0 as u32);
        let end = assign
            .rhs
            .last()
            .map(|e| e.end().0 as u32)
            .unwrap_or(assign.tok_pos.0 as u32);
        pending.push(Diagnostic {
            pos,
            end,
            category: String::new(),
            message: "copying variable is unneeded".into(),
            suggested_fixes: vec![SuggestedFix {
                message: "Remove redundant re-declaration".into(),
                text_edits: vec![TextEdit {
                    pos,
                    end,
                    new_text: String::new(),
                }],
            }],
            related: Vec::new(),
            url: String::new(),
            severity: String::new(),
        });
    }
}

fn is_simple_inc(post: &Stmt, index_name: &str) -> bool {
    match post {
        Stmt::IncDecStmt(IncDecStmt { x, tok, .. }) => {
            *tok == Token::INC && ident_name(x) == Some(index_name)
        }
        Stmt::AssignStmt(a)
            if a.tok == Some(Token::AddAssign)
                && a.lhs.len() == 1
                && a.rhs.len() == 1
                && ident_name(&a.lhs[0]) == Some(index_name) =>
        {
            matches!(&a.rhs[0], Expr::BasicLit(lit) if lit.value == "1")
        }
        _ => false,
    }
}

fn limit_is_safe(pass: &Pass<'_>, limit: &Expr) -> bool {
    match limit {
        Expr::Ident(_) | Expr::BasicLit(_) | Expr::SelectorExpr(_) => true,
        Expr::CallExpr(call) => {
            // Allow len(slice) only.
            code::is_call_to(pass, call, "len")
                && call.args.len() == 1
                && matches!(
                    type_kind(pass, &call.args[0]),
                    Some(TypeKind::Slice | TypeKind::Array | TypeKind::String)
                )
        }
        Expr::ParenExpr(p) => limit_is_safe(pass, &p.x),
        _ => false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TypeKind {
    Slice,
    Array,
    String,
    Struct,
    Map,
    Other,
}

fn type_kind(pass: &Pass<'_>, expr: &Expr) -> Option<TypeKind> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = info.types.get(&expr.id())?.typ;
    let typ = unalias_readonly(&artifacts.types, typ);
    let under = typ.underlying(&artifacts.types);
    Some(match artifacts.types.get(under) {
        TypeData::Slice(_) => TypeKind::Slice,
        TypeData::Array(_) => TypeKind::Array,
        TypeData::Struct(_) => TypeKind::Struct,
        TypeData::Map(_) => TypeKind::Map,
        _ if is_string(&artifacts.types, under) => TypeKind::String,
        _ => TypeKind::Other,
    })
}

fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    if let Some(tav) = info.types.get(&expr.id()) {
        return Some(tav.typ);
    }
    // Range variables (and other defs) may only appear in defs/uses.
    let Expr::Ident(id) = expr else {
        return None;
    };
    let obj = ident_obj(pass, id)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    obj.typ(&artifacts.objects)
}

fn types_identical(pass: &Pass<'_>, a: TypeId, b: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    api_identical(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        a,
        b,
    )
}

fn underlying_map(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = type_of(pass, expr)?;
    let typ = unalias_readonly(&artifacts.types, typ);
    let under = typ.underlying(&artifacts.types);
    match artifacts.types.get(under) {
        TypeData::Map(_) => Some(under),
        _ => None,
    }
}

fn ident_obj(pass: &Pass<'_>, id: &guff::ast::Ident) -> Option<ObjectId> {
    let info = pass.types_info()?;
    info.defs
        .get(&id.id)
        .copied()
        .flatten()
        .or_else(|| info.uses.get(&id.id).copied())
}

fn expr_uses_loop_vars(pass: &Pass<'_>, expr: &Expr, key: &Expr, value: &Expr) -> bool {
    let Some(k_obj) = (match key {
        Expr::Ident(id) => ident_obj(pass, id),
        _ => None,
    }) else {
        return false;
    };
    let Some(v_obj) = (match value {
        Expr::Ident(id) => ident_obj(pass, id),
        _ => None,
    }) else {
        return false;
    };
    let mut used = false;
    walk::inspect(walk::expr_ref(expr), |n| {
        let Some(n) = n else {
            return true;
        };
        if let NodeRef::Ident(id) = n {
            if let Some(obj) = ident_obj(pass, id) {
                if obj == k_obj || obj == v_obj {
                    used = true;
                    return false;
                }
            }
        }
        true
    });
    used
}

fn is_float_expr(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(tav) = info.types.get(&expr.id()) else {
        return false;
    };
    is_float(&artifacts.types, tav.typ.underlying(&artifacts.types))
}

fn field_type_is_struct(pass: &Pass<'_>, field: &Field) -> bool {
    let Some(ty) = field.ty.as_ref() else {
        return false;
    };
    type_kind(pass, ty) == Some(TypeKind::Struct)
}

fn check_rangeint(pass: &Pass<'_>, for_stmt: &ForStmt, pending: &mut Vec<Diagnostic>) {
    let pos = for_stmt.for_.0 as u32;
    if !go_at_least(pass, pos, "go1.22") {
        return;
    }
    let Some(Stmt::AssignStmt(init)) = for_stmt.init.as_deref() else {
        return;
    };
    if init.lhs.len() != 1 || init.rhs.len() != 1 {
        return;
    }
    if init.tok != Some(Token::DEFINE) && init.tok != Some(Token::ASSIGN) {
        return;
    }
    let Some(index_name) = ident_name(&init.lhs[0]) else {
        return;
    };
    if !code::is_integer_literal(pass, &init.rhs[0], 0) {
        return;
    }
    let Some(Expr::BinaryExpr(BinaryExpr { x, op, y, .. })) = for_stmt.cond.as_ref() else {
        return;
    };
    if *op != Token::LSS || ident_name(x) != Some(index_name) {
        return;
    }
    let Some(post) = for_stmt.post.as_deref() else {
        return;
    };
    if !is_simple_inc(post, index_name) {
        return;
    }
    if !limit_is_safe(pass, y) {
        return;
    }
    let Some(limit_text) = expr_text(y) else {
        return;
    };
    // Prefer `range slice` when limit is len(slice).
    let range_expr = if let Expr::CallExpr(call) = y.as_ref() {
        if code::is_call_to(pass, call, "len") && call.args.len() == 1 {
            expr_text(&call.args[0]).unwrap_or(limit_text.clone())
        } else {
            limit_text.clone()
        }
    } else {
        limit_text.clone()
    };

    let end = for_stmt
        .post
        .as_ref()
        .map(|p| p.end().0 as u32)
        .unwrap_or(for_stmt.for_.0 as u32);

    let new_text = if init.tok == Some(Token::DEFINE) {
        format!("for {index_name} := range {range_expr}")
    } else {
        format!("for {index_name} = range {range_expr}")
    };

    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: format!("for loop can be modernized using range over {range_expr}"),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace 3-clause for with range-over-int".into(),
            text_edits: vec![TextEdit {
                pos,
                end,
                new_text,
            }],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
}

fn is_assign_block(body: &guff::ast::BlockStmt) -> Option<&AssignStmt> {
    if body.list.len() != 1 {
        return None;
    }
    match &body.list[0] {
        Stmt::AssignStmt(a)
            if a.tok == Some(Token::ASSIGN) && a.lhs.len() == 1 && a.rhs.len() == 1 =>
        {
            Some(a)
        }
        _ => None,
    }
}

fn inequality_sign(op: Token) -> Option<i32> {
    match op {
        Token::LSS | Token::LEQ => Some(-1),
        Token::GTR | Token::GEQ => Some(1),
        _ => None,
    }
}

fn check_minmax(pass: &Pass<'_>, if_stmt: &IfStmt, pending: &mut Vec<Diagnostic>) {
    if if_stmt.init.is_some() {
        return;
    }
    let pos = if_stmt.if_.0 as u32;
    if !go_at_least(pass, pos, "go1.21") {
        return;
    }
    let Expr::BinaryExpr(compare) = &if_stmt.cond else {
        return;
    };
    let Some(mut sign) = inequality_sign(compare.op) else {
        return;
    };
    let Some(tassign) = is_assign_block(&if_stmt.body) else {
        return;
    };
    let Some(Stmt::BlockStmt(fblock)) = if_stmt.else_.as_deref() else {
        return;
    };
    let Some(fassign) = is_assign_block(fblock) else {
        return;
    };
    if !code::same_non_dynamic(pass, &tassign.lhs[0], &fassign.lhs[0]) {
        return;
    }
    let a = compare.x.as_ref();
    let b = compare.y.as_ref();
    let rhs = &tassign.rhs[0];
    let rhs2 = &fassign.rhs[0];
    if code::same_non_dynamic(pass, rhs, a) && code::same_non_dynamic(pass, rhs2, b) {
        // keep sign
    } else if code::same_non_dynamic(pass, rhs2, a) && code::same_non_dynamic(pass, rhs, b) {
        sign = -sign;
    } else {
        return;
    }
    // Skip floats (NaN concerns).
    if is_float_expr(pass, a) || is_float_expr(pass, b) {
        return;
    }
    let sym = if sign < 0 { "min" } else { "max" };
    let Some(lhs_text) = expr_text(&tassign.lhs[0]) else {
        return;
    };
    let Some(a_text) = expr_text(a) else {
        return;
    };
    let Some(b_text) = expr_text(b) else {
        return;
    };
    let end = if_stmt
        .else_
        .as_ref()
        .map(|e| e.end().0 as u32)
        .unwrap_or(if_stmt.body.rbrace.0 as u32);
    pending.push(Diagnostic {
        pos: compare.op_pos.0 as u32,
        end: compare.y.end().0 as u32,
        category: String::new(),
        message: format!("if/else statement can be modernized using {sym}"),
        suggested_fixes: vec![SuggestedFix {
            message: format!("Replace if statement with {sym}"),
            text_edits: vec![TextEdit {
                pos,
                end,
                new_text: format!("{lhs_text} = {sym}({a_text}, {b_text})"),
            }],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
}

fn is_byte_slice_type_expr(fun: &Expr) -> bool {
    // `[]byte` is an ArrayType with no length (Go AST convention for slices).
    let Expr::ArrayType(arr) = fun else {
        return false;
    };
    if arr.len.is_some() {
        return false;
    }
    matches!(arr.elt.as_ref(), Expr::Ident(id) if id.name == "byte" || id.name == "uint8")
}

fn is_byte_slice_conversion(pass: &Pass<'_>, call: &CallExpr) -> bool {
    if call.args.len() != 1 {
        return false;
    }
    if is_byte_slice_type_expr(&call.fun) {
        return true;
    }
    // Fallback: typed conversion via types info.
    let info = pass.types_info();
    let artifacts = pass.pkg().type_artifacts.as_ref();
    let (Some(info), Some(artifacts)) = (info, artifacts) else {
        return false;
    };
    let Some(tav) = info.types.get(&call.fun.id()) else {
        return false;
    };
    if tav.mode != OperandMode::TypeExpr {
        return false;
    }
    let typ = unalias_readonly(&artifacts.types, tav.typ);
    let under = typ.underlying(&artifacts.types);
    match artifacts.types.get(under) {
        TypeData::Slice(s) => {
            let elem = s.elem().underlying(&artifacts.types);
            matches!(
                artifacts.types.get(elem),
                TypeData::Basic(b) if b.kind() == BasicKind::Uint8
            )
        }
        _ => false,
    }
}

fn check_fmtappendf(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<Diagnostic>) {
    // Look for []byte(fmt.Sprintf/Sprint/Sprintln(...))
    if !is_byte_slice_conversion(pass, call) {
        return;
    }
    let Expr::CallExpr(inner) = &call.args[0] else {
        return;
    };
    let Some(name) = code::call_name(pass, &inner.fun) else {
        return;
    };
    let append_name = match name.as_str() {
        "fmt.Sprintf" => "Appendf",
        "fmt.Sprint" => "Append",
        "fmt.Sprintln" => "Appendln",
        _ => return,
    };
    let pos = call.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.19") {
        return;
    }
    if inner.args.is_empty() {
        return;
    }
    let args: Option<Vec<String>> = inner.args.iter().map(expr_text).collect();
    let Some(args) = args else {
        return;
    };
    let args_joined = args.join(", ");
    let end = call.end().0 as u32;
    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: format!("[]byte(fmt.{}) can be modernized using fmt.{append_name}", {
            name.strip_prefix("fmt.").unwrap_or(&name)
        }),
        suggested_fixes: vec![SuggestedFix {
            message: format!("Replace with fmt.{append_name}"),
            text_edits: vec![TextEdit {
                pos,
                end,
                new_text: format!("fmt.{append_name}(nil, {args_joined})"),
            }],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
}

fn json_omitempty_span(tag_value: &str) -> Option<(usize, usize)> {
    // tag_value includes surrounding quotes, e.g. `"json:\"foo,omitempty\""`
    let unquoted = if (tag_value.starts_with('"') && tag_value.ends_with('"'))
        || (tag_value.starts_with('`') && tag_value.ends_with('`'))
    {
        &tag_value[1..tag_value.len() - 1]
    } else {
        tag_value
    };
    // Decode simple Go string escapes for \" inside double-quoted tags.
    let decoded = if tag_value.starts_with('"') {
        unquoted.replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        unquoted.to_string()
    };
    for part in decoded.split_whitespace() {
        let Some(rest) = part.strip_prefix("json:") else {
            continue;
        };
        let val = rest.trim_matches('"');
        if let Some(idx) = val.find(",omitempty") {
            // Report on the whole tag literal; SuggestedFix replaces omitempty → omitzero in raw.
            let _ = idx;
            return Some((0, tag_value.len()));
        }
    }
    None
}

fn check_omitzero(pass: &Pass<'_>, field: &Field, pending: &mut Vec<Diagnostic>) {
    if !field_type_is_struct(pass, field) {
        return;
    }
    let Some(tag) = field.tag.as_ref() else {
        return;
    };
    let pos = tag.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.24") {
        return;
    }
    if json_omitempty_span(&tag.value).is_none() {
        return;
    }
    let end = tag.end().0 as u32;
    let new_tag = tag.value.replace(",omitempty", ",omitzero");
    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: "Omitempty has no effect on nested struct fields".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace omitempty with omitzero (behavior change)".into(),
            text_edits: vec![TextEdit {
                pos,
                end,
                new_text: new_tag,
            }],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
}

fn is_natural_less(pass: &Pass<'_>, lit: &FuncLit, slice: &Expr) -> bool {
    if lit.body.list.len() != 1 {
        return false;
    }
    let Stmt::ReturnStmt(ret) = &lit.body.list[0] else {
        return false;
    };
    if ret.results.len() != 1 {
        return false;
    }
    let Expr::BinaryExpr(BinaryExpr { x, op, y, .. }) = &ret.results[0] else {
        return false;
    };
    if *op != Token::LSS {
        return false;
    }
    let (Expr::IndexExpr(ix), Expr::IndexExpr(iy)) = (x.as_ref(), y.as_ref()) else {
        return false;
    };
    code::same_non_dynamic(pass, &ix.x, slice)
        && code::same_non_dynamic(pass, &iy.x, slice)
        && ident_name(&ix.index) == Some("i")
        && ident_name(&iy.index) == Some("j")
}

fn check_slicessort(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<Diagnostic>) {
    if !code::is_call_to(pass, call, "sort.Slice") || call.args.len() != 2 {
        return;
    }
    let pos = call.fun.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.21") {
        return;
    }
    let Expr::FuncLit(lit) = &call.args[1] else {
        return;
    };
    if !is_natural_less(pass, lit, &call.args[0]) {
        return;
    }
    let Some(slice_text) = expr_text(&call.args[0]) else {
        return;
    };
    let end = call.end().0 as u32;
    pending.push(Diagnostic {
        pos,
        end: call.fun.end().0 as u32,
        category: String::new(),
        message: "sort.Slice can be modernized using slices.Sort".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace sort.Slice call by slices.Sort".into(),
            text_edits: vec![TextEdit {
                pos,
                end,
                new_text: format!("slices.Sort({slice_text})"),
            }],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
}

fn find_first_call_named<'a>(
    pass: &Pass<'_>,
    stmt: &'a Stmt,
    name: &str,
) -> Option<&'a CallExpr> {
    let mut found = None;
    walk::inspect(walk::stmt_ref(stmt), |n| {
        let Some(n) = n else {
            return true;
        };
        if found.is_some() {
            return false;
        }
        if let NodeRef::CallExpr(c) = n {
            if code::is_call_to(pass, c, name) {
                found = Some(c);
                return false;
            }
        }
        true
    });
    found
}

fn check_stringscutprefix(pass: &Pass<'_>, if_stmt: &IfStmt, pending: &mut Vec<Diagnostic>) {
    if if_stmt.init.is_some() || if_stmt.body.list.is_empty() {
        return;
    }
    let Expr::CallExpr(has_call) = &if_stmt.cond else {
        return;
    };
    let pos = has_call.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.20") {
        return;
    }
    if has_call.args.len() != 2 {
        return;
    }
    let (trim_name, cut_name, var_name, message, fix_message) =
        if code::is_call_to(pass, has_call, "strings.HasPrefix") {
            (
                "strings.TrimPrefix",
                "CutPrefix",
                "after",
                "HasPrefix + TrimPrefix can be simplified to CutPrefix",
                "Replace HasPrefix/TrimPrefix with CutPrefix",
            )
        } else if code::is_call_to(pass, has_call, "strings.HasSuffix") {
            (
                "strings.TrimSuffix",
                "CutSuffix",
                "before",
                "HasSuffix + TrimSuffix can be simplified to CutSuffix",
                "Replace HasSuffix/TrimSuffix with CutSuffix",
            )
        } else {
            return;
        };
    let Some(trim_call) = find_first_call_named(pass, &if_stmt.body.list[0], trim_name) else {
        return;
    };
    if trim_call.args.len() != 2 {
        return;
    }
    if !code::same_non_dynamic(pass, &has_call.args[0], &trim_call.args[0])
        || !code::same_non_dynamic(pass, &has_call.args[1], &trim_call.args[1])
    {
        return;
    }
    let Some(s_text) = expr_text(&has_call.args[0]) else {
        return;
    };
    let Some(affix_text) = expr_text(&has_call.args[1]) else {
        return;
    };
    let end = has_call.end().0 as u32;
    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: message.into(),
        suggested_fixes: vec![SuggestedFix {
            message: fix_message.into(),
            text_edits: vec![
                TextEdit {
                    pos,
                    end,
                    new_text: format!(
                        "{var_name}, ok := strings.{cut_name}({s_text}, {affix_text}); ok"
                    ),
                },
                TextEdit {
                    pos: trim_call.pos().0 as u32,
                    end: trim_call.end().0 as u32,
                    new_text: var_name.into(),
                },
            ],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
}

fn is_return_bool(ret: &ReturnStmt, want: bool) -> bool {
    if ret.results.len() != 1 {
        return false;
    }
    matches!(
        &ret.results[0],
        Expr::Ident(id) if id.name == if want { "true" } else { "false" }
    )
}

fn check_slicescontains(
    pass: &Pass<'_>,
    block: &BlockStmt,
    pending: &mut Vec<Diagnostic>,
) {
    for i in 0..block.list.len().saturating_sub(1) {
        let Stmt::RangeStmt(rng) = &block.list[i] else {
            continue;
        };
        let Stmt::ReturnStmt(after) = &block.list[i + 1] else {
            continue;
        };
        if !is_return_bool(after, false) {
            continue;
        }
        let pos = rng.for_.0 as u32;
        if !go_at_least(pass, pos, "go1.21") {
            continue;
        }
        if type_kind(pass, &rng.x) != Some(TypeKind::Slice) {
            continue;
        }
        if rng.body.list.len() != 1 {
            continue;
        }
        let Stmt::IfStmt(if_stmt) = &rng.body.list[0] else {
            continue;
        };
        if if_stmt.init.is_some() || if_stmt.else_.is_some() {
            continue;
        }
        if if_stmt.body.list.len() != 1 {
            continue;
        }
        let Stmt::ReturnStmt(ret_true) = &if_stmt.body.list[0] else {
            continue;
        };
        if !is_return_bool(ret_true, true) {
            continue;
        }
        let Expr::BinaryExpr(BinaryExpr { x, op, y, .. }) = &if_stmt.cond else {
            continue;
        };
        if *op != Token::EQL {
            continue;
        }
        let elem = rng.value.as_ref();
        let needle = if elem.is_some_and(|e| code::same_non_dynamic(pass, e, x)) {
            y.as_ref()
        } else if elem.is_some_and(|e| code::same_non_dynamic(pass, e, y)) {
            x.as_ref()
        } else {
            continue;
        };
        let Some(slice_text) = expr_text(&rng.x) else {
            continue;
        };
        let Some(needle_text) = expr_text(needle) else {
            continue;
        };
        let end = after.return_.0 as u32;
        // Prefer end of last result if present.
        let end = after
            .results
            .last()
            .map(|e| e.end().0 as u32)
            .unwrap_or(end);
        pending.push(Diagnostic {
            pos,
            end,
            category: String::new(),
            message: "loop can be modernized using slices.Contains".into(),
            suggested_fixes: vec![SuggestedFix {
                message: "Replace loop with slices.Contains".into(),
                text_edits: vec![TextEdit {
                    pos,
                    end,
                    new_text: format!("return slices.Contains({slice_text}, {needle_text})"),
                }],
            }],
            related: Vec::new(),
            url: String::new(),
            severity: String::new(),
        });
    }
}

fn check_mapsloop(pass: &Pass<'_>, range_stmt: &RangeStmt, pending: &mut Vec<Diagnostic>) {
    // Skip stdlib packages where a maps import would cycle (maps itself).
    if pass.pkg().pkg_path == "maps" || pass.pkg().pkg_path.starts_with("maps/") {
        return;
    }
    let pos = range_stmt.for_.0 as u32;
    if !go_at_least(pass, pos, "go1.23") {
        return;
    }
    if range_stmt.tok != Some(Token::DEFINE) {
        return;
    }
    let Some(key) = range_stmt.key.as_ref() else {
        return;
    };
    let Some(value) = range_stmt.value.as_ref() else {
        return;
    };
    // Body must be a single `m[k] = v` (or `:=`) assignment.
    if range_stmt.body.list.len() != 1 {
        return;
    }
    let Stmt::AssignStmt(assign) = &range_stmt.body.list[0] else {
        return;
    };
    if assign.tok != Some(Token::ASSIGN) && assign.tok != Some(Token::DEFINE) {
        return;
    }
    if assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return;
    }
    let Expr::IndexExpr(index) = &assign.lhs[0] else {
        return;
    };
    if !code::same_non_dynamic(pass, key, &index.index) {
        return;
    }
    if !code::same_non_dynamic(pass, value, &assign.rhs[0]) {
        return;
    }
    // Reject e.g. f(k, v)[k] = v
    if expr_uses_loop_vars(pass, &index.x, key, value) {
        return;
    }
    // Source x must be a map (iter.Seq2 → Insert/Collect is DEFERRED).
    let Some(src_map) = underlying_map(pass, &range_stmt.x) else {
        return;
    };
    let Some(dst_map) = underlying_map(pass, &index.x) else {
        return;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let Some(index_ty) = type_of(pass, &assign.lhs[0]) else {
        return;
    };
    let Some(value_ty) = type_of(pass, value) else {
        return;
    };
    let Some(key_ty) = type_of(pass, key) else {
        return;
    };
    // No implicit conversion of key or value.
    if !types_identical(pass, index_ty, value_ty) {
        return;
    }
    if !types_identical(pass, map_key(&artifacts.types, dst_map), key_ty) {
        return;
    }
    // Also require source map key/elem match (defensive; usually follows from assignability).
    if !types_identical(pass, map_key(&artifacts.types, src_map), key_ty)
        || !types_identical(pass, map_elem(&artifacts.types, src_map), value_ty)
    {
        return;
    }

    let Some(m_text) = expr_text(&index.x) else {
        return;
    };
    let Some(x_text) = expr_text(&range_stmt.x) else {
        return;
    };
    let end = range_stmt.body.end().0 as u32;
    let report_pos = assign.lhs[0].pos().0 as u32;
    let report_end = assign.lhs[0].end().0 as u32;
    pending.push(Diagnostic {
        pos: report_pos,
        end: report_end,
        category: String::new(),
        message: "Replace m[k]=v loop with maps.Copy".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace m[k]=v loop with maps.Copy".into(),
            text_edits: vec![TextEdit {
                pos,
                end,
                new_text: format!("maps.Copy({m_text}, {x_text})"),
            }],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
}

fn split_or_fields_seq_name(pass: &Pass<'_>, call: &CallExpr) -> Option<&'static str> {
    if code::is_call_to(pass, call, "strings.Split") {
        Some("SplitSeq")
    } else if code::is_call_to(pass, call, "strings.Fields") {
        Some("FieldsSeq")
    } else {
        None
    }
}

fn check_stringsseq(pass: &Pass<'_>, range_stmt: &RangeStmt, pending: &mut Vec<Diagnostic>) {
    let pos = range_stmt.for_.0 as u32;
    if !go_at_least(pass, pos, "go1.24") {
        return;
    }
    // SplitSeq/FieldsSeq are iter.Seq (value only); reject non-blank keys.
    if let Some(key) = range_stmt.key.as_ref() {
        if ident_name(key) != Some("_") {
            return;
        }
    }
    let Expr::CallExpr(call) = &range_stmt.x else {
        return;
    };
    let Some(seq_name) = split_or_fields_seq_name(pass, call) else {
        return;
    };
    let Some(fun_text) = expr_text(&call.fun) else {
        return;
    };
    // strings.Split → strings.SplitSeq (replace the selector leaf).
    let new_fun = if let Some(prefix) = fun_text.rsplit_once('.') {
        format!("{}.{}", prefix.0, seq_name)
    } else {
        seq_name.to_string()
    };
    let end = call.fun.end().0 as u32;
    pending.push(Diagnostic {
        pos: call.fun.pos().0 as u32,
        end,
        category: String::new(),
        message: format!(
            "Ranging over {} allocates a slice; consider using {}",
            fun_text,
            new_fun
        ),
        suggested_fixes: vec![SuggestedFix {
            message: format!("Replace with {new_fun}"),
            text_edits: vec![TextEdit {
                pos: call.fun.pos().0 as u32,
                end,
                new_text: new_fun,
            }],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
}

fn is_named_pkg_type(pass: &Pass<'_>, typ: TypeId, pkg: &str, name: &str) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let typ = match artifacts.types.get(typ) {
        TypeData::Pointer(p) => unalias_readonly(&artifacts.types, p.elem()),
        _ => typ,
    };
    let TypeData::Named(_) = artifacts.types.get(typ) else {
        return false;
    };
    let obj = named_obj(&artifacts.types, typ);
    if obj.name(&artifacts.objects) != name {
        return false;
    }
    obj.pkg(&artifacts.objects)
        .is_some_and(|p| artifacts.packages.get(p).path() == pkg)
}

fn is_waitgroup_method(pass: &Pass<'_>, call: &CallExpr, method: &str) -> bool {
    let Expr::SelectorExpr(sel) = &*call.fun else {
        return false;
    };
    if sel.sel.name != method {
        return false;
    }
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(obj_id) = info.uses.get(&sel.sel.id).copied() else {
        return false;
    };
    if !matches!(artifacts.objects.get(obj_id), ObjectData::Func(_)) {
        return false;
    }
    let Some(sig_id) = obj_id.typ(&artifacts.objects) else {
        return false;
    };
    let Some(recv) = signature_recv(&artifacts.types, sig_id) else {
        return false;
    };
    let Some(recv_typ) = recv.typ(&artifacts.objects) else {
        return false;
    };
    is_named_pkg_type(pass, recv_typ, "sync", "WaitGroup")
}

fn waitgroup_recv(call: &CallExpr) -> Option<&Expr> {
    match &*call.fun {
        Expr::SelectorExpr(sel) => Some(sel.x.as_ref()),
        _ => None,
    }
}

fn check_waitgroupgo(pass: &Pass<'_>, block: &BlockStmt, pending: &mut Vec<Diagnostic>) {
    for i in 0..block.list.len().saturating_sub(1) {
        let Stmt::ExprStmt(add_stmt) = &block.list[i] else {
            continue;
        };
        let Expr::CallExpr(add_call) = &add_stmt.x else {
            continue;
        };
        if !is_waitgroup_method(pass, add_call, "Add") || add_call.args.len() != 1 {
            continue;
        }
        if !code::is_integer_literal(pass, &add_call.args[0], 1) {
            continue;
        }
        let Some(add_recv) = waitgroup_recv(add_call) else {
            continue;
        };
        let Stmt::GoStmt(GoStmt { go_, call: go_call }) = &block.list[i + 1] else {
            continue;
        };
        if !go_call.args.is_empty() {
            continue;
        }
        let Expr::FuncLit(lit) = &*go_call.fun else {
            continue;
        };
        if lit.ty.results.as_ref().is_some_and(|f| !f.list.is_empty()) {
            continue;
        }
        if lit.body.list.is_empty() {
            continue;
        }
        let Stmt::DeferStmt(defer_stmt) = &lit.body.list[0] else {
            continue;
        };
        if !is_waitgroup_method(pass, &defer_stmt.call, "Done") {
            continue;
        }
        let Some(done_recv) = waitgroup_recv(&defer_stmt.call) else {
            continue;
        };
        if !code::same_non_dynamic(pass, add_recv, done_recv) {
            continue;
        }
        let pos = go_.0 as u32;
        if !go_at_least(pass, pos, "go1.25") {
            continue;
        }
        let Some(recv_text) = expr_text(add_recv) else {
            continue;
        };
        pending.push(Diagnostic {
            pos,
            end: lit.ty.end().0 as u32,
            category: String::new(),
            message: "Goroutine creation can be simplified using WaitGroup.Go".into(),
            suggested_fixes: vec![SuggestedFix {
                message: "Simplify by using WaitGroup.Go".into(),
                text_edits: vec![
                    TextEdit {
                        pos: add_stmt.x.pos().0 as u32,
                        end: add_stmt.x.end().0 as u32,
                        new_text: String::new(),
                    },
                    TextEdit {
                        pos,
                        end: go_call.pos().0 as u32,
                        new_text: format!("{recv_text}.Go("),
                    },
                    TextEdit {
                        pos: defer_stmt.defer_.0 as u32,
                        end: defer_stmt.call.end().0 as u32,
                        new_text: String::new(),
                    },
                ],
            }],
            related: Vec::new(),
            url: String::new(),
            severity: String::new(),
        });
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "modernize requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<ModernizeOptions>("modernize")
        .cloned()
        .unwrap_or_default();

    let mut pending = Vec::new();
    for file in pass.files() {
        if enabled(&options, "plusbuild") && go_at_least(pass, file.package.0 as u32, "go1.18") {
            check_plusbuild(file, &mut pending);
        }
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::InterfaceType(iface) if enabled(&options, "any") => {
                    check_any(pass, iface, &mut pending);
                }
                NodeRef::RangeStmt(s) => {
                    if enabled(&options, "forvar") {
                        check_forvar(pass, s, &mut pending);
                    }
                    if enabled(&options, "stringsseq") {
                        check_stringsseq(pass, s, &mut pending);
                    }
                    if enabled(&options, "mapsloop") {
                        check_mapsloop(pass, s, &mut pending);
                    }
                }
                NodeRef::ForStmt(s) if enabled(&options, "rangeint") => {
                    check_rangeint(pass, s, &mut pending);
                }
                NodeRef::IfStmt(s) => {
                    if enabled(&options, "minmax") {
                        check_minmax(pass, s, &mut pending);
                    }
                    if enabled(&options, "stringscutprefix") {
                        check_stringscutprefix(pass, s, &mut pending);
                    }
                }
                NodeRef::BlockStmt(b) => {
                    if enabled(&options, "slicescontains") {
                        check_slicescontains(pass, b, &mut pending);
                    }
                    if enabled(&options, "waitgroupgo") {
                        check_waitgroupgo(pass, b, &mut pending);
                    }
                }
                NodeRef::CallExpr(c) => {
                    if enabled(&options, "fmtappendf") {
                        check_fmtappendf(pass, c, &mut pending);
                    }
                    if enabled(&options, "slicessort") {
                        check_slicessort(pass, c, &mut pending);
                    }
                }
                NodeRef::StructType(StructType { fields, .. }) if enabled(&options, "omitzero") => {
                    for field in &fields.list {
                        check_omitzero(pass, field, &mut pending);
                    }
                }
                _ => {}
            }
            true
        });
    }

    for d in pending {
        pass.report(d);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "modernize",
        doc: "suggests simplifications to Go code using modern language and library features",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/modernize",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
