//! Pattern parser implementation.

use guff::token::Token;

use crate::lexer::{ItemType::*, Lexer};
use crate::pattern::{collect_entry_kinds, collect_root_call_symbols, *};

impl Parser {
    pub fn parse(&mut self, s: &str) -> Result<Pattern, String> {
        self.input = s.to_string();
        self.bindings.clear();

        let mut lx = Lexer::new(s)?;
        let root = self.parse_root(&mut lx)?;

        if lx.peek().typ != Eof {
            return Err("unexpected token after end of pattern".into());
        }

        if self.bindings.len() > 64 {
            return Err("encountered more than 64 bindings".into());
        }

        let mut bindings = vec![String::new(); self.bindings.len()];
        for (name, idx) in &self.bindings {
            bindings[*idx] = name.clone();
        }

        let entry_kinds = collect_entry_kinds(&root);
        let root_call_symbols = collect_root_call_symbols(&root);

        Ok(Pattern {
            root,
            entry_kinds,
            bindings,
            root_call_symbols,
        })
    }

    fn parse_root(&mut self, lx: &mut Lexer) -> Result<Node, String> {
        if lx.accept(LeftParen).is_none() {
            return Err(lx.unexpected("'('"));
        }
        let typ = lx
            .accept(TypeName)
            .ok_or_else(|| lx.unexpected("Node type"))?;
        let mut objs = Vec::new();
        loop {
            if lx.accept(RightParen).is_some() {
                break;
            }
            lx.rewind();
            objs.push(self.object(lx)?);
        }
        let mut node = populate_node(&typ.val, objs, self.allow_type_info)?;
        if let Node::Binding(ref mut b) = node {
            b.idx = self.binding_index(&b.name);
        }
        Ok(node)
    }

    fn object(&mut self, lx: &mut Lexer) -> Result<Node, String> {
        let n = lx.next();
        match n.typ {
            LeftParen => {
                lx.rewind();
                let node = self.parse_root(lx)?;
                if lx.peek().typ == Colon {
                    lx.next();
                    let tail = self.object(lx)?;
                    return Ok(Node::List(List {
                        head: Some(Box::new(node)),
                        tail: Some(Box::new(tail)),
                    }));
                }
                Ok(node)
            }
            LeftBracket => {
                lx.rewind();
                self.array(lx)
            }
            Variable => {
                if n.val == "nil" {
                    if lx.peek().typ == Colon {
                        lx.next();
                        let tail = self.object(lx)?;
                        return Ok(Node::List(List {
                            head: Some(Box::new(Node::Nil)),
                            tail: Some(Box::new(tail)),
                        }));
                    }
                    return Ok(Node::Nil);
                }
                let name = n.val;
                let inner = if lx.peek().typ == At {
                    lx.next();
                    Some(Box::new(self.parse_root(lx)?))
                } else {
                    None
                };
                let b = Binding {
                    name: name.clone(),
                    node: inner,
                    idx: self.binding_index(&name),
                };
                if lx.peek().typ == Colon {
                    lx.next();
                    let tail = self.object(lx)?;
                    return Ok(Node::List(List {
                        head: Some(Box::new(Node::Binding(b))),
                        tail: Some(Box::new(tail)),
                    }));
                }
                Ok(Node::Binding(b))
            }
            Blank => {
                if lx.peek().typ == Colon {
                    lx.next();
                    let tail = self.object(lx)?;
                    return Ok(Node::List(List {
                        head: Some(Box::new(Node::Any)),
                        tail: Some(Box::new(tail)),
                    }));
                }
                Ok(Node::Any)
            }
            ItemString => Ok(Node::String(n.val)),
            _ => Err(lx.unexpected("object")),
        }
    }

