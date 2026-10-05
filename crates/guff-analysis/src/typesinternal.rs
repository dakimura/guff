//! Ports from `golang.org/x/tools/internal/typesinternal`.

use guff::ast::Expr;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_types::arena::TypeData;
use guff_types::selection::SelectionKind;

use crate::Pass;

fn same(a: NodeRef<'_>, b: NodeRef<'_>) -> bool {
    a.erased_ptr() == b.erased_ptr()
}

fn is_expr(node: NodeRef<'_>, e: &Expr) -> bool {
    same(node, guff::walk::expr_ref(e))
}

/// `IsAssignedOrAddressTaken` (x/tools v0.50): whether the expression `node`
/// denotes a variable in a context that assigns it or takes its address —
/// `x = 1`, `x++`, `x[i] = 1` with `x` an array, `x.a[i] = 1`, `&x`, a method
/// call with a pointer receiver on an addressable `x`. A declaration
/// (`x := 1`, `var x int`) is not an assignment.
///
/// `ancestors` are the enclosing nodes from the root down to `node`'s parent,
/// as [`guff::walk::preorder_stack`] hands them out.
pub fn is_assigned_or_address_taken(
    pass: &Pass<'_>,
    node: NodeRef<'_>,
    ancestors: &[NodeRef<'_>],
) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let (types, objects) = (&artifacts.types, &artifacts.objects);
    let mut cur = node;
    let mut depth = ancestors.len();
    // Ascend to the outermost aggregate of which `node` is a part:
    //    x -> (x) | x.f | x[i] | x[i:j]
    while depth > 0 {
        let parent = ancestors[depth - 1];
        match parent {
            NodeRef::ParenExpr(p) if is_expr(cur, &p.x) => {}
            NodeRef::SelectorExpr(sel) if is_expr(cur, &sel.x) => {
                if let Some(seln) = info.selections.get(&sel.id) {
                    if seln.indirect() {
                        return false;
                    }
                    if seln.kind() == SelectionKind::MethodVal {
                        let recv_is_ptr = seln
                            .obj()
                            .typ(objects)
                            .and_then(|s| guff_types::signature::signature_recv(types, s))
                            .and_then(|r| r.typ(objects))
                            .is_some_and(|t| {
                                matches!(types.get(t.underlying(types)), TypeData::Pointer(_))
                            });
                        if recv_is_ptr {
                            // The receiver may be an embedded field: walk to
                            // the innermost type before the method.
                            let mut t = seln.recv();
                            let idx = seln.index();
                            for &i in &idx[..idx.len().saturating_sub(1)] {
                                let TypeData::Struct(s) = types.get(t.underlying(types)) else {
                                    break;
                                };
                                match s.field(i as usize).typ(objects) {
                                    Some(ft) => t = ft,
                                    None => break,
                                }
                            }
                            if !matches!(types.get(t.underlying(types)), TypeData::Pointer(_)) {
                                return true; // takes the receiver's address
                            }
                        }
                        return false;
                    }
                }
            }
            NodeRef::IndexExpr(ix) if is_expr(cur, &ix.x) => {
                if !is_array(pass, &ix.x) {
                    return false;
                }
            }
            NodeRef::SliceExpr(sl) if is_expr(cur, &sl.x) => {
                if !is_array(pass, &sl.x) {
                    return false;
                }
            }
            _ => break,
        }
        cur = parent;
        depth -= 1;
    }
    let Some(&parent) = depth.checked_sub(1).and_then(|d| ancestors.get(d)) else {
        return false;
    };
    match parent {
        NodeRef::AssignStmt(a) if a.lhs.iter().any(|l| is_expr(cur, l)) => {
            if a.tok != Some(Token::DEFINE) {
                return true; // x = j or x += j
            }
            // A re-assigned identifier of `x, y := 1, 2` is in Uses.
            matches!(cur, NodeRef::Ident(id) if info.uses.contains_key(&id.id))
        }
        NodeRef::RangeStmt(r)
            if [r.key.as_ref(), r.value.as_ref()]
                .into_iter()
                .flatten()
                .any(|e| is_expr(cur, e)) =>
        {
            r.tok == Some(Token::ASSIGN)
        }
        NodeRef::IncDecStmt(s) => is_expr(cur, &s.x),
        NodeRef::UnaryExpr(u) => u.op == Token::AND && is_expr(cur, &u.x),
        _ => false,
    }
}

fn is_array(pass: &Pass<'_>, e: &Expr) -> bool {
    let (Some(info), Some(artifacts)) = (pass.types_info(), pass.pkg().type_artifacts.as_ref())
    else {
        return false;
    };
    info.types.get(&e.id()).is_some_and(|tv| {
        matches!(artifacts.types.get(tv.typ.underlying(&artifacts.types)), TypeData::Array(_))
    })
}

/// `FileQualifier(f, pkg)`: a type is written relative to `pkg`, and another
/// package by the name this file imports it under — its renaming, nothing for
/// a dot import, else the package's own name.
pub fn file_qualifier(
    file: &guff::ast::File,
    here: guff_types::PackageId,
) -> impl Fn(guff_types::PackageId, &guff_types::PackageArena) -> String + '_ {
    move |p, packages| {
        if p == here {
            return String::new();
        }
        let path = packages.get(p).path();
        for imp in &file.imports {
            let Some(name) = imp.name.as_ref().filter(|n| n.name != "_") else {
                continue;
            };
            if imp.path.value.trim_matches(|c| c == '"' || c == '`') == path {
                return if name.name == "." { String::new() } else { name.name.clone() };
            }
        }
        packages.get(p).name().to_string()
    }
}
