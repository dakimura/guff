//! CFG builder for ineffectual-assignment detection.
//!
//! Port of `github.com/gordonklaus/ineffassign/pkg/ineffassign` control-flow graph.

use std::collections::{HashMap, HashSet};

use guff::ast::{
    AssignStmt, BlockStmt, BranchStmt, Decl, Expr, File, ForStmt, FuncType, GenDecl, Ident, IfStmt,
    IncDecStmt, RangeStmt, ReturnStmt, SelectStmt, Spec, Stmt, SwitchStmt, TypeSwitchStmt, ValueSpec,
};
use guff::token::Token;
use guff_types::arena::ObjectId;

pub fn analyze_file(
    decls: &[Decl],
    defs: &HashMap<u32, Option<ObjectId>>,
    uses: &HashMap<u32, ObjectId>,
    package_escape_objs: &HashSet<ObjectId>,
) -> Vec<(u32, String)> {
    let (roots, blocks, vars) = CfgBuilder::build_file(decls, defs, uses, package_escape_objs);
    CfgChecker::check(&roots, &blocks, &vars)
}

/// Collect package-level `var` objects across all files in a package.
///
/// Upstream ineffassign never flags these from a single function CFG because
/// they escape across functions/files; per-file analysis alone misses decls in
/// sibling files (gin `codec/json.API`).
pub fn package_level_var_objs(
    files: &[File],
    defs: &HashMap<u32, Option<ObjectId>>,
    uses: &HashMap<u32, ObjectId>,
) -> HashSet<ObjectId> {
    let mut out = HashSet::new();
    for file in files {
        for decl in &file.decls {
            let Decl::GenDecl(GenDecl {
                tok: Some(Token::VAR),
                specs,
                ..
            }) = decl
            else {
                continue;
            };
            for spec in specs {
                let Spec::ValueSpec(ValueSpec { names, .. }) = spec else {
                    continue;
                };
                for name in names {
                    if let Some(obj) = resolve_obj_maps(defs, uses, name) {
                        out.insert(obj);
                    }
                }
            }
        }
    }
    out
}

/// The node id of the identifier that declared `obj`.
///
/// `Info.defs` is keyed on that id, so this is the bridge from a go/parser
/// [`Object`](guff::scope::Object) — which owns a *clone* of its declaring
/// node, ids and all — back to a go/types object. Mirrors
/// `ast::scope::Object::pos()`, which picks the same identifier out of the
/// same declarations; only the kinds that can declare a function-local are
/// worth following, because upstream's `bld.vars` holds nothing else.
fn decl_ident_id(obj: &guff::scope::Object) -> Option<u32> {
    use guff::scope::ObjDecl;
    let name = obj.name.as_str();
    match &obj.decl {
        ObjDecl::Field(d) => d.names.iter().find(|n| n.name == name).map(|n| n.id),
        ObjDecl::ValueSpec(d) => d.names.iter().find(|n| n.name == name).map(|n| n.id),
        ObjDecl::AssignStmt(d) => d.lhs.iter().find_map(|x| match x {
            Expr::Ident(id) if id.name == name => Some(id.id),
            _ => None,
        }),
        _ => None,
    }
}

fn resolve_obj_maps(
    defs: &HashMap<u32, Option<ObjectId>>,
    uses: &HashMap<u32, ObjectId>,
    id: &Ident,
) -> Option<ObjectId> {
    uses.get(&id.id)
        .copied()
        .or_else(|| defs.get(&id.id).and_then(|o| *o))
}

