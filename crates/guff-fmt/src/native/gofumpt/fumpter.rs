//! Port of gofumpt `format/format.go` fumpter (AST + line-table rules).

use std::sync::{Arc, OnceLock};

use regex::Regex;

use guff::ast::{
    unparen, AssignStmt, BlockStmt, CallExpr, CaseClause, Comment, CommentGroup, CompositeLit,
    Decl, Expr, Field, FieldList, File, FuncType, GenDecl, Ident, ImportSpec,
    ParenExpr, ReturnStmt, Spec, Stmt,
};
use guff::format;
use guff::import::sort_imports;
use guff::printer::PrintNode;
use guff::token::Token;
use guff::{File as PosFile, FileSet, Pos, NO_POS};

use super::simplify::simplify;
use super::ByteCounter;

const SHORT_LINE_LIMIT: i32 = 60;

fn rx_octal() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\A0[0-7_]+\z").unwrap())
}
fn rx_directive() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"^(?:[a-z0-9-]+:[a-z0-9]|export |extern |line |no(?:inspection|lint)\b|#nosec\b|NOSONAR\b|sys(?:nb)?\b)",
        )
        .unwrap()
    })
}
fn rx_shebang() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^//[^ /].*\bbin/").unwrap())
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Extra {
    pub group_params: bool,
    pub clothe_returns: bool,
}

