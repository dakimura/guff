//! Purity fact analyzer — marks functions without side effects.
//!
//! Port of `honnef.co/go/tools/analysis/facts/purity` (v0.7.0, the version
//! golangci-lint 2.12.2 pins).
//!
//! # The one place this is not a straight port
//!
//! Upstream infers purity for **every** package it analyzes — including
//! dependencies — and propagates the answer through object facts. `pureStdlib`
//! is consulted inside `check`, which is only reached for functions in the
//! package currently being analyzed; a call to `strings.TrimSpace` is pure
//! because the fact was exported while analyzing `strings`, not because the
//! caller's pass knows the name.
//!
//! guff builds IR for the root package only (dependencies get member shells
//! with no blocks — see `ssautil::load::build_package_for_analysis`), so there
//! is no body to infer from on the other side of a package boundary. The table
//! is therefore consulted at the *call site* as well, via
//! [`is_pure_stdlib_name`]: for the names upstream ships in `pureStdlib` the
//! two mechanisms agree exactly, because upstream's inference on those packages
//! short-circuits on the same table.
//!
//! What that leaves missing is purity that upstream *infers* across a package
//! boundary — `strings.ReplaceAll` (pure only because its body calls
//! `strings.Replace`), `net/http.StatusText`, or a user-defined helper in an
//! imported package. Those are recorded in
//! `docs/COMPAT-HARDENING.md` §7 rather than papered over here.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::token::Token;
use guff_ssa::function::Function;
use guff_ssa::ids::FuncId;
use guff_ssa::instr::{CallCommon, InstrData};
use guff_ssa::program::Program;
use guff_ssa::value::Value;
use guff_types::arena::{ObjectId, TypeArena};
use guff_types::TypeData;

use crate::analyzer::{AnalysisResult, Analyzer, RunError, RunFn};
use crate::callcheck::static_callee;
use crate::code::type_func_name;
use crate::facts::{Fact, FactTypeId};
use crate::pass::Pass;
use crate::passes::buildir;

/// Fact attached to functions that have no side effects.
///
/// Port of `purity.IsPure`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsPure;

impl Fact for IsPure {
    fn fact_type_id(&self) -> FactTypeId {
        FactTypeId::of::<Self>()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn clone_fact(&self) -> Box<dyn Fact> {
        Box::new(self.clone())
    }

    fn type_name(&self) -> &'static str {
        "IsPure"
    }

    fn encode_payload(&self) -> serde_json::Value {
        serde_json::Value::Object(serde_json::Map::new())
    }
}

fn decode_is_pure(_payload: serde_json::Value) -> Option<Box<dyn Fact>> {
    Some(Box::new(IsPure))
}

/// Register the [`IsPure`] fact decoder (called from the analyzer singleton).
pub(crate) fn register_purity_fact_decoder() {
    crate::fact_codec::register_fact_decoder("IsPure", decode_is_pure);
}

/// Functions known to be pure, keyed by type-checker object.
///
/// Port of `purity.Result`.
#[derive(Clone, Default)]
pub struct PurityResult {
    pub pure: HashSet<ObjectId>,
}

impl PurityResult {
    /// Whether `obj` (a `*types.Func`) is pure — either inferred in this
    /// package or named by upstream's `pureStdlib` table.
    pub fn is_pure(&self, prog: &Program, obj: ObjectId) -> bool {
        if self.pure.contains(&obj) {
            return true;
        }
        is_pure_stdlib(prog, obj)
    }
}

/// `honnef.co/go/tools/analysis/facts/purity.pureStdlib`, verbatim.
///
/// Sorted so the binary search below is valid; keep it that way. Names are
/// `types.Func.FullName()` — `code::type_func_name` renders the same string.
const PURE_STDLIB: &[&str] = &[
    "(*net/http.Request).WithContext",
    "(time.Time).Add",
    "(time.Time).AddDate",
    "(time.Time).After",
    "(time.Time).Before",
    "(time.Time).Clock",
    "(time.Time).Compare",
    "(time.Time).Date",
    "(time.Time).Day",
    "(time.Time).Equal",
    "(time.Time).Format",
    "(time.Time).GoString",
    "(time.Time).GobEncode",
    "(time.Time).Hour",
    "(time.Time).ISOWeek",
    "(time.Time).In",
    "(time.Time).IsDST",
    "(time.Time).IsZero",
    "(time.Time).Local",
    "(time.Time).Location",
    "(time.Time).MarshalBinary",
    "(time.Time).MarshalJSON",
    "(time.Time).MarshalText",
    "(time.Time).Minute",
    "(time.Time).Month",
    "(time.Time).Nanosecond",
    "(time.Time).Round",
    "(time.Time).Second",
    "(time.Time).String",
    "(time.Time).Sub",
    "(time.Time).Truncate",
    "(time.Time).UTC",
    "(time.Time).Unix",
    "(time.Time).UnixMicro",
    "(time.Time).UnixMilli",
    "(time.Time).UnixNano",
    "(time.Time).Weekday",
    "(time.Time).Year",
    "(time.Time).YearDay",
    "(time.Time).Zone",
    "(time.Time).ZoneBounds",
    "errors.New",
    "fmt.Errorf",
    "fmt.Sprint",
    "fmt.Sprintf",
    "sort.Reverse",
    "strings.Map",
    "strings.Repeat",
    "strings.Replace",
    "strings.Title",
    "strings.ToLower",
    "strings.ToLowerSpecial",
    "strings.ToTitle",
    "strings.ToTitleSpecial",
    "strings.ToUpper",
    "strings.ToUpperSpecial",
    "strings.Trim",
    "strings.TrimFunc",
    "strings.TrimLeft",
    "strings.TrimLeftFunc",
    "strings.TrimPrefix",
    "strings.TrimRight",
    "strings.TrimRightFunc",
    "strings.TrimSpace",
    "strings.TrimSuffix",
    "time.Now",
    "time.Parse",
    "time.ParseInLocation",
    "time.Unix",
    "time.UnixMicro",
    "time.UnixMilli",
];

