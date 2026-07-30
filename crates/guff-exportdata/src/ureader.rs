//! Port of `internal/gcimporter/ureader_yes.go`.

use rustc_hash::FxHashMap as HashMap;
use std::sync::Arc;

use guff::position::FileSet;
use guff_types::alias::{alias_set_type_params, new_alias};
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::{lookup_basic, init_universe, BasicKind, BASIC_KIND_COUNT};
use guff_types::chan::ChanDir;
use guff_types::importer::{ImportCtx, Importer};
use guff_types::instantiate::instantiate;
use guff_types::interface::{interface_compute_typeset, interface_mark_implicit, new_interface_type};
use guff_types::named::{add_method, named_set_type_params, new_named, set_underlying};
use guff_types::object::const_::new_const;
use guff_types::object::func::new_func;
use guff_types::object::type_name::new_type_name;
use guff_types::object::var::{new_field, new_param, new_var, VarKind};
use guff_types::package::new_package;
use guff_types::scope::{insert as scope_insert, lookup as scope_lookup};
use guff_types::signature::{new_signature_type, signature_set_recv_type_params, signature_set_type_params};
use guff_types::tuple::new_tuple;
use guff_types::typelists::bind_tparams;
use guff_types::typeparam::{new_type_param, set_constraint};
use guff_types::union::new_term;
use guff_types::universe::Universe;
use guff_types::{
    new_array, new_chan, new_map, new_pointer, new_slice, new_struct, new_union, Context, ObjectId,
    PackageId, TypeId,
};

use crate::fake_fileset::FakeFileSet;
use crate::pkgbits::{
    CodeObj, CodeType, Decoder, Index, PkgDecoder, RelocKind, SyncMarker, PUBLIC_ROOT_IDX,
};
use crate::pkgbits::Field;

#[derive(Clone, Copy)]
struct TypeInfo {
    idx: Index,
    derived: bool,
}

#[derive(Clone, Copy)]
struct DerivedInfo {
    idx: Index,
}

struct ReaderDict {
    bounds: Vec<TypeInfo>,
    tparams: Vec<TypeId>,
    derived: Vec<DerivedInfo>,
    derived_types: Vec<Option<TypeId>>,
}

enum Deferred {
    SetTypeParamConstraints {
        tparams: Vec<TypeId>,
        bounds: Vec<TypeId>,
    },
    SetNamedUnderlying {
        named: TypeId,
        rhs: TypeId,
    },
}

struct PkgState<'a, 'imp, 'u, 'ctx> {
    fake: FakeFileSet,
    ctxt: Context,
    imports: HashMap<String, PackageId>,
    universe: &'u Universe,
    ctx: &'ctx mut ImportCtx<'a>,
    importer: &'imp mut dyn Importer,
    unsafe_pkg: PackageId,
    pos_bases: Vec<String>,
    pkgs: Vec<Option<PackageId>>,
    typs: Vec<Option<TypeId>>,
    later: Vec<Deferred>,
    ifaces: Vec<TypeId>,
    /// Objects assigned a provisional [`FakeFileSet`] position during this
    /// import. Their `pos` fields are rewritten to real `FileSet` offsets in
    /// the finalize step (see `set_obj_pos` and the end of
    /// `read_unified_package`). Recorded explicitly rather than by arena index
    /// range because `do_pkg` can recursively import other packages mid-decode,
    /// interleaving their (already-finalized) objects into this arena.
    prov_objs: Vec<ObjectId>,
}

impl PkgState<'_, '_, '_, '_> {
    /// Set an object's declaration position to a provisional [`FakeFileSet`]
    /// handle and remember it so the finalize step can rewrite it to the real
    /// `FileSet` offset once every file's size is known.
    fn set_obj_pos(&mut self, obj: ObjectId, p: u32) {
        obj.set_pos(self.ctx.objects, p);
        if p != 0 {
            self.prov_objs.push(obj);
        }
    }
}

struct Reader<'dec> {
    dec: Decoder<'dec>,
    dict: Option<ReaderDict>,
}

