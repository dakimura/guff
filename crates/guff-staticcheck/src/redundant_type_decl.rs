//! Shared body of ST1023 and QF1011 — a port of
//! `honnef.co/go/tools/internal/sharedcheck.RedundantTypeInDeclarationChecker`.
//!
//! Upstream decides whether the declared type is redundant by re-type-checking
//! the right-hand side *out of context* (`types.CheckExpr`) and asking two
//! questions of the result: is it untyped, and if so does its default type
//! match the declared one. guff has no standalone expression checker, so
//! [`isolated_type`] reconstructs the same answer from the AST and the
//! constants it can see.

use guff::ast::{Expr, Ident, Spec};
use guff::token::Token;
use guff_analysis::code::object_of;
use guff_analysis::{Diagnostic, Pass, SuggestedFix, TextEdit};
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::BasicKind;
use guff_types::predicates::identical;
use guff_types::TypeId;

use crate::render::render_expr;

fn types_identical(pass: &Pass<'_>, a: TypeId, b: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    identical(&mut types, &artifacts.objects, &artifacts.packages, a, b)
}

fn is_basic_kind(pass: &Pass<'_>, typ: TypeId, kind: BasicKind) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    matches!(artifacts.types.get(typ), TypeData::Basic(b) if b.kind() == kind)
}

/// The untyped kinds, in Go's widening order: an operation between untyped
/// constants takes the wider of its operands.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Untyped {
    Nil,
    Bool,
    String,
    Int,
    Rune,
    Float,
    Complex,
}

impl Untyped {
    /// `types.Default`, as the kind guff's arena stores plus the name Go gives
    /// it. The name matters: `types.Default` of an untyped rune is the *alias*
    /// `rune`, and upstream compares it to the declared type by identity, so
    /// `var v rune = 'a'` drops its type but `var v int32 = 'a'` keeps it.
    /// guff folds the alias into `int32`, so the difference has to come from
    /// how the declaration spells the type.
    fn default_type(self) -> Option<(BasicKind, &'static str)> {
        Some(match self {
            // Untyped nil has no default type, so it never matches.
            Untyped::Nil => return None,
            Untyped::Bool => (BasicKind::Bool, "bool"),
            Untyped::String => (BasicKind::String, "string"),
            Untyped::Int => (BasicKind::Int, "int"),
            Untyped::Rune => (BasicKind::Int32, "rune"),
            Untyped::Float => (BasicKind::Float64, "float64"),
            Untyped::Complex => (BasicKind::Complex128, "complex128"),
        })
    }
}

/// What the right-hand side is on its own, before the declaration converts it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Isolated {
    Untyped(Untyped),
    Typed,
}

fn untyped_of_basic(kind: BasicKind) -> Option<Untyped> {
    Some(match kind {
        BasicKind::UntypedBool => Untyped::Bool,
        BasicKind::UntypedString => Untyped::String,
        BasicKind::UntypedInt => Untyped::Int,
        BasicKind::UntypedRune => Untyped::Rune,
        BasicKind::UntypedFloat => Untyped::Float,
        BasicKind::UntypedComplex => Untyped::Complex,
        BasicKind::UntypedNil => Untyped::Nil,
        _ => return None,
    })
}

