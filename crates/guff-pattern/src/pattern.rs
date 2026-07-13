//! Pattern AST node types.
//!
//! Port of `honnef.co/go/tools/pattern`.

use std::fmt;

use guff::token::Token;

/// A compiled pattern ready for matching.
#[derive(Debug, Clone)]
pub struct Pattern {
    pub root: Node,
    /// AST kind names (`"RangeStmt"`, …) that may initiate a match.
    pub entry_kinds: Vec<&'static str>,
    pub bindings: Vec<String>,
}

impl Pattern {
    /// Parse `s` and panic on error (for `lazy_static` / `OnceLock` init).
    pub fn must_parse(s: &str) -> Self {
        Parser::new().parse(s).unwrap_or_else(|e| {
            panic!("pattern parse error: {e}")
        })
    }
}

/// Any pattern node.
#[derive(Debug, Clone)]
pub enum Node {
    Any,
    Nil,
    String(String),
    Tok(Tok),
    Binding(Binding),
    List(List),
    Or(Or),
    Not(Not),
    Builtin(Builtin),
    Object(Object),
    Symbol(Symbol),
    IntegerLiteral(IntegerLiteral),
    TrulyConstantExpression(TrulyConstantExpression),
    Ellipsis(Ellipsis),
    RangeStmt(RangeStmt),
    AssignStmt(AssignStmt),
    IndexExpr(IndexExpr),
    IndexListExpr(IndexListExpr),
    Ident(Ident),
    ValueSpec(ValueSpec),
    GenDecl(GenDecl),
    BinaryExpr(BinaryExpr),
    ForStmt(ForStmt),
    ArrayType(ArrayType),
    DeferStmt(DeferStmt),
    MapType(MapType),
    ReturnStmt(ReturnStmt),
    SliceExpr(SliceExpr),
    StarExpr(StarExpr),
    UnaryExpr(UnaryExpr),
    SendStmt(SendStmt),
    SelectStmt(SelectStmt),
    ImportSpec(ImportSpec),
    IfStmt(IfStmt),
    GoStmt(GoStmt),
    Field(Field),
    SelectorExpr(SelectorExpr),
    StructType(StructType),
    KeyValueExpr(KeyValueExpr),
    FuncType(FuncType),
    FuncLit(FuncLit),
    FuncDecl(FuncDecl),
    ChanType(ChanType),
    CallExpr(CallExpr),
    CaseClause(CaseClause),
    CommClause(CommClause),
    CompositeLit(CompositeLit),
    EmptyStmt(EmptyStmt),
    SwitchStmt(SwitchStmt),
    TypeSwitchStmt(TypeSwitchStmt),
    TypeAssertExpr(TypeAssertExpr),
    TypeSpec(TypeSpec),
    InterfaceType(InterfaceType),
    BranchStmt(BranchStmt),
    IncDecStmt(IncDecStmt),
    BasicLit(BasicLit),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tok(pub Token);

#[derive(Debug, Clone)]
pub struct Binding {
    pub name: String,
    pub node: Option<Box<Node>>,
    pub idx: usize,
}

#[derive(Debug, Clone)]
pub struct List {
    pub head: Option<Box<Node>>,
    pub tail: Option<Box<Node>>,
}

#[derive(Debug, Clone)]
pub struct Or {
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct Not {
    pub node: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct Builtin {
    pub name: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct Object {
    pub name: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct IntegerLiteral {
    pub value: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct TrulyConstantExpression {
    pub value: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct Ellipsis {
    pub elt: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct RangeStmt {
    pub key: Box<Node>,
    pub value: Box<Node>,
    pub tok: Box<Node>,
    pub x: Box<Node>,
    pub body: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct AssignStmt {
    pub lhs: Box<Node>,
    pub tok: Box<Node>,
    pub rhs: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct IndexExpr {
    pub x: Box<Node>,
    pub index: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct IndexListExpr {
    pub x: Box<Node>,
    pub indices: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct Ident {
    pub name: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct ValueSpec {
    pub names: Box<Node>,
    pub ty: Box<Node>,
    pub values: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct GenDecl {
    pub tok: Box<Node>,
    pub specs: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct BinaryExpr {
    pub x: Box<Node>,
    pub op: Box<Node>,
    pub y: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct ForStmt {
    pub init: Box<Node>,
    pub cond: Box<Node>,
    pub post: Box<Node>,
    pub body: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct ArrayType {
    pub len: Box<Node>,
    pub elt: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct DeferStmt {
    pub call: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct MapType {
    pub key: Box<Node>,
    pub value: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct ReturnStmt {
    pub results: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct SliceExpr {
    pub x: Box<Node>,
    pub low: Box<Node>,
    pub high: Box<Node>,
    pub max: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct StarExpr {
    pub x: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct UnaryExpr {
    pub op: Box<Node>,
    pub x: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct SendStmt {
    pub chan_: Box<Node>,
    pub value: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct SelectStmt {
    pub body: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct ImportSpec {
    pub name: Box<Node>,
    pub path: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub init: Box<Node>,
    pub cond: Box<Node>,
    pub body: Box<Node>,
    pub else_: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct GoStmt {
    pub call: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub names: Box<Node>,
    pub ty: Box<Node>,
    pub tag: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct SelectorExpr {
    pub x: Box<Node>,
    pub sel: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct StructType {
    pub fields: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct KeyValueExpr {
    pub key: Box<Node>,
    pub value: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct FuncType {
    pub params: Box<Node>,
    pub results: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct FuncLit {
    pub ty: Box<Node>,
    pub body: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct FuncDecl {
    pub recv: Box<Node>,
    pub name: Box<Node>,
    pub ty: Box<Node>,
    pub body: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct ChanType {
    pub dir: Box<Node>,
    pub value: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct CallExpr {
    pub fun: Box<Node>,
    pub args: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct CaseClause {
    pub list: Box<Node>,
    pub body: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct CommClause {
    pub comm: Box<Node>,
    pub body: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct CompositeLit {
    pub ty: Box<Node>,
    pub elts: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct EmptyStmt {}

#[derive(Debug, Clone)]
pub struct SwitchStmt {
    pub init: Box<Node>,
    pub tag: Box<Node>,
    pub body: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct TypeSwitchStmt {
    pub init: Box<Node>,
    pub assign: Box<Node>,
    pub body: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct TypeAssertExpr {
    pub x: Box<Node>,
    pub ty: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct TypeSpec {
    pub name: Box<Node>,
    pub ty: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct InterfaceType {
    pub methods: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct BranchStmt {
    pub tok: Box<Node>,
    pub label: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct IncDecStmt {
    pub x: Box<Node>,
    pub tok: Box<Node>,
}

#[derive(Debug, Clone)]
pub struct BasicLit {
    pub kind: Box<Node>,
    pub value: Box<Node>,
}

/// Pattern parser.
pub struct Parser {
    pub allow_type_info: bool,
    pub(crate) input: String,
    pub(crate) pos: usize,
    pub(crate) start: usize,
    pub(crate) bindings: std::collections::HashMap<String, usize>,
}

impl Parser {
    pub fn new() -> Self {
        Self {
            allow_type_info: true,
            input: String::new(),
            pos: 0,
            start: 0,
            bindings: std::collections::HashMap::new(),
        }
    }

    pub(crate) fn binding_index(&mut self, name: &str) -> usize {
        if let Some(&idx) = self.bindings.get(name) {
            return idx;
        }
        let idx = self.bindings.len();
        self.bindings.insert(name.to_string(), idx);
        idx
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

pub fn must_parse(s: &str) -> Pattern {
    Pattern::must_parse(s)
}

pub(crate) fn collect_entry_kinds(node: &Node) -> Vec<&'static str> {
    let mut set = std::collections::HashSet::new();
    collect_entry_kinds_rec(node, &mut set);
    let mut out: Vec<_> = set.into_iter().collect();
    out.sort_unstable();
    out
}

fn collect_entry_kinds_rec(node: &Node, set: &mut std::collections::HashSet<&'static str>) {
    match node {
        Node::Or(or) => {
            for n in &or.nodes {
                collect_entry_kinds_rec(n, set);
            }
        }
        Node::Not(n) => collect_entry_kinds_rec(&n.node, set),
        Node::Binding(b) => {
            if let Some(ref inner) = b.node {
                collect_entry_kinds_rec(inner, set);
            } else {
                for k in ALL_ENTRY_KINDS {
                    set.insert(k);
                }
            }
        }
        Node::Nil | Node::Any | Node::String(_) | Node::Tok(_) => {}
        other => {
            if let Some(kind) = node_kind_name(other) {
                set.insert(kind);
            }
        }
    }
}

pub(crate) fn node_kind_name(node: &Node) -> Option<&'static str> {
    Some(match node {
        Node::RangeStmt(_) => "RangeStmt",
        Node::AssignStmt(_) => "AssignStmt",
        Node::IndexExpr(_) => "IndexExpr",
        Node::IndexListExpr(_) => "IndexListExpr",
        Node::Ident(_) => "Ident",
        Node::ValueSpec(_) => "ValueSpec",
        Node::GenDecl(_) => "GenDecl",
        Node::BinaryExpr(_) => "BinaryExpr",
        Node::ForStmt(_) => "ForStmt",
        Node::ArrayType(_) => "ArrayType",
        Node::DeferStmt(_) => "DeferStmt",
        Node::MapType(_) => "MapType",
        Node::ReturnStmt(_) => "ReturnStmt",
        Node::SliceExpr(_) => "SliceExpr",
        Node::StarExpr(_) => "StarExpr",
        Node::UnaryExpr(_) => "UnaryExpr",
        Node::SendStmt(_) => "SendStmt",
        Node::SelectStmt(_) => "SelectStmt",
        Node::ImportSpec(_) => "ImportSpec",
        Node::IfStmt(_) => "IfStmt",
        Node::GoStmt(_) => "GoStmt",
        Node::Field(_) => "Field",
        Node::SelectorExpr(_) => "SelectorExpr",
        Node::StructType(_) => "StructType",
        Node::KeyValueExpr(_) => "KeyValueExpr",
        Node::FuncType(_) => "FuncType",
        Node::FuncLit(_) => "FuncLit",
        Node::FuncDecl(_) => "FuncDecl",
        Node::ChanType(_) => "ChanType",
        Node::CallExpr(_) => "CallExpr",
        Node::CaseClause(_) => "CaseClause",
        Node::CommClause(_) => "CommClause",
        Node::CompositeLit(_) => "CompositeLit",
        Node::EmptyStmt(_) => "EmptyStmt",
        Node::SwitchStmt(_) => "SwitchStmt",
        Node::TypeSwitchStmt(_) => "TypeSwitchStmt",
        Node::TypeAssertExpr(_) => "TypeAssertExpr",
        Node::TypeSpec(_) => "TypeSpec",
        Node::InterfaceType(_) => "InterfaceType",
        Node::BranchStmt(_) => "BranchStmt",
        Node::IncDecStmt(_) => "IncDecStmt",
        Node::BasicLit(_) => "BasicLit",
        Node::IntegerLiteral(_) => "BasicLit",
        Node::TrulyConstantExpression(_) => return None,
        _ => return None,
    })
}

pub(crate) const ALL_ENTRY_KINDS: &[&str] = &[
    "RangeStmt",
    "AssignStmt",
    "IndexExpr",
    "IndexListExpr",
    "Ident",
    "ValueSpec",
    "GenDecl",
    "BinaryExpr",
    "ForStmt",
    "ArrayType",
    "DeferStmt",
    "MapType",
    "ReturnStmt",
    "SliceExpr",
    "StarExpr",
    "UnaryExpr",
    "SendStmt",
    "SelectStmt",
    "ImportSpec",
    "IfStmt",
    "GoStmt",
    "Field",
    "SelectorExpr",
    "StructType",
    "KeyValueExpr",
    "FuncType",
    "FuncLit",
    "FuncDecl",
    "ChanType",
    "CallExpr",
    "CaseClause",
    "CommClause",
    "CompositeLit",
    "EmptyStmt",
    "SwitchStmt",
    "TypeSwitchStmt",
    "TypeAssertExpr",
    "TypeSpec",
    "InterfaceType",
    "BranchStmt",
    "IncDecStmt",
    "BasicLit",
];

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_binding_forms() {
        let mut p = Parser::new();
        for input in [
            r#"(Binding "name" _)"#,
            r#"(Binding "name" _:[])"#,
            r#"(Binding "name" _:_:[])"#,
        ] {
            p.parse(input).unwrap_or_else(|e| panic!("{input}: {e}"));
        }
    }
}