pub fn read_unified_package<'a, 'imp, 'u, 'ctx>(
    importer: &'imp mut dyn Importer,
    ctx: &'ctx mut ImportCtx<'a>,
    universe: &'u Universe,
    imports: HashMap<String, PackageId>,
    fset: Arc<FileSet>,
    data: &[u8],
    path: &str,
) -> Result<PackageId, String> {
    let decoder = PkgDecoder::new(path, data).map_err(|e| e.to_string())?;
    let n_pos = decoder.num_elems(RelocKind::POS_BASE);
    let n_pkg = decoder.num_elems(RelocKind::PKG);
    let n_typ = decoder.num_elems(RelocKind::TYPE);

    let mut state = PkgState {
        fake: FakeFileSet::new(fset),
        ctxt: Context::new(),
        imports,
        universe,
        ctx,
        importer,
        unsafe_pkg: universe.unsafe_pkg,
        pos_bases: vec![String::new(); n_pos],
        pkgs: vec![None; n_pkg],
        typs: vec![None; n_typ],
        later: Vec::new(),
        ifaces: Vec::new(),
        prov_objs: Vec::new(),
    };

    if lookup_basic(state.ctx.types, BasicKind::Int).is_none() {
        let (arena, _) = init_universe();
        *state.ctx.types = arena;
    }

    {
        let mut root = new_reader(&decoder, RelocKind::META, PUBLIC_ROOT_IDX, SyncMarker::PUBLIC);
        let _root_pkg = root.pkg(&mut state);
        if root.dec.version().has(Field::HasInit) {
            let _ = root.dec.bool();
        }
        let n = root.dec.len();
        let mut obj_indices = Vec::with_capacity(n);
        for _ in 0..n {
            root.dec.sync(SyncMarker::OBJECT);
            if root.dec.version().has(Field::DerivedFuncInstance) {
                assert!(!root.dec.bool());
            }
            obj_indices.push(root.dec.reloc(RelocKind::OBJ));
            assert_eq!(root.dec.len(), 0);
        }
        root.dec.sync(SyncMarker::EOF);
        for idx in obj_indices {
            obj_idx(&decoder, &mut state, idx);
        }
    }

    let mut pending_named = Vec::new();
    for deferred in std::mem::take(&mut state.later) {
        match deferred {
            Deferred::SetTypeParamConstraints { tparams, bounds } => {
                for (tp, bound) in tparams.into_iter().zip(bounds) {
                    set_constraint(state.ctx.types, tp, bound);
                }
            }
            Deferred::SetNamedUnderlying { named, rhs } => {
                let rhs_u = match state.ctx.types.get(rhs) {
                    TypeData::Named(n) => match n.underlying() {
                        Some(u) => u,
                        None => {
                            pending_named.push(Deferred::SetNamedUnderlying { named, rhs });
                            continue;
                        }
                    },
                    _ => rhs.underlying(state.ctx.types),
                };
                let underlying = prepare_named_underlying(&mut state, named, rhs_u);
                set_underlying(state.ctx.types, named, underlying);
            }
        }
    }
    if !pending_named.is_empty() {
        state.later.extend(pending_named);
        // Second pass after all type names from the export blob are read.
        for deferred in std::mem::take(&mut state.later) {
            if let Deferred::SetNamedUnderlying { named, rhs } = deferred {
                let rhs_u = match state.ctx.types.get(rhs) {
                    TypeData::Named(n) => n.underlying().unwrap_or(rhs),
                    _ => rhs.underlying(state.ctx.types),
                };
                let underlying = prepare_named_underlying(&mut state, named, rhs_u);
                set_underlying(state.ctx.types, named, underlying);
            }
        }
    }

    for iface in std::mem::take(&mut state.ifaces) {
        interface_compute_typeset(state.ctx.types, state.ctx.objects, state.ctx.packages, iface);
    }

    // Register the fake files (sized to their actual line counts) in the shared
    // FileSet, then rewrite every provisional object position to the real
    // offset now that the per-file bases are known.
    let bases = state.fake.finalize();
    for obj in std::mem::take(&mut state.prov_objs) {
        let prov = obj.pos(state.ctx.objects);
        let real = state.fake.translate(&bases, prov);
        obj.set_pos(state.ctx.objects, real);
    }

    let root_id = *state
        .imports
        .get(path)
        .ok_or_else(|| format!("package {path} missing from imports map after read"))?;
    state.ctx.packages.get_mut(root_id).mark_complete();

    let mut imps: Vec<PackageId> = state
        .pkgs
        .iter()
        .filter_map(|p| *p)
        .filter(|&p| p != root_id)
        .collect();
    imps.sort_by_key(|p| state.ctx.packages.get(*p).path().to_string());
    imps.dedup();
    state.ctx.packages.get_mut(root_id).set_imports(imps);

    Ok(root_id)
}

