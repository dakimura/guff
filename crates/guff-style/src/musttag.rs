//! Port of [`go-simpler.org/musttag`](https://github.com/go-simpler/musttag)
//! (golangci-lint wrapper in `pkg/golinters/musttag`).
//!
//! Ensures that structs passed to (un)marshal helpers have the expected
//! struct tags on exported fields (e.g. `json` for `encoding/json.Marshal`).
//!
//! Defaults match upstream builtins (json/xml/yaml/toml/mapstructure/sqlx).
//! Custom functions come from `linters.settings.musttag.functions`.
//!
//! DEFERRED: full `types.Implements` iface whitelist via imported interface
//! objects (method-name heuristic used instead); `go mod edit` main-module
//! discovery when `Package.module` is absent (fixture fallback = pkg path).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::lookup::{lookup_field_or_method, LookupResult};
use guff_types::object::is_exported;
use guff_types::TypeId;

use crate::options::{MusttagFunc, MusttagOptions};

/// A function call to look for (upstream `musttag.Func`).
#[derive(Clone, Debug)]
struct Func {
    name: String,
    tag: String,
    arg_pos: usize,
    iface_whitelist: Vec<&'static str>,
}

fn builtins() -> Vec<Func> {
    vec![
        // encoding/json
        Func {
            name: "encoding/json.Marshal".into(),
            tag: "json".into(),
            arg_pos: 0,
            iface_whitelist: vec!["encoding/json.Marshaler", "encoding.TextMarshaler"],
        },
        Func {
            name: "encoding/json.MarshalIndent".into(),
            tag: "json".into(),
            arg_pos: 0,
            iface_whitelist: vec!["encoding/json.Marshaler", "encoding.TextMarshaler"],
        },
        Func {
            name: "encoding/json.Unmarshal".into(),
            tag: "json".into(),
            arg_pos: 1,
            iface_whitelist: vec!["encoding/json.Unmarshaler", "encoding.TextUnmarshaler"],
        },
        Func {
            name: "(*encoding/json.Encoder).Encode".into(),
            tag: "json".into(),
            arg_pos: 0,
            iface_whitelist: vec!["encoding/json.Marshaler", "encoding.TextMarshaler"],
        },
        Func {
            name: "(*encoding/json.Decoder).Decode".into(),
            tag: "json".into(),
            arg_pos: 0,
            iface_whitelist: vec!["encoding/json.Unmarshaler", "encoding.TextUnmarshaler"],
        },
        // encoding/xml
        Func {
            name: "encoding/xml.Marshal".into(),
            tag: "xml".into(),
            arg_pos: 0,
            iface_whitelist: vec!["encoding/xml.Marshaler", "encoding.TextMarshaler"],
        },
        Func {
            name: "encoding/xml.MarshalIndent".into(),
            tag: "xml".into(),
            arg_pos: 0,
            iface_whitelist: vec!["encoding/xml.Marshaler", "encoding.TextMarshaler"],
        },
        Func {
            name: "encoding/xml.Unmarshal".into(),
            tag: "xml".into(),
            arg_pos: 1,
            iface_whitelist: vec!["encoding/xml.Unmarshaler", "encoding.TextUnmarshaler"],
        },
        Func {
            name: "(*encoding/xml.Encoder).Encode".into(),
            tag: "xml".into(),
            arg_pos: 0,
            iface_whitelist: vec!["encoding/xml.Marshaler", "encoding.TextMarshaler"],
        },
        Func {
            name: "(*encoding/xml.Decoder).Decode".into(),
            tag: "xml".into(),
            arg_pos: 0,
            iface_whitelist: vec!["encoding/xml.Unmarshaler", "encoding.TextUnmarshaler"],
        },
        Func {
            name: "(*encoding/xml.Encoder).EncodeElement".into(),
            tag: "xml".into(),
            arg_pos: 0,
            iface_whitelist: vec!["encoding/xml.Marshaler", "encoding.TextMarshaler"],
        },
        Func {
            name: "(*encoding/xml.Decoder).DecodeElement".into(),
            tag: "xml".into(),
            arg_pos: 0,
            iface_whitelist: vec!["encoding/xml.Unmarshaler", "encoding.TextUnmarshaler"],
        },
        // gopkg.in/yaml.v3
        Func {
            name: "gopkg.in/yaml.v3.Marshal".into(),
            tag: "yaml".into(),
            arg_pos: 0,
            iface_whitelist: vec!["gopkg.in/yaml.v3.Marshaler", "encoding.TextMarshaler"],
        },
        Func {
            name: "gopkg.in/yaml.v3.Unmarshal".into(),
            tag: "yaml".into(),
            arg_pos: 1,
            iface_whitelist: vec!["gopkg.in/yaml.v3.Unmarshaler", "encoding.TextUnmarshaler"],
        },
        Func {
            name: "(*gopkg.in/yaml.v3.Encoder).Encode".into(),
            tag: "yaml".into(),
            arg_pos: 0,
            iface_whitelist: vec!["gopkg.in/yaml.v3.Marshaler", "encoding.TextMarshaler"],
        },
        Func {
            name: "(*gopkg.in/yaml.v3.Decoder).Decode".into(),
            tag: "yaml".into(),
            arg_pos: 0,
            iface_whitelist: vec!["gopkg.in/yaml.v3.Unmarshaler", "encoding.TextUnmarshaler"],
        },
        // BurntSushi/toml
        Func {
            name: "github.com/BurntSushi/toml.Unmarshal".into(),
            tag: "toml".into(),
            arg_pos: 1,
            iface_whitelist: vec![
                "github.com/BurntSushi/toml.Unmarshaler",
                "encoding.TextUnmarshaler",
            ],
        },
        Func {
            name: "github.com/BurntSushi/toml.Decode".into(),
            tag: "toml".into(),
            arg_pos: 1,
            iface_whitelist: vec![
                "github.com/BurntSushi/toml.Unmarshaler",
                "encoding.TextUnmarshaler",
            ],
        },
        Func {
            name: "github.com/BurntSushi/toml.DecodeFS".into(),
            tag: "toml".into(),
            arg_pos: 2,
            iface_whitelist: vec![
                "github.com/BurntSushi/toml.Unmarshaler",
                "encoding.TextUnmarshaler",
            ],
        },
        Func {
            name: "github.com/BurntSushi/toml.DecodeFile".into(),
            tag: "toml".into(),
            arg_pos: 1,
            iface_whitelist: vec![
                "github.com/BurntSushi/toml.Unmarshaler",
                "encoding.TextUnmarshaler",
            ],
        },
        Func {
            name: "(*github.com/BurntSushi/toml.Encoder).Encode".into(),
            tag: "toml".into(),
            arg_pos: 0,
            iface_whitelist: vec!["encoding.TextMarshaler"],
        },
        Func {
            name: "(*github.com/BurntSushi/toml.Decoder).Decode".into(),
            tag: "toml".into(),
            arg_pos: 0,
            iface_whitelist: vec![
                "github.com/BurntSushi/toml.Unmarshaler",
                "encoding.TextUnmarshaler",
            ],
        },
        // mitchellh/mapstructure
        Func {
            name: "github.com/mitchellh/mapstructure.Decode".into(),
            tag: "mapstructure".into(),
            arg_pos: 1,
            iface_whitelist: vec![],
        },
        Func {
            name: "github.com/mitchellh/mapstructure.DecodeMetadata".into(),
            tag: "mapstructure".into(),
            arg_pos: 1,
            iface_whitelist: vec![],
        },
        Func {
            name: "github.com/mitchellh/mapstructure.WeakDecode".into(),
            tag: "mapstructure".into(),
            arg_pos: 1,
            iface_whitelist: vec![],
        },
        Func {
            name: "github.com/mitchellh/mapstructure.WeakDecodeMetadata".into(),
            tag: "mapstructure".into(),
            arg_pos: 1,
            iface_whitelist: vec![],
        },
        // jmoiron/sqlx (common entry points)
        Func {
            name: "github.com/jmoiron/sqlx.Get".into(),
            tag: "db".into(),
            arg_pos: 1,
            iface_whitelist: vec!["database/sql.Scanner"],
        },
        Func {
            name: "github.com/jmoiron/sqlx.GetContext".into(),
            tag: "db".into(),
            arg_pos: 2,
            iface_whitelist: vec!["database/sql.Scanner"],
        },
        Func {
            name: "github.com/jmoiron/sqlx.Select".into(),
            tag: "db".into(),
            arg_pos: 1,
            iface_whitelist: vec!["database/sql.Scanner"],
        },
        Func {
            name: "github.com/jmoiron/sqlx.SelectContext".into(),
            tag: "db".into(),
            arg_pos: 2,
            iface_whitelist: vec!["database/sql.Scanner"],
        },
        Func {
            name: "github.com/jmoiron/sqlx.StructScan".into(),
            tag: "db".into(),
            arg_pos: 1,
            iface_whitelist: vec!["database/sql.Scanner"],
        },
        Func {
            name: "(*github.com/jmoiron/sqlx.DB).Get".into(),
            tag: "db".into(),
            arg_pos: 0,
            iface_whitelist: vec!["database/sql.Scanner"],
        },
        Func {
            name: "(*github.com/jmoiron/sqlx.DB).Select".into(),
            tag: "db".into(),
            arg_pos: 0,
            iface_whitelist: vec!["database/sql.Scanner"],
        },
        Func {
            name: "(*github.com/jmoiron/sqlx.Row).StructScan".into(),
            tag: "db".into(),
            arg_pos: 0,
            iface_whitelist: vec!["database/sql.Scanner"],
        },
        Func {
            name: "(*github.com/jmoiron/sqlx.Rows).StructScan".into(),
            tag: "db".into(),
            arg_pos: 0,
            iface_whitelist: vec!["database/sql.Scanner"],
        },
    ]
}

