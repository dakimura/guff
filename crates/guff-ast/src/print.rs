// Port of Go's go/ast/print.go to Rust.
//
// Original: Copyright 2010 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// Go's `Fprint` uses runtime reflection to pretty-print arbitrary
// values. Rust doesn't have that, so this port is *specialized for AST
// nodes*: dispatch goes through [`NodeRef`] and each variant prints
// itself via hand-written code. Primitive-value tests from
// `print_test.go` don't apply; the AST-shaped equivalents live below.
//
// Output format mirrors the Go original:
//
// ```text
//      0  Ident {
//      1  .  name_pos: 1
//      2  .  name: "foo"
//      3  }
// ```

use std::fmt::Write as FmtWrite;
use std::io::{self, Write};
use std::sync::Arc;

use crate::ast::{
    BasicLit, BlockStmt, Comment, CommentGroup, Decl, Expr, Field, FieldList, File, FuncDecl,
    FuncType, GenDecl, Ident, ImportSpec, Package, Spec, Stmt, TypeSpec, ValueSpec,
};
use crate::position::{FileSet, Pos};
use crate::token::Token;
use crate::walk::NodeRef;

// ====================================================================
// Printer
// ====================================================================

/// Pretty-print `root` into `w`. When `fset` is provided, `Pos` fields
/// are rendered as resolved positions (`file:line:col`); otherwise raw
/// integer offsets are used.
pub fn fprint<W: Write>(
    w: &mut W,
    fset: Option<&Arc<FileSet>>,
    root: NodeRef<'_>,
) -> io::Result<()> {
    let mut p = Printer::new(fset);
    p.print_node(root);
    p.flush(w)
}

/// Convenience: print to stdout with the given `fset`.
pub fn print(fset: Option<&Arc<FileSet>>, root: NodeRef<'_>) -> io::Result<()> {
    fprint(&mut io::stdout(), fset, root)
}

struct Printer<'f> {
    fset: Option<&'f Arc<FileSet>>,
    buf: String,
    indent: usize,
    /// Number of '\n' bytes emitted so far — used as the line counter.
    line: usize,
    /// True iff the next character starts a new line (and so should be
    /// prefixed by the "<line>  " header).
    new_line: bool,
}

impl<'f> Printer<'f> {
    fn new(fset: Option<&'f Arc<FileSet>>) -> Self {
        Self {
            fset,
            buf: String::new(),
            indent: 0,
            line: 0,
            new_line: true,
        }
    }