fn new_reader<'dec>(
    decoder: &'dec PkgDecoder,
    k: RelocKind,
    idx: Index,
    marker: SyncMarker,
) -> Reader<'dec> {
    Reader {
        dec: decoder.new_decoder(k, idx, marker),
        dict: None,
    }
}

fn temp_reader<'dec>(
    decoder: &'dec PkgDecoder,
    k: RelocKind,
    idx: Index,
    marker: SyncMarker,
) -> Reader<'dec> {
    Reader {
        dec: decoder.temp_decoder(k, idx, marker),
        dict: None,
    }
}

fn pos_base_idx(decoder: &PkgDecoder, state: &mut PkgState<'_, '_, '_, '_>, idx: Index) -> String {
    let i = idx.0 as usize;
    if !state.pos_bases[i].is_empty() {
        return state.pos_bases[i].clone();
    }
    let mut r = temp_reader(decoder, RelocKind::POS_BASE, idx, SyncMarker::POS_BASE);
    let filename = r.dec.string();
    let _ = r.dec.bool();
    decoder.retire_decoder(r.dec);
    state.pos_bases[i] = filename.clone();
    filename
}

fn pkg_idx(decoder: &PkgDecoder, state: &mut PkgState<'_, '_, '_, '_>, idx: Index) -> Option<PackageId> {
    let i = idx.0 as usize;
    if let Some(pkg) = state.pkgs[i] {
        return Some(pkg);
    }
    let mut r = new_reader(decoder, RelocKind::PKG, idx, SyncMarker::PKG_DEF);
    let pkg = r.do_pkg(state);
    state.pkgs[i] = pkg;
    pkg
}

fn typ_idx(
    decoder: &PkgDecoder,
    state: &mut PkgState<'_, '_, '_, '_>,
    info: TypeInfo,
    dict: Option<&ReaderDict>,
) -> TypeId {
    let (idx, derived_slot) = if info.derived {
        let dict = dict.expect("derived type requires dict");
        let slot = info.idx.0 as usize;
        (dict.derived[slot].idx, Some((dict, slot)))
    } else {
        (info.idx, None)
    };

    let i = idx.0 as usize;
    if let Some(existing) = state.typs[i] {
        return existing;
    }

    if let Some((dict, slot)) = derived_slot {
        if let Some(t) = dict.derived_types[slot] {
            return t;
        }
    }

    let mut r = temp_reader(decoder, RelocKind::TYPE, idx, SyncMarker::TYPE_IDX);
    if let Some((dict, _)) = derived_slot {
        r.dict = Some(ReaderDict {
            bounds: dict.bounds.clone(),
            tparams: dict.tparams.clone(),
            derived: dict.derived.clone(),
            derived_types: dict.derived_types.clone(),
        });
    }
    let typ = r.do_typ(decoder, state);
    decoder.retire_decoder(r.dec);

    if state.typs[i].is_none() {
        state.typs[i] = Some(typ);
    }
    state.typs[i].unwrap()
}

fn obj_idx(decoder: &PkgDecoder, state: &mut PkgState<'_, '_, '_, '_>, idx: Index) -> (Option<PackageId>, String) {
    let mut rname = temp_reader(decoder, RelocKind::NAME, idx, SyncMarker::OBJECT1);
    let (obj_pkg, obj_name) = rname.qualified_ident(state);
    let tag = code_obj_from_usize(rname.dec.code(SyncMarker::CODE_OBJ));
    decoder.retire_decoder(rname.dec);

    if tag == CodeObj::Stub || !split_vargen_suffix(&obj_name).1.is_empty() {
        return (obj_pkg, obj_name);
    }

    let scope = pkg_scope(state.ctx, obj_pkg);
    if scope_lookup(state.ctx.scopes, scope, &obj_name).is_none() {
        read_object(decoder, state, idx, tag, obj_pkg, &obj_name);
    }

    (obj_pkg, obj_name)
}