fn cut_vendor(path: &str) -> String {
    let (prefix, rest) = if let Some(r) = path.strip_prefix("(*") {
        ("(*", r)
    } else if let Some(r) = path.strip_prefix('(') {
        ("(", r)
    } else {
        ("", path)
    };
    if let Some(i) = rest.rfind("/vendor/") {
        return format!("{prefix}{}", &rest[i + "/vendor/".len()..]);
    }
    if let Some(r) = rest.strip_prefix("vendor/") {
        return format!("{prefix}{r}");
    }
    format!("{prefix}{rest}")
}

fn merge_funcs(extra: &[MusttagFunc]) -> HashMap<String, Func> {
    let mut map: HashMap<String, Func> = HashMap::new();
    for f in builtins() {
        map.insert(f.name.clone(), f);
    }
    for f in extra {
        map.insert(
            f.name.clone(),
            Func {
                name: f.name.clone(),
                tag: f.tag.clone(),
                arg_pos: f.arg_pos,
                iface_whitelist: Vec::new(),
            },
        );
    }
    map
}

fn main_module(pass: &Pass<'_>) -> String {
    if let Some(m) = &pass.pkg().module {
        return m.path.clone();
    }
    // Fixture / no Module metadata: treat the package path as the module root.
    pass.pkg().pkg_path.clone()
}