    fn flush<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(self.buf.as_bytes())
    }

    fn write(&mut self, s: &str) {
        for c in s.chars() {
            if self.new_line {
                let _ = write!(&mut self.buf, "{:6}  ", self.line);
                for _ in 0..self.indent {
                    self.buf.push_str(".  ");
                }
                self.new_line = false;
            }
            self.buf.push(c);
            if c == '\n' {
                self.line += 1;
                self.new_line = true;
            }
        }
    }

    fn writef(&mut self, args: std::fmt::Arguments<'_>) {
        let mut tmp = String::new();
        let _ = std::fmt::write(&mut tmp, args);
        self.write(&tmp);
    }

    fn print_field(&mut self, name: &str, value: impl FnOnce(&mut Self)) {
        self.write(name);
        self.write(": ");
        value(self);
        self.write("\n");
    }

    fn print_pos(&mut self, p: Pos) {
        if let Some(fset) = self.fset {
            let pos = fset.position(p);
            if pos.is_valid() {
                self.writef(format_args!("{}", pos));
                return;
            }
        }
        self.writef(format_args!("{}", p.0));
    }

    fn print_string(&mut self, s: &str) {
        self.writef(format_args!("{:?}", s));
    }

    #[allow(dead_code)]
    fn print_bool(&mut self, b: bool) {
        self.write(if b { "true" } else { "false" });
    }

    fn print_token(&mut self, tok: Token) {
        self.writef(format_args!("{}", tok));
    }

    fn print_option_token(&mut self, tok: Option<Token>) {
        match tok {
            Some(t) => self.print_token(t),
            None => self.write("nil"),
        }
    }

    fn open(&mut self, kind: &str) {
        self.writef(format_args!("{} {{\n", kind));
        self.indent += 1;
    }

    fn close(&mut self) {
        self.indent -= 1;
        self.write("}");
    }

    fn print_vec<T>(&mut self, name: &str, list: &[T], mut item: impl FnMut(&mut Self, &T)) {
        if list.is_empty() {
            self.writef(format_args!("[]{} (len = 0) {{}}", name));
            return;
        }
        self.writef(format_args!("[]{} (len = {}) {{\n", name, list.len()));
        self.indent += 1;
        for (i, t) in list.iter().enumerate() {
            self.writef(format_args!("{}: ", i));
            item(self, t);
            self.write("\n");
        }
        self.indent -= 1;
        self.write("}");
    }

    fn print_option<T>(&mut self, opt: Option<&T>, mut some: impl FnMut(&mut Self, &T)) {
        match opt {
            None => self.write("nil"),
            Some(v) => some(self, v),
        }
    }

    // ----------------------------------------------------------------

    fn print_node(&mut self, n: NodeRef<'_>) {
        match n {
            NodeRef::Comment(c) => self.print_comment(c),
            NodeRef::CommentGroup(c) => self.print_comment_group(c),
            NodeRef::Field(f) => self.print_field_node(f),
            NodeRef::FieldList(fl) => self.print_field_list(fl),
            NodeRef::Ident(id) => self.print_ident(id),
            NodeRef::BasicLit(bl) => self.print_basic_lit(bl),
            NodeRef::BlockStmt(b) => self.print_block_stmt(b),
            NodeRef::FuncType(ft) => self.print_func_type(ft),
            NodeRef::File(f) => self.print_file(f),
            NodeRef::Package(p) => self.print_package(p),
            NodeRef::FuncDecl(d) => self.print_func_decl(d),
            NodeRef::GenDecl(d) => self.print_gen_decl(d),
            NodeRef::ImportSpec(s) => self.print_import_spec(s),
            NodeRef::ValueSpec(s) => self.print_value_spec(s),
            NodeRef::TypeSpec(s) => self.print_type_spec(s),
            // Generic fallback: print the kind name and any children
            // we can reach via for_each_child. This loses field names
            // but at least makes the structure visible.
            other => {
                self.open(other.kind_name());
                let children: Vec<NodeRef> = collect_children(other);
                if !children.is_empty() {
                    self.write("children: ");
                    self.write("[\n");
                    self.indent += 1;
                    for c in children {
                        self.print_node(c);
                        self.write("\n");
                    }
                    self.indent -= 1;
                    self.write("]\n");
                }
                self.close();
            }
        }
    }

    fn print_expr(&mut self, e: &Expr) {
        self.print_node(crate::walk::expr_ref(e));
    }

    fn print_stmt(&mut self, s: &Stmt) {
        self.print_node(crate::walk::stmt_ref(s));
    }

    fn print_decl(&mut self, d: &Decl) {
        self.print_node(crate::walk::decl_ref(d));
    }

    fn print_spec(&mut self, s: &Spec) {
        self.print_node(crate::walk::spec_ref(s));
    }

    // ----------------------------------------------------------------
    // Per-node printers

    fn print_comment(&mut self, c: &Comment) {
        self.open("Comment");
        self.print_field("slash", |p| p.print_pos(c.slash));
        self.print_field("text", |p| p.print_string(&c.text));
        self.close();
    }

    fn print_comment_group(&mut self, c: &CommentGroup) {
        self.open("CommentGroup");
        self.write("list: ");
        self.print_vec("Comment", &c.list, |p, item| p.print_comment(item));
        self.write("\n");
        self.close();
    }

    fn print_ident(&mut self, id: &Ident) {
        self.open("Ident");
        self.print_field("name_pos", |p| p.print_pos(id.name_pos));
        self.print_field("name", |p| p.print_string(&id.name));
        // obj is deprecated; skip for terseness.
        self.close();
    }

    fn print_basic_lit(&mut self, bl: &BasicLit) {
        self.open("BasicLit");
        self.print_field("value_pos", |p| p.print_pos(bl.value_pos));
        self.print_field("kind", |p| p.print_option_token(bl.kind));
        self.print_field("value", |p| p.print_string(&bl.value));
        self.close();
    }

    fn print_field_node(&mut self, f: &Field) {
        self.open("Field");
        if let Some(d) = &f.doc {
            self.write("doc: ");
            self.print_comment_group(d);
            self.write("\n");
        }
        self.write("names: ");
        self.print_vec("Ident", &f.names, |p, item| p.print_ident(item));
        self.write("\n");
        if let Some(t) = &f.ty {
            self.write("ty: ");
            self.print_expr(t);
            self.write("\n");
        }
        if let Some(t) = &f.tag {
            self.write("tag: ");
            self.print_basic_lit(t);
            self.write("\n");
        }
        if let Some(c) = &f.comment {
            self.write("comment: ");
            self.print_comment_group(c);
            self.write("\n");
        }
        self.close();
    }

    fn print_field_list(&mut self, fl: &FieldList) {
        self.open("FieldList");
        self.print_field("opening", |p| p.print_pos(fl.opening));
        self.write("list: ");
        self.print_vec("Field", &fl.list, |p, item| p.print_field_node(item));
        self.write("\n");
        self.print_field("closing", |p| p.print_pos(fl.closing));
        self.close();
    }

    fn print_block_stmt(&mut self, b: &BlockStmt) {
        self.open("BlockStmt");
        self.print_field("lbrace", |p| p.print_pos(b.lbrace));
        self.write("list: ");
        self.print_vec("Stmt", &b.list, |p, item| p.print_stmt(item));
        self.write("\n");
        self.print_field("rbrace", |p| p.print_pos(b.rbrace));
        self.close();
    }

    fn print_func_type(&mut self, ft: &FuncType) {
        self.open("FuncType");
        self.print_field("func", |p| p.print_pos(ft.func));
        self.write("type_params: ");
        self.print_option(ft.type_params.as_ref(), |p, x| p.print_field_list(x));
        self.write("\n");
        self.write("params: ");
        self.print_option(ft.params.as_ref(), |p, x| p.print_field_list(x));
        self.write("\n");
        self.write("results: ");
        self.print_option(ft.results.as_ref(), |p, x| p.print_field_list(x));
        self.write("\n");
        self.close();
    }

    fn print_file(&mut self, f: &File) {
        self.open("File");
        if let Some(d) = &f.doc {
            self.write("doc: ");
            self.print_comment_group(d);
            self.write("\n");
        }
        self.print_field("package", |p| p.print_pos(f.package));
        self.write("name: ");
        self.print_ident(&f.name);
        self.write("\n");
        self.write("decls: ");
        self.print_vec("Decl", &f.decls, |p, item| p.print_decl(item));
        self.write("\n");
        self.print_field("go_version", |p| p.print_string(&f.go_version));
        self.close();
    }

    fn print_package(&mut self, pkg: &Package) {
        self.open("Package");
        self.print_field("name", |p| p.print_string(&pkg.name));
        self.write("files: map (len = ");
        self.writef(format_args!("{}", pkg.files.len()));
        self.write(") {\n");
        self.indent += 1;
        for (k, v) in &pkg.files {
            self.print_string(k);
            self.write(": ");
            self.print_file(v);
            self.write("\n");
        }
        self.indent -= 1;
        self.write("}\n");
        self.close();
    }

    fn print_func_decl(&mut self, d: &FuncDecl) {
        self.open("FuncDecl");
        if let Some(doc) = &d.doc {
            self.write("doc: ");
            self.print_comment_group(doc);
            self.write("\n");
        }
        if let Some(recv) = &d.recv {
            self.write("recv: ");
            self.print_field_list(recv);
            self.write("\n");
        }
        self.write("name: ");
        self.print_ident(&d.name);
        self.write("\n");
        self.write("ty: ");
        self.print_func_type(&d.ty);
        self.write("\n");
        if let Some(body) = &d.body {
            self.write("body: ");
            self.print_block_stmt(body);
            self.write("\n");
        }
        self.close();
    }

    fn print_gen_decl(&mut self, d: &GenDecl) {
        self.open("GenDecl");
        if let Some(doc) = &d.doc {
            self.write("doc: ");
            self.print_comment_group(doc);
            self.write("\n");
        }
        self.print_field("tok_pos", |p| p.print_pos(d.tok_pos));
        self.print_field("tok", |p| p.print_option_token(d.tok));
        self.print_field("lparen", |p| p.print_pos(d.lparen));
        self.write("specs: ");
        self.print_vec("Spec", &d.specs, |p, item| p.print_spec(item));
        self.write("\n");
        self.print_field("rparen", |p| p.print_pos(d.rparen));
        self.close();
    }

    fn print_import_spec(&mut self, s: &ImportSpec) {
        self.open("ImportSpec");
        if let Some(d) = &s.doc {
            self.write("doc: ");
            self.print_comment_group(d);
            self.write("\n");
        }
        if let Some(n) = &s.name {
            self.write("name: ");
            self.print_ident(n);
            self.write("\n");
        }
        self.write("path: ");
        self.print_basic_lit(&s.path);
        self.write("\n");
        if let Some(c) = &s.comment {
            self.write("comment: ");
            self.print_comment_group(c);
            self.write("\n");
        }
        self.close();
    }

    fn print_value_spec(&mut self, s: &ValueSpec) {
        self.open("ValueSpec");
        self.write("names: ");
        self.print_vec("Ident", &s.names, |p, item| p.print_ident(item));
        self.write("\n");
        if let Some(t) = &s.ty {
            self.write("ty: ");
            self.print_expr(t);
            self.write("\n");
        }
        if !s.values.is_empty() {
            self.write("values: ");
            self.print_vec("Expr", &s.values, |p, item| p.print_expr(item));
            self.write("\n");
        }
        self.close();
    }

    fn print_type_spec(&mut self, s: &TypeSpec) {
        self.open("TypeSpec");
        self.write("name: ");
        self.print_ident(&s.name);
        self.write("\n");
        self.write("ty: ");
        self.print_expr(&s.ty);
        self.write("\n");
        self.close();
    }
}