#[derive(Default)]
pub struct CfgBuilder {
    pub roots: Vec<BlockId>,
    block: Option<BlockId>,
    blocks: Vec<CfgBlock>,
    vars: HashMap<ObjectId, VarInfo>,
    /// Named result idents per enclosing function (empty = no named results).
    /// Naked `return` uses these; explicit `return …` assigns then uses them
    /// (gordonklaus/ineffassign parity).
    results: Vec<Vec<Ident>>,
    defers: Vec<bool>,
    breaks: BranchStack,
    continues: BranchStack,
    /// `goto` edges keyed by label **name** (guff has no Label ObjectId).
    gotos: HashMap<String, Branch>,
    /// Label of the statement currently being walked, by name. Labels are not
    /// in `defs`/`uses`, so the name is the only key available.
    label_stmt: Option<String>,
    defs: HashMap<u32, Option<ObjectId>>,
    uses: HashMap<u32, ObjectId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockId(usize);

#[derive(Default)]
struct CfgBlock {
    children: Vec<BlockId>,
    ops: HashMap<ObjectId, Vec<Operation>>,
}

#[derive(Clone)]
struct Operation {
    pos: u32,
    name: String,
    assign: bool,
}

#[derive(Default)]
struct VarInfo {
    fundept: i32,
    escapes: bool,
}

#[derive(Default)]
struct BranchStack(Vec<Branch>);

#[derive(Default)]
struct Branch {
    /// Name of the label this loop/switch carries, if any.
    label: Option<String>,
    srcs: Vec<BlockId>,
    dst: Option<BlockId>,
}

impl CfgBuilder {
    pub fn build_file(
        decls: &[Decl],
        defs: &HashMap<u32, Option<ObjectId>>,
        uses: &HashMap<u32, ObjectId>,
        package_escape_objs: &HashSet<ObjectId>,
    ) -> (Vec<BlockId>, Vec<CfgBlock>, HashMap<ObjectId, VarInfo>) {
        let mut b = Self::default();
        b.defs = defs.clone();
        b.uses = uses.clone();
        // Package-level variables escape across functions (e.g. assigned in
        // init, read elsewhere). Upstream ineffassign never flags them as
        // ineffectual from a single function's CFG. Include decls from sibling
        // files via `package_escape_objs`.
        for obj in package_escape_objs {
            b.vars.entry(*obj).or_default().escapes = true;
        }
        for decl in decls {
            if let Decl::GenDecl(GenDecl {
                tok: Some(Token::VAR),
                specs,
                ..
            }) = decl
            {
                for spec in specs {
                    let Spec::ValueSpec(ValueSpec { names, .. }) = spec else {
                        continue;
                    };
                    for name in names {
                        if let Some(obj) = b.resolve_obj(name) {
                            b.vars.entry(obj).or_default().escapes = true;
                        }
                    }
                }
            }
        }
        for decl in decls {
            if let Decl::FuncDecl(f) = decl {
                if f.body.is_some() {
                    b.walk_func(f.recv.as_ref(), &f.ty, f.body.as_ref().unwrap());
                }
            }
        }
        (b.roots, b.blocks, b.vars)
    }

    fn walk_func(
        &mut self,
        recv: Option<&guff::ast::FieldList>,
        typ: &FuncType,
        body: &BlockStmt,
    ) {
        for v in self.vars.values_mut() {
            v.fundept += 1;
        }
        let result_names: Vec<Ident> = typ
            .results
            .as_ref()
            .map(|fl| {
                fl.list
                    .iter()
                    .flat_map(|f| f.names.iter().cloned())
                    .collect()
            })
            .unwrap_or_default();
        self.results.push(result_names);
        self.defers.push(false);
        // Labels are function-scoped; drop leftover goto edges from prior funcs.
        self.gotos.clear();

        let saved = self.block;
        self.new_block();
        self.roots.push(self.block.unwrap());
        if let Some(recv) = recv {
            self.walk_field_list(recv);
        }
        if let Some(params) = &typ.params {
            self.walk_field_list(params);
        }
        if let Some(results) = &typ.results {
            self.walk_field_list(results);
        }
        self.walk_block(body);

        self.block = saved;
        self.results.pop();
        self.defers.pop();
        for v in self.vars.values_mut() {
            v.fundept -= 1;
        }
    }