fn read_object(
    decoder: &PkgDecoder,
    state: &mut PkgState<'_, '_, '_, '_>,
    idx: Index,
    tag: CodeObj,
    obj_pkg: Option<PackageId>,
    obj_name: &str,
) {
    let dict = obj_dict_idx(decoder, state, idx);
    let mut r = new_reader(decoder, RelocKind::OBJ, idx, SyncMarker::OBJECT1);
    r.dict = Some(dict);
    let scope = pkg_scope(state.ctx, obj_pkg);

    match tag {
        CodeObj::Alias => {
            let p = r.pos(decoder, state);
            let mut tparams = Vec::new();
            if r.dec.version().has(Field::AliasTypeParamNames) {
                tparams = r.type_param_names(decoder, state);
            }
            let typ = r.read_typ(decoder, state);
            let tn = new_type_name(state.ctx.objects, obj_name, None);
            state.set_obj_pos(tn, p);
            if let Some(pkg) = obj_pkg {
                tn.set_pkg(state.ctx.objects, pkg);
            }
            let alias = new_alias(state.ctx.types, state.ctx.objects, tn, Some(typ));
            if let Some(list) = bind_tparams(state.ctx.types, tparams) {
                alias_set_type_params(state.ctx.types, alias, list);
            }
            scope_insert(state.ctx.scopes, state.ctx.objects, scope, tn);
        }
        CodeObj::Const => {
            let p = r.pos(decoder, state);
            let typ = r.read_typ(decoder, state);
            let val = r.dec.value();
            let c = new_const(state.ctx.objects, obj_name, typ, val);
            state.set_obj_pos(c, p);
            if let Some(pkg) = obj_pkg {
                c.set_pkg(state.ctx.objects, pkg);
            }
            scope_insert(state.ctx.scopes, state.ctx.objects, scope, c);
        }
        CodeObj::Func => {
            let p = r.pos(decoder, state);
            let tparams = r.type_param_names(decoder, state);
            let sig = r.signature(decoder, state, None, &[], &tparams);
            let f = new_func(state.ctx.objects, obj_name, Some(sig));
            state.set_obj_pos(f, p);
            if let Some(pkg) = obj_pkg {
                f.set_pkg(state.ctx.objects, pkg);
            }
            scope_insert(state.ctx.scopes, state.ctx.objects, scope, f);
        }
        CodeObj::Type => {
            let scope = pkg_scope(state.ctx, obj_pkg);
            let p = r.pos(decoder, state);
            let tn = new_type_name(state.ctx.objects, obj_name, None);
            state.set_obj_pos(tn, p);
            if let Some(pkg) = obj_pkg {
                tn.set_pkg(state.ctx.objects, pkg);
            }
            let named = new_named(state.ctx.types, state.ctx.objects, tn, None, Vec::new());
            scope_insert(state.ctx.scopes, state.ctx.objects, scope, tn);

            let tparams = r.type_param_names(decoder, state);
            if let Some(list) = bind_tparams(state.ctx.types, tparams) {
                named_set_type_params(state.ctx.types, named, list);
            }

            let rhs = r.read_typ(decoder, state);
            let n = r.dec.len();
            let mut methods = Vec::with_capacity(n);
            for _ in 0..n {
                methods.push(r.method(decoder, state));
            }

            match state.ctx.types.get(rhs) {
                TypeData::Named(n) if n.underlying().is_none() => {
                    state.later.push(Deferred::SetNamedUnderlying { named, rhs });
                }
                _ => {
                    let rhs_u = rhs.underlying(state.ctx.types);
                    let underlying = prepare_named_underlying(state, named, rhs_u);
                    set_underlying(state.ctx.types, named, underlying);
                }
            }
            for m in methods {
                add_method(state.ctx.types, state.ctx.objects, named, m);
            }
        }
        CodeObj::Var => {
            let p = r.pos(decoder, state);
            let typ = r.read_typ(decoder, state);
            let v = new_var(state.ctx.objects, obj_name, typ);
            set_var_kind(state.ctx.objects, v, VarKind::Package);
            state.set_obj_pos(v, p);
            if let Some(pkg) = obj_pkg {
                v.set_pkg(state.ctx.objects, pkg);
            }
            scope_insert(state.ctx.scopes, state.ctx.objects, scope, v);
        }
        CodeObj::Stub => {}
    }
}