    fn array(&mut self, lx: &mut Lexer) -> Result<Node, String> {
        if lx.accept(LeftBracket).is_none() {
            return Err(lx.unexpected("'['"));
        }
        let mut objs = Vec::new();
        loop {
            if lx.accept(RightBracket).is_some() {
                break;
            }
            lx.rewind();
            objs.push(self.object(lx)?);
        }
        let mut tail = List {
            head: None,
            tail: None,
        };
        for obj in objs.into_iter().rev() {
            tail = List {
                head: Some(Box::new(obj)),
                tail: Some(Box::new(Node::List(tail))),
            };
        }
        Ok(Node::List(tail))
    }
}

fn box_node(n: Node) -> Box<Node> {
    Box::new(n)
}

fn take1(nodes: Vec<Node>) -> Result<Box<Node>, String> {
    nodes
        .into_iter()
        .next()
        .map(box_node)
        .ok_or_else(|| "missing argument".into())
}

fn take2(nodes: Vec<Node>) -> Result<(Box<Node>, Box<Node>), String> {
    let mut it = nodes.into_iter();
    let a = it.next().ok_or("missing argument")?;
    let b = it.next().ok_or("missing argument")?;
    Ok((box_node(a), box_node(b)))
}

fn take3(nodes: Vec<Node>) -> Result<(Box<Node>, Box<Node>, Box<Node>), String> {
    let mut it = nodes.into_iter();
    let a = it.next().ok_or("missing argument")?;
    let b = it.next().ok_or("missing argument")?;
    let c = it.next().ok_or("missing argument")?;
    Ok((box_node(a), box_node(b), box_node(c)))
}

fn take4(nodes: Vec<Node>) -> Result<(Box<Node>, Box<Node>, Box<Node>, Box<Node>), String> {
    let mut it = nodes.into_iter();
    let a = it.next().ok_or("missing argument")?;
    let b = it.next().ok_or("missing argument")?;
    let c = it.next().ok_or("missing argument")?;
    let d = it.next().ok_or("missing argument")?;
    Ok((box_node(a), box_node(b), box_node(c), box_node(d)))
}

fn take5(
    nodes: Vec<Node>,
) -> Result<(Box<Node>, Box<Node>, Box<Node>, Box<Node>, Box<Node>), String> {
    let mut it = nodes.into_iter();
    let a = it.next().ok_or("missing argument")?;
    let b = it.next().ok_or("missing argument")?;
    let c = it.next().ok_or("missing argument")?;
    let d = it.next().ok_or("missing argument")?;
    let e = it.next().ok_or("missing argument")?;
    Ok((box_node(a), box_node(b), box_node(c), box_node(d), box_node(e)))
}