    fn walk_block(&mut self, block: &BlockStmt) {
        for stmt in &block.list {
            self.walk_stmt(stmt);
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::BlockStmt(b) => self.walk_block(b),
            Stmt::IfStmt(s) => self.walk_if(s),
            Stmt::ForStmt(s) => self.walk_for(s),
            Stmt::RangeStmt(s) => self.walk_range(s),
            Stmt::SwitchStmt(s) => self.walk_switch(s),
            Stmt::TypeSwitchStmt(s) => self.walk_typeswitch(s),
            Stmt::SelectStmt(s) => self.walk_select(s),
            Stmt::AssignStmt(s) => self.walk_assign(s),
            Stmt::IncDecStmt(s) => self.walk_incdec(s),
            Stmt::ReturnStmt(s) => self.walk_return(s),
            Stmt::BranchStmt(s) => self.walk_branch(s),
            Stmt::DeferStmt(s) => {
                self.walk_expr(&s.call.fun);
                for a in &s.call.args {
                    self.walk_expr(a);
                }
                if let Some(last) = self.defers.last_mut() {
                    *last = true;
                }
            }
            Stmt::ExprStmt(s) => self.walk_expr(&s.x),
            Stmt::DeclStmt(d) => {
                if let Decl::GenDecl(g) = &d.decl {
                    self.walk_gendecl(g);
                }
            }
            Stmt::LabeledStmt(s) => {
                // Upstream keys gotos by label Obj; guff tracks labels by name.
                let parents = self.block.map(|b| vec![b]).unwrap_or_default();
                let dst = self.new_block_from(&parents);
                self.goto_set_destination(s.label.name.clone(), dst);
                self.label_stmt = Some(s.label.name.clone());
                self.walk_stmt(&s.stmt);
                self.label_stmt = None;
            }
            Stmt::GoStmt(s) => {
                self.walk_expr(&s.call.fun);
                for a in &s.call.args {
                    self.walk_expr(a);
                }
            }
            Stmt::SendStmt(s) => {
                self.walk_expr(&s.chan_);
                self.walk_expr(&s.value);
            }
            _ => {}
        }
    }

    fn walk_if(&mut self, s: &IfStmt) {
        if let Some(init) = &s.init {
            self.walk_stmt(init);
        }
        self.walk_expr(&s.cond);
        let b0 = self.block;
        self.new_block_from(&[b0.unwrap()]);
        self.walk_block(&s.body);
        let b1 = self.block;
        if let Some(else_) = &s.else_ {
            self.new_block_from(&[b0.unwrap()]);
            self.walk_stmt(else_);
            self.new_block_from(&[self.block.unwrap(), b1.unwrap()]);
        } else {
            self.new_block_from(&[b0.unwrap(), b1.unwrap()]);
        }
    }

    fn walk_for(&mut self, s: &ForStmt) {
        // The label belongs to this loop only: clear it so loops nested in the
        // body do not also claim it and swallow `continue <label>`.
        let lbl = self.take_stmt_label();
        let brek_idx = self.breaks.push(lbl.clone());
        let continu_idx = self.continues.push(lbl);
        if let Some(init) = &s.init {
            self.walk_stmt(init);
        }
        let start = self.new_block_from(&[self.block.unwrap()]);
        if let Some(cond) = &s.cond {
            self.walk_expr(cond);
        }
        let cond = self.block;
        self.new_block_from(&[cond.unwrap()]);
        self.walk_block(&s.body);
        let continu_dst = self.new_block_from(&[self.block.unwrap()]);
        self.continues.set_destination(continu_idx, continu_dst, &mut self.blocks);
        if let Some(post) = &s.post {
            self.walk_stmt(post);
        }
        // Back-edge from the end of the loop (post block) to the condition, so
        // the post-statement's store (`i++`) is seen as used by the next
        // iteration's condition/body. Mirrors `walk_range`; using `cond` here
        // instead of the current block made it a `cond -> cond` self-loop
        // (start == cond), leaving `i++` with no successor and thus falsely
        // "ineffectual" (and staticcheck "value never used") on stepped loops.
        self.block_mut(self.block.unwrap()).children.push(start);
        let brek_dst = self.new_block_from(&[cond.unwrap()]);
        self.breaks.set_destination(brek_idx, brek_dst, &mut self.blocks);
        self.breaks.pop();
        self.continues.pop();
    }