fn prepare_named_underlying(state: &mut PkgState<'_, '_, '_, '_>, named: TypeId, underlying: TypeId) -> TypeId {
    let method_specs: Vec<(String, TypeId)> = match state.ctx.types.get(underlying) {
        TypeData::Interface(iface) if iface.num_explicit_methods() > 0 => (0..iface.num_explicit_methods())
            .filter_map(|i| {
                let m = iface.explicit_method(i);
                match state.ctx.objects.get(m) {
                    ObjectData::Func(f) => Some((f.name().to_string(), f.typ()?)),
                    _ => None,
                }
            })
            .collect(),
        _ => return underlying,
    };
    if method_specs.is_empty() {
        return underlying;
    }
    let embeds: Vec<TypeId> = match state.ctx.types.get(underlying) {
        TypeData::Interface(iface) => (0..iface.num_embeddeds())
            .map(|i| iface.embedded_type(i))
            .collect(),
        _ => Vec::new(),
    };
    let mut new_methods = Vec::new();
    for (name, sig) in method_specs {
        let recv = new_param(state.ctx.objects, "", named);
        set_var_kind(state.ctx.objects, recv, VarKind::Recv);
        let new_sig = clone_signature_with_recv(state.ctx.types, sig, recv);
        new_methods.push(new_func(state.ctx.objects, name, Some(new_sig)));
    }
    let new_iface = new_interface_type(state.ctx.types, new_methods, embeds);
    state.ifaces.push(new_iface);
    new_iface
}

fn obj_dict_idx(decoder: &PkgDecoder, _state: &mut PkgState<'_, '_, '_, '_>, idx: Index) -> ReaderDict {
    let mut r = temp_reader(decoder, RelocKind::OBJ_DICT, idx, SyncMarker::OBJECT1);
    let implicits = r.dec.len();
    if implicits != 0 {
        decoder.retire_decoder(r.dec);
        return ReaderDict {
            bounds: Vec::new(),
            tparams: Vec::new(),
            derived: Vec::new(),
            derived_types: Vec::new(),
        };
    }
    let n_bounds = r.dec.len();
    let mut bounds = Vec::with_capacity(n_bounds);
    for _ in 0..n_bounds {
        bounds.push(r.typ_info());
    }
    let n_derived = r.dec.len();
    let mut derived = Vec::with_capacity(n_derived);
    for _ in 0..n_derived {
        derived.push(DerivedInfo {
            idx: r.dec.reloc(RelocKind::TYPE),
        });
        if r.dec.version().has(Field::DerivedInfoNeeded) {
            assert!(!r.dec.bool());
        }
    }
    decoder.retire_decoder(r.dec);
    ReaderDict {
        bounds,
        tparams: Vec::new(),
        derived,
        derived_types: vec![None; n_derived],
    }
}

impl<'dec> Reader<'dec> {
    fn pos(&mut self, decoder: &PkgDecoder, state: &mut PkgState<'_, '_, '_, '_>) -> u32 {
        self.dec.sync(SyncMarker::POS);
        if !self.dec.bool() {
            return 0;
        }
        let base = pos_base_idx(decoder, state, self.dec.reloc(RelocKind::POS_BASE));
        let line = self.dec.uint();
        let _col = self.dec.uint();
        state.fake.pos(&base, line as i32, 0)
    }

    fn pkg(&mut self, state: &mut PkgState<'_, '_, '_, '_>) -> Option<PackageId> {
        self.dec.sync(SyncMarker::PKG);
        pkg_idx(self.dec.common, state, self.dec.reloc(RelocKind::PKG))
    }

    fn do_pkg(&mut self, state: &mut PkgState<'_, '_, '_, '_>) -> Option<PackageId> {
        let mut path = self.dec.string();
        match path.as_str() {
            "" | "main" => path = self.dec.common.pkg_path().to_string(),
            "builtin" => return None,
            "unsafe" => return Some(state.unsafe_pkg),
            _ => {}
        }

        if let Some(&pkg) = state.imports.get(&path) {
            return Some(pkg);
        }

        if let Some(pkg) = state.importer.import(state.ctx, &path) {
            state.imports.insert(path.clone(), pkg);
            return Some(pkg);
        }

        let name = self.dec.string();
        let pkg = new_package(
            state.ctx.packages,
            state.ctx.scopes,
            state.ctx.universe_scope,
            path.clone(),
            name,
        );
        state.imports.insert(path, pkg);
        Some(pkg)
    }

    fn typ_info(&mut self) -> TypeInfo {
        self.dec.sync(SyncMarker::TYPE);
        if self.dec.bool() {
            TypeInfo {
                idx: Index(self.dec.len() as i32),
                derived: true,
            }
        } else {
            TypeInfo {
                idx: self.dec.reloc(RelocKind::TYPE),
                derived: false,
            }
        }
    }