fn isolated_type(pass: &Pass<'_>, e: &Expr) -> Isolated {
    match e {
        Expr::BasicLit(lit) => match lit.kind {
            Some(Token::INT) => Isolated::Untyped(Untyped::Int),
            Some(Token::FLOAT) => Isolated::Untyped(Untyped::Float),
            Some(Token::IMAG) => Isolated::Untyped(Untyped::Complex),
            Some(Token::CHAR) => Isolated::Untyped(Untyped::Rune),
            Some(Token::STRING) => Isolated::Untyped(Untyped::String),
            _ => Isolated::Typed,
        },
        Expr::Ident(id) => isolated_ident(pass, id),
        Expr::SelectorExpr(se) => isolated_ident(pass, &se.sel),
        Expr::ParenExpr(p) => isolated_type(pass, &p.x),
        // `+x`, `-x`, `^x` and `!x` keep their operand's kind; `&x` and `<-ch`
        // are typed.
        Expr::UnaryExpr(u) => match u.op {
            Token::ADD | Token::SUB | Token::XOR | Token::NOT => isolated_type(pass, &u.x),
            _ => Isolated::Typed,
        },
        Expr::BinaryExpr(b) => {
            // A comparison yields an untyped bool however typed its operands
            // are, so `var ok bool = x == y` really is redundant.
            if matches!(
                b.op,
                Token::EQL | Token::NEQ | Token::LSS | Token::LEQ | Token::GTR | Token::GEQ
            ) {
                return Isolated::Untyped(Untyped::Bool);
            }
            // A shift takes the left operand's type; the count plays no part,
            // so `var n uint = 1 << uint(x)` is still an untyped int and needs
            // its `uint`.
            if matches!(b.op, Token::SHL | Token::SHR) {
                return isolated_type(pass, &b.x);
            }
            match (isolated_type(pass, &b.x), isolated_type(pass, &b.y)) {
                (Isolated::Untyped(l), Isolated::Untyped(r)) => Isolated::Untyped(l.max(r)),
                _ => Isolated::Typed,
            }
        }
        // A call to one of the constant builtins over untyped constant
        // arguments is itself an untyped constant. Everything else — a
        // conversion, an ordinary call, and `len("abc")`, whose result the spec
        // makes a *typed* `int` constant — is typed.
        Expr::CallExpr(call) => isolated_builtin_call(pass, call),
        _ => Isolated::Typed,
    }
}

/// The three groups of builtins the spec keeps untyped:
///
/// - `complex(a, b)` — "if the operands are untyped constants, the result is an
///   untyped complex constant";
/// - `real(z)` / `imag(z)` — untyped constant argument, untyped float result;
/// - `min` / `max` — all arguments untyped constants, result untyped of the
///   widest kind.
///
/// Upstream reads this off `types.CheckExpr`, which knows the spec. guff has to
/// name them. Without it `var c complex64 = complex(2, 3)` looked like a typed
/// right-hand side, the untyped branch never ran, and both the default-type
/// test and the expression-kind gate below were skipped — fiber's
/// `state_test.go` carries exactly that line.
fn isolated_builtin_call(pass: &Pass<'_>, call: &guff::ast::CallExpr) -> Isolated {
    let Expr::Ident(name) = unparen_expr(&call.fun) else {
        return Isolated::Typed;
    };
    if !is_builtin_object(pass, name) {
        return Isolated::Typed;
    }
    let args: Vec<Isolated> = call.args.iter().map(|a| isolated_type(pass, a)).collect();
    let all_untyped = |a: &[Isolated]| a.iter().all(|i| matches!(i, Isolated::Untyped(_)));
    match name.name.as_str() {
        "complex" if call.args.len() == 2 && all_untyped(&args) => {
            Isolated::Untyped(Untyped::Complex)
        }
        "real" | "imag" if call.args.len() == 1 && all_untyped(&args) => {
            Isolated::Untyped(Untyped::Float)
        }
        "min" | "max" if !call.args.is_empty() && all_untyped(&args) => {
            let widest = args
                .iter()
                .filter_map(|i| match i {
                    Isolated::Untyped(u) => Some(*u),
                    Isolated::Typed => None,
                })
                .max()
                .unwrap_or(Untyped::Int);
            Isolated::Untyped(widest)
        }
        _ => Isolated::Typed,
    }
}

fn unparen_expr(e: &Expr) -> &Expr {
    match e {
        Expr::ParenExpr(p) => unparen_expr(&p.x),
        other => other,
    }
}

/// The identifier denotes a predeclared builtin, not a function of the same
/// name declared in this package.
fn is_builtin_object(pass: &Pass<'_>, ident: &Ident) -> bool {
    let Some(obj) = object_of(pass, ident) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    matches!(artifacts.objects.get(obj), ObjectData::Builtin(_))
}

fn isolated_ident(pass: &Pass<'_>, ident: &Ident) -> Isolated {
    let Some(obj) = object_of(pass, ident) else {
        return Isolated::Typed;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return Isolated::Typed;
    };
    match artifacts.objects.get(obj) {
        ObjectData::Nil(_) => return Isolated::Untyped(Untyped::Nil),
        ObjectData::Const(_) => {}
        _ => return Isolated::Typed,
    }
    let Some(typ) = obj.typ(&artifacts.objects) else {
        return Isolated::Typed;
    };
    let TypeData::Basic(b) = artifacts.types.get(typ) else {
        return Isolated::Typed;
    };
    match untyped_of_basic(b.kind()) {
        Some(u) => Isolated::Untyped(u),
        None => Isolated::Typed,
    }
}