    fn walk_range(&mut self, s: &RangeStmt) {
        let lbl = self.take_stmt_label();
        let brek_idx = self.breaks.push(lbl.clone());
        let continu_idx = self.continues.push(lbl);
        self.walk_expr(&s.x);
        let pre = self.new_block_from(&[self.block.unwrap()]);
        let start = self.new_block_from(&[pre]);
        if let Some(key) = &s.key {
            if let Expr::Ident(id) = key {
                self.assign_ident(id);
            }
        }
        if let Some(Expr::Ident(val)) = &s.value {
            self.assign_ident(val);
        }
        self.walk_block(&s.body);
        self.block_mut(self.block.unwrap()).children.push(start);
        self.continues.set_destination(continu_idx, pre, &mut self.blocks);
        let brek_dst = self.new_block_from(&[pre, self.block.unwrap()]);
        self.breaks.set_destination(brek_idx, brek_dst, &mut self.blocks);
        self.breaks.pop();
        self.continues.pop();
    }

    fn walk_switch(&mut self, s: &SwitchStmt) {
        if let Some(init) = &s.init {
            self.walk_stmt(init);
        }
        if let Some(tag) = &s.tag {
            self.walk_expr(tag);
        }
        self.walk_case_body(&s.body.list);
    }

    fn walk_typeswitch(&mut self, s: &TypeSwitchStmt) {
        if let Some(init) = &s.init {
            self.walk_stmt(init);
        }
        self.walk_stmt(&s.assign);
        self.walk_case_body(&s.body.list);
    }

    fn walk_case_body(&mut self, cases: &[Stmt]) {
        let brek_idx = self.breaks.push(None);
        let b0 = self.block.unwrap();
        let mut exits = Vec::new();
        let mut fallthru = None;
        for case in cases {
            let Stmt::CaseClause(c) = case else { continue };
            let mut parents = vec![b0];
            if !c.list.is_empty() {
                let list = self.new_block_from(&[b0]);
                for x in &c.list {
                    self.walk_expr(x);
                }
                parents = vec![list];
            }
            if let Some(f) = fallthru {
                parents.push(f);
                fallthru = None;
            }
            self.new_block_from(&parents);
            for stmt in &c.body {
                self.walk_stmt(stmt);
                if let Stmt::BranchStmt(BranchStmt { tok: Token::FALLTHROUGH, .. }) = stmt {
                    fallthru = self.block;
                }
            }
            if fallthru.is_none() {
                exits.push(self.block.unwrap());
            }
        }
        exits.push(b0);
        let dst = self.new_block_from(&exits);
        self.breaks.set_destination(brek_idx, dst, &mut self.blocks);
        self.breaks.pop();
    }

    fn walk_select(&mut self, s: &SelectStmt) {
        let brek_idx = self.breaks.push(None);
        let b0 = self.block.unwrap();
        let mut exits = Vec::new();
        for case in &s.body.list {
            let Stmt::CommClause(c) = case else { continue };
            self.new_block_from(&[b0]);
            if let Some(comm) = &c.comm {
                self.walk_stmt(comm);
            }
            for stmt in &c.body {
                self.walk_stmt(stmt);
            }
            exits.push(self.block.unwrap());
        }
        exits.push(b0);
        let dst = self.new_block_from(&exits);
        self.breaks.set_destination(brek_idx, dst, &mut self.blocks);
        self.breaks.pop();
    }

    fn walk_assign(&mut self, s: &AssignStmt) {
        for r in &s.rhs {
            self.walk_expr(r);
        }
        for (i, l) in s.lhs.iter().enumerate() {
            if let Some(id) = ident(l) {
                if matches!(
                    s.tok,
                    Some(
                        Token::AddAssign
                            | Token::SubAssign
                            | Token::MulAssign
                            | Token::QuoAssign
                            | Token::RemAssign
                            | Token::AndAssign
                            | Token::OrAssign
                            | Token::XorAssign
                            | Token::ShlAssign
                            | Token::ShrAssign
                            | Token::AndNotAssign
                    )
                ) {
                    self.use_ident(id);
                }
                if s.tok == Some(Token::DEFINE)
                    && i < s.rhs.len()
                    && is_zero_initializer(&s.rhs[i])
                {
                    self.use_ident(id);
                } else {
                    self.assign_ident(id);
                }
            } else {
                self.walk_expr(l);
            }
        }
    }