    fn read_typ(&mut self, decoder: &PkgDecoder, state: &mut PkgState<'_, '_, '_, '_>) -> TypeId {
        let info = self.typ_info();
        let dict = self.dict.as_ref();
        typ_idx(decoder, state, info, dict)
    }

    fn do_typ(&mut self, decoder: &PkgDecoder, state: &mut PkgState<'_, '_, '_, '_>) -> TypeId {
        let tag = code_type_from_usize(self.dec.code(SyncMarker::TYPE));
        match tag {
            CodeType::Basic => {
                let kind = basic_kind_from_index(self.dec.len());
                lookup_basic(state.ctx.types, kind)
                    .expect("predeclared basic type missing from checker arena")
            }
            CodeType::Named => {
                let (obj, targs) = self.obj(decoder, state);
                let name_typ = obj.typ(state.ctx.objects).expect("typename type");
                if targs.is_empty() {
                    name_typ
                } else {
                    instantiate(
                        state.ctx.types,
                        state.ctx.objects,
                        &mut state.ctxt,
                        name_typ,
                        targs,
                    )
                }
            }
            CodeType::TypeParam => {
                let dict = self.dict.as_ref().expect("type param needs dict");
                dict.tparams[self.dec.len()]
            }
            CodeType::Array => {
                let len = self.dec.uint64() as i64;
                let elem = self.read_typ(decoder, state);
                new_array(state.ctx.types, elem, len)
            }
            CodeType::Chan => {
                let dir = match self.dec.len() {
                    1 => ChanDir::SendOnly,
                    2 => ChanDir::RecvOnly,
                    _ => ChanDir::SendRecv,
                };
                let elem = self.read_typ(decoder, state);
                new_chan(state.ctx.types, dir, elem)
            }
            CodeType::Map => {
                let key = self.read_typ(decoder, state);
                let val = self.read_typ(decoder, state);
                new_map(state.ctx.types, key, val)
            }
            CodeType::Pointer => {
                let elem = self.read_typ(decoder, state);
                new_pointer(state.ctx.types, elem)
            }
            CodeType::Signature => self.signature(decoder, state, None, &[], &[]),
            CodeType::Slice => {
                let elem = self.read_typ(decoder, state);
                new_slice(state.ctx.types, elem)
            }
            CodeType::Struct => self.struct_type(decoder, state),
            CodeType::Interface => self.interface_type(decoder, state),
            CodeType::Union => self.union_type(decoder, state),
        }
    }

    fn struct_type(&mut self, decoder: &PkgDecoder, state: &mut PkgState<'_, '_, '_, '_>) -> TypeId {
        let n = self.dec.len();
        let mut fields = Vec::with_capacity(n);
        let mut tags = Vec::new();
        for i in 0..n {
            let p = self.pos(decoder, state);
            let (pkg, name) = self.selector(state);
            let ftyp = self.read_typ(decoder, state);
            let tag = self.dec.string();
            let embedded = self.dec.bool();
            let f = new_field(state.ctx.objects, name, ftyp, embedded);
            state.set_obj_pos(f, p);
            if let Some(pkg) = pkg {
                f.set_pkg(state.ctx.objects, pkg);
            }
            fields.push(f);
            if !tag.is_empty() {
                while tags.len() < i {
                    tags.push(String::new());
                }
                tags.push(tag);
            }
        }
        new_struct(state.ctx.types, fields, tags)
    }

    fn union_type(&mut self, decoder: &PkgDecoder, state: &mut PkgState<'_, '_, '_, '_>) -> TypeId {
        let n = self.dec.len();
        let mut terms = Vec::with_capacity(n);
        for _ in 0..n {
            let tilde = self.dec.bool();
            terms.push(new_term(tilde, self.read_typ(decoder, state)));
        }
        new_union(state.ctx.types, terms)
    }