/// Every package that appears in [`PURE_STDLIB`]. A function declared anywhere
/// else cannot be in the table, and checking its package path is a `&str`
/// comparison — much cheaper than rendering a full name for every function in
/// every package of a large corpus.
const PURE_STDLIB_PKGS: &[&str] = &["errors", "fmt", "net/http", "sort", "strings", "time"];

/// Whether `name` (a `types.Func.FullName()`) is in upstream's `pureStdlib`.
pub fn is_pure_stdlib_name(name: &str) -> bool {
    PURE_STDLIB.binary_search(&name).is_ok()
}

/// [`is_pure_stdlib_name`] without rendering the name when the package rules it
/// out. Equivalent to `is_pure_stdlib_name(&full_name(prog, obj))`.
fn is_pure_stdlib(prog: &Program, obj: ObjectId) -> bool {
    let pkg_path = match obj.pkg(&prog.object_arena) {
        Some(pkg) => prog.package_arena.get(pkg).path(),
        // Methods render as `(time.Time).Equal`; their object still belongs to
        // the declaring package, so a missing package really means "not ours".
        None => return false,
    };
    if !PURE_STDLIB_PKGS.contains(&pkg_path) {
        return false;
    }
    is_pure_stdlib_name(&full_name(prog, obj))
}

fn full_name(prog: &Program, obj: ObjectId) -> String {
    type_func_name(
        &prog.type_arena,
        &prog.object_arena,
        &prog.package_arena,
        obj,
    )
}

/// Port of `irutil.IsStub`: a body that does nothing but return or panic with
/// constants.
///
/// honnef's IR materializes constants as instructions (`*ir.Const`) and so has
/// to allow them here; guff models them as `Value::Const` operands, which never
/// appear in an instruction list. The set of accepted bodies is the same.
fn is_stub(func: &Function) -> bool {
    for (_, block) in func.live_blocks() {
        for &iid in &block.instrs {
            match func.instrs.get(iid) {
                InstrData::Panic(_)
                | InstrData::Return(_)
                | InstrData::DebugRef(_)
                | InstrData::Jump(_) => {}
                _ => return false,
            }
        }
    }
    true
}

/// Port of the local `isBasic` in `purity`: basic types, and structs all of
/// whose fields are (transitively) basic.
fn is_basic(arena: &TypeArena, objects: &guff_types::arena::ObjectArena, typ: guff_types::TypeId) -> bool {
    match arena.get(typ.underlying(arena)) {
        TypeData::Basic(_) => true,
        TypeData::Struct(s) => (0..s.num_fields()).all(|i| {
            s.field(i)
                .typ(objects)
                .is_some_and(|ft| is_basic(arena, objects, ft))
        }),
        _ => false,
    }
}

struct Purity<'a> {
    prog: &'a Program,
    root_pkg: guff_ssa::ids::PackageId,
    seen: HashSet<FuncId>,
    pure: HashSet<ObjectId>,
    /// Memoized `check` answers, so a helper called from twenty places is not
    /// re-walked twenty times.
    answers: HashMap<FuncId, bool>,
}

impl<'a> Purity<'a> {
    /// Port of the `check` closure in `purity.purity`.
    fn check(&mut self, fid: FuncId) -> bool {
        let func = self.prog.functions.get(fid);
        // TODO(upstream): closures are unsupported there too.
        let Some(obj) = func.object else {
            return false;
        };
        if self.pure.contains(&obj) {
            return true;
        }
        if func.pkg != Some(self.root_pkg) {
            // Upstream: "function is in another package but wasn't marked as
            // pure, ergo it isn't pure" — the mark being an imported fact. guff
            // has no dependency IR to have produced one, so the stdlib table
            // stands in for the facts upstream would have imported. See the
            // module doc.
            return is_pure_stdlib(self.prog, obj);
        }
        if let Some(&answer) = self.answers.get(&fid) {
            return answer;
        }
        // Break recursion.
        if !self.seen.insert(fid) {
            return false;
        }
        let ret = self.compute(fid, obj);
        self.answers.insert(fid, ret);
        if ret {
            self.pure.insert(obj);
        }
        ret
    }