    fn walk_gendecl(&mut self, g: &GenDecl) {
        if g.tok != Some(Token::VAR) {
            return;
        }
        for spec in &g.specs {
            let Spec::ValueSpec(ValueSpec { names, values, .. }) = spec else {
                continue;
            };
            for v in values {
                self.walk_expr(v);
            }
            for id in names {
                if values.is_empty() {
                    self.use_ident(id);
                } else {
                    self.assign_ident(id);
                }
            }
        }
    }

    fn walk_incdec(&mut self, s: &IncDecStmt) {
        if let Some(id) = ident(&s.x) {
            self.use_ident(id);
            self.assign_ident(id);
        } else {
            self.walk_expr(&s.x);
        }
    }

    fn walk_return(&mut self, s: &ReturnStmt) {
        for r in &s.results {
            self.walk_expr(r);
        }
        // Named results are always used by a return. Explicit results also
        // assign (overwrite) them — so a prior `x = …` before `return y` is
        // ineffectual, while `x = …; return` (naked) is not.
        if let Some(names) = self.results.last().filter(|n| !n.is_empty()).cloned() {
            let explicit = !s.results.is_empty();
            for id in &names {
                if explicit {
                    self.assign_ident(id);
                }
                self.use_ident(id);
            }
        }
        self.new_block();
    }

    fn walk_branch(&mut self, s: &BranchStmt) {
        match s.tok {
            Token::BREAK => {
                let idx = self.breaks.index_for(s.label.as_ref());
                self.breaks.add_source(idx, self.block.unwrap(), &mut self.blocks);
                self.new_block();
            }
            Token::CONTINUE => {
                let idx = self.continues.index_for(s.label.as_ref());
                self.continues.add_source(idx, self.block.unwrap(), &mut self.blocks);
                self.new_block();
            }
            Token::GOTO => {
                if let Some(label) = s.label.as_ref() {
                    self.goto_add_source(label.name.clone(), self.block.unwrap());
                }
                // Unreachable fall-through after goto (no parents).
                self.new_block();
            }
            _ => {}
        }
    }

    fn walk_expr(&mut self, e: &Expr) {
        match e {
            Expr::Ident(id) => self.use_ident(id),
            Expr::BinaryExpr(b) => {
                self.walk_expr(&b.x);
                self.walk_expr(&b.y);
            }
            Expr::CallExpr(c) => {
                self.maybe_panic();
                self.walk_expr(&c.fun);
                for a in &c.args {
                    self.walk_expr(a);
                }
            }
            Expr::SelectorExpr(s) => {
                if let Some(id) = ident(&s.x) {
                    self.escape(id);
                }
                self.walk_expr(&s.x);
            }
            Expr::UnaryExpr(u) => {
                if u.op == Token::AND {
                    if let Some(id) = ident(&u.x) {
                        self.escape(id);
                    }
                }
                self.walk_expr(&u.x);
            }
            Expr::IndexExpr(i) => {
                self.maybe_panic();
                if let Some(id) = ident(&i.x) {
                    self.escape(id);
                }
                self.walk_expr(&i.x);
                self.walk_expr(&i.index);
            }
            // `x[A, B]` generic instantiation: `x` is a use (the type args are
            // types, no local reads).
            Expr::IndexListExpr(i) => self.walk_expr(&i.x),
            // `x.(T)` type assertion: `x` is a use of the operand. Without this
            // arm a variable used only in a type assertion (e.g. a range var
            // `for _, cfg := range … { cfg.(T) }`) is falsely ineffectual.
            Expr::TypeAssertExpr(t) => self.walk_expr(&t.x),
            Expr::SliceExpr(s) => {
                if let Some(id) = ident(&s.x) {
                    self.escape(id);
                }
                self.walk_expr(&s.x);
                // The index expressions carry real uses (`s[a:b]` reads a, b).
                for idx in [&s.low, &s.high, &s.max].into_iter().flatten() {
                    self.walk_expr(idx);
                }
            }
            Expr::StarExpr(s) => self.walk_expr(&s.x),
            Expr::ParenExpr(p) => self.walk_expr(&p.x),
            Expr::FuncLit(fl) => self.walk_func(None, &fl.ty, &fl.body),
            // Composite-literal elements carry real value uses (and escapes,
            // e.g. `&T{F: &x}`). Without this arm those idents are invisible,
            // so a live variable used only inside a composite literal is
            // falsely flagged ineffectual. Walk each element; keyed elements
            // walk both sides, the key through `walk_composite_key`.
            Expr::CompositeLit(cl) => {
                for elt in &cl.elts {
                    self.walk_expr(elt);
                }
            }
            Expr::KeyValueExpr(kv) => {
                self.walk_composite_key(&kv.key);
                self.walk_expr(&kv.value);
            }
            _ => {}
        }
    }