fn populate_node(typ: &str, objs: Vec<Node>, allow_type_info: bool) -> Result<Node, String> {
    if !allow_type_info {
        match typ {
            "Symbol" | "Builtin" | "Object" | "IntegerLiteral" | "TrulyConstantExpression" => {
                return Err(format!("Node {typ} requires type information"));
            }
            _ => {}
        }
    }

    match typ {
        "Any" => Ok(Node::Any),
        "Nil" => Ok(Node::Nil),
        "Or" => Ok(Node::Or(Or { nodes: objs })),
        "Not" => {
            let node = take1(objs)?;
            Ok(Node::Not(Not { node }))
        }
        "Binding" => {
            if objs.len() != 2 {
                return Err(format!(
                    "tried to initialize node Binding with {} values, expected 2",
                    objs.len()
                ));
            }
            let mut it = objs.into_iter();
            let name = match it.next().unwrap() {
                Node::String(s) => s,
                other => return Err(format!("Binding name must be string, got {other:?}")),
            };
            let node = it.next().map(box_node);
            Ok(Node::Binding(Binding {
                name,
                node,
                idx: 0,
            }))
        }
        "Ellipsis" => {
            let elt = take1(objs)?;
            Ok(Node::Ellipsis(Ellipsis { elt }))
        }
        "Builtin" => {
            let name = take1(objs)?;
            Ok(Node::Builtin(Builtin { name }))
        }
        "Object" => {
            let name = take1(objs)?;
            Ok(Node::Object(Object { name }))
        }
        "Symbol" => {
            let name = take1(objs)?;
            Ok(Node::Symbol(Symbol { name }))
        }
        "IntegerLiteral" => {
            let value = take1(objs)?;
            Ok(Node::IntegerLiteral(IntegerLiteral { value }))
        }
        "TrulyConstantExpression" => {
            let value = take1(objs)?;
            Ok(Node::TrulyConstantExpression(TrulyConstantExpression { value }))
        }
        "RangeStmt" => {
            let (key, value, tok, x, body) = take5(objs)?;
            Ok(Node::RangeStmt(RangeStmt {
                key,
                value,
                tok,
                x,
                body,
            }))
        }
        "AssignStmt" => {
            let (lhs, tok, rhs) = take3(objs)?;
            Ok(Node::AssignStmt(AssignStmt { lhs, tok, rhs }))
        }
        "IndexExpr" => {
            let (x, index) = take2(objs)?;
            Ok(Node::IndexExpr(IndexExpr { x, index }))
        }
        "IndexListExpr" => {
            let (x, indices) = take2(objs)?;
            Ok(Node::IndexListExpr(IndexListExpr { x, indices }))
        }
        "Ident" => {
            let name = take1(objs)?;
            Ok(Node::Ident(Ident { name }))
        }
        "ValueSpec" => {
            let (names, ty, values) = take3(objs)?;
            Ok(Node::ValueSpec(ValueSpec { names, ty, values }))
        }
        "GenDecl" => {
            let (tok, specs) = take2(objs)?;
            Ok(Node::GenDecl(GenDecl { tok, specs }))
        }
        "BinaryExpr" => {
            let (x, op, y) = take3(objs)?;
            Ok(Node::BinaryExpr(BinaryExpr { x, op, y }))
        }
        "ForStmt" => {
            let (init, cond, post, body) = take4(objs)?;
            Ok(Node::ForStmt(ForStmt {
                init,
                cond,
                post,
                body,
            }))
        }
        "ArrayType" => {
            let (len, elt) = take2(objs)?;
            Ok(Node::ArrayType(ArrayType { len, elt }))
        }
        "DeferStmt" => {
            let call = take1(objs)?;
            Ok(Node::DeferStmt(DeferStmt { call }))
        }
        "MapType" => {
            let (key, value) = take2(objs)?;
            Ok(Node::MapType(MapType { key, value }))
        }
        "ReturnStmt" => {
            let results = take1(objs)?;
            Ok(Node::ReturnStmt(ReturnStmt { results }))
        }
        "SliceExpr" => {
            let (x, low, high, max) = take4(objs)?;
            Ok(Node::SliceExpr(SliceExpr { x, low, high, max }))
        }
        "StarExpr" => {
            let x = take1(objs)?;
            Ok(Node::StarExpr(StarExpr { x }))
        }
        "UnaryExpr" => {
            let (op, x) = take2(objs)?;
            Ok(Node::UnaryExpr(UnaryExpr { op, x }))
        }
        "SendStmt" => {
            let (chan_, value) = take2(objs)?;
            Ok(Node::SendStmt(SendStmt { chan_, value }))
        }
        "SelectStmt" => {
            let body = take1(objs)?;
            Ok(Node::SelectStmt(SelectStmt { body }))
        }
        "ImportSpec" => {
            let (name, path) = take2(objs)?;
            Ok(Node::ImportSpec(ImportSpec { name, path }))
        }
        "IfStmt" => {
            let (init, cond, body, else_) = take4(objs)?;
            Ok(Node::IfStmt(IfStmt {
                init,
                cond,
                body,
                else_,
            }))
        }
        "GoStmt" => {
            let call = take1(objs)?;
            Ok(Node::GoStmt(GoStmt { call }))
        }
        "Field" => {
            let (names, ty, tag) = take3(objs)?;
            Ok(Node::Field(Field { names, ty, tag }))
        }
        "SelectorExpr" => {
            let (x, sel) = take2(objs)?;
            Ok(Node::SelectorExpr(SelectorExpr { x, sel }))
        }
        "StructType" => {
            let fields = take1(objs)?;
            Ok(Node::StructType(StructType { fields }))
        }
        "KeyValueExpr" => {
            let (key, value) = take2(objs)?;
            Ok(Node::KeyValueExpr(KeyValueExpr { key, value }))
        }
        "FuncType" => {
            let (params, results) = take2(objs)?;
            Ok(Node::FuncType(FuncType { params, results }))
        }
        "FuncLit" => {
            let (ty, body) = take2(objs)?;
            Ok(Node::FuncLit(FuncLit { ty, body }))
        }
        "FuncDecl" => {
            let (recv, name, ty, body) = take4(objs)?;
            Ok(Node::FuncDecl(FuncDecl {
                recv,
                name,
                ty,
                body,
            }))
        }
        "ChanType" => {
            let (dir, value) = take2(objs)?;
            Ok(Node::ChanType(ChanType { dir, value }))
        }
        "CallExpr" => {
            let (fun, args) = take2(objs)?;
            Ok(Node::CallExpr(CallExpr { fun, args }))
        }
        "CaseClause" => {
            let (list, body) = take2(objs)?;
            Ok(Node::CaseClause(CaseClause { list, body }))
        }
        "CommClause" => {
            let (comm, body) = take2(objs)?;
            Ok(Node::CommClause(CommClause { comm, body }))
        }
        "CompositeLit" => {
            let (ty, elts) = take2(objs)?;
            Ok(Node::CompositeLit(CompositeLit { ty, elts }))
        }
        "EmptyStmt" => Ok(Node::EmptyStmt(EmptyStmt {})),
        "SwitchStmt" => {
            let (init, tag, body) = take3(objs)?;
            Ok(Node::SwitchStmt(SwitchStmt { init, tag, body }))
        }
        "TypeSwitchStmt" => {
            let (init, assign, body) = take3(objs)?;
            Ok(Node::TypeSwitchStmt(TypeSwitchStmt {
                init,
                assign,
                body,
            }))
        }
        "TypeAssertExpr" => {
            let (x, ty) = take2(objs)?;
            Ok(Node::TypeAssertExpr(TypeAssertExpr { x, ty }))
        }
        "TypeSpec" => {
            let (name, ty) = take2(objs)?;
            Ok(Node::TypeSpec(TypeSpec { name, ty }))
        }
        "InterfaceType" => {
            let methods = take1(objs)?;
            Ok(Node::InterfaceType(InterfaceType { methods }))
        }
        "BranchStmt" => {
            let (tok, label) = take2(objs)?;
            Ok(Node::BranchStmt(BranchStmt { tok, label }))
        }
        "IncDecStmt" => {
            let (x, tok) = take2(objs)?;
            Ok(Node::IncDecStmt(IncDecStmt { x, tok }))
        }
        "BasicLit" => {
            let (kind, value) = take2(objs)?;
            Ok(Node::BasicLit(BasicLit { kind, value }))
        }
        other => Err(format!("unknown node {other}")),
    }
}