impl Extra {
    fn string(&self) -> String {
        let mut active = Vec::new();
        if self.group_params {
            active.push("group_params");
        }
        if self.clothe_returns {
            active.push("clothe_returns");
        }
        active.join(",")
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Options {
    pub lang_version: String,
    pub module_path: String,
    pub extra: Extra,
    /// When true, skip gofumpt ≥v0.10 multiline call / paren-removal rules.
    pub omit_v010_rules: bool,
}

/// Apply gofumpt rules to `file` / `fset` in place.
pub(crate) fn apply_file(fset: &Arc<FileSet>, file: &mut File, mut opts: Options) {
    simplify(file);

    if opts.lang_version.is_empty() {
        // Unset -lang: use a modern baseline so version-gated rules (0o octals
        // since go1.13) still apply. Callers should prefer an explicit version
        // from `run.go` / toolchain detection.
        opts.lang_version = "go1.22".into();
    } else {
        let lang = lang_version(&opts.lang_version);
        if lang.is_empty() {
            panic!("invalid Go version: {}", opts.lang_version);
        }
        opts.lang_version = lang;
    }

    let pos_file = fset
        .file(file.pos())
        .expect("parsed file must have a FileSet entry");

    let mut f = Fumpter {
        opts,
        pos_file,
        fset: Arc::clone(fset),
        // Safety: Fumpter methods that use `ast_file` do not reborrow `file`
        // in a way that invalidates this pointer while `file` is exclusively
        // borrowed by `walk_file`. We only read comments / mutate comment text
        // and decls through the same exclusive `&mut File`.
        ast_file: file as *mut File,
        block_level: 0,
        min_split_factor: 0.4,
        parent_func_results: Vec::new(),
        need_import_sort: false,
    };

    f.file_rules(file);
    walk_file(&mut f, file);
    if f.need_import_sort {
        sort_imports(fset, file);
    }
}

struct Fumpter {
    opts: Options,
    pos_file: Arc<PosFile>,
    fset: Arc<FileSet>,
    ast_file: *mut File,
    block_level: i32,
    min_split_factor: f64,
    parent_func_results: Vec<Option<FieldList>>,
    need_import_sort: bool,
}

impl Fumpter {
    fn ast(&self) -> &File {
        unsafe { &*self.ast_file }
    }
    
    fn line(&self, p: Pos) -> i64 {
        self.pos_file.position_for(p, false).line
    }
    fn offset(&self, p: Pos) -> i64 {
        self.pos_file.offset(p)
    }
    fn comments_between(&self, p1: Pos, p2: Pos) -> Vec<&CommentGroup> {
        let comments = &self.ast().comments;
        let i1 = comments.partition_point(|c| c.pos() < p1);
        let i2 = i1 + comments[i1..].partition_point(|c| c.pos() < p2);
        comments[i1..i2].iter().collect()
    }

    fn inline_comment(&self, pos: Pos) -> Option<&Comment> {
        let comments = &self.ast().comments;
        let i = comments.partition_point(|c| c.pos() < pos);
        if i >= comments.len() {
            return None;
        }
        let line = self.line(pos);
        for c in &comments[i].list {
            if self.line(c.pos()) == line {
                return Some(c);
            }
        }
        None
    }

    fn add_newline(&self, at: Pos) {
        let offset = self.offset(at);
        let mut lines = self.pos_file.lines();
        match lines.binary_search(&offset) {
            Ok(_) => {}
            Err(i) => {
                lines.insert(i, offset);
                assert!(self.pos_file.set_lines(lines), "could not set lines");
            }
        }
    }

    fn remove_lines(&self, from_line: i64, to_line: i64) {
        let from = from_line;
        let mut to = to_line;
        while from < to {
            self.pos_file.merge_line(from as usize);
            to -= 1;
        }
    }

    fn remove_lines_between(&self, from: Pos, to: Pos) {
        self.remove_lines(self.line(from) + 1, self.line(to));
    }

    fn print_length(&self, node: PrintNode<'_>, end: Pos) -> i32 {
        let mut count = ByteCounter(0);
        let _ = format::node(&mut count, &self.fset, node);
        if let Some(c) = self.inline_comment(end) {
            count.0 += 1 + c.text.len();
        }
        (count.0 as i32) + (self.block_level * 8)
    }

    fn file_rules(&mut self, file: &mut File) {
        self.join_lone_decls(file);

        // Multiline top-level decls get a blank line between them.
        let mut last_multi = false;
        let mut last_end = NO_POS;
        let infos: Vec<(Pos, Pos, bool)> = file
            .decls
            .iter()
            .map(|decl| {
                let pos = decl.pos();
                let end = decl.end();
                let mut multi = self.line(pos) < self.line(Pos(end.0.saturating_sub(1)));
                if let Decl::FuncDecl(fn_) = decl {
                    if !multi && fn_.body.is_some() && self.offset(end) - self.offset(pos) > 100 {
                        multi = true;
                    }
                }
                (pos, end, multi)
            })
            .collect();

        for (pos0, end, multi) in infos {
            let mut pos = pos0;
            let mut effective_end = last_end;
            if last_end.is_valid() {
                let last_end_line = self.line(last_end);
                for cg in self.comments_between(last_end, pos0) {
                    if self.line(cg.pos()) != last_end_line {
                        pos = cg.pos();
                        break;
                    }
                    effective_end = cg.end();
                }
            }
            if multi && last_multi && self.line(effective_end) + 1 == self.line(pos) {
                self.add_newline(effective_end);
            }
            last_multi = multi;
            last_end = end;
        }

        self.fix_comments(file);
    }

    fn join_lone_decls(&mut self, file: &mut File) {
        let old = std::mem::take(&mut file.decls);
        let mut new_decls = Vec::with_capacity(old.len());
        let mut i = 0;
        let mut any_merged_import = false;
        while i < old.len() {
            let mut start_decl = old[i].clone();
            let can_join = match &start_decl {
                Decl::GenDecl(start) => {
                    !is_cgo_import(start) && !contains_any_directive(start.doc.as_ref())
                }
                _ => false,
            };
            if !can_join {
                new_decls.push(start_decl);
                i += 1;
                continue;
            }
            let start_tok = match &start_decl {
                Decl::GenDecl(g) => g.tok,
                _ => unreachable!(),
            };
            let mut last_pos = match &start_decl {
                Decl::GenDecl(g) => g.tok_pos,
                _ => unreachable!(),
            };
            let mut merged = false;
            i += 1;
            while i < old.len() {
                let cont = match &old[i] {
                    Decl::GenDecl(c)
                        if c.tok == start_tok && !c.lparen.is_valid() && !is_cgo_import(c) =>
                    {
                        c
                    }
                    _ => break,
                };
                if self.line(last_pos) < self.line(cont.tok_pos) - 1 {
                    let Some(doc) = &cont.doc else { break };
                    let mut abort = false;
                    for (j, comment) in doc.list.iter().enumerate() {
                        if self.line(comment.slash) != self.line(last_pos) + 1 + j as i64
                            || rx_directive().is_match(comment.text.strip_prefix("//").unwrap_or(""))
                        {
                            abort = true;
                            break;
                        }
                    }
                    if abort {
                        break;
                    }
                }
                let cont = match &old[i] {
                    Decl::GenDecl(c) => c.clone(),
                    _ => break,
                };
                let cont_end = Decl::GenDecl(cont.clone()).end();
                let rparen = self
                    .inline_comment(cont_end)
                    .map(|c| c.end())
                    .unwrap_or(cont_end);
                if let Decl::GenDecl(start) = &mut start_decl {
                    start.specs.extend(cont.specs);
                    start.rparen = rparen;
                }
                merged = true;
                last_pos = cont.tok_pos;
                i += 1;
            }
            if merged {
                if let Decl::GenDecl(start) = &mut start_decl {
                    if start.tok == Some(Token::IMPORT) {
                        start.lparen = Pos(start.tok_pos.0 + "import".len() as i64);
                        any_merged_import = true;
                    }
                }
            }
            new_decls.push(start_decl);
        }
        file.decls = new_decls;
        if any_merged_import {
            sort_imports(&self.fset, file);
        }
    }

    fn fix_comments(&mut self, file: &mut File) {
        // Diagnose first so later spacing logic sees final text.
        for group in &mut file.comments {
            for comment in &mut group.list {
                if comment.text == "//gofumpt:diagnose"
                    || comment.text.starts_with("//gofumpt:diagnose ")
                {
                    let mut parts = vec![
                        "//gofumpt:diagnose".to_string(),
                        "version:".into(),
                        "v0.10.0 (go1.26.4)".into(),
                        "flags:".into(),
                        format!("-lang={}", self.opts.lang_version),
                        format!("-modpath={}", self.opts.module_path),
                    ];
                    let extra = self.opts.extra.string();
                    if !extra.is_empty() {
                        parts.push(format!("-extra={extra}"));
                    }
                    comment.text = parts.join(" ");
                }
            }
        }

        let n = file.comments.len();
        for gi in 0..n {
            let skip = {
                let group = &file.comments[gi];
                let mut skip = false;
                for comment in &group.list {
                    if self.line(comment.slash) == 1 && rx_shebang().is_match(&comment.text) {
                        skip = true;
                        break;
                    }
                    let Some(body) = comment.text.strip_prefix("//") else {
                        skip = true; // /*-style
                        break;
                    };
                    if rx_directive().is_match(body) {
                        skip = true;
                        break;
                    }
                    if let Some(r) = body.chars().next() {
                        if !r.is_alphabetic() && !r.is_numeric() && !r.is_whitespace() {
                            skip = true;
                            break;
                        }
                    }
                }
                skip || comment_group_looks_like_code(&file.comments[gi])
            };
            if skip {
                continue;
            }
            for comment in &mut file.comments[gi].list {
                if let Some(body) = comment.text.strip_prefix("//") {
                    if body.chars().next().is_some_and(|r| !r.is_whitespace()) {
                        comment.text = format!("// {body}");
                    }
                }
            }
        }
    }

    fn join_std_imports(&mut self, d: &mut GenDecl) {
        let mut std = Vec::new();
        let mut other = Vec::new();
        let mut first_group = true;
        let mut last_end = d.tok_pos;
        let mut needs_sort = false;

        let module_prefix = if self.opts.module_path.is_empty() {
            String::new()
        } else if let Some(i) = self.opts.module_path.find('/') {
            self.opts.module_path[..i].to_string()
        } else {
            self.opts.module_path.clone()
        };

        let specs = std::mem::take(&mut d.specs);
        for (i, spec) in specs.into_iter().enumerate() {
            let Spec::ImportSpec(spec) = spec else {
                continue;
            };
            if let Some(last) = self.comments_between(last_end, spec_pos(&spec)).last() {
                last_end = last.end();
            }
            if i > 0 && first_group && self.line(spec_pos(&spec)) > self.line(last_end) + 1 {
                first_group = false;
            } else {
                last_end = import_end(&spec);
            }

            let path = unquote_path(&spec.path.value);
            let period = path.find('.');
            let slash = path.find('/');
            let is_other = match period {
                Some(p) if p > 0 && (slash.is_none() || p < slash.unwrap()) => true,
                _ if path.starts_with("test/") || path.starts_with("example/") => true,
                _ if !module_prefix.is_empty()
                    && (path == module_prefix || path.starts_with(&(module_prefix.clone() + "/"))) =>
                {
                    true
                }
                _ if !first_group && (spec.name.is_some() || spec.comment.is_some()) => true,
                _ => false,
            };
            if is_other {
                other.push(Spec::ImportSpec(spec));
                continue;
            }
            if !first_group || !other.is_empty() {
                let mut spec = spec;
                set_import_pos(&mut spec, d.tok_pos);
                needs_sort = true;
                std.push(Spec::ImportSpec(spec));
            } else {
                std.push(Spec::ImportSpec(spec));
            }
        }

        if !std.is_empty() && !other.is_empty() {
            let std_end = match std.last().unwrap() {
                Spec::ImportSpec(s) => import_end(s),
                _ => NO_POS,
            };
            let other_pos = match other.first().unwrap() {
                Spec::ImportSpec(s) => spec_pos(s),
                _ => NO_POS,
            };
            if self.line(std_end) + 1 >= self.line(other_pos) {
                self.add_newline(Pos(other_pos.0 - 1));
                self.add_newline(other_pos);
            }
        }
        d.specs = std;
        d.specs.append(&mut other);
        if needs_sort {
            self.need_import_sort = true;
        }
    }

    fn stmts(&self, list: &[Stmt]) {
        for i in 1..list.len() {
            let Stmt::IfStmt(ifs) = &list[i] else { continue };
            let Stmt::AssignStmt(as_) = &list[i - 1] else { continue };
            if as_.tok != Some(Token::DEFINE) && as_.tok != Some(Token::ASSIGN) {
                continue;
            }
            if !ident_equal(as_.lhs.last().unwrap(), "err") {
                continue;
            }
            let Expr::BinaryExpr(be) = &ifs.cond else { continue };
            if ifs.init.is_some() || ifs.else_.is_some() {
                continue;
            }
            if be.op != Token::NEQ || !ident_equal(&be.x, "err") || !ident_equal(&be.y, "nil") {
                continue;
            }
            self.remove_lines_between(list[i - 1].end(), ifs.if_);
        }
    }

    fn merge_adjacent_fields(&self, fields: &mut Vec<Field>) {
        if fields.len() < 2 {
            return;
        }
        let mut i = 0;
        let mut j = 1;
        while j < fields.len() {
            if self.should_merge(&fields[i], &fields[j]) {
                let extra = fields[j].names.clone();
                fields[i].names.extend(extra);
            } else {
                i += 1;
                fields[i] = fields[j].clone();
            }
            j += 1;
        }
        fields.truncate(i + 1);
    }

    fn should_merge(&self, f1: &Field, f2: &Field) -> bool {
        if f1.names.is_empty() || f2.names.is_empty() {
            return false;
        }
        if self.line(f1.pos()) != self.line(f2.pos()) {
            return false;
        }
        let (Some(t1), Some(t2)) = (&f1.ty, &f2.ty) else {
            return false;
        };
        let empty = Arc::new(FileSet::new());
        let mut s1 = Vec::new();
        let mut s2 = Vec::new();
        if format::node(&mut s1, &empty, PrintNode::Expr(t1)).is_err() {
            return false;
        }
        if format::node(&mut s2, &empty, PrintNode::Expr(t2)).is_err() {
            return false;
        }
        s1 == s2
    }
}

fn walk_file(f: &mut Fumpter, file: &mut File) {
    for d in &mut file.decls {
        walk_decl(f, d, true);
    }
}

fn walk_decl(f: &mut Fumpter, decl: &mut Decl, is_top: bool) {
    match decl {
        Decl::FuncDecl(fn_) => {
            f.parent_func_results.push(fn_.ty.results.clone());
            let old = f.min_split_factor;
            if let Some(recv) = &mut fn_.recv {
                walk_field_list(f, recv, FieldParent::Other);
            }
            if let Some(tp) = &mut fn_.ty.type_params {
                walk_field_list(f, tp, FieldParent::FuncType);
            }
            if let Some(params) = &mut fn_.ty.params {
                if is_top {
                    f.min_split_factor = 0.6;
                }
                walk_field_list(f, params, FieldParent::FuncDecl);
                f.min_split_factor = old;
            }
            if let Some(results) = &mut fn_.ty.results {
                if is_top {
                    f.min_split_factor = 1000.0;
                }
                walk_field_list(f, results, FieldParent::FuncDecl);
                f.min_split_factor = old;
            }
            let sign_ptr = &mut fn_.ty as *mut FuncType;
            let body_ptr = fn_.body.as_mut().map(|b| b as *mut BlockStmt);
            if let Some(bp) = body_ptr {
                walk_block(f, unsafe { &mut *bp }, BlockParent::Func { sign: Some(sign_ptr) });
            }
            f.parent_func_results.pop();
        }
        Decl::GenDecl(g) => walk_gen_decl(f, g),
        Decl::BadDecl(_) => {}
    }
}

#[derive(Clone)]
enum BlockParent {
    None,
    /// `sign` points at the enclosing `FuncDecl`/`FuncLit` type (same pointer
    /// gofumpt mutates for multi-line signature newlines).
    Func { sign: Option<*mut FuncType> },
    If { cond: Expr },
    For { cond: Option<Expr> },
}

#[derive(Clone, Copy)]
enum FieldParent {
    FuncDecl,
    FuncType,
    InterfaceType,
    StructType,
    Other,
}

fn walk_gen_decl(f: &mut Fumpter, node: &mut GenDecl) {
    if node.tok == Some(Token::IMPORT) && node.lparen.is_valid() {
        f.join_std_imports(node);
    }

    // Single var (...) → drop parens
    if node.tok == Some(Token::VAR)
        && node.specs.len() == 1
        && node.lparen.is_valid()
        && node.doc.is_none()
    {
        let spec_pos = node.specs[0].pos();
        let spec_end = node.specs[0].end();
        if !f.comments_between(node.tok_pos, spec_pos).is_empty() {
            node.tok_pos = spec_pos;
        } else {
            f.remove_lines(f.line(node.tok_pos), f.line(spec_pos));
        }
        if !f.comments_between(spec_end, node.rparen).is_empty() {
            f.remove_lines(f.line(spec_end) + 1, f.line(node.rparen));
        } else {
            f.remove_lines(f.line(spec_end), f.line(node.rparen));
        }
        node.lparen = NO_POS;
        node.rparen = NO_POS;
    }

    for s in &mut node.specs {
        match s {
            Spec::ValueSpec(v) => {
                if let Some(ty) = &mut v.ty {
                    walk_expr(f, ty);
                }
                for e in &mut v.values {
                    walk_expr(f, e);
                }
            }
            Spec::TypeSpec(t) => {
                if let Some(tp) = &mut t.type_params {
                    walk_field_list(f, tp, FieldParent::Other);
                }
                walk_expr(f, &mut t.ty);
            }
            Spec::ImportSpec(_) => {}
        }
    }
}

fn walk_field_list(f: &mut Fumpter, node: &mut FieldList, parent: FieldParent) {
    let num_fields = node.num_fields();
    let comments = {
        let c: Vec<Pos> = f
            .comments_between(node.pos(), node.end())
            .into_iter()
            .map(|cg| cg.pos())
            .collect();
        c
    };
    let comment_groups = f.comments_between(node.pos(), node.end());

    if num_fields == 0 && comment_groups.is_empty() {
        let open_line = f.line(node.pos());
        let close_line = f.line(node.end());
        f.remove_lines(open_line, close_line);
    } else {
        let mut body_pos = NO_POS;
        let mut body_end = NO_POS;
        if num_fields > 0 {
            body_pos = node.list[0].pos();
            body_end = node.list[node.list.len() - 1].end();
        }
        if let Some(first) = comment_groups.first() {
            if !body_pos.is_valid() || first.pos() < body_pos {
                body_pos = first.pos();
            }
        }
        if let Some(last) = comment_groups.last() {
            if !body_end.is_valid() || last.end() > body_end {
                body_end = last.end();
            }
        }
        if body_pos.is_valid() {
            f.remove_lines_between(node.pos(), body_pos);
        }
        if body_end.is_valid() {
            f.remove_lines_between(body_end, node.end());
        }
    }
    let _ = comments;

    if f.opts.extra.group_params {
        match parent {
            FieldParent::FuncDecl | FieldParent::FuncType | FieldParent::InterfaceType => {
                f.merge_adjacent_fields(&mut node.list);
            }
            FieldParent::StructType | FieldParent::Other => {}
        }
    }

    for field in &mut node.list {
        if let Some(ty) = &mut field.ty {
            walk_expr(f, ty);
        }
    }
}

fn walk_block(f: &mut Fumpter, node: &mut BlockStmt, parent: BlockParent) {
    f.block_level += 1;
    // Pre: stmts + newline rules need list; walk children first like Apply?
    // gofumpt applyPre on BlockStmt runs stmts() and newline logic BEFORE
    // walking children... actually Apply calls pre, then children, then post.
    // So pre runs before children. stmts() only looks at current list.
    f.stmts(&node.list);

    let comments: Vec<(Pos, Pos)> = f
        .comments_between(node.lbrace, node.rbrace)
        .into_iter()
        .map(|c| (c.pos(), c.end()))
        .collect();

    if node.list.is_empty() && comments.is_empty() {
        f.remove_lines_between(node.lbrace, node.rbrace);
        // still walk (nothing)
        f.block_level -= 1;
        return;
    }

    let is_func = matches!(parent, BlockParent::Func { .. });
    let cond_multiline = match &parent {
        BlockParent::If { cond } => f.line(cond.pos()) != f.line(cond.end()),
        BlockParent::For { cond: Some(c) } => f.line(c.pos()) != f.line(c.end()),
        _ => false,
    };

    if node.list.len() > 1 && !is_func {
        for s in &mut node.list {
            walk_stmt(f, s);
        }
        f.block_level -= 1;
        return;
    }

    let mut body_pos = NO_POS;
    let mut body_end = NO_POS;
    if let Some(first) = node.list.first() {
        body_pos = first.pos();
        body_end = node.list.last().unwrap().end();
    }
    if let Some((p, _)) = comments.first() {
        if !body_pos.is_valid() || *p < body_pos {
            body_pos = *p;
        }
    }
    if let Some((_, e)) = comments.last() {
        if !body_end.is_valid() || *e > body_end {
            body_end = *e;
        }
    }

    if body_end.is_valid() {
        f.remove_lines_between(body_end, node.rbrace);
    }

    if let BlockParent::Func {
        sign: Some(sign_ptr),
    } = parent
    {
        let sign = unsafe { &mut *sign_ptr };
        let end_line = f.line(sign.end());
        if f.line(sign.pos()) != end_line {
            let handle = |f: &Fumpter, fl: &mut FieldList, end_line: i64| {
                if fl.list.is_empty() {
                    return;
                }
                let open_l = f.line(fl.opening);
                let close_l = f.line(fl.closing);
                if open_l == close_l {
                    return;
                }
                let last_field_end = fl.list.last().unwrap().end();
                let last_field_line = f.line(last_field_end);
                let on_close = last_field_line == close_l;
                let on_sig = last_field_line == end_line;
                let mut cmt_on_close = false;
                let mut cmt_on_sig = false;
                if let Some(last) = f.comments_between(last_field_end, fl.closing).last() {
                    let l = f.line(last.end());
                    cmt_on_close = l == close_l;
                    cmt_on_sig = l == end_line;
                }
                if (on_close && on_sig) || (cmt_on_close && cmt_on_sig) {
                    fl.closing = Pos(fl.closing.0 + 1);
                    f.add_newline(fl.closing);
                }
            };
            if let Some(params) = &mut sign.params {
                handle(f, params, end_line);
            }
            if let Some(results) = &mut sign.results {
                if !results.list.is_empty() {
                    let last_result_line = f.line(results.list.last().unwrap().end());
                    let on_param_close = sign
                        .params
                        .as_ref()
                        .is_some_and(|p| last_result_line == f.line(p.closing));
                    if !on_param_close {
                        handle(f, results, end_line);
                    }
                }
            }
        }
    }

    if !cond_multiline && body_pos.is_valid() {
        f.remove_lines_between(node.lbrace, body_pos);
    }

    for s in &mut node.list {
        walk_stmt(f, s);
    }
    f.block_level -= 1;
}

fn walk_stmt(f: &mut Fumpter, stmt: &mut Stmt) {
    // DeclStmt → short var
    if let Stmt::DeclStmt(d) = stmt {
        if let Decl::GenDecl(decl) = &d.decl {
            if decl.tok == Some(Token::VAR) && decl.specs.len() == 1 {
                if let Spec::ValueSpec(spec) = &decl.specs[0] {
                    if spec.ty.is_none() {
                        let mut tok = Token::ASSIGN;
                        let names: Vec<Expr> = spec
                            .names
                            .iter()
                            .map(|n| {
                                if n.name != "_" {
                                    tok = Token::DEFINE;
                                }
                                Expr::Ident(n.clone())
                            })
                            .collect();
                        *stmt = Stmt::AssignStmt(AssignStmt {
                            lhs: names,
                            tok_pos: decl.tok_pos,
                            tok: Some(tok),
                            rhs: spec.values.clone(),
                        });
                        // fall through to AssignStmt handling
                    }
                }
            }
        }
    }

    match stmt {
        Stmt::DeclStmt(d) => walk_decl(f, &mut d.decl, false),
        Stmt::BlockStmt(b) => walk_block(f, b, BlockParent::None),
        Stmt::IfStmt(i) => {
            if let Some(init) = &mut i.init {
                walk_stmt(f, init);
            }
            walk_expr(f, &mut i.cond);
            let cond = i.cond.clone();
            walk_block(f, &mut i.body, BlockParent::If { cond });
            if let Some(e) = &mut i.else_ {
                walk_stmt(f, e);
            }
        }
        Stmt::ForStmt(fo) => {
            if let Some(init) = &mut fo.init {
                walk_stmt(f, init);
            }
            if let Some(c) = &mut fo.cond {
                walk_expr(f, c);
            }
            if let Some(p) = &mut fo.post {
                walk_stmt(f, p);
            }
            let cond = fo.cond.clone();
            walk_block(f, &mut fo.body, BlockParent::For { cond });
        }
        Stmt::RangeStmt(r) => {
            if let Some(k) = &mut r.key {
                walk_expr(f, k);
            }
            if let Some(v) = &mut r.value {
                walk_expr(f, v);
            }
            walk_expr(f, &mut r.x);
            walk_block(f, &mut r.body, BlockParent::None);
        }
        Stmt::SwitchStmt(s) => {
            if let Some(init) = &mut s.init {
                walk_stmt(f, init);
            }
            if let Some(tag) = &mut s.tag {
                walk_expr(f, tag);
            }
            walk_block(f, &mut s.body, BlockParent::None);
        }
        Stmt::TypeSwitchStmt(s) => {
            if let Some(init) = &mut s.init {
                walk_stmt(f, init);
            }
            walk_stmt(f, &mut s.assign);
            walk_block(f, &mut s.body, BlockParent::None);
        }
        Stmt::SelectStmt(s) => walk_block(f, &mut s.body, BlockParent::None),
        Stmt::CaseClause(c) => walk_case(f, c),
        Stmt::CommClause(c) => {
            if let Some(comm) = &mut c.comm {
                walk_stmt(f, comm);
            }
            f.stmts(&c.body);
            for s in &mut c.body {
                walk_stmt(f, s);
            }
        }
        Stmt::AssignStmt(a) => {
            for e in &mut a.lhs {
                walk_expr(f, e);
            }
            for e in &mut a.rhs {
                walk_expr(f, e);
            }
            if a.rhs.len() == 1 && !matches!(&a.rhs[0], Expr::BinaryExpr(_)) {
                f.remove_lines(f.line(a.tok_pos), f.line(a.rhs[0].pos()));
            }
        }
        Stmt::ReturnStmt(r) => {
            clothe_return(f, r);
            for e in &mut r.results {
                walk_expr(f, e);
            }
        }
        Stmt::ExprStmt(e) => walk_expr(f, &mut e.x),
        Stmt::SendStmt(s) => {
            walk_expr(f, &mut s.chan_);
            walk_expr(f, &mut s.value);
        }
        Stmt::IncDecStmt(i) => walk_expr(f, &mut i.x),
        Stmt::GoStmt(g) => walk_call(f, &mut g.call),
        Stmt::DeferStmt(d) => walk_call(f, &mut d.call),
        Stmt::LabeledStmt(l) => walk_stmt(f, &mut l.stmt),
        Stmt::BadStmt(_) | Stmt::EmptyStmt(_) | Stmt::BranchStmt(_) => {}
    }
}

fn walk_case(f: &mut Fumpter, node: &mut CaseClause) {
    f.stmts(&node.body);
    let open_line = f.line(node.case);
    let close_line = f.line(node.colon);
    if open_line != close_line && f.comments_between(node.case, node.colon).is_empty() {
        let without_body = CaseClause {
            case: node.case,
            list: node.list.clone(),
            colon: node.colon,
            body: Vec::new(),
            id: node.id,
        };
        let stmt = Stmt::CaseClause(without_body);
        if f.print_length(PrintNode::Stmt(&stmt), node.colon) <= SHORT_LINE_LIMIT {
            f.remove_lines(open_line, close_line);
        }
    }
    for e in &mut node.list {
        walk_expr(f, e);
    }
    for s in &mut node.body {
        walk_stmt(f, s);
    }
}

fn clothe_return(f: &mut Fumpter, node: &mut ReturnStmt) {
    if !node.results.is_empty() || !f.opts.extra.clothe_returns {
        return;
    }
    let Some(Some(results)) = f.parent_func_results.last() else {
        return;
    };
    if results.num_fields() == 0 {
        return;
    }
    let mut new_results = Vec::new();
    for result in &results.list {
        for ident in &result.names {
            if ident.name == "_" {
                return;
            }
            new_results.push(Expr::Ident(Ident {
                name_pos: node.return_,
                name: ident.name.clone(),
                obj: std::sync::Mutex::new(None),
                id: 0,
            }));
        }
    }
    if !new_results.is_empty() {
        node.results = new_results;
    }
}

fn walk_expr(f: &mut Fumpter, expr: &mut Expr) {
    match expr {
        Expr::ParenExpr(p) => {
            let inner = unparen(std::mem::replace(
                p.x.as_mut(),
                Expr::BadExpr(guff::ast::BadExpr {
                    from: NO_POS,
                    to: NO_POS,
                    id: 0,
                }),
            ));
            *p.x = inner;
            if !f.opts.omit_v010_rules && can_remove_parens(f, p) {
                *expr = std::mem::replace(
                    p.x.as_mut(),
                    Expr::BadExpr(guff::ast::BadExpr {
                        from: NO_POS,
                        to: NO_POS,
                    id: 0,
                    }),
                );
                walk_expr(f, expr);
                return;
            }
            walk_expr(f, &mut p.x);
        }
        Expr::BasicLit(lit) => {
            if version_ge(&f.opts.lang_version, "go1.13")
                && lit.kind == Some(Token::INT)
                && rx_octal().is_match(&lit.value)
            {
                lit.value = format!("0o{}", &lit.value[1..]);
            }
        }
        Expr::CallExpr(c) => {
            walk_call(f, c);
            if !f.opts.omit_v010_rules {
                call_post(f, c);
            }
        }
        Expr::CompositeLit(c) => {
            if let Some(ty) = &mut c.ty {
                walk_expr(f, ty);
            }
            for e in &mut c.elts {
                walk_expr(f, e);
            }
            composite_post(f, c);
        }
        Expr::FuncLit(fl) => {
            f.parent_func_results.push(fl.ty.results.clone());
            if let Some(p) = &mut fl.ty.params {
                walk_field_list(f, p, FieldParent::FuncType);
            }
            if let Some(r) = &mut fl.ty.results {
                walk_field_list(f, r, FieldParent::FuncType);
            }
            let sign_ptr = &mut fl.ty as *mut FuncType;
            let body_ptr = &mut fl.body as *mut BlockStmt;
            walk_block(f, unsafe { &mut *body_ptr }, BlockParent::Func { sign: Some(sign_ptr) });
            f.parent_func_results.pop();
        }
        Expr::SelectorExpr(s) => walk_expr(f, &mut s.x),
        Expr::IndexExpr(i) => {
            walk_expr(f, &mut i.x);
            walk_expr(f, &mut i.index);
        }
        Expr::IndexListExpr(i) => {
            walk_expr(f, &mut i.x);
            for idx in &mut i.indices {
                walk_expr(f, idx);
            }
        }
        Expr::SliceExpr(s) => {
            walk_expr(f, &mut s.x);
            if let Some(l) = &mut s.low {
                walk_expr(f, l);
            }
            if let Some(h) = &mut s.high {
                walk_expr(f, h);
            }
            if let Some(m) = &mut s.max {
                walk_expr(f, m);
            }
        }
        Expr::TypeAssertExpr(t) => {
            walk_expr(f, &mut t.x);
            if let Some(ty) = &mut t.ty {
                walk_expr(f, ty);
            }
        }
        Expr::StarExpr(s) => walk_expr(f, &mut s.x),
        Expr::UnaryExpr(u) => walk_expr(f, &mut u.x),
        Expr::BinaryExpr(b) => {
            walk_expr(f, &mut b.x);
            walk_expr(f, &mut b.y);
        }
        Expr::KeyValueExpr(kv) => {
            walk_expr(f, &mut kv.key);
            walk_expr(f, &mut kv.value);
        }
        Expr::ArrayType(a) => {
            if let Some(len) = &mut a.len {
                walk_expr(f, len);
            }
            walk_expr(f, &mut a.elt);
        }
        Expr::StructType(s) => walk_field_list(f, &mut s.fields, FieldParent::StructType),
        Expr::InterfaceType(i) => {
            if !i.methods.list.is_empty() {
                let mut remove_to = i.methods.list[0].pos();
                if let Some(c) = f.comments_between(i.interface_, i.methods.list[0].pos()).first() {
                    remove_to = c.pos();
                }
                f.remove_lines(f.line(i.interface_) + 1, f.line(remove_to));
            }
            walk_field_list(f, &mut i.methods, FieldParent::InterfaceType);
        }
        Expr::MapType(m) => {
            walk_expr(f, &mut m.key);
            walk_expr(f, &mut m.value);
        }
        Expr::ChanType(c) => walk_expr(f, &mut c.value),
        Expr::FuncType(ft) => {
            if let Some(p) = &mut ft.params {
                walk_field_list(f, p, FieldParent::FuncType);
            }
            if let Some(r) = &mut ft.results {
                walk_field_list(f, r, FieldParent::FuncType);
            }
        }
        Expr::Ellipsis(e) => {
            if let Some(x) = &mut e.elt {
                walk_expr(f, x);
            }
        }
        Expr::BadExpr(_) | Expr::Ident(_) => {}
    }
}

fn walk_call(f: &mut Fumpter, c: &mut CallExpr) {
    walk_expr(f, &mut c.fun);
    for a in &mut c.args {
        walk_expr(f, a);
    }
}

fn can_remove_parens(f: &Fumpter, node: &ParenExpr) -> bool {
    match &*node.x {
        Expr::BinaryExpr(_)
        | Expr::UnaryExpr(_)
        | Expr::StarExpr(_)
        | Expr::CompositeLit(_)
        | Expr::ChanType(_)
        | Expr::ArrayType(_)
        | Expr::MapType(_)
        | Expr::FuncType(_)
        | Expr::InterfaceType(_)
        | Expr::StructType(_) => false,
        _ => f.comments_between(node.lparen, node.rparen).is_empty(),
    }
}

fn call_post(f: &mut Fumpter, node: &CallExpr) {
    if node.args.is_empty() {
        return;
    }
    let open_line = f.line(node.lparen);
    let close_line = f.line(node.rparen);
    if open_line == close_line {
        return;
    }
    let first_line = f.line(node.args[0].pos());
    let mut last_end = node.args[node.args.len() - 1].end();
    if let Some(c) = f.inline_comment(last_end) {
        last_end = c.end();
    }
    let last_line = f.line(last_end);
    let open_at_eol = open_line != first_line;
    let close_at_bol = close_line != last_line;
    if open_at_eol && !close_at_bol {
        f.add_newline(node.rparen);
    } else if close_at_bol && !open_at_eol {
        f.add_newline(Pos(node.lparen.0 + 1));
    }
}

fn composite_post(f: &mut Fumpter, node: &CompositeLit) {
    if node.elts.is_empty() {
        return;
    }
    let open_line = f.line(node.lbrace);
    let mut close_line = f.line(node.rbrace);
    if open_line == close_line {
        return;
    }
    let mut newline_around = false;
    let mut newline_between = false;
    let mut last_end = node.lbrace;
    let mut last_line = open_line;
    for (i, elem) in node.elts.iter().enumerate() {
        let mut pos = elem.pos();
        if let Some(c) = f.comments_between(last_end, pos).first() {
            pos = c.pos();
        }
        let cur_line = f.line(pos);
        if cur_line > last_line {
            if i == 0 {
                newline_around = true;
                f.remove_lines(open_line + 1, cur_line);
            } else {
                newline_between = true;
            }
        }
        last_end = elem.end();
        last_line = f.line(last_end);
    }
    if close_line > last_line {
        newline_around = true;
    }
    if newline_between || newline_around {
        if open_line == f.line(node.elts[0].pos()) {
            f.add_newline(Pos(node.lbrace.0 + 1));
            close_line = f.line(node.rbrace);
        }
        let last = node.elts.last().unwrap();
        if close_line == f.line(last.end()) {
            f.add_newline(node.rbrace);
        }
    }
    if !newline_between {
        return;
    }
    for i in 0..node.elts.len().saturating_sub(1) {
        let e1 = &node.elts[i];
        let e2 = &node.elts[i + 1];
        let ok1 = matches!(e1, Expr::CompositeLit(_));
        let ok2 = matches!(e2, Expr::CompositeLit(_));
        if !ok1 && !ok2 {
            continue;
        }
        if f.line(e1.end()) == f.line(e2.pos()) {
            f.add_newline(e1.end());
        }
    }
}

// --- helpers ---

fn lang_version(s: &str) -> String {
    let s = s.trim();
    if !s.starts_with("go") {
        return String::new();
    }
    let rest = &s[2..];
    let mut parts = rest.split('.');
    let major = parts.next().unwrap_or("");
    if major.is_empty() || !major.chars().all(|c| c.is_ascii_digit()) {
        return String::new();
    }
    match parts.next() {
        None => format!("go{major}"),
        Some(minor) => {
            let minor: String = minor.chars().take_while(|c| c.is_ascii_digit()).collect();
            if minor.is_empty() {
                String::new()
            } else {
                format!("go{major}.{minor}")
            }
        }
    }
}

fn version_ge(a: &str, b: &str) -> bool {
    fn nums(v: &str) -> (i32, i32) {
        let v = v.strip_prefix("go").unwrap_or(v);
        let mut p = v.split('.');
        let maj = p.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let min = p.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        (maj, min)
    }
    nums(a) >= nums(b)
}

fn is_cgo_import(decl: &GenDecl) -> bool {
    if decl.tok != Some(Token::IMPORT) || decl.specs.len() != 1 {
        return false;
    }
    matches!(&decl.specs[0], Spec::ImportSpec(s) if unquote_path(&s.path.value) == "C")
}

fn contains_any_directive(group: Option<&CommentGroup>) -> bool {
    group.is_some_and(|g| {
        g.list.iter().any(|c| {
            rx_directive().is_match(c.text.strip_prefix("//").unwrap_or(""))
        })
    })
}

fn unquote_path(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'`') && b[b.len() - 1] == b[0] {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn comment_group_looks_like_code(group: &CommentGroup) -> bool {
    let src = format!("package p\nfunc _() {{\n{}}}\n", group.text());
    let fset = Arc::new(FileSet::new());
    let mode = guff::parser::Mode(guff::parser::SKIP_OBJECT_RESOLUTION.0);
    let Ok(file) = guff::parser_interface::parse_file(&fset, "", Some(src.as_bytes()), mode) else {
        return false;
    };
    let Some(Decl::FuncDecl(fn_)) = file.decls.first() else {
        return false;
    };
    let Some(body) = &fn_.body else {
        return false;
    };
    body.list.iter().any(|s| !is_trivial_stmt(s))
}

fn is_trivial_stmt(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::ExprStmt(e) => is_ident_path(&e.x),
        Stmt::LabeledStmt(l) => is_trivial_stmt(&l.stmt),
        Stmt::EmptyStmt(_) => true,
        _ => false,
    }
}

fn is_ident_path(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(_) => true,
        Expr::SelectorExpr(s) => is_ident_path(&s.x),
        _ => false,
    }
}

fn ident_equal(expr: &Expr, name: &str) -> bool {
    matches!(expr, Expr::Ident(id) if id.name == name)
}

fn spec_pos(s: &ImportSpec) -> Pos {
    if let Some(n) = &s.name {
        n.pos()
    } else {
        s.path.value_pos
    }
}

fn import_end(s: &ImportSpec) -> Pos {
    if s.end_pos.is_valid() {
        s.end_pos
    } else {
        s.path.end()
    }
}

fn set_import_pos(s: &mut ImportSpec, pos: Pos) {
    if let Some(n) = &mut s.name {
        n.name_pos = pos;
    }
    s.path.value_pos = pos;
    if s.end_pos.is_valid() {
        s.end_pos = pos;
    }
}