fn callee_name(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    let info = pass.types_info()?;
    let obj_id = match &*call.fun {
        Expr::Ident(id) => info.uses.get(&id.id).copied()?,
        Expr::SelectorExpr(sel) => info.uses.get(&sel.sel.id).copied()?,
        _ => return None,
    };
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if !matches!(artifacts.objects.get(obj_id), ObjectData::Func(_)) {
        return None;
    }
    let mut name = code::type_func_name(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        obj_id,
    );
    // Fixture typechecks leave Package.path empty → bare "Func". Qualify with
    // the analysis package path so settings keys like `example.com/pkg.F` work.
    if !name.contains('.') && !name.starts_with('(') {
        if let Some(pkg) = obj_id.pkg(&artifacts.objects) {
            if artifacts.packages.get(pkg).path().is_empty()
                && !pass.pkg().pkg_path.is_empty()
            {
                name = format!("{}.{}", pass.pkg().pkg_path, name);
            }
        }
    }
    Some(cut_vendor(&name))
}

fn type_of_expr(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn type_key(pass: &Pass<'_>, typ: TypeId) -> String {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return format!("{typ:?}");
    };
    guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    )
}

/// Approximate `types.Implements` via known interface → method names.
///
/// DEFERRED: look up interface objects from imports and call `api_implements`.
fn implements_whitelisted(pass: &Pass<'_>, typ: TypeId, ifaces: &[&str]) -> bool {
    for iface in ifaces {
        let Some(method) = iface_method_name(iface) else {
            continue;
        };
        if has_method(pass, typ, method) {
            return true;
        }
    }
    false
}

fn iface_method_name(iface: &str) -> Option<&'static str> {
    Some(match iface {
        "encoding/json.Marshaler" => "MarshalJSON",
        "encoding.TextMarshaler" => "MarshalText",
        "encoding/json.Unmarshaler" => "UnmarshalJSON",
        "encoding.TextUnmarshaler" => "UnmarshalText",
        "encoding/xml.Marshaler" => "MarshalXML",
        "encoding/xml.Unmarshaler" => "UnmarshalXML",
        "gopkg.in/yaml.v3.Marshaler" => "MarshalYAML",
        "gopkg.in/yaml.v3.Unmarshaler" => "UnmarshalYAML",
        "github.com/BurntSushi/toml.Unmarshaler" => "UnmarshalTOML",
        "database/sql.Scanner" => "Scan",
        _ => return None,
    })
}

fn has_method(pass: &Pass<'_>, typ: TypeId, name: &str) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    match lookup_field_or_method(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        true,
        None,
        name,
    ) {
        LookupResult::Found { obj, .. } => matches!(artifacts.objects.get(obj), ObjectData::Func(_)),
        _ => false,
    }
}