    fn interface_type(&mut self, decoder: &PkgDecoder, state: &mut PkgState<'_, '_, '_, '_>) -> TypeId {
        let n_methods = self.dec.len();
        let n_embed = self.dec.len();
        let implicit = n_methods == 0 && n_embed == 1 && self.dec.bool();

        let mut methods = Vec::with_capacity(n_methods);
        for _ in 0..n_methods {
            let p = self.pos(decoder, state);
            let (pkg, name) = self.selector(state);
            let sig = self.signature(decoder, state, None, &[], &[]);
            let f = new_func(state.ctx.objects, name, Some(sig));
            state.set_obj_pos(f, p);
            if let Some(pkg) = pkg {
                f.set_pkg(state.ctx.objects, pkg);
            }
            methods.push(f);
        }

        let mut embeddeds = Vec::with_capacity(n_embed);
        for _ in 0..n_embed {
            embeddeds.push(self.read_typ(decoder, state));
        }

        let iface = new_interface_type(state.ctx.types, methods, embeddeds);
        if implicit {
            interface_mark_implicit(state.ctx.types, iface);
        }
        state.ifaces.push(iface);
        iface
    }

    fn signature(
        &mut self,
        decoder: &PkgDecoder,
        state: &mut PkgState<'_, '_, '_, '_>,
        recv: Option<ObjectId>,
        rtparams: &[TypeId],
        tparams: &[TypeId],
    ) -> TypeId {
        self.dec.sync(SyncMarker::SIGNATURE);
        let params = self.params(decoder, state);
        let results = self.params(decoder, state);
        let variadic = self.dec.bool();
        let sig = new_signature_type(
            state.ctx.types,
            recv,
            &[],
            &[],
            params,
            results,
            variadic,
        );
        if !rtparams.is_empty() {
            if let Some(list) = bind_tparams(state.ctx.types, rtparams.to_vec()) {
                signature_set_recv_type_params(state.ctx.types, sig, list);
            }
        }
        if !tparams.is_empty() {
            if let Some(list) = bind_tparams(state.ctx.types, tparams.to_vec()) {
                signature_set_type_params(state.ctx.types, sig, list);
            }
        }
        let _ = decoder;
        sig
    }

    fn params(&mut self, decoder: &PkgDecoder, state: &mut PkgState<'_, '_, '_, '_>) -> Option<TypeId> {
        self.dec.sync(SyncMarker::PARAMS);
        let n = self.dec.len();
        let mut params = Vec::with_capacity(n);
        for _ in 0..n {
            params.push(self.param(decoder, state));
        }
        new_tuple(state.ctx.types, &params)
    }

    fn param(&mut self, decoder: &PkgDecoder, state: &mut PkgState<'_, '_, '_, '_>) -> ObjectId {
        self.dec.sync(SyncMarker::PARAM);
        let p = self.pos(decoder, state);
        let (pkg, name) = self.local_ident(state);
        let typ = self.read_typ(decoder, state);
        let par = new_param(state.ctx.objects, name, typ);
        state.set_obj_pos(par, p);
        if let Some(pkg) = pkg {
            par.set_pkg(state.ctx.objects, pkg);
        }
        par
    }

    fn obj(&mut self, decoder: &PkgDecoder, state: &mut PkgState<'_, '_, '_, '_>) -> (ObjectId, Vec<TypeId>) {
        self.dec.sync(SyncMarker::OBJECT);
        if self.dec.version().has(Field::DerivedFuncInstance) {
            assert!(!self.dec.bool());
        }
        let idx = self.dec.reloc(RelocKind::OBJ);
        let (pkg, name) = obj_idx(decoder, state, idx);
        let scope = pkg_scope(state.ctx, pkg);
        let obj = scope_lookup(state.ctx.scopes, scope, &name)
            .expect("exported object");
        let n = self.dec.len();
        let mut targs = Vec::with_capacity(n);
        for _ in 0..n {
            targs.push(self.read_typ(decoder, state));
        }
        (obj, targs)
    }

    fn type_param_names(&mut self, decoder: &PkgDecoder, state: &mut PkgState<'_, '_, '_, '_>) -> Vec<TypeId> {
        self.dec.sync(SyncMarker::TYPE_PARAM_NAMES);
        let bounds_snapshot = match self.dict.as_ref() {
            Some(d) if !d.bounds.is_empty() => d.bounds.clone(),
            _ => return Vec::new(),
        };

        let mut tparams = Vec::with_capacity(bounds_snapshot.len());
        for _ in &bounds_snapshot {
            let p = self.pos(decoder, state);
            let (pkg, name) = self.local_ident(state);
            let tn = new_type_name(state.ctx.objects, name, None);
            state.set_obj_pos(tn, p);
            if let Some(pkg) = pkg {
                tn.set_pkg(state.ctx.objects, pkg);
            }
            tparams.push(new_type_param(state.ctx.types, tn, None));
        }

        if let Some(dict) = self.dict.as_mut() {
            dict.tparams = tparams.clone();
        }

        let bounds: Vec<TypeId> = bounds_snapshot
            .iter()
            .map(|b| typ_idx(decoder, state, *b, self.dict.as_ref()))
            .collect();

        state.later.push(Deferred::SetTypeParamConstraints {
            tparams: tparams.clone(),
            bounds,
        });
        tparams
    }

