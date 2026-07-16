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
//! - `stringscutprefix` — `HasPrefix`+`TrimPrefix` → `CutPrefix` (Go 1.20+; pattern 1+2;
//!   strings+bytes)
//! - `slicescontains` — search loop → `slices.Contains`/`ContainsFunc` (Go 1.21+;
//!   return true/false, break body, found=true/false)
//! - `stringsseq` — `range strings.Split/Fields` → `SplitSeq`/`FieldsSeq` (Go 1.24+)
//! - `waitgroupgo` — `Add(1)`+`go`+`Done` → `WaitGroup.Go` (Go 1.25+)
//! - `mapsloop` — `for k, v := range x { m[k] = v }` → `maps.Copy` (Go 1.23+; map→map)
//! - `slicesbackward` — reverse index loop → `slices.Backward` (Go 1.23+; simplified)
//! - `reflecttypefor` — `reflect.TypeOf` → `TypeFor` (Go 1.22+; `(*T)(nil).Elem` + simple vars)
//! - `testingcontext` — `WithCancel(Background/TODO)`+`defer cancel` → `t.Context` (Go 1.24+)
//! - `unsafefuncs` — `unsafe.Pointer(uintptr(ptr)+…)` → `unsafe.Add` (Go 1.17+)
//! - `importcomment` — obsolete `package p // import "path"` comments
//! - `stringscut` — `Split(N)(…)[0]` → `Cut` (Go 1.18+; strings+bytes Split/SplitN)
//! - `newexpr` — `func f(x T) *T { return &x }` → `new(x)` wrappers + call sites
//!   (Go 1.26+; `NewLike` facts)
//!
//! DEFERRED (recognized in `disable` / documented): atomictypes, embedlit,
//! errorsastype, stditerators, stringsbuilder, stringscut Index/Contains
//! patterns, unsafefuncs Slice/String helpers, importcomment Module==nil
//! (GOPATH) skip, mapsloop Insert/Collect (iter.Seq2) / Clone (nil-preserving),
//! slicescontains nested free break/continue analysis full parity,
//! waitgroupgo trailing-Done /
//! SuggestedFix import edits, reflecttypefor complicated/unnamed types &
//! unused-var deletion, slicesbackward mutation/non-`s[i]` use analysis full
//! parity, testingcontext sole-use via typeindex, newexpr `new` shadowing /
//! CheckExpr untyped-constant re-typecheck full parity, and full rangeint/minmax
//! edge-case parity with upstream.

use std::collections::HashSet;
use std::fs;
use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, BinaryExpr, BlockStmt, BranchStmt, CallExpr, CommentGroup, Decl, Expr, Field,
    File, ForStmt, FuncDecl, FuncLit, GoStmt, IfStmt, IncDecStmt, InterfaceType, RangeStmt, Stmt,
    StructType, UnaryExpr,
};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::FileSet;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Fact, FactTypeId, Pass, RunError, RunFn, SuggestedFix,
    TextEdit,
};
use guff_types::alias::unalias_readonly;
use guff_types::api_predicates::api_identical;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::BasicKind;
use guff_types::map::{map_elem, map_key};
use guff_types::named::named_obj;
use guff_types::pointer::pointer_elem;
use guff_types::predicates::{is_float, is_integer, is_interface, is_string};
use guff_types::signature::{
    signature_params, signature_recv, signature_results, signature_variadic,
};
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::typestring::type_string;
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

fn cutprefix_kind(pass: &Pass<'_>, call: &CallExpr) -> Option<(&'static str, &'static str, bool)> {
    // (pkg, cut_name, is_prefix)
    if code::is_call_to(pass, call, "strings.HasPrefix") {
        Some(("strings", "CutPrefix", true))
    } else if code::is_call_to(pass, call, "strings.HasSuffix") {
        Some(("strings", "CutSuffix", false))
    } else if code::is_call_to(pass, call, "bytes.HasPrefix") {
        Some(("bytes", "CutPrefix", true))
    } else if code::is_call_to(pass, call, "bytes.HasSuffix") {
        Some(("bytes", "CutSuffix", false))
    } else {
        None
    }
}

fn trim_kind(pass: &Pass<'_>, call: &CallExpr) -> Option<(&'static str, &'static str, bool)> {
    // (pkg, cut_name, is_prefix)
    if code::is_call_to(pass, call, "strings.TrimPrefix") {
        Some(("strings", "CutPrefix", true))
    } else if code::is_call_to(pass, call, "strings.TrimSuffix") {
        Some(("strings", "CutSuffix", false))
    } else if code::is_call_to(pass, call, "bytes.TrimPrefix") {
        Some(("bytes", "CutPrefix", true))
    } else if code::is_call_to(pass, call, "bytes.TrimSuffix") {
        Some(("bytes", "CutSuffix", false))
    } else {
        None
    }
}