    /// The key of a keyed composite-literal element.
    ///
    /// Upstream has no `CompositeLit` or `KeyValueExpr` case at all, so its
    /// walk reaches the key like any other expression and `bld.use(id)` looks
    /// it up by `id.Obj` — **go/parser's lexical resolution**, not go/types'.
    /// go/parser cannot tell `T{v: x}` (field name) from `map[K]V{v: x}` (a
    /// read of `v`), so it resolves the key against the enclosing scopes and
    /// binds a struct field key to the local variable of the same name when
    /// one is in scope (go.dev/issue/45160 is the same ambiguity).
    ///
    /// Following go/types instead — which knows the key is a field — makes
    /// guff report an assignment upstream considers used. grafana's
    /// `NewKVStorageBackend` is exactly that shape: a local `searchLookback`
    /// assigned in an `if`, then never read, in a function whose return
    /// literal has a `searchLookback:` **field**.
    ///
    /// guff's parser runs the same resolution (`parser_resolver`
    /// `walk_composite_lit`), so the faithful lookup is `Ident.obj` — mapped
    /// back to an `ObjectId` through the id of the identifier that declared it.
    fn walk_composite_key(&mut self, key: &Expr) {
        if let Expr::Ident(id) = key {
            if let Some(obj) = self.scope_resolved_obj(id) {
                self.new_op_for(obj, id, false);
                return;
            }
        }
        self.walk_expr(key);
    }

    /// The `ObjectId` of whatever go/parser's scopes bound `id` to, or `None`
    /// when it bound nothing this analysis tracks (a package from another file,
    /// a func, a type — upstream's `bld.vars` lookup misses those too).
    fn scope_resolved_obj(&self, id: &Ident) -> Option<ObjectId> {
        let obj = id.obj.lock().ok()?.clone()?;
        let decl_id = decl_ident_id(&obj)?;
        self.defs.get(&decl_id).and_then(|o| *o)
    }

    fn walk_field_list(&mut self, fl: &guff::ast::FieldList) {
        for f in &fl.list {
            // Register param/result idents as uses so they exist in `vars`
            // before nested funcs run. Nested assigns then see fundept > 0 and
            // mark the outer var as escaping (gordonklaus/ineffassign parity —
            // `bld.walk(typ)` visits Field names as Idents → use).
            for id in &f.names {
                self.use_ident(id);
            }
            if let Some(ty) = &f.ty {
                self.walk_expr(ty);
            }
        }
    }

    fn assign_ident(&mut self, id: &Ident) {
        self.new_op(id, true);
    }

    fn use_ident(&mut self, id: &Ident) {
        self.new_op(id, false);
    }

    fn escape(&mut self, id: &Ident) {
        let Some(obj) = self.resolve_obj(id) else {
            return;
        };
        if let Some(v) = self.vars.get_mut(&obj) {
            v.escapes = true;
        }
    }