    fn method(&mut self, decoder: &PkgDecoder, state: &mut PkgState<'_, '_, '_, '_>) -> ObjectId {
        self.dec.sync(SyncMarker::METHOD);
        let p = self.pos(decoder, state);
        let (pkg, name) = self.selector(state);
        let rparams = self.type_param_names(decoder, state);
        let recv = self.param(decoder, state);
        let sig = self.signature(decoder, state, Some(recv), &rparams, &[]);
        let f = new_func(state.ctx.objects, name, Some(sig));
        state.set_obj_pos(f, p);
        if let Some(pkg) = pkg {
            f.set_pkg(state.ctx.objects, pkg);
        }
        let _ = self.pos(decoder, state);
        f
    }

    fn qualified_ident(&mut self, state: &mut PkgState<'_, '_, '_, '_>) -> (Option<PackageId>, String) {
        self.ident(SyncMarker::SYM, state)
    }

    fn local_ident(&mut self, state: &mut PkgState<'_, '_, '_, '_>) -> (Option<PackageId>, String) {
        self.ident(SyncMarker::LOCAL_IDENT, state)
    }

    fn selector(&mut self, state: &mut PkgState<'_, '_, '_, '_>) -> (Option<PackageId>, String) {
        self.ident(SyncMarker::SELECTOR, state)
    }

    fn ident(&mut self, marker: SyncMarker, state: &mut PkgState<'_, '_, '_, '_>) -> (Option<PackageId>, String) {
        self.dec.sync(marker);
        let pkg = self.pkg(state);
        let name = self.dec.string();
        (pkg, name)
    }
}

fn pkg_scope(ctx: &ImportCtx<'_>, pkg: Option<PackageId>) -> guff_types::ScopeId {
    match pkg {
        Some(p) => ctx.packages.get(p).scope(),
        None => ctx.universe_scope,
    }
}

fn split_vargen_suffix(name: &str) -> (&str, &str) {
    let mut i = name.len();
    while i > 0 && name.as_bytes()[i - 1].is_ascii_digit() {
        i -= 1;
    }
    const DOT: &str = "·";
    if i >= DOT.len() && &name[i - DOT.len()..i] == DOT {
        i -= DOT.len();
        return (&name[..i], &name[i..]);
    }
    (name, "")
}

fn basic_kind_from_index(i: usize) -> BasicKind {
    if i < BASIC_KIND_COUNT {
        unsafe { std::mem::transmute::<u8, BasicKind>(i as u8) }
    } else {
        BasicKind::Invalid
    }
}

fn set_var_kind(objects: &mut guff_types::arena::ObjectArena, id: ObjectId, kind: VarKind) {
    if let ObjectData::Var(v) = objects.get_mut(id) {
        v.set_kind(kind);
    }
}

fn code_obj_from_usize(v: usize) -> CodeObj {
    match v {
        0 => CodeObj::Alias,
        1 => CodeObj::Const,
        2 => CodeObj::Type,
        3 => CodeObj::Func,
        4 => CodeObj::Var,
        _ => CodeObj::Stub,
    }
}

fn code_type_from_usize(v: usize) -> CodeType {
    match v {
        0 => CodeType::Basic,
        1 => CodeType::Named,
        2 => CodeType::Pointer,
        3 => CodeType::Slice,
        4 => CodeType::Array,
        5 => CodeType::Chan,
        6 => CodeType::Map,
        7 => CodeType::Signature,
        8 => CodeType::Struct,
        9 => CodeType::Interface,
        10 => CodeType::Union,
        11 => CodeType::TypeParam,
        _ => CodeType::Basic,
    }
}

fn clone_signature_with_recv(
    types: &mut guff_types::arena::TypeArena,
    sig: TypeId,
    recv: ObjectId,
) -> TypeId {
    let TypeData::Signature(s) = types.get(sig) else {
        panic!("expected signature");
    };
    new_signature_type(
        types,
        Some(recv),
        &[],
        &[],
        s.params(),
        s.results(),
        s.variadic(),
    )
}