fn check_stringscutprefix(pass: &Pass<'_>, if_stmt: &IfStmt, pending: &mut Vec<Diagnostic>) {
    // Pattern 1: if pkg.HasPrefix(s, affix) { use(pkg.TrimPrefix(s, affix)) }
    if if_stmt.init.is_none() && !if_stmt.body.list.is_empty() {
        if let Expr::CallExpr(has_call) = &if_stmt.cond {
            let pos = has_call.pos().0 as u32;
            if go_at_least(pass, pos, "go1.20") && has_call.args.len() == 2 {
                if let Some((pkg, cut_name, is_prefix)) = cutprefix_kind(pass, has_call) {
                    let trim_name = if is_prefix {
                        format!("{pkg}.TrimPrefix")
                    } else {
                        format!("{pkg}.TrimSuffix")
                    };
                    if let Some(trim_call) =
                        find_first_call_named(pass, &if_stmt.body.list[0], &trim_name)
                    {
                        if trim_call.args.len() == 2
                            && code::same_non_dynamic(pass, &has_call.args[0], &trim_call.args[0])
                            && code::same_non_dynamic(pass, &has_call.args[1], &trim_call.args[1])
                        {
                            if let (Some(s_text), Some(affix_text)) = (
                                expr_text(&has_call.args[0]),
                                expr_text(&has_call.args[1]),
                            ) {
                                let var_name = if is_prefix { "after" } else { "before" };
                                let (message, fix_message) = if is_prefix {
                                    (
                                        "HasPrefix + TrimPrefix can be simplified to CutPrefix",
                                        "Replace HasPrefix/TrimPrefix with CutPrefix",
                                    )
                                } else {
                                    (
                                        "HasSuffix + TrimSuffix can be simplified to CutSuffix",
                                        "Replace HasSuffix/TrimSuffix with CutSuffix",
                                    )
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
                                                    "{var_name}, ok := {pkg}.{cut_name}({s_text}, {affix_text}); ok"
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
                                return;
                            }
                        }
                    }
                }
            }
        }
    }

    // Pattern 2: if after := pkg.TrimPrefix(s, affix); after != s { use(after) }
    let Some(Stmt::AssignStmt(init)) = if_stmt.init.as_deref() else {
        return;
    };
    if init.tok != Some(Token::DEFINE) || init.lhs.len() != 1 || init.rhs.len() != 1 {
        return;
    }
    let Expr::CallExpr(trim_call) = &init.rhs[0] else {
        return;
    };
    let Some((_pkg, cut_name, is_prefix)) = trim_kind(pass, trim_call) else {
        return;
    };
    if trim_call.args.len() != 2 {
        return;
    }
    let Expr::BinaryExpr(bin) = &if_stmt.cond else {
        return;
    };
    if bin.op != Token::NEQ {
        return;
    }
    let lhs = &init.lhs[0];
    let s_arg = &trim_call.args[0];
    let cond_ok = (code::same_non_dynamic(pass, lhs, &bin.x)
        && code::same_non_dynamic(pass, s_arg, &bin.y))
        || (code::same_non_dynamic(pass, lhs, &bin.y)
            && code::same_non_dynamic(pass, s_arg, &bin.x));
    if !cond_ok {
        return;
    }
    let pos = init.lhs[0].pos().0 as u32;
    if !go_at_least(pass, pos, "go1.20") {
        return;
    }
    let Expr::SelectorExpr(sel) = trim_call.fun.as_ref() else {
        return; // e.g. dot-import
    };
    let (message, fix_message) = if is_prefix {
        (
            "TrimPrefix can be simplified to CutPrefix",
            "Replace TrimPrefix with CutPrefix",
        )
    } else {
        (
            "TrimSuffix can be simplified to CutSuffix",
            "Replace TrimSuffix with CutSuffix",
        )
    };
    let end = if_stmt.cond.end().0 as u32;
    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: message.into(),
        suggested_fixes: vec![SuggestedFix {
            message: fix_message.into(),
            text_edits: vec![
                TextEdit {
                    pos: init.lhs[0].end().0 as u32,
                    end: init.lhs[0].end().0 as u32,
                    new_text: ", ok".into(),
                },
                TextEdit {
                    pos: sel.sel.pos().0 as u32,
                    end: sel.sel.end().0 as u32,
                    new_text: cut_name.into(),
                },
                TextEdit {
                    pos: if_stmt.cond.pos().0 as u32,
                    end: if_stmt.cond.end().0 as u32,
                    new_text: "ok".into(),
                },
            ],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
}

fn is_true_or_false_lit(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::Ident(id) if id.name == "true" => Some(true),
        Expr::Ident(id) if id.name == "false" => Some(false),
        _ => None,
    }
}

fn is_blank_ident(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(id) if id.name == "_")
}

/// Whether `e` is the range element (`elem`) or `s[i]`.
fn is_slice_elem(pass: &Pass<'_>, rng: &RangeStmt, e: &Expr) -> bool {
    if let Some(val) = rng.value.as_ref() {
        if !is_blank_ident(val) && code::same_non_dynamic(pass, e, val) {
            return true;
        }
    }
    if let (Some(key), Expr::IndexExpr(ix)) = (rng.key.as_ref(), e) {
        if !is_blank_ident(key)
            && code::same_non_dynamic(pass, &ix.x, &rng.x)
            && code::same_non_dynamic(pass, &ix.index, key)
        {
            return true;
        }
    }
    false
}

fn range_var_objs(pass: &Pass<'_>, rng: &RangeStmt) -> Vec<ObjectId> {
    let mut objs = Vec::new();
    for opt in [&rng.key, &rng.value] {
        let Some(Expr::Ident(id)) = opt.as_ref() else {
            continue;
        };
        if id.name == "_" {
            continue;
        }
        if let Some(obj) = ident_obj(pass, id) {
            objs.push(obj);
        }
    }
    objs
}

fn node_uses_objs(pass: &Pass<'_>, node: NodeRef<'_>, objs: &[ObjectId]) -> bool {
    if objs.is_empty() {
        return false;
    }
    let mut used = false;
    walk::inspect(node, |n| {
        let Some(n) = n else {
            return true;
        };
        if let NodeRef::Ident(id) = n {
            if let Some(obj) = ident_obj(pass, id) {
                if objs.contains(&obj) {
                    used = true;
                    return false;
                }
            }
        }
        true
    });
    used
}

fn stmt_uses_range_vars(pass: &Pass<'_>, stmt: &Stmt, rng: &RangeStmt) -> bool {
    node_uses_objs(pass, walk::stmt_ref(stmt), &range_var_objs(pass, rng))
}

fn expr_uses_range_vars(pass: &Pass<'_>, expr: &Expr, rng: &RangeStmt) -> bool {
    node_uses_objs(pass, walk::expr_ref(expr), &range_var_objs(pass, rng))
}

/// Side-effect heuristic: reject call / unary / composite needles for Contains.
fn expr_may_have_effects(expr: &Expr) -> bool {
    match expr {
        Expr::CallExpr(_) | Expr::UnaryExpr(_) | Expr::CompositeLit(_) | Expr::FuncLit(_) => true,
        Expr::ParenExpr(p) => expr_may_have_effects(&p.x),
        Expr::SelectorExpr(sel) => expr_may_have_effects(&sel.x),
        Expr::IndexExpr(ix) => expr_may_have_effects(&ix.x) || expr_may_have_effects(&ix.index),
        Expr::BinaryExpr(b) => expr_may_have_effects(&b.x) || expr_may_have_effects(&b.y),
        _ => false,
    }
}

/// Analyze `if cond` for Contains / ContainsFunc. Returns (func_name, arg2_text).
fn slicescontains_cond(
    pass: &Pass<'_>,
    rng: &RangeStmt,
    cond: &Expr,
) -> Option<(&'static str, String)> {
    match cond {
        Expr::BinaryExpr(BinaryExpr { x, op, y, .. }) if *op == Token::EQL => {
            let (elem, needle) = if is_slice_elem(pass, rng, x) {
                (x.as_ref(), y.as_ref())
            } else if is_slice_elem(pass, rng, y) {
                (y.as_ref(), x.as_ref())
            } else {
                return None;
            };
            let Some(elem_ty) = type_of(pass, elem) else {
                return None;
            };
            let Some(needle_ty) = type_of(pass, needle) else {
                return None;
            };
            if !types_identical(pass, elem_ty, needle_ty) {
                return None;
            }
            if expr_may_have_effects(needle) || expr_uses_range_vars(pass, needle, rng) {
                return None;
            }
            let needle_text = expr_text(needle)?;
            Some(("Contains", needle_text))
        }
        Expr::CallExpr(call) if call.args.len() == 1 && !call.ellipsis.is_valid() => {
            // Skip type conversions: `T(x)`.
            let is_type = pass.types_info().is_some_and(|info| {
                info.types
                    .get(&call.fun.id())
                    .is_some_and(|tav| tav.mode == OperandMode::TypeExpr)
            });
            if is_type {
                return None;
            }
            if !is_slice_elem(pass, rng, &call.args[0]) {
                return None;
            }
            if expr_uses_range_vars(pass, &call.fun, rng) {
                return None;
            }
            let pred_text = expr_text(&call.fun)?;
            if expr_may_have_effects(&call.fun) {
                return None;
            }
            Some(("ContainsFunc", pred_text))
        }
        _ => None,
    }
}

fn is_unlabeled_break(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::BranchStmt(BranchStmt {
            tok: Token::BREAK,
            label: None,
            ..
        })
    )
}

fn body_has_free_branch(stmts: &[Stmt], skip_last: bool) -> bool {
    let end = if skip_last {
        stmts.len().saturating_sub(1)
    } else {
        stmts.len()
    };
    for stmt in &stmts[..end] {
        let mut found = false;
        walk::inspect(walk::stmt_ref(stmt), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::BranchStmt(b)
                    if matches!(b.tok, Token::BREAK | Token::CONTINUE) && b.label.is_none() =>
                {
                    found = true;
                    return false;
                }
                NodeRef::ReturnStmt(_) => {
                    found = true;
                    return false;
                }
                // Don't descend into nested function literals.
                NodeRef::FuncLit(_) => return false,
                _ => {}
            }
            true
        });
        if found {
            return true;
        }
    }
    false
}