pub(crate) fn token_from_str(s: &str) -> Option<Token> {
    Some(match s {
        "INT" => Token::INT,
        "FLOAT" => Token::FLOAT,
        "IMAG" => Token::IMAG,
        "CHAR" => Token::CHAR,
        "STRING" => Token::STRING,
        "+" => Token::ADD,
        "-" => Token::SUB,
        "*" => Token::MUL,
        "/" => Token::QUO,
        "%" => Token::REM,
        "&" => Token::AND,
        "|" => Token::OR,
        "^" => Token::XOR,
        "<<" => Token::SHL,
        ">>" => Token::SHR,
        "&^" => Token::AndNot,
        "+=" => Token::AddAssign,
        "-=" => Token::SubAssign,
        "*=" => Token::MulAssign,
        "/=" => Token::QuoAssign,
        "%=" => Token::RemAssign,
        "&=" => Token::AndAssign,
        "|=" => Token::OrAssign,
        "^=" => Token::XorAssign,
        "<<=" => Token::ShlAssign,
        ">>=" => Token::ShrAssign,
        "&^=" => Token::AndNotAssign,
        "&&" => Token::LAND,
        "||" => Token::LOR,
        "<-" => Token::ARROW,
        "++" => Token::INC,
        "--" => Token::DEC,
        "==" => Token::EQL,
        "<" => Token::LSS,
        ">" => Token::GTR,
        "=" => Token::ASSIGN,
        "!" => Token::NOT,
        "!=" => Token::NEQ,
        "<=" => Token::LEQ,
        ">=" => Token::GEQ,
        ":=" => Token::DEFINE,
        "..." => Token::ELLIPSIS,
        "IMPORT" => Token::IMPORT,
        "VAR" => Token::VAR,
        "TYPE" => Token::TYPE,
        "CONST" => Token::CONST,
        "BREAK" => Token::BREAK,
        "CONTINUE" => Token::CONTINUE,
        "GOTO" => Token::GOTO,
        "FALLTHROUGH" => Token::FALLTHROUGH,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_s1004_pattern() {
        let pat = Parser::new().parse(
            r#"(BinaryExpr (CallExpr (Symbol "bytes.Compare") args) op@(Or "==" "!=") (IntegerLiteral "0"))"#,
        );
        assert!(pat.is_ok(), "{:?}", pat.err());
        let pat = pat.unwrap();
        // Root is BinaryExpr, not CallExpr(Symbol…), so no root call symbols.
        assert!(pat.root_call_symbols.is_empty());
    }

    #[test]
    fn parse_ident_only() {
        let pat = Parser::new().parse(r#"(Ident "a")"#);
        assert!(pat.is_ok(), "{:?}", pat.err());
    }

    #[test]
    fn parse_or_and_list() {
        let pat = Parser::new()
            .parse(r#"(Or (Ident "a") (Ident "b"))"#)
            .unwrap();
        assert!(matches!(pat.root, Node::Or(_)));
    }

    #[test]
    fn root_call_symbols_for_symbol_call() {
        let pat = Parser::new()
            .parse(r#"(CallExpr (Symbol "errors.New") [x])"#)
            .unwrap();
        assert_eq!(pat.root_call_symbols.len(), 1);
        assert_eq!(pat.root_call_symbols[0].path, "errors");
        assert_eq!(pat.root_call_symbols[0].typename, "");
        assert_eq!(pat.root_call_symbols[0].ident, "New");
    }

    #[test]
    fn root_call_symbols_for_method() {
        let pat = Parser::new()
            .parse(r#"(CallExpr (Symbol "(time.Time).Sub") [x])"#)
            .unwrap();
        assert_eq!(pat.root_call_symbols.len(), 1);
        assert_eq!(pat.root_call_symbols[0].path, "time");
        assert_eq!(pat.root_call_symbols[0].typename, "Time");
        assert_eq!(pat.root_call_symbols[0].ident, "Sub");
    }

    #[test]
    fn root_call_symbols_for_or() {
        let pat = Parser::new()
            .parse(
                r#"(CallExpr (Symbol (Or "(*text/template.Template).Parse" "(*html/template.Template).Parse")) [s])"#,
            )
            .unwrap();
        assert_eq!(pat.root_call_symbols.len(), 2);
        assert_eq!(pat.root_call_symbols[0].path, "text/template");
        assert_eq!(pat.root_call_symbols[0].typename, "Template");
        assert_eq!(pat.root_call_symbols[1].path, "html/template");
    }
}