    /// If a defer is live and an operation might panic+recover, named results
    /// are considered used (gordonklaus/ineffassign `maybePanic`).
    fn maybe_panic(&mut self) {
        if !self.defers.last().copied().unwrap_or(false) {
            return;
        }
        let Some(names) = self.results.last().filter(|n| !n.is_empty()).cloned() else {
            return;
        };
        for id in &names {
            self.use_ident(id);
        }
    }

    fn resolve_obj(&self, id: &Ident) -> Option<ObjectId> {
        self.uses
            .get(&id.id)
            .copied()
            .or_else(|| self.defs.get(&id.id).and_then(|o| *o))
    }

    fn new_op(&mut self, id: &Ident, assign: bool) {
        let Some(obj) = self.resolve_obj(id) else {
            return;
        };
        self.new_op_for(obj, id, assign);
    }

    fn new_op_for(&mut self, obj: ObjectId, id: &Ident, assign: bool) {
        if id.name == "_" {
            return;
        }
        let v = self.vars.entry(obj).or_default();
        v.escapes = v.escapes || v.fundept > 0 || self.block.is_none();
        if let Some(b) = self.block {
            if !v.escapes {
                self.blocks[b.0]
                    .ops
                    .entry(obj)
                    .or_default()
                    .push(Operation {
                        pos: id.name_pos.0 as u32,
                        name: id.name.clone(),
                        assign,
                    });
            }
        }
    }

    fn take_stmt_label(&mut self) -> Option<String> {
        self.label_stmt.take()
    }

    fn goto_add_source(&mut self, name: String, src: BlockId) {
        let br = self.gotos.entry(name).or_default();
        br.srcs.push(src);
        if let Some(dst) = br.dst {
            self.blocks[src.0].children.push(dst);
        }
    }

    fn goto_set_destination(&mut self, name: String, dst: BlockId) {
        let br = self.gotos.entry(name).or_default();
        br.dst = Some(dst);
        for src in br.srcs.clone() {
            self.blocks[src.0].children.push(dst);
        }
    }

    fn new_block(&mut self) -> BlockId {
        let id = BlockId(self.blocks.len());
        self.blocks.push(CfgBlock::default());
        self.block = Some(id);
        id
    }

    fn new_block_from(&mut self, parents: &[BlockId]) -> BlockId {
        let id = self.new_block();
        for p in parents {
            self.block_mut(*p).children.push(id);
        }
        id
    }

    fn block_mut(&mut self, id: BlockId) -> &mut CfgBlock {
        &mut self.blocks[id.0]
    }
}

impl BranchStack {
    fn push(&mut self, label: Option<String>) -> usize {
        self.0.push(Branch {
            label,
            ..Default::default()
        });
        self.0.len() - 1
    }

    fn pop(&mut self) {
        self.0.pop();
    }

    /// Innermost enclosing branch target for `break`/`continue`.
    ///
    /// A labelled branch must resolve to the loop carrying that label, not the
    /// innermost one: `continue walk` from a nested loop jumps to the outer
    /// header, where the values assigned just before it are read (grafana
    /// `pipeline/tree.getValue`, which this reported as ineffectual).
    fn index_for(&self, label: Option<&Ident>) -> usize {
        let last = self.0.len().saturating_sub(1);
        let Some(label) = label else {
            return last;
        };
        for (i, br) in self.0.iter().enumerate().rev() {
            if br.label.as_deref() == Some(label.name.as_str()) {
                return i;
            }
        }
        last
    }

    fn set_destination(&mut self, idx: usize, dst: BlockId, blocks: &mut [CfgBlock]) {
        let br = &mut self.0[idx];
        br.dst = Some(dst);
        for src in br.srcs.clone() {
            blocks[src.0].children.push(dst);
        }
    }

    fn add_source(&mut self, idx: usize, src: BlockId, blocks: &mut [CfgBlock]) {
        let br = &mut self.0[idx];
        br.srcs.push(src);
        if let Some(dst) = br.dst {
            blocks[src.0].children.push(dst);
        }
    }
}

pub struct CfgChecker {
    seen: HashMap<BlockId, ()>,
    ineff: Vec<(u32, String)>,
}

impl CfgChecker {
    pub fn check(roots: &[BlockId], blocks: &[CfgBlock], vars: &HashMap<ObjectId, VarInfo>) -> Vec<(u32, String)> {
        let mut chk = Self {
            seen: HashMap::new(),
            ineff: Vec::new(),
        };
        for root in roots {
            chk.check_block(*root, blocks, vars);
        }
        chk.ineff
    }