fn collect_children<'a>(n: NodeRef<'a>) -> Vec<NodeRef<'a>> {
    let mut out = Vec::new();
    crate::walk::for_each_child(n, |c| out.push(c));
    out
}

// ====================================================================
// Tests
// ====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Ident, ImportSpec};
    use crate::position::FileSet;

    fn fprint_str(root: NodeRef<'_>) -> String {
        let mut buf: Vec<u8> = Vec::new();
        fprint(&mut buf, None, root).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn ident_emits_line_numbers_and_field_names() {
        let id = Ident::new_ident("foo");
        let out = fprint_str(NodeRef::Ident(&id));
        // The output should start with line "0", contain the struct
        // header, then name_pos and name lines.
        assert!(out.contains("Ident {"));
        assert!(out.contains("name_pos: 0"));
        assert!(out.contains("name: \"foo\""));
        // Line numbers in left margin (each line begins with 6-wide
        // integer + 2 spaces).
        for (i, line) in out.lines().enumerate() {
            assert!(
                line.starts_with(&format!("{:6}  ", i)),
                "line {} missing prefix: {:?}",
                i,
                line
            );
        }
    }

    #[test]
    fn empty_vec_uses_zero_len_form() {
        let id = Ident::new_ident("");
        let out = fprint_str(NodeRef::Ident(&id));
        assert!(out.contains("name: \"\""));
    }

    #[test]
    fn import_spec_prints_path_basic_lit() {
        let spec = ImportSpec {
            doc: None,
            name: Some(Ident::new_ident("fmt")),
            path: BasicLit {
                id: 0,
                value_pos: Pos(0),
                value_end: Pos(0),
                kind: Some(Token::STRING),
                value: "\"fmt\"".to_string(),
            },
            comment: None,
            end_pos: Pos(0),
            id: 0,
        };
        let out = fprint_str(NodeRef::ImportSpec(&spec));
        assert!(out.contains("ImportSpec {"));
        assert!(out.contains("name: "));
        assert!(out.contains("\"fmt\""));
        assert!(out.contains("BasicLit {"));
        assert!(out.contains("STRING"));
    }

    #[test]
    fn fset_aware_positions_resolve_to_line_col() {
        let fset = FileSet::new();
        let file = fset.add_file("t.go", fset.base(), 100);
        file.add_line(10);
        file.add_line(20);
        let id = Ident {
            name_pos: file.pos(15), // line 2, col 6
            name: "x".to_string(),
            ..Default::default()
        };
        let mut buf: Vec<u8> = Vec::new();
        fprint(&mut buf, Some(&fset), NodeRef::Ident(&id)).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("t.go:2:6"), "got:\n{}", out);
    }
}