fn lookup_struct_tag<'a>(tag: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}:");
    for part in tag.split(|c: char| c == ' ' || c == '\t') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(rest) = part.strip_prefix(&prefix) {
            return Some(rest.trim_matches('"'));
        }
    }
    None
}

struct Checker<'a> {
    pass: &'a Pass<'a>,
    main_module: String,
    seen: HashSet<String>,
    iface_whitelist: Vec<&'static str>,
}

impl<'a> Checker<'a> {
    fn is_valid_type(&mut self, typ: TypeId, tag: &str) -> bool {
        let key = type_key(self.pass, typ);
        if !self.seen.insert(key) {
            return true;
        }
        let Some(styp) = self.parse_struct(typ) else {
            return true;
        };
        self.is_valid_struct(styp, tag)
    }

    fn parse_struct(&self, typ: TypeId) -> Option<TypeId> {
        if implements_whitelisted(self.pass, typ, &self.iface_whitelist) {
            return None;
        }
        let artifacts = self.pass.pkg().type_artifacts.as_ref()?;
        let typ = unalias_readonly(&artifacts.types, typ);
        match artifacts.types.get(typ) {
            TypeData::Pointer(p) => self.parse_struct(p.elem()),
            TypeData::Array(a) => self.parse_struct(a.elem()),
            TypeData::Slice(s) => self.parse_struct(s.elem()),
            TypeData::Map(m) => self.parse_struct(m.elem()),
            TypeData::Named(n) => {
                let obj = n.obj();
                let pkg = obj.pkg(&artifacts.objects)?;
                let path = artifacts.packages.get(pkg).path();
                // Fixture typechecks often leave Package.path empty; treat that
                // as the package under analysis (always in the main module).
                let path = if path.is_empty() {
                    self.pass.pkg().pkg_path.as_str()
                } else {
                    path
                };
                if !path.starts_with(&self.main_module) {
                    return None;
                }
                let under = typ.underlying(&artifacts.types);
                match artifacts.types.get(under) {
                    TypeData::Struct(_) => Some(under),
                    _ => None,
                }
            }
            TypeData::Struct(_) => Some(typ),
            _ => None,
        }
    }

    fn is_valid_struct(&mut self, styp: TypeId, tag: &str) -> bool {
        let Some(artifacts) = self.pass.pkg().type_artifacts.as_ref() else {
            return true;
        };
        let TypeData::Struct(s) = artifacts.types.get(styp) else {
            return true;
        };
        for i in 0..s.num_fields() {
            let field = s.field(i);
            let ObjectData::Var(v) = artifacts.objects.get(field) else {
                continue;
            };
            let name = v.name();
            if !is_exported(name) {
                continue;
            }
            let embedded = v.embedded();
            let field_tag = s.tag(i);
            match lookup_struct_tag(field_tag, tag) {
                None => {
                    if !embedded {
                        return false;
                    }
                }
                Some("-") => continue,
                Some(_) => {}
            }
            let field_ty = v.typ();
            if !self.is_valid_type(field_ty, tag) {
                return false;
            }
        }
        true
    }
}

fn check_call(
    pass: &Pass<'_>,
    call: &CallExpr,
    funcs: &HashMap<String, Func>,
    main_mod: &str,
    pending: &mut Vec<(u32, String)>,
) {
    let Some(name) = callee_name(pass, call) else {
        return;
    };
    let Some(fn_desc) = funcs.get(&name) else {
        return;
    };
    if call.args.len() <= fn_desc.arg_pos {
        return;
    }
    let arg = &call.args[fn_desc.arg_pos];
    if let Expr::Ident(id) = arg {
        if id.name == "nil" {
            return;
        }
    }
    let Some(typ) = type_of_expr(pass, arg) else {
        return;
    };
    let mut checker = Checker {
        pass,
        main_module: main_mod.to_string(),
        seen: HashSet::new(),
        iface_whitelist: fn_desc.iface_whitelist.clone(),
    };
    if checker.is_valid_type(typ, &fn_desc.tag) {
        return;
    }
    pending.push((
        arg.pos().0 as u32,
        format!(
            "the given struct should be annotated with the `{}` tag",
            fn_desc.tag
        ),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "musttag requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<MusttagOptions>("musttag")
        .cloned()
        .unwrap_or_default();
    let funcs = merge_funcs(&options.functions);
    let main_mod = main_module(pass);

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::preorder(NodeRef::File(file), |n| {
            if let NodeRef::CallExpr(call) = n {
                check_call(pass, call, &funcs, &main_mod, &mut pending);
            }
            true
        });
    }
    for (pos, msg) in pending {
        pass.reportf(pos, &msg);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "musttag",
        doc: "enforce field tags in (un)marshaled structs",
        url: "https://github.com/go-simpler/musttag",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: Vec::new(),
    })
}