    fn check_block(&mut self, b: BlockId, blocks: &[CfgBlock], vars: &HashMap<ObjectId, VarInfo>) {
        if self.seen.contains_key(&b) {
            return;
        }
        self.seen.insert(b, ());

        let block = &blocks[b.0];
        // Sort by (pos, name) so multi-var assignment sites report in a stable order
        // (HashMap iteration order previously flipped names across -j / RAYON modes).
        let mut ops_by_obj: Vec<_> = block.ops.iter().collect();
        ops_by_obj.sort_by(|(_, a), (_, b)| {
            let a_pos = a.first().map(|o| o.pos).unwrap_or(0);
            let b_pos = b.first().map(|o| o.pos).unwrap_or(0);
            a_pos.cmp(&b_pos).then_with(|| {
                let a_name = a.first().map(|o| o.name.as_str()).unwrap_or("");
                let b_name = b.first().map(|o| o.name.as_str()).unwrap_or("");
                a_name.cmp(b_name)
            })
        });
        for (obj, ops) in ops_by_obj {
            'ops: for (i, op) in ops.iter().enumerate() {
                if !op.assign {
                    continue;
                }
                if i + 1 < ops.len() {
                    if ops[i + 1].assign {
                        self.ineff.push((op.pos, format!("ineffectual assignment to {}", op.name)));
                    }
                    continue;
                }
                let mut seen_blocks = HashMap::new();
                for child in &block.children {
                    if used(*obj, *child, blocks, &mut seen_blocks) {
                        continue 'ops;
                    }
                }
                if !vars.get(obj).is_some_and(|v| v.escapes) {
                    self.ineff.push((op.pos, format!("ineffectual assignment to {}", op.name)));
                }
            }
        }
        for child in block.children.clone() {
            self.check_block(child, blocks, vars);
        }
    }
}

fn used(obj: ObjectId, b: BlockId, blocks: &[CfgBlock], seen: &mut HashMap<BlockId, ()>) -> bool {
    if seen.contains_key(&b) {
        return false;
    }
    seen.insert(b, ());
    let block = &blocks[b.0];
    if let Some(ops) = block.ops.get(&obj) {
        if let Some(first) = ops.first() {
            return !first.assign;
        }
    }
    for child in &block.children {
        if used(obj, *child, blocks, seen) {
            return true;
        }
    }
    false
}

fn ident(e: &Expr) -> Option<&Ident> {
    match e {
        Expr::Ident(id) => Some(id),
        Expr::ParenExpr(p) => ident(&p.x),
        _ => None,
    }
}

fn is_zero_initializer(e: &Expr) -> bool {
    // Match gordonklaus/ineffassign: treat a single-arg call whose fun looks
    // like a type name as a conversion, then check the arg (no types available).
    let e = match e {
        Expr::CallExpr(c) if c.args.len() == 1 => {
            let mut fun = c.fun.as_ref();
            if let Expr::ParenExpr(p) = fun {
                fun = p.x.as_ref();
            }
            if let Expr::StarExpr(s) = fun {
                fun = s.x.as_ref();
            }
            match fun {
                Expr::Ident(_)
                | Expr::SelectorExpr(_)
                | Expr::ArrayType(_)
                | Expr::StructType(_)
                | Expr::FuncType(_)
                | Expr::InterfaceType(_)
                | Expr::MapType(_)
                | Expr::ChanType(_) => &c.args[0],
                _ => return false,
            }
        }
        other => other,
    };
    match e {
        Expr::BasicLit(l) => matches!(l.value.as_str(), "0" | "0.0" | "0." | ".0" | "\"\""),
        Expr::Ident(id) => {
            (id.name == "false" || id.name == "nil") && id.obj.lock().unwrap().is_none()
        }
        _ => false,
    }
}