    fn compute(&mut self, fid: FuncId, obj: ObjectId) -> bool {
        let func = self.prog.functions.get(fid);
        if is_stub(func) {
            return false;
        }
        if is_pure_stdlib(self.prog, obj) {
            return true;
        }
        // A function with no return values is empty or is doing work we cannot
        // see (build tags); don't consider it pure.
        let results_len = func
            .signature
            .and_then(|sig| guff_types::signature::signature_results(&self.prog.type_arena, sig))
            .map(|results| match self.prog.type_arena.get(results) {
                TypeData::Tuple(t) => t.len(),
                _ => 1,
            })
            .unwrap_or(0);
        if results_len == 0 {
            return false;
        }
        for (_, param) in func.params.iter() {
            if !is_basic(&self.prog.type_arena, &self.prog.object_arena, param.typ) {
                return false;
            }
        }
        // Don't consider external functions pure.
        if func.blocks.is_empty() {
            return false;
        }

        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                let ok = match func.instrs.get(iid) {
                    InstrData::Call(c) => self.check_call(fid, &c.call),
                    InstrData::Defer(d) => self.check_call(fid, &d.call),
                    InstrData::Select(_)
                    | InstrData::Send(_)
                    | InstrData::Go(_)
                    | InstrData::Panic(_) => false,
                    InstrData::Store(s) => is_stack_addr(func, s.addr),
                    InstrData::FieldAddr(fa) => is_stack_addr(func, fa.x),
                    // TODO(upstream): make use of proper escape analysis.
                    InstrData::Alloc(a) => !a.heap,
                    // honnef's IR has a dedicated `*ir.Load`; guff models a load
                    // as go/ssa does, `UnOp(MUL)`.
                    InstrData::UnOp(u) if u.op == Token::MUL => is_stack_addr(func, u.x),
                    _ => true,
                };
                if !ok {
                    return false;
                }
            }
        }
        true
    }

    fn check_call(&mut self, fid: FuncId, common: &CallCommon) -> bool {
        if common.method.is_some() {
            // CallCommon.IsInvoke: dynamic dispatch through an interface.
            return false;
        }
        if let Value::Builtin(b) = common.value {
            return matches!(self.prog.builtins.get(b).name.as_str(), "len" | "cap");
        }
        let Some(callee) = static_callee(common) else {
            return false;
        };
        if callee == fid {
            return true;
        }
        self.check(callee)
    }
}

/// Port of the local `isStackAddr`.
fn is_stack_addr(func: &Function, v: Value) -> bool {
    let Value::Instr(iid) = v else {
        return false;
    };
    match func.instrs.get(iid) {
        InstrData::Alloc(a) => !a.heap,
        InstrData::FieldAddr(fa) => is_stack_addr(func, fa.x),
        _ => false,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let ir = pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .ok_or_else(|| "purity requires buildir analyzer".to_string())?;

    let mut state = Purity {
        prog: &ir.prog,
        root_pkg: ir.pkg,
        seen: HashSet::new(),
        pure: HashSet::new(),
        answers: HashMap::new(),
    };
    for &fid in &ir.src_funcs {
        state.check(fid);
    }
    let pure = state.pure;

    // Upstream builds its Result from pass.AllObjectFacts(), which is the union
    // of imported facts and the ones just exported. Exporting keeps the fact
    // visible to the persistent cache and to any future dependency pass; the
    // returned Result is what SA4017 reads.
    for &obj in &pure {
        pass.export_object_fact(obj, Box::new(IsPure));
    }
    let mut result = PurityResult { pure };
    for fact in pass.all_object_facts() {
        if fact.fact.as_any().downcast_ref::<IsPure>().is_some() {
            result.pure.insert(fact.object);
        }
    }
    Ok(Some(Box::new(result)))
}

fn purity_analyzer_impl() -> Analyzer {
    register_purity_fact_decoder();
    Analyzer {
        name: "fact_purity",
        doc: "mark pure functions",
        url: "https://staticcheck.dev/docs/checks/",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![FactTypeId::of::<IsPure>()],
    }
}

/// Purity fact analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(purity_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate;

    #[test]
    fn purity_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn pure_stdlib_table_is_sorted() {
        let mut sorted = PURE_STDLIB.to_vec();
        sorted.sort_unstable();
        assert_eq!(PURE_STDLIB, sorted.as_slice());
    }

    #[test]
    fn pure_stdlib_lookup_matches_full_names() {
        assert!(is_pure_stdlib_name("errors.New"));
        assert!(is_pure_stdlib_name("time.Parse"));
        assert!(is_pure_stdlib_name("(time.Time).Equal"));
        assert!(is_pure_stdlib_name("(*net/http.Request).WithContext"));
        // Never in upstream's table, despite being pure in practice: guff used
        // to carry them and reported SA4017 where golangci-lint does not.
        assert!(!is_pure_stdlib_name("strconv.Itoa"));
        assert!(!is_pure_stdlib_name("strconv.FormatInt"));
        // Inferred upstream (its body calls strings.Replace), not tabled.
        assert!(!is_pure_stdlib_name("strings.ReplaceAll"));
    }
}