fn check_slicescontains(
    pass: &Pass<'_>,
    block: &BlockStmt,
    pending: &mut Vec<Diagnostic>,
) {
    if pass.pkg().pkg_path == "slices" || pass.pkg().pkg_path.starts_with("slices/") {
        return;
    }
    for i in 0..block.list.len() {
        let Stmt::RangeStmt(rng) = &block.list[i] else {
            continue;
        };
        let pos = rng.for_.0 as u32;
        if !go_at_least(pass, pos, "go1.21") {
            continue;
        }
        if type_kind(pass, &rng.x) != Some(TypeKind::Slice) {
            continue;
        }
        if rng.tok != Some(Token::DEFINE) {
            continue;
        }
        // Need at least a key ident (may be `_`).
        if !matches!(rng.key.as_ref(), Some(Expr::Ident(_))) {
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
        let body = &if_stmt.body.list;
        if body.is_empty() {
            continue;
        }
        let Some((func_name, arg2_text)) = slicescontains_cond(pass, rng, &if_stmt.cond) else {
            continue;
        };
        // Upstream rejects any body use of range vars (including last stmt).
        if body.iter().any(|s| stmt_uses_range_vars(pass, s, rng)) {
            continue;
        }
        if body_has_free_branch(body, true) {
            continue;
        }
        let Some(slice_text) = expr_text(&rng.x) else {
            continue;
        };
        let contains = format!("slices.{func_name}({slice_text}, {arg2_text})");
        let last = body.last().unwrap();
        let msg = format!("loop can be modernized using slices.{func_name}");

        // Special case: body={ return true/false } next={ return false/true }
        if let Stmt::ReturnStmt(ret_last) = last {
            if body.len() == 1 {
                if let Some(Stmt::ReturnStmt(after)) = block.list.get(i + 1) {
                    let tval = if ret_last.results.len() == 1 {
                        is_true_or_false_lit(&ret_last.results[0])
                    } else {
                        None
                    };
                    let fval = if after.results.len() == 1 {
                        is_true_or_false_lit(&after.results[0])
                    } else {
                        None
                    };
                    if let (Some(t), Some(f)) = (tval, fval) {
                        if t != f {
                            let neg = if t { "" } else { "!" };
                            let end = after
                                .results
                                .last()
                                .map(|e| e.end().0 as u32)
                                .unwrap_or(after.return_.0 as u32);
                            pending.push(Diagnostic {
                                pos,
                                end,
                                category: String::new(),
                                message: msg.clone(),
                                suggested_fixes: vec![SuggestedFix {
                                    message: format!("Replace loop with slices.{func_name}"),
                                    text_edits: vec![TextEdit {
                                        pos,
                                        end,
                                        new_text: format!("return {neg}{contains}"),
                                    }],
                                }],
                                related: Vec::new(),
                                url: String::new(),
                                severity: String::new(),
                            });
                            continue;
                        }
                    }
                }
            }
            // General return: for ... { if cond { stmts; return x } } → if Contains { ... }
            let end = rng.body.end().0 as u32;
            pending.push(Diagnostic {
                pos,
                end,
                category: String::new(),
                message: msg,
                suggested_fixes: vec![SuggestedFix {
                    message: format!("Replace loop with if slices.{func_name}"),
                    text_edits: vec![
                        TextEdit {
                            pos,
                            end: if_stmt.body.pos().0 as u32,
                            new_text: format!("if {contains} "),
                        },
                        TextEdit {
                            pos: if_stmt.body.end().0 as u32,
                            end,
                            new_text: String::new(),
                        },
                    ],
                }],
                related: Vec::new(),
                url: String::new(),
                severity: String::new(),
            });
            continue;
        }

        // break variants
        if !is_unlabeled_break(last) {
            continue;
        }
        // Sole break → empty if; skip (upstream #77677).
        if body.len() == 1 {
            continue;
        }

        // Special: prev=`lhs = false`; body=`lhs = true; break`
        if body.len() == 2 {
            if let Stmt::AssignStmt(assign) = &body[0] {
                if assign.tok == Some(Token::ASSIGN)
                    && assign.lhs.len() == 1
                    && assign.rhs.len() == 1
                {
                    if let Some(assign_bool) = is_true_or_false_lit(&assign.rhs[0]) {
                        if let Some(j) = i.checked_sub(1) {
                            if let Stmt::AssignStmt(prev) = &block.list[j] {
                                if (prev.tok == Some(Token::ASSIGN)
                                    || prev.tok == Some(Token::DEFINE))
                                    && prev.lhs.len() == 1
                                    && prev.rhs.len() == 1
                                    && code::same_non_dynamic(pass, &prev.lhs[0], &assign.lhs[0])
                                {
                                    if let Some(prev_bool) = is_true_or_false_lit(&prev.rhs[0]) {
                                        if assign_bool != prev_bool {
                                            let neg = if assign_bool { "" } else { "!" };
                                            let Some(lhs_text) = expr_text(&assign.lhs[0]) else {
                                                continue;
                                            };
                                            let end = rng.body.end().0 as u32;
                                            let prev_pos = prev.lhs[0].pos().0 as u32;
                                            pending.push(Diagnostic {
                                                pos: prev_pos,
                                                end,
                                                category: String::new(),
                                                message: msg.clone(),
                                                suggested_fixes: vec![SuggestedFix {
                                                    message: format!(
                                                        "Replace loop with slices.{func_name}"
                                                    ),
                                                    text_edits: vec![TextEdit {
                                                        pos: prev_pos,
                                                        end,
                                                        new_text: format!(
                                                            "{lhs_text} = {neg}{contains}"
                                                        ),
                                                    }],
                                                }],
                                                related: Vec::new(),
                                                url: String::new(),
                                                severity: String::new(),
                                            });
                                            continue;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // General: for ... { if cond { stmts; break } } → if Contains { stmts }
        let end = rng.body.end().0 as u32;
        let before_break_end = body[body.len() - 2].end().0 as u32;
        pending.push(Diagnostic {
            pos,
            end,
            category: String::new(),
            message: msg,
            suggested_fixes: vec![SuggestedFix {
                message: format!("Replace loop with if slices.{func_name}"),
                text_edits: vec![
                    TextEdit {
                        pos,
                        end: if_stmt.body.pos().0 as u32,
                        new_text: format!("if {contains} "),
                    },
                    TextEdit {
                        pos: before_break_end,
                        end: last.end().0 as u32,
                        new_text: String::new(),
                    },
                    TextEdit {
                        pos: if_stmt.body.end().0 as u32,
                        end,
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

fn is_simple_dec(post: &Stmt, index_name: &str) -> bool {
    match post {
        Stmt::IncDecStmt(IncDecStmt { x, tok, .. }) => {
            *tok == Token::DEC && ident_name(x) == Some(index_name)
        }
        Stmt::AssignStmt(a)
            if a.tok == Some(Token::SubAssign)
                && a.lhs.len() == 1
                && a.rhs.len() == 1
                && ident_name(&a.lhs[0]) == Some(index_name) =>
        {
            matches!(&a.rhs[0], Expr::BasicLit(lit) if lit.value == "1")
        }
        _ => false,
    }
}

fn index_mutated_in_body(pass: &Pass<'_>, body: &BlockStmt, index_obj: ObjectId) -> bool {
    let mut mutated = false;
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        match n {
            NodeRef::AssignStmt(a) => {
                for lhs in &a.lhs {
                    if let Expr::Ident(id) = lhs {
                        if ident_obj(pass, id) == Some(index_obj) {
                            mutated = true;
                            return false;
                        }
                    }
                }
            }
            NodeRef::IncDecStmt(IncDecStmt { x, .. }) => {
                if let Expr::Ident(id) = x {
                    if ident_obj(pass, id) == Some(index_obj) {
                        mutated = true;
                        return false;
                    }
                }
            }
            NodeRef::UnaryExpr(u) if u.op == Token::AND => {
                if let Expr::Ident(id) = u.x.as_ref() {
                    if ident_obj(pass, id) == Some(index_obj) {
                        mutated = true;
                        return false;
                    }
                }
            }
            _ => {}
        }
        true
    });
    mutated
}

fn check_slicesbackward(pass: &Pass<'_>, for_stmt: &ForStmt, pending: &mut Vec<Diagnostic>) {
    if pass.pkg().pkg_path == "slices" || pass.pkg().pkg_path.starts_with("slices/") {
        return;
    }
    let pos = for_stmt.for_.0 as u32;
    if !go_at_least(pass, pos, "go1.23") {
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
    let Expr::BinaryExpr(bin) = &init.rhs[0] else {
        return;
    };
    if bin.op != Token::SUB || !code::is_integer_literal(pass, &bin.y, 1) {
        return;
    }
    let Expr::CallExpr(len_call) = bin.x.as_ref() else {
        return;
    };
    if !code::is_call_to(pass, len_call, "len") || len_call.args.len() != 1 {
        return;
    }
    if type_kind(pass, &len_call.args[0]) != Some(TypeKind::Slice) {
        return;
    }
    let Some(Expr::BinaryExpr(cond)) = for_stmt.cond.as_ref() else {
        return;
    };
    if cond.op != Token::GEQ
        || ident_name(&cond.x) != Some(index_name)
        || !code::is_integer_literal(pass, &cond.y, 0)
    {
        return;
    }
    let Some(post) = for_stmt.post.as_deref() else {
        return;
    };
    if !is_simple_dec(post, index_name) {
        return;
    }
    let Expr::Ident(index_id) = &init.lhs[0] else {
        return;
    };
    let Some(index_obj) = ident_obj(pass, index_id) else {
        return;
    };
    if index_mutated_in_body(pass, &for_stmt.body, index_obj) {
        return;
    }

    let slice_expr = &len_call.args[0];
    let Some(slice_text) = expr_text(slice_expr) else {
        return;
    };

    // Classify body uses of i: pure s[i] vs other.
    let mut slice_indexes: Vec<(u32, u32)> = Vec::new();
    let mut other_uses = 0usize;
    walk::inspect(NodeRef::BlockStmt(&for_stmt.body), |n| {
        let Some(n) = n else {
            return true;
        };
        let NodeRef::Ident(id) = n else {
            return true;
        };
        if ident_obj(pass, id) != Some(index_obj) {
            return true;
        }
        // Walk parent is not available; approximate: collect all idents and
        // separately find IndexExpr where index is this ident and x is slice.
        other_uses += 1;
        true
    });
    // Re-scan for s[i] patterns and subtract them from other_uses.
    walk::inspect(NodeRef::BlockStmt(&for_stmt.body), |n| {
        let Some(n) = n else {
            return true;
        };
        if let NodeRef::IndexExpr(ix) = n {
            if code::same_non_dynamic(pass, &ix.x, slice_expr)
                && code::same_non_dynamic(pass, &ix.index, &init.lhs[0])
            {
                slice_indexes.push((ix.x.pos().0 as u32, (ix.rbrack.0 + 1) as u32));
            }
        }
        true
    });
    other_uses = other_uses.saturating_sub(slice_indexes.len());

    let end = for_stmt
        .post
        .as_ref()
        .map(|p| p.end().0 as u32)
        .unwrap_or(pos);
    let header_pos = init.lhs[0].pos().0 as u32;
    let elem_name = "v";
    let header = if other_uses == 0 && !slice_indexes.is_empty() {
        format!("_, {elem_name} := range slices.Backward({slice_text})")
    } else {
        format!("{index_name}, {elem_name} := range slices.Backward({slice_text})")
    };
    let mut text_edits = vec![TextEdit {
        pos: header_pos,
        end,
        new_text: header,
    }];
    if other_uses == 0 {
        for (ipos, iend) in &slice_indexes {
            text_edits.push(TextEdit {
                pos: *ipos,
                end: *iend,
                new_text: elem_name.into(),
            });
        }
    }
    pending.push(Diagnostic {
        pos: header_pos,
        end,
        category: String::new(),
        message: "backward loop over slice can be modernized using slices.Backward".into(),
        suggested_fixes: vec![SuggestedFix {
            message: format!("Replace with range slices.Backward({slice_text})"),
            text_edits,
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
}

fn format_type(pass: &Pass<'_>, typ: TypeId) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let pkg = pass.type_pkg();
    let qf = pkg.map(|p| {
        move |id: guff_types::arena::PackageId, parena: &guff_types::arena::PackageArena| {
            if id == p {
                String::new()
            } else {
                parena.get(id).name().to_string()
            }
        }
    });
    let qf_ref = qf
        .as_ref()
        .map(|f| f as &dyn Fn(guff_types::arena::PackageId, &guff_types::arena::PackageArena) -> String);
    Some(type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        qf_ref,
    ))
}

fn is_complicated_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return true;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Named(_) | TypeData::Alias(_) | TypeData::Basic(_) | TypeData::TypeParam(_) => {
            false
        }
        TypeData::Pointer(p) => is_complicated_type(pass, p.elem()),
        TypeData::Slice(s) => is_complicated_type(pass, s.elem()),
        TypeData::Array(a) => is_complicated_type(pass, a.elem()),
        TypeData::Chan(c) => is_complicated_type(pass, c.elem()),
        TypeData::Map(m) => {
            is_complicated_type(pass, m.key()) || is_complicated_type(pass, m.elem())
        }
        TypeData::Struct(_) | TypeData::Interface(_) | TypeData::Signature(_) => true,
        _ => true,
    }
}

fn expr_has_effects(pass: &Pass<'_>, expr: &Expr) -> bool {
    match expr {
        Expr::Ident(_) | Expr::BasicLit(_) | Expr::SelectorExpr(_) | Expr::CompositeLit(_) => false,
        Expr::ParenExpr(p) => expr_has_effects(pass, &p.x),
        Expr::UnaryExpr(u) => expr_has_effects(pass, &u.x),
        Expr::StarExpr(s) => expr_has_effects(pass, &s.x),
        Expr::IndexExpr(ix) => {
            expr_has_effects(pass, &ix.x) || expr_has_effects(pass, &ix.index)
        }
        Expr::CallExpr(call) => {
            // Type conversion T(x) is effect-free if x is.
            let info = pass.types_info();
            let is_conv = info
                .and_then(|i| i.types.get(&call.fun.id()))
                .is_some_and(|tav| tav.mode == OperandMode::TypeExpr);
            if is_conv && call.args.len() == 1 {
                return expr_has_effects(pass, &call.args[0]);
            }
            true
        }
        _ => true,
    }
}

fn is_nil_typed_conversion(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Expr::CallExpr(call) = expr else {
        return false;
    };
    if call.args.len() != 1 || !code::is_nil(pass, &call.args[0]) {
        return false;
    }
    pass.types_info()
        .and_then(|i| i.types.get(&call.fun.id()))
        .is_some_and(|tav| tav.mode == OperandMode::TypeExpr)
}

fn check_reflecttypefor(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<Diagnostic>) {
    if !code::is_call_to(pass, call, "reflect.TypeOf") || call.args.len() != 1 {
        return;
    }
    // Skip `TypeOf((*T)(nil))` / `TypeOf([]T(nil))` — usually paired with `.Elem()`
    // (handled by `check_reflecttypefor_elem`). Reporting both would duplicate.
    if is_nil_typed_conversion(pass, &call.args[0]) {
        return;
    }
    let pos = call.fun.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.22") {
        return;
    }
    if code::is_nil(pass, &call.args[0]) {
        return;
    }
    if expr_has_effects(pass, &call.args[0]) {
        return;
    }
    let Some(arg_ty) = type_of(pass, &call.args[0]) else {
        return;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let under = unalias_readonly(&artifacts.types, arg_ty).underlying(&artifacts.types);
    if is_interface(&artifacts.types, under) {
        return;
    }
    if is_complicated_type(pass, arg_ty) {
        return;
    }
    let Some(tstr) = format_type(pass, arg_ty) else {
        return;
    };
    if tstr.len() >= 16 {
        let old_len = (call.args[0].end().0 - call.args[0].pos().0).max(1) as usize;
        if tstr.len() > 3 * old_len {
            return;
        }
    }
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return; // e.g. dot-import
    };
    let end = call.fun.end().0 as u32;
    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: "reflect.TypeOf call can be simplified using TypeFor".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace TypeOf by TypeFor".into(),
            text_edits: vec![
                TextEdit {
                    pos: sel.sel.pos().0 as u32,
                    end: sel.sel.end().0 as u32,
                    new_text: format!("TypeFor[{tstr}]"),
                },
                TextEdit {
                    pos: (call.lparen.0 + 1) as u32,
                    end: call.rparen.0 as u32,
                    new_text: String::new(),
                },
            ],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
}

fn check_reflecttypefor_elem(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<Diagnostic>) {
    // Match reflect.TypeOf(expr).Elem()
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return;
    };
    if sel.sel.name != "Elem" || !call.args.is_empty() {
        return;
    }
    let Expr::CallExpr(typeof_call) = sel.x.as_ref() else {
        return;
    };
    if !code::is_call_to(pass, typeof_call, "reflect.TypeOf") || typeof_call.args.len() != 1 {
        return;
    }
    let pos = typeof_call.fun.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.22") {
        return;
    }
    if expr_has_effects(pass, &typeof_call.args[0]) {
        return;
    }
    let Some(arg_ty) = type_of(pass, &typeof_call.args[0]) else {
        return;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let under = unalias_readonly(&artifacts.types, arg_ty).underlying(&artifacts.types);
    let elem = match artifacts.types.get(under) {
        TypeData::Pointer(p) => p.elem(),
        TypeData::Slice(s) => s.elem(),
        TypeData::Array(a) => a.elem(),
        TypeData::Chan(c) => c.elem(),
        TypeData::Map(m) => m.elem(),
        _ => return,
    };
    if is_complicated_type(pass, elem) {
        return;
    }
    let Some(tstr) = format_type(pass, elem) else {
        return;
    };
    let Expr::SelectorExpr(typeof_sel) = typeof_call.fun.as_ref() else {
        return;
    };
    pending.push(Diagnostic {
        pos,
        end: call.end().0 as u32,
        category: String::new(),
        message: "reflect.TypeOf call can be simplified using TypeFor".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Replace TypeOf by TypeFor".into(),
            text_edits: vec![
                TextEdit {
                    pos: typeof_sel.sel.pos().0 as u32,
                    end: typeof_sel.sel.end().0 as u32,
                    new_text: format!("TypeFor[{tstr}]"),
                },
                TextEdit {
                    pos: (typeof_call.lparen.0 + 1) as u32,
                    end: typeof_call.rparen.0 as u32,
                    new_text: String::new(),
                },
                // delete `.Elem()`
                TextEdit {
                    pos: typeof_call.end().0 as u32,
                    end: call.end().0 as u32,
                    new_text: String::new(),
                },
            ],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
}

fn test_kind_prefix(name: &str) -> Option<&'static str> {
    for prefix in ["Test", "Benchmark", "Fuzz"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            if rest.is_empty() {
                return Some(prefix);
            }
            let first = rest.chars().next()?;
            if first.is_uppercase() {
                return Some(prefix);
            }
        }
    }
    None
}

fn testing_param_name(fd: &FuncDecl) -> Option<&str> {
    test_kind_prefix(&fd.name.name)?;
    let params = fd.ty.params.as_ref()?;
    if params.list.len() != 1 {
        return None;
    }
    if fd.ty.results.as_ref().is_some_and(|r| !r.list.is_empty()) {
        return None;
    }
    let field = &params.list[0];
    if field.names.len() != 1 || field.names[0].name == "_" {
        return None;
    }
    let ty = field.ty.as_ref()?;
    let Expr::StarExpr(star) = ty else {
        return None;
    };
    let Expr::SelectorExpr(sel) = star.x.as_ref() else {
        return None;
    };
    let Expr::Ident(pkg) = sel.x.as_ref() else {
        return None;
    };
    if pkg.name != "testing" {
        return None;
    }
    let want = match test_kind_prefix(&fd.name.name)? {
        "Test" => "T",
        "Benchmark" => "B",
        "Fuzz" => "F",
        _ => return None,
    };
    if sel.sel.name != want {
        return None;
    }
    Some(field.names[0].name.as_str())
}

fn count_ident_uses(pass: &Pass<'_>, body: &BlockStmt, obj: ObjectId) -> usize {
    let mut n = 0;
    walk::inspect(NodeRef::BlockStmt(body), |node| {
        let Some(node) = node else {
            return true;
        };
        if let NodeRef::Ident(id) = node {
            if ident_obj(pass, id) == Some(obj) {
                n += 1;
            }
        }
        true
    });
    n
}

fn check_testingcontext_block(
    pass: &Pass<'_>,
    body: &BlockStmt,
    test_name: &str,
    pending: &mut Vec<Diagnostic>,
) {
    for i in 0..body.list.len().saturating_sub(1) {
        let Stmt::AssignStmt(assign) = &body.list[i] else {
            continue;
        };
        let Stmt::DeferStmt(defr) = &body.list[i + 1] else {
            continue;
        };
        if assign.tok != Some(Token::DEFINE) || assign.lhs.len() != 2 || assign.rhs.len() != 1 {
            continue;
        }
        let Expr::CallExpr(with_cancel) = &assign.rhs[0] else {
            continue;
        };
        if !code::is_call_to(pass, with_cancel, "context.WithCancel") || with_cancel.args.len() != 1
        {
            continue;
        }
        let Expr::CallExpr(bg) = &with_cancel.args[0] else {
            continue;
        };
        if !code::is_call_to_any(pass, bg, &["context.Background", "context.TODO"]) {
            continue;
        }
        let pos = with_cancel.fun.pos().0 as u32;
        if !go_at_least(pass, pos, "go1.24") {
            continue;
        }
        let Some(ctx_name) = ident_name(&assign.lhs[0]) else {
            continue;
        };
        let Expr::Ident(cancel_id) = &assign.lhs[1] else {
            continue;
        };
        if cancel_id.name == "_" {
            continue;
        }
        let Some(cancel_obj) = ident_obj(pass, cancel_id) else {
            continue;
        };
        // defer cancel() — sole use of cancel (assignment lhs + this call = 2).
        let Expr::Ident(defer_fun) = defr.call.fun.as_ref() else {
            continue;
        };
        if ident_obj(pass, defer_fun) != Some(cancel_obj) || !defr.call.args.is_empty() {
            continue;
        }
        if count_ident_uses(pass, body, cancel_obj) != 2 {
            continue;
        }
        pending.push(Diagnostic {
            pos,
            end: with_cancel.fun.end().0 as u32,
            category: String::new(),
            message: format!("context.WithCancel can be modernized using {test_name}.Context"),
            suggested_fixes: vec![SuggestedFix {
                message: format!("Replace context.WithCancel with {test_name}.Context"),
                text_edits: vec![TextEdit {
                    pos: assign.lhs[0].pos().0 as u32,
                    end: defr.call.end().0 as u32,
                    new_text: format!("{ctx_name} := {test_name}.Context()"),
                }],
            }],
            related: Vec::new(),
            url: String::new(),
            severity: String::new(),
        });
    }
}

fn check_testingcontext(pass: &Pass<'_>, file: &File, pending: &mut Vec<Diagnostic>) {
    for decl in &file.decls {
        let guff::ast::Decl::FuncDecl(fd) = decl else {
            continue;
        };
        let Some(t_name) = testing_param_name(fd) else {
            continue;
        };
        let Some(body) = fd.body.as_ref() else {
            continue;
        };
        check_testingcontext_block(pass, body, t_name, pending);
        // Also scan nested t.Run(..., func(t *testing.T) { ... }) bodies.
        walk::inspect(NodeRef::BlockStmt(body), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::FuncLit(fl) = n {
                if let Some(field) = fl.ty.params.as_ref().and_then(|p| p.list.first()) {
                    if field.names.len() == 1 && field.names[0].name != "_" {
                        if let Some(ty) = field.ty.as_ref() {
                            if let Expr::StarExpr(star) = ty {
                                if let Expr::SelectorExpr(sel) = star.x.as_ref() {
                                    if let Expr::Ident(pkg) = sel.x.as_ref() {
                                        if pkg.name == "testing"
                                            && matches!(sel.sel.name.as_str(), "T" | "B" | "F")
                                        {
                                            check_testingcontext_block(
                                                pass,
                                                &fl.body,
                                                field.names[0].name.as_str(),
                                                pending,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            true
        });
    }
}

fn is_basic_kind(pass: &Pass<'_>, typ: TypeId, kind: BasicKind) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    matches!(artifacts.types.get(typ), TypeData::Basic(b) if b.kind() == kind)
}

fn as_type_conversion<'a>(pass: &Pass<'_>, expr: &'a Expr) -> Option<(TypeId, &'a Expr)> {
    let expr = match expr {
        Expr::ParenExpr(p) => p.x.as_ref(),
        other => other,
    };
    let Expr::CallExpr(call) = expr else {
        return None;
    };
    if call.args.len() != 1 {
        return None;
    }
    let info = pass.types_info()?;
    let tav = info.types.get(&call.fun.id())?;
    if tav.mode != OperandMode::TypeExpr {
        return None;
    }
    Some((tav.typ, &call.args[0]))
}

/// `unsafe.Pointer(uintptr(ptr) + offset)` → `unsafe.Add(ptr, offset)` (Go 1.17+).
fn check_unsafefuncs(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<Diagnostic>) {
    if call.args.len() != 1 {
        return;
    }
    let Some(info) = pass.types_info() else {
        return;
    };
    let Some(tav) = info.types.get(&call.fun.id()) else {
        return;
    };
    if tav.mode != OperandMode::TypeExpr {
        return;
    }
    if !is_basic_kind(pass, tav.typ, BasicKind::UnsafePointer) {
        return;
    }
    let pos = call.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.17") {
        return;
    }
    let Expr::BinaryExpr(sum) = &call.args[0] else {
        return;
    };
    if sum.op != Token::ADD {
        return;
    }
    let Some(x_ty) = type_of(pass, &sum.x) else {
        return;
    };
    if !is_basic_kind(pass, x_ty, BasicKind::Uintptr) {
        return;
    }
    let Some((_, ptr_expr)) = as_type_conversion(pass, &sum.x) else {
        return;
    };
    let Some(ptr_ty) = type_of(pass, ptr_expr) else {
        return;
    };
    if !is_basic_kind(pass, ptr_ty, BasicKind::UnsafePointer) {
        return;
    }
    let Some(ptr_text) = expr_text(ptr_expr) else {
        return;
    };
    // Drop uintptr(...) around the offset when the conversion target is uintptr.
    let offset_expr = match as_type_conversion(pass, &sum.y) {
        Some((t, inner)) if is_basic_kind(pass, t, BasicKind::Uintptr) => {
            let artifacts = pass.pkg().type_artifacts.as_ref();
            let ok = artifacts.is_some_and(|a| {
                matches!(inner, Expr::BasicLit(_))
                    || type_of(pass, inner).is_some_and(|it| {
                        let under = unalias_readonly(&a.types, it).underlying(&a.types);
                        is_integer(&a.types, under)
                    })
            });
            if ok {
                inner
            } else {
                sum.y.as_ref()
            }
        }
        _ => sum.y.as_ref(),
    };
    let Some(offset_text) = expr_text(offset_expr) else {
        return;
    };
    let pos = sum.x.pos().0 as u32;
    let end = sum.y.end().0 as u32;
    pending.push(Diagnostic {
        pos,
        end,
        category: String::new(),
        message: "pointer + integer can be simplified using unsafe.Add".into(),
        suggested_fixes: vec![SuggestedFix {
            message: "Simplify pointer addition using unsafe.Add".into(),
            text_edits: vec![TextEdit {
                pos: call.pos().0 as u32,
                end: call.end().0 as u32,
                new_text: format!("unsafe.Add({ptr_text}, {offset_text})"),
            }],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
}

fn is_import_comment(text: &str) -> bool {
    let text = text.trim();
    text.starts_with("import \"") && text.ends_with('"')
}

/// Obsolete `package p // import "path"` comments (ignored in module mode).
///
/// Package-line trailing comments are dropped unless `PARSE_COMMENTS` is set,
/// so we re-parse like gocritic's comment checkers.
fn check_importcomment(pass: &Pass<'_>, file_idx: usize, file: &File, pending: &mut Vec<Diagnostic>) {
    // DEFERRED: skip when Package.module is None (GOPATH mode), matching upstream.
    let Some(path) = pass.pkg().compiled_go_files.get(file_idx) else {
        return;
    };
    let Ok(src) = fs::read(path) else {
        return;
    };
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return;
    };
    let re_fset = FileSet::new();
    let Ok(parsed) = parse_file(&re_fset, name, &src, PARSE_COMMENTS) else {
        return;
    };
    let pkg_end = parsed.name.end();
    let pkg_line = re_fset.position(pkg_end).line;
    let Some(ft) = pass.fset().file(file.pos()) else {
        return;
    };
    for group in &parsed.comments {
        if group.list.len() != 1 {
            continue;
        }
        let c = &group.list[0];
        if c.pos().0 < pkg_end.0 {
            continue;
        }
        let comment_line = re_fset.position(c.pos()).line;
        if comment_line > pkg_line {
            break;
        }
        if comment_line != pkg_line {
            continue;
        }
        let text = CommentGroup {
            list: group.list.clone(),
        }
        .text();
        if !is_import_comment(&text) {
            continue;
        }
        if pkg_line < 1 || pkg_line as usize > ft.line_count() {
            continue;
        }
        let re_pos = re_fset.position(c.pos());
        let mapped_start =
            ft.line_start(pkg_line as usize).0 as u32 + (re_pos.column as u32).saturating_sub(1);
        let mapped_end = mapped_start + (c.end().0 - c.pos().0) as u32;
        pending.push(Diagnostic {
            pos: mapped_start,
            end: mapped_end,
            category: String::new(),
            message: "canonical import path comment is ignored in module mode".into(),
            suggested_fixes: vec![SuggestedFix {
                message: "Remove obsolete import path comment".into(),
                text_edits: vec![TextEdit {
                    pos: file.name.end().0 as u32,
                    end: mapped_end,
                    new_text: String::new(),
                }],
            }],
            related: Vec::new(),
            url: String::new(),
            severity: String::new(),
        });
    }
}

/// `x := strings.Split(N)(s, sep[, 2])[0]` → `x, _, _ := strings.Cut(s, sep)`.
fn check_stringscut(pass: &Pass<'_>, assign: &AssignStmt, pending: &mut Vec<Diagnostic>) {
    if assign.tok != Some(Token::DEFINE) || assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return;
    }
    let Some(lhs_name) = ident_name(&assign.lhs[0]) else {
        return;
    };
    if lhs_name == "_" {
        return;
    }
    let Expr::IndexExpr(ix) = &assign.rhs[0] else {
        return;
    };
    if !code::is_integer_literal(pass, &ix.index, 0) {
        return;
    }
    let Expr::CallExpr(call) = ix.x.as_ref() else {
        return;
    };
    let (pkg, split_name, need_n) = if code::is_call_to(pass, call, "strings.Split") {
        ("strings", "Split", false)
    } else if code::is_call_to(pass, call, "strings.SplitN") {
        ("strings", "SplitN", true)
    } else if code::is_call_to(pass, call, "bytes.Split") {
        ("bytes", "Split", false)
    } else if code::is_call_to(pass, call, "bytes.SplitN") {
        ("bytes", "SplitN", true)
    } else {
        return;
    };
    if need_n {
        if call.args.len() != 3 || !code::is_integer_literal(pass, &call.args[2], 2) {
            return;
        }
    } else if call.args.len() != 2 {
        return;
    }
    // strings: require non-empty constant separator; bytes: allow any (often []byte lit).
    if pkg == "strings" {
        let Some(sep) = code::expr_to_string(pass, &call.args[1]) else {
            return;
        };
        if sep.is_empty() {
            return;
        }
    }
    let pos = call.fun.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.18") {
        return;
    }
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return; // e.g. dot-import
    };
    let mut text_edits = vec![
        TextEdit {
            pos: assign.lhs[0].end().0 as u32,
            end: assign.lhs[0].end().0 as u32,
            new_text: ", _, _".into(),
        },
        TextEdit {
            pos: sel.sel.pos().0 as u32,
            end: sel.sel.end().0 as u32,
            new_text: "Cut".into(),
        },
        TextEdit {
            pos: ix.lbrack.0 as u32,
            end: ix.rbrack.0 as u32 + 1,
            new_text: String::new(),
        },
    ];
    if need_n {
        text_edits.push(TextEdit {
            pos: call.args[1].end().0 as u32,
            end: call.rparen.0 as u32,
            new_text: String::new(),
        });
    }
    pending.push(Diagnostic {
        pos,
        end: call.fun.end().0 as u32,
        category: "stringscut".into(),
        message: format!("{pkg}.{split_name} call can be simplified using {pkg}.Cut"),
        suggested_fixes: vec![SuggestedFix {
            message: format!("Replace {pkg}.{split_name} with {pkg}.Cut"),
            text_edits,
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
}

/// Fact marking a function as "new-like": `func f(x T) *T { return &x }`.
#[derive(Clone, Debug, Default)]
struct NewLikeFact;

impl Fact for NewLikeFact {
    fn fact_type_id(&self) -> FactTypeId {
        FactTypeId::of::<Self>()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn clone_fact(&self) -> Box<dyn Fact> {
        Box::new(self.clone())
    }
}

fn is_pointer_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    matches!(artifacts.types.get(typ), TypeData::Pointer(_))
}

fn type_name_of(pass: &Pass<'_>, typ: TypeId) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    Some(type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    ))
}

fn has_go_fix_inline(doc: &Option<CommentGroup>) -> bool {
    let Some(doc) = doc else {
        return false;
    };
    for c in &doc.list {
        let text = c.text.trim();
        // Match `//go:fix inline` (and block-comment equivalents).
        let stripped = text
            .trim_start_matches("//")
            .trim_start_matches("/*")
            .trim_end_matches("*/")
            .trim();
        if stripped.starts_with("go:fix") && stripped.contains("inline") {
            return true;
        }
    }
    false
}

fn newlike_unary<'a>(body: &'a BlockStmt) -> Option<&'a UnaryExpr> {
    if body.list.len() != 1 {
        return None;
    }
    let Stmt::ReturnStmt(ret) = &body.list[0] else {
        return None;
    };
    if ret.results.len() != 1 {
        return None;
    }
    let Expr::UnaryExpr(u) = &ret.results[0] else {
        return None;
    };
    if u.op != Token::AND {
        return None;
    }
    Some(u)
}

fn call_callee_object(pass: &Pass<'_>, fun: &Expr) -> Option<ObjectId> {
    match fun {
        Expr::Ident(id) => code::object_of(pass, id),
        Expr::SelectorExpr(sel) => code::object_of(pass, &sel.sel),
        Expr::IndexExpr(ix) => call_callee_object(pass, &ix.x),
        Expr::IndexListExpr(ix) => call_callee_object(pass, &ix.x),
        Expr::ParenExpr(p) => call_callee_object(pass, &p.x),
        _ => None,
    }
}

/// Conservative stand-in for upstream's `types.CheckExpr` on untyped constants:
/// BasicLit defaults must match the pointer element type name, otherwise skip
/// (avoids false positives like `int64Var(123)` where TypesInfo already shows
/// the converted type).
fn untyped_lit_matches_elem(pass: &Pass<'_>, arg: &Expr, elem: TypeId) -> bool {
    let Expr::BasicLit(lit) = arg else {
        // DEFERRED: re-typecheck complex constant expressions via CheckExpr.
        return false;
    };
    let Some(elem_name) = type_name_of(pass, elem) else {
        return false;
    };
    match lit.kind {
        Some(Token::INT) => elem_name == "int",
        Some(Token::STRING) => elem_name == "string",
        Some(Token::FLOAT) => elem_name == "float64",
        Some(Token::CHAR) => elem_name == "rune" || elem_name == "int32",
        _ => false,
    }
}

fn newexpr_arg_ok(pass: &Pass<'_>, arg: &Expr, call_typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let call_typ = unalias_readonly(&artifacts.types, call_typ);
    if !matches!(artifacts.types.get(call_typ), TypeData::Pointer(_)) {
        return false;
    }
    let elem = pointer_elem(&artifacts.types, call_typ);
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(tav) = info.types.get(&arg.id()) else {
        return false;
    };
    if tav.val.is_some() {
        return untyped_lit_matches_elem(pass, arg, elem);
    }
    types_identical(pass, tav.typ, elem)
}

struct NewExprDeclCand {
    obj: ObjectId,
    name: String,
    name_pos: u32,
    name_end: u32,
    decl_pos: u32,
    amp_pos: u32,
    x_end: u32,
    need_inline_comment: bool,
    go126: bool,
}

fn collect_newexpr_decls(pass: &Pass<'_>) -> Vec<NewExprDeclCand> {
    let mut out = Vec::new();
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return out;
    };
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            let Some(body) = fd.body.as_ref() else {
                continue;
            };
            let Some(unary) = newlike_unary(body) else {
                continue;
            };
            let Expr::Ident(x_id) = unary.x.as_ref() else {
                continue;
            };
            let Some(fn_obj) = code::object_of(pass, &fd.name) else {
                continue;
            };
            if !matches!(artifacts.objects.get(fn_obj), ObjectData::Func(_)) {
                continue;
            }
            let Some(sig) = fn_obj.typ(&artifacts.objects) else {
                continue;
            };
            if signature_variadic(&artifacts.types, sig) {
                continue;
            }
            // Methods are fine; Params excludes the receiver.
            let Some(params) = signature_params(&artifacts.types, sig) else {
                continue;
            };
            let Some(results) = signature_results(&artifacts.types, sig) else {
                continue;
            };
            if tuple_len(&artifacts.types, Some(params)) != 1 {
                continue;
            }
            if tuple_len(&artifacts.types, Some(results)) != 1 {
                continue;
            }
            let param = tuple_at(&artifacts.types, params, 0);
            let result = tuple_at(&artifacts.types, results, 0);
            let Some(result_typ) = result.typ(&artifacts.objects) else {
                continue;
            };
            if !is_pointer_type(pass, result_typ) {
                continue;
            }
            let Some(x_obj) = ident_obj(pass, x_id) else {
                continue;
            };
            if x_obj != param {
                continue;
            }
            let pos = fd.name.pos().0 as u32;
            out.push(NewExprDeclCand {
                obj: fn_obj,
                name: fd.name.name.clone(),
                name_pos: pos,
                name_end: fd.name.end().0 as u32,
                decl_pos: fd.ty.func.0 as u32,
                amp_pos: unary.op_pos.0 as u32,
                x_end: unary.x.end().0 as u32,
                need_inline_comment: !has_go_fix_inline(&fd.doc),
                go126: go_at_least(pass, pos, "go1.26"),
            });
        }
    }
    out
}

fn export_newexpr_decls(
    pass: &mut Pass<'_>,
    cands: Vec<NewExprDeclCand>,
    pending: &mut Vec<Diagnostic>,
) {
    for c in cands {
        pass.export_object_fact(c.obj, Box::new(NewLikeFact));
        if !c.go126 {
            continue;
        }
        let mut text_edits = vec![
            TextEdit {
                pos: c.amp_pos,
                end: c.amp_pos + 1,
                new_text: "new(".into(),
            },
            TextEdit {
                pos: c.x_end,
                end: c.x_end,
                new_text: ")".into(),
            },
        ];
        if c.need_inline_comment {
            text_edits.push(TextEdit {
                pos: c.decl_pos,
                end: c.decl_pos,
                new_text: "//go:fix inline\n".into(),
            });
        }
        pending.push(Diagnostic {
            pos: c.name_pos,
            end: c.name_end,
            category: "newexpr".into(),
            message: format!("{} can be an inlinable wrapper around new(expr)", c.name),
            suggested_fixes: vec![SuggestedFix {
                message: format!("Make {} an inlinable wrapper around new(expr)", c.name),
                text_edits,
            }],
            related: Vec::new(),
            url: String::new(),
            severity: String::new(),
        });
    }
}

fn check_newexpr_call(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<Diagnostic>) {
    if call.args.len() != 1 {
        return;
    }
    let pos = call.pos().0 as u32;
    if !go_at_least(pass, pos, "go1.26") {
        return;
    }
    let Some(fn_obj) = call_callee_object(pass, &call.fun) else {
        return;
    };
    let mut fact = NewLikeFact;
    if !pass.import_object_fact(fn_obj, &mut fact) {
        return;
    }
    let Some(info) = pass.types_info() else {
        return;
    };
    let Some(tav) = info.types.get(&call.id) else {
        return;
    };
    if !newexpr_arg_ok(pass, &call.args[0], tav.typ) {
        return;
    }
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let fname = fn_obj.name(&artifacts.objects).to_string();
    pending.push(Diagnostic {
        pos,
        end: call.end().0 as u32,
        category: "newexpr".into(),
        message: format!("call of {fname}(x) can be simplified to new(x)"),
        suggested_fixes: vec![SuggestedFix {
            message: format!("Simplify {fname}(x) to new(x)"),
            text_edits: vec![TextEdit {
                pos: call.fun.pos().0 as u32,
                end: call.fun.end().0 as u32,
                new_text: "new".into(),
            }],
        }],
        related: Vec::new(),
        url: String::new(),
        severity: String::new(),
    });
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
    if enabled(&options, "newexpr") {
        let cands = collect_newexpr_decls(pass);
        export_newexpr_decls(pass, cands, &mut pending);
    }
    for (file_idx, file) in pass.files().iter().enumerate() {
        if enabled(&options, "plusbuild") && go_at_least(pass, file.package.0 as u32, "go1.18") {
            check_plusbuild(file, &mut pending);
        }
        if enabled(&options, "testingcontext") {
            check_testingcontext(pass, file, &mut pending);
        }
        if enabled(&options, "importcomment") {
            check_importcomment(pass, file_idx, file, &mut pending);
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
                NodeRef::ForStmt(s) => {
                    if enabled(&options, "rangeint") {
                        check_rangeint(pass, s, &mut pending);
                    }
                    if enabled(&options, "slicesbackward") {
                        check_slicesbackward(pass, s, &mut pending);
                    }
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
                NodeRef::AssignStmt(a) if enabled(&options, "stringscut") => {
                    check_stringscut(pass, a, &mut pending);
                }
                NodeRef::CallExpr(c) => {
                    if enabled(&options, "fmtappendf") {
                        check_fmtappendf(pass, c, &mut pending);
                    }
                    if enabled(&options, "slicessort") {
                        check_slicessort(pass, c, &mut pending);
                    }
                    if enabled(&options, "reflecttypefor") {
                        // Prefer Elem() special-case; plain TypeOf is handled when
                        // this call is not itself the X of a `.Elem()` selector.
                        check_reflecttypefor_elem(pass, c, &mut pending);
                        check_reflecttypefor(pass, c, &mut pending);
                    }
                    if enabled(&options, "unsafefuncs") {
                        check_unsafefuncs(pass, c, &mut pending);
                    }
                    if enabled(&options, "newexpr") {
                        check_newexpr_call(pass, c, &mut pending);
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
        fact_types: vec![FactTypeId::of::<NewLikeFact>()],
    })
}