/// `Tlhs != types.Default(b)`, with the alias caveat from
/// [`Untyped::default_type`].
fn default_matches_lhs(pass: &Pass<'_>, u: Untyped, tlhs: TypeId, ty_expr: &Expr) -> bool {
    let Some((kind, name)) = u.default_type() else {
        return false;
    };
    matches!(ty_expr, Expr::Ident(id) if id.name == name) && is_basic_kind(pass, tlhs, kind)
}

fn object_has_package(pass: &Pass<'_>, ident: &Ident) -> bool {
    let Some(obj) = object_of(pass, ident) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    obj.pkg(&artifacts.objects).is_some()
}

/// One `var` declaration statement. `flag_helpful_types` is upstream's
/// parameter of the same name: QF1011 passes `true` and so also flags blank
/// identifiers, named constants and expressions, where ST1023 leaves the type
/// alone because it may aid the reader.
pub(crate) fn check_gen_decl(
    pass: &Pass<'_>,
    gen: &guff::ast::GenDecl,
    flag_helpful_types: bool,
    verb: &str,
    pending: &mut Vec<(u32, u32, String)>,
) {
    if gen.tok != Some(Token::VAR) {
        return;
    }
    'spec: for spec in &gen.specs {
        let Spec::ValueSpec(vs) = spec else {
            continue;
        };
        let Some(ty_expr) = &vs.ty else {
            continue;
        };
        if vs.names.len() != vs.values.len() {
            continue;
        }
        let info = match pass.types_info() {
            Some(i) => i,
            None => continue,
        };
        let Some(tlhs) = info.types.get(&ty_expr.id()).map(|tv| tv.typ) else {
            continue;
        };
        for (i, v) in vs.values.iter().enumerate() {
            if !flag_helpful_types && vs.names[i].name == "_" {
                continue 'spec;
            }
            let Some(trhs) = info.types.get(&v.id()).map(|tv| tv.typ) else {
                continue 'spec;
            };
            if !types_identical(pass, tlhs, trhs) {
                continue 'spec;
            }
            // Some expressions are untyped and get converted to the declared
            // type implicitly; the type is only redundant when it matches what
            // the right-hand side would have become on its own.
            if let Isolated::Untyped(u) = isolated_type(pass, v) {
                if !default_matches_lhs(pass, u, tlhs, ty_expr) {
                    continue 'spec;
                }
                match v {
                    // Named constants keep their type as a hint; predeclared
                    // ones (`true`, `iota`) have no package and do not.
                    Expr::Ident(id) => {
                        if !flag_helpful_types && object_has_package(pass, id) {
                            continue 'spec;
                        }
                    }
                    // Basic literals are always flagged.
                    Expr::BasicLit(_) => {}
                    // Anything else — a parenthesised literal, an arithmetic
                    // expression, a qualified constant — only when helpful
                    // types are wanted.
                    _ => {
                        if !flag_helpful_types {
                            continue 'spec;
                        }
                    }
                }
            }
        }
        // Upstream renders the type *expression* (honnef `report.Render`), not
        // the type: with `import t "time"`, `var d t.Duration` reports
        // `t.Duration`, and a local type reports its bare name. Verified
        // against golangci-lint 2.12.2.
        let typ_s = render_expr(ty_expr);
        pending.push((
            ty_expr.pos().0 as u32,
            ty_expr.end().0 as u32,
            format!(
                "{verb} omit type {typ_s} from declaration; it will be inferred from the right-hand side"
            ),
        ));
    }
}

pub(crate) fn report(pass: &mut Pass<'_>, pending: Vec<(u32, u32, String)>) {
    for (pos, end, message) in pending {
        pass.report(Diagnostic {
            pos,
            end,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Remove redundant type".into(),
                text_edits: vec![TextEdit {
                    pos,
                    end,
                    new_text: String::new(),
                }],
            }],
            ..Diagnostic::default()
        });
    }
}
