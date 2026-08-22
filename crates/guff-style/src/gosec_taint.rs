//! Gosec's taint engine — **G702 / G703 / G706 / G710** (SSA).
//!
//! Port of securego/gosec v2.26.1 `taint/taint.go` plus the four rule
//! configurations that drive it (`analyzers/commandinjection.go`,
//! `analyzers/pathtraversal.go`, `analyzers/loginjection.go`,
//! `analyzers/openredirect.go`). One engine, four tables:
//!
//! | id | what | sinks |
//! |---|---|---|
//! | G702 | Command injection | `os/exec.Command*`, `syscall.Exec` / `ForkExec` / `StartProcess`, `os.StartProcess` |
//! | G703 | Path traversal | the `os` / `io/ioutil` / `path/filepath` file API, plus `http.ServeFile{,FS}` |
//! | G706 | Log injection | `log.*`, and `log/slog`'s four level functions — **message argument only** |
//! | G710 | Open redirect | `http.Redirect`, **URL argument only** |
//!
//! ## What a source is
//!
//! Two kinds, and the difference decides most findings.
//!
//! A **function source** (`os.Getenv`, `os.ReadFile`, and the `os.Args` global)
//! taints its result wherever it is called. A **type source** — `*http.Request`,
//! `*url.URL`, `url.Values`, `*bufio.Reader`, `*bufio.Scanner` — taints nothing
//! by itself: only a *parameter* of that type is tainted, and only when the
//! function may be reached from outside the analysed package. `http.NewRequest`
//! with a hard-coded URL is not a source, and that is the point of the
//! distinction ([`Taint::is_tainted`]'s header in upstream).
//!
//! "May be reached from outside" is answered by the call graph: a function with
//! no callers is an entry point, and so is an exported bare function, because a
//! framework can register one through dispatch the graph cannot see. Everything
//! else has to earn its taint from an actual caller — which is how a handler's
//! `*http.Request` reaches a helper three calls down, and how a `string`
//! parameter that was never a source type gets tainted at all.
//!
//! **A field read is the exception**: `r.URL.Path` on a source-typed *parameter*
//! is tainted unconditionally, with no entry-point test
//! (`isFieldAccessTainted`, CASE 1). syncthing's `rhost := r.RemoteAddr` is that
//! shape.
//!
//! ## The call graph, and which functions are even in it
//!
//! Upstream runs `cha.CallGraph(prog)` — and CHA's node set is
//! `ssautil.AllFunctions`, a **reachability** set, not "every function".
//! It is seeded with package-level functions, the methods of *exported* types,
//! and the methods of types that were converted to an interface somewhere
//! (`Program.RuntimeTypes`). A method of an unexported type that is never boxed
//! is in none of those, so it is not in the graph at all — and that decides
//! findings, because `isParameterTainted` reads the graph:
//!
//! ```go
//! type rec struct{}
//! func (p *rec) sink(id string)  { log.Printf("got %s", id) }        // silent
//! func (p *rec) handler(w http.ResponseWriter, r *http.Request) { p.sink(r.URL.Path) }
//! ```
//!
//! `handler` is not a node, so `sink` has no incoming edge and its `id` never
//! learns it came from a request. Make `rec` exported, or register it as an
//! `http.Handler` — syncthing's `crashReceiver` does — and the same code
//! reports. The node set here is [`guff_ssa::ssautil::all_functions`], guff's
//! port of the same walk.
//!
//! Edges come from every `Call` / `Go` / `Defer` in those functions. CHA
//! resolves three kinds of callee and so does this: a static one; an interface
//! invoke, over-approximated to every same-named method of the package; and a
//! call through a **func-typed value**, over-approximated to every bare
//! function whose signature matches. The third is not a corner case —
//! syncthing's `unixConfigDir(…, fileExists)` calls `os.Lstat` through a
//! parameter, and without those edges the path never reaches the sink.
//!
//! ## Deliberate difference: variadic arguments
//!
//! go/ssa packs a variadic call's arguments into a fresh array and passes one
//! `*ssa.Slice`; guff passes them individually and records the spread in
//! `CallCommon::ellipsis`. For an all-arguments sink the answer is the same
//! either way — upstream reaches the elements through the array's `Alloc`
//! referrers. It matters for `CheckArgs`, and there the indices still line up:
//! every index any of these four rules names is a **fixed** parameter
//! (`slog.Warn`'s `msg`, `http.Redirect`'s `url`), which precedes the variadic
//! one.

use std::collections::{HashMap, HashSet};

use guff_analysis::callcheck::{render_type, static_callee};
use guff_analysis::referrers;
use guff_ssa::function::Function;
use guff_ssa::ids::{FuncId, InstrId};
use guff_ssa::instr::{CallCommon, InstrData};
use guff_ssa::program::{value_type_of, Program};
use guff_ssa::value::Value;
use guff_types::arena::TypeData;
use guff_types::TypeId;

/// `maxTaintDepth`.
const MAX_TAINT_DEPTH: u32 = 50;
/// `maxCallerEdges`: CHA fans an interface call out to every implementation, so
/// upstream stops looking after 32 incoming edges.
const MAX_CALLER_EDGES: usize = 32;

/// A type whose *parameters* carry attacker-controlled data.
struct TypeSource {
    pkg: &'static str,
    name: &'static str,
}

/// A function (or package-level variable) whose result is always tainted.
struct FuncSource {
    pkg: &'static str,
    name: &'static str,
}

struct Sink {
    pkg: &'static str,
    method: &'static str,
    /// Argument positions to check. Empty means every argument, which is what
    /// most of gosec's sinks say — `os.WriteFile(path, data, perm)` fires on
    /// tainted *content* as much as on a tainted path.
    check_args: &'static [usize],
}

struct Sanitizer {
    pkg: &'static str,
    method: &'static str,
}

pub(crate) struct TaintRule {
    pub(crate) id: &'static str,
    what: &'static str,
    type_sources: &'static [TypeSource],
    func_sources: &'static [FuncSource],
    sinks: &'static [Sink],
    sanitizers: &'static [Sanitizer],
}

impl TaintRule {
    fn message(&self) -> String {
        format!("{}: {}", self.id, self.what)
    }
}

// ---------------------------------------------------------------------------
// The four configurations
// ---------------------------------------------------------------------------

/// `analyzers/commandinjection.go`. No sanitizers: gosec's own comment says
/// there is no stdlib one, because the fix is to stop building a shell string.
static G702: TaintRule = TaintRule {
    id: "G702",
    what: "Command injection via taint analysis",
    type_sources: &[
        TypeSource { pkg: "net/http", name: "Request" },
        TypeSource { pkg: "bufio", name: "Reader" },
        TypeSource { pkg: "bufio", name: "Scanner" },
    ],
    func_sources: &[
        FuncSource { pkg: "os", name: "Args" },
        FuncSource { pkg: "os", name: "Getenv" },
    ],
    // Detected at command *creation*, not execution, so one `exec.Command` that
    // is later `Run`, `Start`ed and `Wait`ed is one finding.
    sinks: &[
        Sink { pkg: "os/exec", method: "Command", check_args: &[] },
        Sink { pkg: "os/exec", method: "CommandContext", check_args: &[] },
        Sink { pkg: "os", method: "StartProcess", check_args: &[] },
        Sink { pkg: "syscall", method: "Exec", check_args: &[] },
        Sink { pkg: "syscall", method: "ForkExec", check_args: &[] },
        Sink { pkg: "syscall", method: "StartProcess", check_args: &[] },
    ],
    sanitizers: &[],
};

/// `analyzers/pathtraversal.go`. `os.ReadFile` is a source *and* a sink.
static G703: TaintRule = TaintRule {
    id: "G703",
    what: "Path traversal via taint analysis",
    type_sources: &[
        TypeSource { pkg: "net/http", name: "Request" },
        TypeSource { pkg: "net/url", name: "URL" },
        TypeSource { pkg: "bufio", name: "Reader" },
        TypeSource { pkg: "bufio", name: "Scanner" },
    ],
    func_sources: &[
        FuncSource { pkg: "os", name: "Args" },
        FuncSource { pkg: "os", name: "Getenv" },
        FuncSource { pkg: "os", name: "ReadFile" },
    ],
    sinks: &[
        Sink { pkg: "os", method: "Open", check_args: &[] },
        Sink { pkg: "os", method: "OpenFile", check_args: &[] },
        Sink { pkg: "os", method: "Create", check_args: &[] },
        Sink { pkg: "os", method: "ReadFile", check_args: &[] },
        Sink { pkg: "os", method: "WriteFile", check_args: &[] },
        Sink { pkg: "os", method: "Remove", check_args: &[] },
        Sink { pkg: "os", method: "RemoveAll", check_args: &[] },
        Sink { pkg: "os", method: "Rename", check_args: &[] },
        Sink { pkg: "os", method: "Mkdir", check_args: &[] },
        Sink { pkg: "os", method: "MkdirAll", check_args: &[] },
        Sink { pkg: "os", method: "Stat", check_args: &[] },
        Sink { pkg: "os", method: "Lstat", check_args: &[] },
        Sink { pkg: "os", method: "Chmod", check_args: &[] },
        Sink { pkg: "os", method: "Chown", check_args: &[] },
        Sink { pkg: "io/ioutil", method: "ReadFile", check_args: &[] },
        Sink { pkg: "io/ioutil", method: "WriteFile", check_args: &[] },
        Sink { pkg: "io/ioutil", method: "ReadDir", check_args: &[] },
        Sink { pkg: "path/filepath", method: "Walk", check_args: &[] },
        Sink { pkg: "path/filepath", method: "WalkDir", check_args: &[] },
        // Only the path: arg 1 is the *http.Request that made the parameter
        // tainted in the first place, and checking it would fire on every
        // handler that serves a constant file.
        Sink { pkg: "net/http", method: "ServeFile", check_args: &[2] },
        Sink { pkg: "net/http", method: "ServeFileFS", check_args: &[3] },
    ],
    sanitizers: &[
        Sanitizer { pkg: "path/filepath", method: "Clean" },
        Sanitizer { pkg: "path/filepath", method: "Abs" },
        Sanitizer { pkg: "path/filepath", method: "Base" },
        Sanitizer { pkg: "path/filepath", method: "Rel" },
        Sanitizer { pkg: "net/url", method: "PathEscape" },
        Sanitizer { pkg: "path", method: "Base" },
        Sanitizer { pkg: "path", method: "Clean" },
        Sanitizer { pkg: "strconv", method: "Atoi" },
        Sanitizer { pkg: "strconv", method: "ParseInt" },
        Sanitizer { pkg: "strconv", method: "ParseUint" },
        Sanitizer { pkg: "strconv", method: "ParseFloat" },
        Sanitizer { pkg: "strconv", method: "ParseBool" },
    ],
};

/// `analyzers/loginjection.go`.
static G706: TaintRule = TaintRule {
    id: "G706",
    what: "Log injection via taint analysis",
    type_sources: &[
        TypeSource { pkg: "net/http", name: "Request" },
        TypeSource { pkg: "net/url", name: "URL" },
        TypeSource { pkg: "bufio", name: "Reader" },
        TypeSource { pkg: "bufio", name: "Scanner" },
    ],
    func_sources: &[
        FuncSource { pkg: "os", name: "Args" },
        FuncSource { pkg: "os", name: "Getenv" },
    ],
    sinks: &[
        Sink { pkg: "log", method: "Print", check_args: &[] },
        Sink { pkg: "log", method: "Printf", check_args: &[] },
        Sink { pkg: "log", method: "Println", check_args: &[] },
        Sink { pkg: "log", method: "Fatal", check_args: &[] },
        Sink { pkg: "log", method: "Fatalf", check_args: &[] },
        Sink { pkg: "log", method: "Fatalln", check_args: &[] },
        Sink { pkg: "log", method: "Panic", check_args: &[] },
        Sink { pkg: "log", method: "Panicf", check_args: &[] },
        Sink { pkg: "log", method: "Panicln", check_args: &[] },
        // `func Warn(msg string, args ...any)`: both handlers escape the
        // attribute *values*, and only `msg` reaches the output verbatim. So
        // `slog.Warn("static", "path", tainted)` is silent and
        // `slog.Warn(tainted)` is not.
        Sink { pkg: "log/slog", method: "Info", check_args: &[0] },
        Sink { pkg: "log/slog", method: "Warn", check_args: &[0] },
        Sink { pkg: "log/slog", method: "Error", check_args: &[0] },
        Sink { pkg: "log/slog", method: "Debug", check_args: &[0] },
    ],
    sanitizers: &[
        Sanitizer { pkg: "strings", method: "ReplaceAll" },
        Sanitizer { pkg: "strconv", method: "Quote" },
        Sanitizer { pkg: "net/url", method: "QueryEscape" },
        Sanitizer { pkg: "encoding/json", method: "Marshal" },
        Sanitizer { pkg: "encoding/json", method: "MarshalIndent" },
        Sanitizer { pkg: "strconv", method: "Atoi" },
        Sanitizer { pkg: "strconv", method: "Itoa" },
        Sanitizer { pkg: "strconv", method: "ParseInt" },
        Sanitizer { pkg: "strconv", method: "ParseUint" },
        Sanitizer { pkg: "strconv", method: "ParseFloat" },
        Sanitizer { pkg: "strconv", method: "FormatInt" },
        Sanitizer { pkg: "strconv", method: "FormatFloat" },
    ],
};

/// `analyzers/openredirect.go`.
static G710: TaintRule = TaintRule {
    id: "G710",
    what: "Open redirect via taint analysis",
    type_sources: &[
        TypeSource { pkg: "net/http", name: "Request" },
        TypeSource { pkg: "net/url", name: "URL" },
        TypeSource { pkg: "net/url", name: "Values" },
    ],
    func_sources: &[],
    // `http.Redirect(w, r, url, code)`: only the redirect target.
    sinks: &[Sink { pkg: "net/http", method: "Redirect", check_args: &[2] }],
    sanitizers: &[
        Sanitizer { pkg: "net/url", method: "PathEscape" },
        Sanitizer { pkg: "net/url", method: "QueryEscape" },
        Sanitizer { pkg: "strconv", method: "Atoi" },
        Sanitizer { pkg: "strconv", method: "Itoa" },
        Sanitizer { pkg: "strconv", method: "ParseInt" },
        Sanitizer { pkg: "strconv", method: "ParseUint" },
        Sanitizer { pkg: "strconv", method: "FormatInt" },
        Sanitizer { pkg: "strconv", method: "FormatUint" },
    ],
};

pub(crate) static TAINT_RULES: &[&TaintRule] = &[&G702, &G703, &G706, &G710];

// ---------------------------------------------------------------------------
// Call graph
// ---------------------------------------------------------------------------

/// Incoming edges per function: who calls it, and from which instruction.
///
/// `cha.CallGraph` restricted to the functions that have bodies, which under
/// `buildssa` is exactly this package.
struct CallGraph {
    /// `cg.Nodes`: a function absent here has no node at all, which upstream
    /// spells `node == nil` and treats very differently from "a node with no
    /// callers" for a parameter that is not of a source type.
    nodes: HashSet<FuncId>,
    in_edges: HashMap<FuncId, Vec<(FuncId, InstrId)>>,
}

impl CallGraph {
    fn build(prog: &Program, nodes: HashSet<FuncId>) -> Self {
        let funcs: Vec<FuncId> = {
            let mut v: Vec<FuncId> = nodes.iter().copied().collect();
            // Iteration order must not decide which caller edge is examined
            // first once `maxCallerEdges` truncates the list. Names are not
            // unique — two types' methods share one, and so do a function's
            // literals — so the id breaks the tie. `FuncId` has no public
            // `Ord`; its `Debug` is stable for a given arena.
            v.sort_by_cached_key(|f| {
                (prog.functions.get(*f).name.clone(), format!("{f:?}"))
            });
            v
        };
        Self::build_over(prog, &funcs, nodes)
    }

    fn build_over(prog: &Program, funcs: &[FuncId], nodes: HashSet<FuncId>) -> Self {
        // CHA resolves an interface call to every method that could implement
        // it. Restricted to built functions that is "every method of this
        // package with that name" — the same over-approximation, one package
        // wide.
        let mut by_method_name: HashMap<&str, Vec<FuncId>> = HashMap::new();
        // `funcsBySig`: CHA's answer for a call through a func value is every
        // *address-taken* bare function with that signature. Restricting to
        // functions the node set already holds is the same restriction upstream
        // gets from iterating `allFuncs`.
        let mut by_signature: HashMap<String, Vec<FuncId>> = HashMap::new();
        for &fid in funcs {
            let f = prog.functions.get(fid);
            if func_recv_type(prog, f).is_some() {
                by_method_name.entry(f.name.as_str()).or_default().push(fid);
            } else if let Some(key) = f.signature.and_then(|sig| signature_key(prog, sig)) {
                by_signature.entry(key).or_default().push(fid);
            }
        }

        let mut in_edges: HashMap<FuncId, Vec<(FuncId, InstrId)>> = HashMap::new();
        for &caller in funcs {
            let f = prog.functions.get(caller);
            if f.blocks.is_empty() {
                continue;
            }
            for (_, block) in f.live_blocks() {
                for &iid in &block.instrs {
                    let Some(common) = call_common(f.instrs.get(iid)) else {
                        continue;
                    };
                    if let Some(callee) = static_callee(common) {
                        in_edges.entry(callee).or_default().push((caller, iid));
                        continue;
                    }
                    if let Some(method) = common.method {
                        let name = method.name(&prog.object_arena);
                        for &callee in by_method_name.get(name).into_iter().flatten() {
                            in_edges.entry(callee).or_default().push((caller, iid));
                        }
                        continue;
                    }
                    // A call through a func-typed value. `*ssa.Builtin` is not
                    // one — upstream excludes it explicitly.
                    if matches!(common.value, Value::Builtin(_)) {
                        continue;
                    }
                    let Some(key) =
                        signature_key(prog, value_type_of(prog, f, common.value))
                    else {
                        continue;
                    };
                    for &callee in by_signature.get(&key).into_iter().flatten() {
                        in_edges.entry(callee).or_default().push((caller, iid));
                    }
                }
            }
        }
        CallGraph { nodes, in_edges }
    }

    fn callers(&self, fid: FuncId) -> &[(FuncId, InstrId)] {
        self.in_edges.get(&fid).map(Vec::as_slice).unwrap_or(&[])
    }

    fn has_node(&self, fid: FuncId) -> bool {
        self.nodes.contains(&fid)
    }
}

/// The `CallCommon` of any instruction that performs a call.
fn call_common(instr: &InstrData) -> Option<&CallCommon> {
    match instr {
        InstrData::Call(c) => Some(&c.call),
        InstrData::Go(g) => Some(&g.call),
        InstrData::Defer(d) => Some(&d.call),
        _ => None,
    }
}

/// A signature's identity, as `types.Identical` sees it: parameter and result
/// **types**, plus the variadic flag.
///
/// Not the type's own `String()`. Go prints a signature with its parameter
/// names (`func(path string) bool`), so `fileExists`'s own type and the type of
/// the `fileExists func(string) bool` parameter it is passed to render
/// differently while being identical — and every dynamic-call edge through a
/// named parameter would be lost. `types.Identical` ignores names;
/// `typeutil.Map`, which is what CHA keys `funcsBySig` on, uses it.
///
/// The component *types* are still compared by rendering: identical types
/// always render identically, and `identical` wants `&mut TypeArena` (an
/// interface's type set is computed on first use) while this walk holds
/// `&Program` — the same trade `gosec_g118::TypeClasses` makes.
fn signature_key(prog: &Program, sig: TypeId) -> Option<String> {
    let TypeData::Signature(s) = prog.type_arena.get(sig) else {
        return None;
    };
    if s.recv().is_some() {
        return None;
    }
    let mut key = String::from("func(");
    for tuple in [s.params(), s.results()].into_iter() {
        if let Some(tuple) = tuple {
            let n = guff_types::tuple::tuple_len(&prog.type_arena, Some(tuple));
            for i in 0..n {
                let var = guff_types::tuple::tuple_at(&prog.type_arena, tuple, i);
                if let Some(t) = var.typ(&prog.object_arena) {
                    key.push_str(&render_type(
                        &prog.type_arena,
                        &prog.object_arena,
                        &prog.package_arena,
                        t,
                    ));
                }
                key.push(',');
            }
        }
        key.push_str(")(");
    }
    if s.variadic() {
        key.push_str("...");
    }
    Some(key)
}

fn func_recv_type(prog: &Program, func: &Function) -> Option<TypeId> {
    let sig = func.signature?;
    let TypeData::Signature(s) = prog.type_arena.get(sig) else {
        return None;
    };
    let recv = s.recv()?;
    recv.typ(&prog.object_arena)
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// A value in the program: values are function-local handles, so the owning
/// function is part of the identity upstream gets for free from pointers.
type Site = (FuncId, Value);

struct Taint<'a> {
    prog: &'a Program,
    rule: &'a TaintRule,
    cg: &'a CallGraph,
    /// `paramTaintCache`. Only `true` results are cached, exactly as upstream:
    /// a `false` may become `true` once a different caller is explored.
    param_cache: HashSet<(FuncId, usize)>,
}

pub(crate) fn collect_taint(
    prog: &Program,
    src_funcs: &[FuncId],
    reachable: HashSet<FuncId>,
    rules: &[&'static TaintRule],
    pending: &mut Vec<(u32, String)>,
) {
    if rules.is_empty() || src_funcs.is_empty() {
        return;
    }
    let cg = CallGraph::build(prog, reachable);
    for rule in rules {
        let mut taint = Taint {
            prog,
            rule,
            cg: &cg,
            param_cache: HashSet::new(),
        };
        for &fid in src_funcs {
            taint.analyze_function_sinks(fid, pending);
        }
    }
}

impl Taint<'_> {
    fn func(&self, fid: FuncId) -> &Function {
        self.prog.functions.get(fid)
    }

    fn analyze_function_sinks(&mut self, fid: FuncId, pending: &mut Vec<(u32, String)>) {
        let func = self.func(fid);
        if func.blocks.is_empty() {
            return;
        }
        // Collected first: the instruction arena is borrowed from `self.prog`,
        // and judging an argument needs `&mut self` for the parameter cache.
        let mut candidates: Vec<(InstrId, Vec<Value>)> = Vec::new();
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                // `analyzeFunctionSinks` looks at `*ssa.Call` only: a `go` or
                // `defer` of a sink is not one.
                let InstrData::Call(call) = func.instrs.get(iid) else {
                    continue;
                };
                let Some(sink) = self.sink_of(&call.call) else {
                    continue;
                };
                let args: Vec<Value> = if sink.check_args.is_empty() {
                    call.call.args.clone()
                } else {
                    sink.check_args
                        .iter()
                        .filter_map(|&i| call.call.args.get(i).copied())
                        .collect()
                };
                candidates.push((iid, args));
            }
        }
        let mut hits: Vec<InstrId> = Vec::new();
        for (iid, args) in candidates {
            if args
                .into_iter()
                .any(|arg| self.is_tainted(arg, fid, &mut HashSet::new(), 0))
            {
                hits.push(iid);
            }
        }
        let msg = self.rule.message();
        let func = self.func(fid);
        for iid in hits {
            // `SinkPos: call.Pos()` on an `*ssa.Call`, which go/ssa sets to the
            // CallExpr's Lparen — not to the callee.
            if let Some(pos) = func.instr_pos.get(&iid) {
                pending.push((pos.0 as u32, msg.clone()));
            }
        }
    }

    // -- configuration lookups ---------------------------------------------

    /// `(package path, name, has receiver)` of a call's static callee.
    fn callee_of(&self, common: &CallCommon) -> Option<(String, String, bool)> {
        let fid = static_callee(common)?;
        let f = self.prog.functions.get(fid);
        let obj = f.object?;
        let pkg = obj.pkg(&self.prog.object_arena)?;
        Some((
            self.prog.package_arena.get(pkg).path().to_string(),
            obj.name(&self.prog.object_arena).to_string(),
            func_recv_type(self.prog, f).is_some(),
        ))
    }

    /// `isSinkCall`, for the function sinks these four rules declare. None of
    /// them names a `Receiver`, and upstream's function branch requires the
    /// callee to have none either.
    fn sink_of(&self, common: &CallCommon) -> Option<&'static Sink> {
        let (pkg, name, has_recv) = self.callee_of(common)?;
        if has_recv {
            return None;
        }
        self.rule
            .sinks
            .iter()
            .find(|s| s.pkg == pkg && s.method == name)
    }

    fn is_sanitizer_call(&self, common: &CallCommon) -> bool {
        if self.rule.sanitizers.is_empty() {
            return false;
        }
        let Some((pkg, name, has_recv)) = self.callee_of(common) else {
            return false;
        };
        if has_recv {
            return false;
        }
        self.rule
            .sanitizers
            .iter()
            .any(|s| s.pkg == pkg && s.method == name)
    }

    fn is_source_func_call(&self, common: &CallCommon) -> bool {
        let Some((pkg, name, _)) = self.callee_of(common) else {
            return false;
        };
        self.rule
            .func_sources
            .iter()
            .any(|s| s.pkg == pkg && s.name == name)
    }

    /// `isSourceType`. Upstream matches the rendered type against the source
    /// keys and then, for a named type, tries both `pkg.Name` and `*pkg.Name` —
    /// so the `Pointer` flag in the table never actually excludes anything, and
    /// this is the same test done structurally.
    fn is_source_type(&self, t: TypeId) -> bool {
        let mut t = guff_types::alias::unalias_readonly(&self.prog.type_arena, t);
        let mut hops = 0;
        while let TypeData::Pointer(ptr) = self.prog.type_arena.get(t) {
            t = guff_types::alias::unalias_readonly(&self.prog.type_arena, ptr.elem());
            hops += 1;
            if hops > 8 {
                return false;
            }
        }
        let TypeData::Named(n) = self.prog.type_arena.get(t) else {
            return false;
        };
        let obj = n.obj();
        let name = obj.name(&self.prog.object_arena);
        let Some(pkg) = obj.pkg(&self.prog.object_arena) else {
            return false;
        };
        let path = self.prog.package_arena.get(pkg).path();
        self.rule
            .type_sources
            .iter()
            .any(|s| s.name == name && s.pkg == path)
    }

    /// `isContextType`: a tainted `context.Context` argument does not carry
    /// user data into a result, so it never propagates.
    fn is_context_type(&self, t: TypeId) -> bool {
        let mut t = guff_types::alias::unalias_readonly(&self.prog.type_arena, t);
        let mut hops = 0;
        while let TypeData::Pointer(ptr) = self.prog.type_arena.get(t) {
            t = guff_types::alias::unalias_readonly(&self.prog.type_arena, ptr.elem());
            hops += 1;
            if hops > 8 {
                return false;
            }
        }
        let TypeData::Named(n) = self.prog.type_arena.get(t) else {
            return false;
        };
        let obj = n.obj();
        obj.name(&self.prog.object_arena) == "Context"
            && obj
                .pkg(&self.prog.object_arena)
                .is_some_and(|p| self.prog.package_arena.get(p).path() == "context")
    }

    // -- the walk ----------------------------------------------------------

    fn is_tainted(
        &mut self,
        v: Value,
        fid: FuncId,
        visited: &mut HashSet<Site>,
        depth: u32,
    ) -> bool {
        if depth > MAX_TAINT_DEPTH || !visited.insert((fid, v)) {
            return false;
        }
        match v {
            Value::Const(_) | Value::Builtin(_) | Value::Function(_) => false,
            Value::Param(pid) => {
                let idx = self
                    .func(fid)
                    .params
                    .iter()
                    .position(|(id, _)| id == pid);
                self.is_parameter_tainted(fid, idx, visited, depth + 1)
            }
            Value::FreeVar(fvid) => {
                // `isFreeVarTainted` searches the parent for the MakeClosure
                // that created this function and reads the matching binding;
                // guff records that binding on the FreeVar itself.
                let f = self.func(fid);
                let Some(parent) = f.parent else { return false };
                let outer = f.freevars.get(fvid).outer;
                self.is_tainted(outer, parent, visited, depth + 1)
            }
            Value::Global(gid) => {
                let g = self.prog.globals.get(gid);
                let tpkg = self.prog.packages.get(g.pkg).pkg;
                let path = self.prog.package_arena.get(tpkg).path();
                self.rule
                    .func_sources
                    .iter()
                    .any(|s| s.pkg == path && s.name == g.name)
            }
            Value::Instr(iid) => self.is_instr_tainted(iid, fid, visited, depth),
        }
    }

    fn is_instr_tainted(
        &mut self,
        iid: InstrId,
        fid: FuncId,
        visited: &mut HashSet<Site>,
        depth: u32,
    ) -> bool {
        // The instruction arena is borrowed from `self.prog`, which the
        // recursive calls also need; every operand is copied out first.
        enum Step {
            None,
            One(Value),
            Two(Value, Value),
            Many(Vec<Value>),
        }
        let step = {
            let func = self.func(fid);
            match func.instrs.get(iid) {
                InstrData::Call(_) => return self.is_call_tainted(iid, fid, visited, depth),
                InstrData::FieldAddr(_) => {
                    return self.is_field_access_tainted(iid, fid, visited, depth + 1)
                }
                InstrData::Alloc(_) => {
                    return self.is_alloc_tainted(iid, fid, visited, depth);
                }
                InstrData::MakeSlice(_) => {
                    return self.is_make_slice_tainted(iid, fid, visited, depth);
                }
                InstrData::IndexAddr(x) => Step::One(x.x),
                InstrData::UnOp(x) => Step::One(x.x),
                InstrData::BinOp(b) => Step::Two(b.x, b.y),
                InstrData::Phi(p) => Step::Many(p.edges.iter().flatten().copied().collect()),
                InstrData::Extract(e) => Step::One(e.tuple),
                InstrData::TypeAssert(t) => Step::One(t.x),
                InstrData::MakeInterface(m) => Step::One(m.x),
                InstrData::Slice(s) => Step::One(s.x),
                InstrData::Convert(c) => Step::One(c.x),
                InstrData::ChangeType(c) => Step::One(c.x),
                InstrData::Lookup(l) => Step::One(l.x),
                // Every other instruction — `MakeMap`, `MakeChan`, `Field`,
                // `ChangeInterface`, … — falls into upstream's `default`, which
                // does not propagate.
                _ => Step::None,
            }
        };
        match step {
            Step::None => false,
            Step::One(x) => self.is_tainted(x, fid, visited, depth + 1),
            Step::Two(x, y) => {
                self.is_tainted(x, fid, visited, depth + 1)
                    || self.is_tainted(y, fid, visited, depth + 1)
            }
            Step::Many(vs) => vs
                .into_iter()
                .any(|x| self.is_tainted(x, fid, visited, depth + 1)),
        }
    }

    fn is_call_tainted(
        &mut self,
        iid: InstrId,
        fid: FuncId,
        visited: &mut HashSet<Site>,
        depth: u32,
    ) -> bool {
        let (is_invoke, recv, args, callee, is_builtin) = {
            let func = self.func(fid);
            let InstrData::Call(call) = func.instrs.get(iid) else {
                return false;
            };
            let common = &call.call;
            if self.is_sanitizer_call(common) {
                return false;
            }
            if self.is_source_func_call(common) {
                return true;
            }
            (
                common.method.is_some(),
                common.value,
                common.args.clone(),
                static_callee(common),
                matches!(common.value, Value::Builtin(_)),
            )
        };

        if is_invoke {
            // Interface method call: the receiver is `Call.Value`, and the rest
            // of the arguments are checked too.
            if self.is_tainted(recv, fid, visited, depth + 1) {
                return true;
            }
            for arg in &args {
                if self.arg_type_is_context(*arg, fid) {
                    continue;
                }
                if self.is_tainted(*arg, fid, visited, depth + 1) {
                    return true;
                }
            }
            return false;
        }

        let Some(callee) = callee else {
            // `append(dst, tainted...)`, `copy`, … — a builtin has no callee to
            // look inside, and upstream propagates from every argument.
            if is_builtin {
                for arg in &args {
                    if self.is_tainted(*arg, fid, visited, depth + 1) {
                        return true;
                    }
                }
            }
            return false;
        };
        let has_recv = func_recv_type(self.prog, self.prog.functions.get(callee)).is_some();
        let has_body = !self.prog.functions.get(callee).blocks.is_empty();

        if has_recv {
            // Static method call: the receiver is args[0].
            if let Some(&recv_arg) = args.first() {
                if self.is_tainted(recv_arg, fid, visited, depth + 1) {
                    return true;
                }
            }
            if has_body {
                return self.tainted_args_flow_to_return(iid, fid, callee, visited, depth + 1);
            }
            for arg in args.iter().skip(1) {
                if self.arg_type_is_context(*arg, fid) {
                    continue;
                }
                if self.is_tainted(*arg, fid, visited, depth + 1) {
                    return true;
                }
            }
            return false;
        }

        if has_body {
            // A function of this package: upstream looks inside rather than
            // assuming, which is what keeps a constructor that parks its
            // tainted argument in an unrelated field from tainting the result.
            return self.tainted_args_flow_to_return(iid, fid, callee, visited, depth + 1);
        }
        // No body — stdlib and anything else outside the analysed package.
        // Every string helper lands here, and a tainted argument taints the
        // result.
        for arg in &args {
            if self.arg_type_is_context(*arg, fid) {
                continue;
            }
            if self.is_tainted(*arg, fid, visited, depth + 1) {
                return true;
            }
        }
        false
    }

    fn arg_type_is_context(&self, v: Value, fid: FuncId) -> bool {
        let t = value_type_of(self.prog, self.func(fid), v);
        self.is_context_type(t)
    }

    /// `isParameterTainted`.
    fn is_parameter_tainted(
        &mut self,
        fid: FuncId,
        param_idx: Option<usize>,
        visited: &mut HashSet<Site>,
        depth: u32,
    ) -> bool {
        if depth > MAX_TAINT_DEPTH {
            return false;
        }
        if let Some(idx) = param_idx {
            if self.param_cache.contains(&(fid, idx)) {
                return true;
            }
        }
        // `node := a.callGraph.Nodes[fn]`, and the two ways it can be empty
        // mean different things below.
        let has_node = self.cg.has_node(fid);
        let callers = self.cg.callers(fid);
        let is_entry_point = !has_node || callers.is_empty();

        // The index came from a `position` over this same arena, so the lookup
        // cannot miss — but a panic in an analyzer silently drops every finding
        // its worker would have produced (`compat/health.py`), so it is not
        // worth asserting.
        let param_type = param_idx.and_then(|idx| {
            let f = self.func(fid);
            f.params.iter().nth(idx).map(|(_, p)| p.typ)
        });
        if param_type.is_some_and(|t| self.is_source_type(t))
            && (is_entry_point || self.may_have_external_callers(fid))
        {
            if let Some(idx) = param_idx {
                self.param_cache.insert((fid, idx));
            }
            return true;
        }

        if !has_node {
            return false;
        }
        let Some(idx) = param_idx else { return false };
        let sites: Vec<(FuncId, InstrId)> =
            callers.iter().take(MAX_CALLER_EDGES).copied().collect();
        for (caller, iid) in sites {
            let arg = {
                let f = self.func(caller);
                let Some(common) = call_common(f.instrs.get(iid)) else {
                    continue;
                };
                common.args.get(idx).copied()
            };
            let Some(arg) = arg else { continue };
            if self.is_tainted(arg, caller, visited, depth + 1) {
                self.param_cache.insert((fid, idx));
                return true;
            }
        }
        false
    }

    /// `mayHaveExternalCallers`: an exported bare function can be registered by
    /// a framework through dispatch the call graph never sees. Methods are
    /// excluded because CHA resolves interface dispatch to them.
    fn may_have_external_callers(&self, fid: FuncId) -> bool {
        let f = self.func(fid);
        if f.signature.is_none() || func_recv_type(self.prog, f).is_some() {
            return false;
        }
        if f.parent.is_some() {
            return false;
        }
        f.name
            .chars()
            .next()
            .is_some_and(|c| c.is_uppercase())
    }

    /// `isFieldAccessTainted` — field-sensitive, and CASE 1 is why a handler's
    /// `r.RemoteAddr` is tainted without any call-graph question being asked.
    fn is_field_access_tainted(
        &mut self,
        iid: InstrId,
        fid: FuncId,
        visited: &mut HashSet<Site>,
        depth: u32,
    ) -> bool {
        if depth > MAX_TAINT_DEPTH {
            return false;
        }
        let (base, field) = {
            let f = self.func(fid);
            let InstrData::FieldAddr(fa) = f.instrs.get(iid) else {
                return false;
            };
            (fa.x, fa.field)
        };
        let base_type = value_type_of(self.prog, self.func(fid), base);

        // CASE 1: the struct is a parameter of a source type. Every field of an
        // externally-supplied source is tainted, with no entry-point test.
        if self.is_source_type(base_type) {
            if matches!(base, Value::Param(_)) {
                return true;
            }
            return self.is_tainted(base, fid, visited, depth);
        }

        match base {
            // CASE 2/3: the struct came out of a call — look inside for a store
            // to *this* field.
            Value::Instr(base_iid) => {
                let kind = {
                    let f = self.func(fid);
                    match f.instrs.get(base_iid) {
                        InstrData::Call(c) => Some(("call", static_callee(&c.call))),
                        InstrData::Extract(e) => match e.tuple {
                            Value::Instr(tid) => match self.func(fid).instrs.get(tid) {
                                InstrData::Call(c) => Some(("extract", static_callee(&c.call))),
                                _ => Some(("extract", None)),
                            },
                            _ => Some(("extract", None)),
                        },
                        InstrData::Alloc(_) => Some(("alloc", None)),
                        InstrData::UnOp(_) => Some(("unop", None)),
                        InstrData::Phi(_) => Some(("phi", None)),
                        InstrData::FieldAddr(_) => Some(("fieldaddr", None)),
                        _ => None,
                    }
                };
                match kind {
                    Some(("call", callee)) => {
                        if let Some(callee) = callee {
                            if !self.prog.functions.get(callee).blocks.is_empty() {
                                return self.is_field_tainted_via_call(
                                    base_iid, fid, field, callee, visited, depth,
                                );
                            }
                        }
                        self.is_tainted(base, fid, visited, depth)
                    }
                    Some(("extract", callee)) => {
                        if let Some(callee) = callee {
                            if !self.prog.functions.get(callee).blocks.is_empty() {
                                let tuple_iid = {
                                    let f = self.func(fid);
                                    match f.instrs.get(base_iid) {
                                        InstrData::Extract(e) => match e.tuple {
                                            Value::Instr(t) => Some(t),
                                            _ => None,
                                        },
                                        _ => None,
                                    }
                                };
                                if let Some(t) = tuple_iid {
                                    return self.is_field_tainted_via_call(
                                        t, fid, field, callee, visited, depth,
                                    );
                                }
                            }
                        }
                        self.is_tainted(base, fid, visited, depth)
                    }
                    Some(("alloc", _)) => {
                        self.is_field_of_alloc_tainted(base_iid, fid, field, visited, depth)
                    }
                    Some(("unop", _)) => {
                        let inner = {
                            let f = self.func(fid);
                            match f.instrs.get(base_iid) {
                                InstrData::UnOp(u) => u.x,
                                _ => return false,
                            }
                        };
                        self.is_field_tainted_on_value(inner, fid, field, visited, depth)
                    }
                    Some(("phi", _)) => {
                        let edges: Vec<Value> = {
                            let f = self.func(fid);
                            match f.instrs.get(base_iid) {
                                InstrData::Phi(p) => p.edges.iter().flatten().copied().collect(),
                                _ => return false,
                            }
                        };
                        edges.into_iter().any(|e| {
                            self.is_field_tainted_on_value(e, fid, field, visited, depth + 1)
                        })
                    }
                    Some(("fieldaddr", _)) => {
                        self.is_field_access_tainted(base_iid, fid, visited, depth)
                    }
                    _ => self.is_tainted(base, fid, visited, depth),
                }
            }
            _ => self.is_tainted(base, fid, visited, depth),
        }
    }

    fn is_field_tainted_on_value(
        &mut self,
        v: Value,
        fid: FuncId,
        field: usize,
        visited: &mut HashSet<Site>,
        depth: u32,
    ) -> bool {
        if depth > MAX_TAINT_DEPTH {
            return false;
        }
        let Value::Instr(iid) = v else {
            return self.is_tainted(v, fid, visited, depth);
        };
        enum K {
            Call(Option<FuncId>),
            ExtractOf(Option<InstrId>, Option<FuncId>),
            Alloc,
            Phi(Vec<Value>),
            Other,
        }
        let k = {
            let f = self.func(fid);
            match f.instrs.get(iid) {
                InstrData::Call(c) => K::Call(static_callee(&c.call)),
                InstrData::Extract(e) => match e.tuple {
                    Value::Instr(t) => match f.instrs.get(t) {
                        InstrData::Call(c) => K::ExtractOf(Some(t), static_callee(&c.call)),
                        _ => K::Other,
                    },
                    _ => K::Other,
                },
                InstrData::Alloc(_) => K::Alloc,
                InstrData::Phi(p) => K::Phi(p.edges.iter().flatten().copied().collect()),
                _ => K::Other,
            }
        };
        match k {
            K::Call(Some(callee)) if !self.prog.functions.get(callee).blocks.is_empty() => {
                self.is_field_tainted_via_call(iid, fid, field, callee, visited, depth)
            }
            K::Call(_) => self.is_tainted(v, fid, visited, depth),
            K::ExtractOf(Some(t), Some(callee))
                if !self.prog.functions.get(callee).blocks.is_empty() =>
            {
                self.is_field_tainted_via_call(t, fid, field, callee, visited, depth)
            }
            K::ExtractOf(..) => self.is_tainted(v, fid, visited, depth),
            K::Alloc => self.is_field_of_alloc_tainted(iid, fid, field, visited, depth),
            K::Phi(edges) => {
                if !visited.insert((fid, v)) {
                    return false;
                }
                edges.into_iter().any(|e| {
                    self.is_field_tainted_on_value(e, fid, field, visited, depth + 1)
                })
            }
            K::Other => self.is_tainted(v, fid, visited, depth),
        }
    }

    /// `isFieldOfAllocTainted`: stores to `alloc.field`, in the same function.
    fn is_field_of_alloc_tainted(
        &mut self,
        alloc: InstrId,
        fid: FuncId,
        field: usize,
        visited: &mut HashSet<Site>,
        depth: u32,
    ) -> bool {
        for val in self.stores_to_field(alloc, fid, field) {
            if self.is_tainted(val, fid, visited, depth + 1) {
                return true;
            }
        }
        false
    }

    /// The values stored into `alloc`'s `field`, via its `FieldAddr` referrers.
    ///
    /// The second loop is guff's, not upstream's, and it is here because the
    /// two SSA builders lower a composite literal differently. go/ssa writes an
    /// addressable one straight into its target:
    ///
    /// ```text
    /// t0 = local holder (h)
    /// t1 = &t0.cmd [#0]
    /// *t1 = os.Getenv("X")
    /// ```
    ///
    /// guff fills a `complit` temporary and copies the whole struct across:
    ///
    /// ```text
    /// t0 = local holder (h)      t1 = local holder (complit)
    /// t2 = &t1.cmd [#0]          *t2 = os.Getenv("X")
    /// t4 = *t1                   *t0 = t4
    /// ```
    ///
    /// so `h`'s own referrers carry no field store at all and upstream's walk —
    /// which only reads `FieldAddr` stores — finds nothing. Following a
    /// whole-struct store back to the temporary it came from recovers exactly
    /// the shape go/ssa would have produced. Left as a taint-side adaptation
    /// rather than a change to `builder`, which every SSA analyzer shares.
    fn stores_to_field(&self, alloc: InstrId, fid: FuncId, field: usize) -> Vec<Value> {
        let func = self.func(fid);
        let mut out = Vec::new();
        for &r in referrers(func, Value::Instr(alloc)) {
            let InstrData::FieldAddr(fa) = func.instrs.get(r) else {
                continue;
            };
            if fa.field != field {
                continue;
            }
            for &fr in referrers(func, Value::Instr(r)) {
                if let InstrData::Store(st) = func.instrs.get(fr) {
                    if st.addr == Value::Instr(r) {
                        out.push(st.val);
                    }
                }
            }
        }
        // A whole-struct store into this cell: follow the value to the cell it
        // was loaded from and read that one's field stores instead.
        let mut seen = HashSet::new();
        seen.insert(Value::Instr(alloc));
        let mut queue: Vec<InstrId> = vec![alloc];
        while let Some(cell) = queue.pop() {
            for &r in referrers(func, Value::Instr(cell)) {
                let InstrData::Store(st) = func.instrs.get(r) else {
                    continue;
                };
                if st.addr != Value::Instr(cell) {
                    continue;
                }
                let Some(src) = self.trace_to_alloc(st.val, fid) else {
                    continue;
                };
                if !seen.insert(Value::Instr(src)) {
                    continue;
                }
                for &sr in referrers(func, Value::Instr(src)) {
                    let InstrData::FieldAddr(fa) = func.instrs.get(sr) else {
                        continue;
                    };
                    if fa.field != field {
                        continue;
                    }
                    for &fr in referrers(func, Value::Instr(sr)) {
                        if let InstrData::Store(st) = func.instrs.get(fr) {
                            if st.addr == Value::Instr(sr) {
                                out.push(st.val);
                            }
                        }
                    }
                }
                queue.push(src);
            }
        }
        out
    }

    /// `isFieldTaintedViaCall`: look inside the callee for the allocation it
    /// returns and ask whether *this* field was assigned from a tainted
    /// argument.
    fn is_field_tainted_via_call(
        &mut self,
        call: InstrId,
        caller: FuncId,
        field: usize,
        callee: FuncId,
        visited: &mut HashSet<Site>,
        depth: u32,
    ) -> bool {
        if depth > MAX_TAINT_DEPTH || !visited.insert((caller, Value::Instr(call))) {
            return false;
        }
        let returned: Vec<Value> = {
            let f = self.prog.functions.get(callee);
            let mut out = Vec::new();
            for (_, block) in f.live_blocks() {
                for &iid in &block.instrs {
                    if let InstrData::Return(r) = f.instrs.get(iid) {
                        out.extend(r.results.iter().copied());
                    }
                }
            }
            out
        };
        for ret in returned {
            let Some(alloc) = self.trace_to_alloc(ret, callee) else {
                continue;
            };
            if !visited.insert((callee, Value::Instr(alloc))) {
                continue;
            }
            for val in self.stores_to_field(alloc, callee, field) {
                if self.is_callee_value_tainted(val, callee, call, caller, visited, depth + 1) {
                    return true;
                }
            }
        }
        false
    }

    /// `traceToAlloc`.
    fn trace_to_alloc(&self, v: Value, fid: FuncId) -> Option<InstrId> {
        let mut seen: HashSet<Value> = HashSet::new();
        let mut cur = v;
        loop {
            if !seen.insert(cur) {
                return None;
            }
            let Value::Instr(iid) = cur else { return None };
            let f = self.prog.functions.get(fid);
            match f.instrs.get(iid) {
                InstrData::Alloc(_) => return Some(iid),
                InstrData::MakeInterface(m) => cur = m.x,
                InstrData::ChangeType(c) => cur = c.x,
                InstrData::Convert(c) => cur = c.x,
                InstrData::UnOp(u) => cur = u.x,
                InstrData::Phi(p) => {
                    let edges: Vec<Value> = p.edges.iter().flatten().copied().collect();
                    for e in edges {
                        if let Some(a) = self.trace_to_alloc_seen(e, fid, &mut seen) {
                            return Some(a);
                        }
                    }
                    return None;
                }
                _ => return None,
            }
        }
    }

    fn trace_to_alloc_seen(
        &self,
        v: Value,
        fid: FuncId,
        seen: &mut HashSet<Value>,
    ) -> Option<InstrId> {
        if !seen.insert(v) {
            return None;
        }
        let Value::Instr(iid) = v else { return None };
        let f = self.prog.functions.get(fid);
        match f.instrs.get(iid) {
            InstrData::Alloc(_) => Some(iid),
            InstrData::MakeInterface(m) => self.trace_to_alloc_seen(m.x, fid, seen),
            InstrData::ChangeType(c) => self.trace_to_alloc_seen(c.x, fid, seen),
            InstrData::Convert(c) => self.trace_to_alloc_seen(c.x, fid, seen),
            InstrData::UnOp(u) => self.trace_to_alloc_seen(u.x, fid, seen),
            InstrData::Phi(p) => {
                let edges: Vec<Value> = p.edges.iter().flatten().copied().collect();
                edges
                    .into_iter()
                    .find_map(|e| self.trace_to_alloc_seen(e, fid, seen))
            }
            _ => None,
        }
    }

    /// `isCalleValueTainted`: a value *inside* the callee, with parameters
    /// mapped back to the caller's arguments.
    fn is_callee_value_tainted(
        &mut self,
        v: Value,
        callee: FuncId,
        call: InstrId,
        caller: FuncId,
        visited: &mut HashSet<Site>,
        depth: u32,
    ) -> bool {
        if depth > MAX_TAINT_DEPTH || !visited.insert((callee, v)) {
            return false;
        }
        if let Value::Param(pid) = v {
            let idx = self
                .prog
                .functions
                .get(callee)
                .params
                .iter()
                .position(|(id, _)| id == pid);
            let Some(idx) = idx else { return false };
            let arg = {
                let f = self.func(caller);
                call_common(f.instrs.get(call)).and_then(|c| c.args.get(idx).copied())
            };
            return arg.is_some_and(|a| self.is_tainted(a, caller, visited, depth));
        }
        if matches!(v, Value::Const(_)) {
            return false;
        }
        let Value::Instr(iid) = v else {
            return self.is_tainted(v, callee, visited, depth);
        };
        enum K {
            Call(Vec<Value>, bool, bool),
            One(Value),
            Two(Value, Value),
            Many(Vec<Value>),
            Fallback,
        }
        let k = {
            let f = self.prog.functions.get(callee);
            match f.instrs.get(iid) {
                InstrData::Call(c) => K::Call(
                    c.call.args.clone(),
                    self.is_sanitizer_call(&c.call),
                    self.is_source_func_call(&c.call),
                ),
                InstrData::Extract(e) => K::One(e.tuple),
                InstrData::Phi(p) => K::Many(p.edges.iter().flatten().copied().collect()),
                InstrData::BinOp(b) => K::Two(b.x, b.y),
                InstrData::Convert(c) => K::One(c.x),
                InstrData::ChangeType(c) => K::One(c.x),
                InstrData::FieldAddr(fa) => K::One(fa.x),
                InstrData::UnOp(u) => K::One(u.x),
                _ => K::Fallback,
            }
        };
        match k {
            K::Call(_, true, _) => false,
            K::Call(_, _, true) => true,
            K::Call(args, _, _) => args.into_iter().any(|a| {
                self.is_callee_value_tainted(a, callee, call, caller, visited, depth + 1)
            }),
            K::One(x) => self.is_callee_value_tainted(x, callee, call, caller, visited, depth + 1),
            K::Two(x, y) => {
                self.is_callee_value_tainted(x, callee, call, caller, visited, depth + 1)
                    || self.is_callee_value_tainted(y, callee, call, caller, visited, depth + 1)
            }
            K::Many(vs) => vs.into_iter().any(|x| {
                self.is_callee_value_tainted(x, callee, call, caller, visited, depth + 1)
            }),
            K::Fallback => self.is_tainted(v, callee, visited, depth),
        }
    }

    /// `doTaintedArgsFlowToReturn`: which arguments are tainted, and does any
    /// of the corresponding parameters reach a `return`?
    fn tainted_args_flow_to_return(
        &mut self,
        call: InstrId,
        caller: FuncId,
        callee: FuncId,
        visited: &mut HashSet<Site>,
        depth: u32,
    ) -> bool {
        if depth > MAX_TAINT_DEPTH {
            return false;
        }
        let args: Vec<Value> = {
            let f = self.func(caller);
            match call_common(f.instrs.get(call)) {
                Some(c) => c.args.clone(),
                None => return false,
            }
        };
        let mut tainted_idx: Vec<usize> = Vec::new();
        for (i, arg) in args.iter().enumerate() {
            if self.arg_type_is_context(*arg, caller) {
                continue;
            }
            if self.is_tainted(*arg, caller, visited, depth) {
                tainted_idx.push(i);
            }
        }
        if tainted_idx.is_empty() {
            return false;
        }
        let tainted_params: HashSet<Value> = {
            let f = self.prog.functions.get(callee);
            let ids: Vec<Value> = f.params.iter().map(|(id, _)| Value::Param(id)).collect();
            tainted_idx
                .iter()
                .filter_map(|&i| ids.get(i).copied())
                .collect()
        };
        let returned: Vec<Value> = {
            let f = self.prog.functions.get(callee);
            let mut out = Vec::new();
            for (_, block) in f.live_blocks() {
                for &iid in &block.instrs {
                    if let InstrData::Return(r) = f.instrs.get(iid) {
                        out.extend(r.results.iter().copied());
                    }
                }
            }
            out
        };
        for ret in returned {
            let mut seen: HashSet<Value> = HashSet::new();
            if self.value_reachable_from_params(ret, callee, &tainted_params, &mut seen, 0) {
                return true;
            }
        }
        false
    }

    /// `valueReachableFromParams`: data-derivation inside one function body.
    fn value_reachable_from_params(
        &self,
        v: Value,
        fid: FuncId,
        tainted_params: &HashSet<Value>,
        seen: &mut HashSet<Value>,
        depth: u32,
    ) -> bool {
        if depth > 30 || !seen.insert(v) {
            return false;
        }
        match v {
            Value::Param(_) => tainted_params.contains(&v),
            Value::Const(_) | Value::Global(_) | Value::FreeVar(_) => false,
            Value::Builtin(_) | Value::Function(_) => false,
            Value::Instr(iid) => {
                let f = self.prog.functions.get(fid);
                let next: Vec<Value> = match f.instrs.get(iid) {
                    InstrData::Alloc(_) => {
                        let mut out = Vec::new();
                        for &r in referrers(f, v) {
                            match f.instrs.get(r) {
                                InstrData::Store(st) if st.addr == v => out.push(st.val),
                                InstrData::FieldAddr(_) => {
                                    for &fr in referrers(f, Value::Instr(r)) {
                                        if let InstrData::Store(st) = f.instrs.get(fr) {
                                            if st.addr == Value::Instr(r) {
                                                out.push(st.val);
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        out
                    }
                    InstrData::Call(c) => {
                        let mut out = c.call.args.clone();
                        out.push(c.call.value);
                        out
                    }
                    InstrData::Phi(p) => p.edges.iter().flatten().copied().collect(),
                    InstrData::UnOp(u) => vec![u.x],
                    InstrData::BinOp(b) => vec![b.x, b.y],
                    InstrData::Convert(c) => vec![c.x],
                    InstrData::ChangeType(c) => vec![c.x],
                    InstrData::MakeInterface(m) => vec![m.x],
                    InstrData::TypeAssert(t) => vec![t.x],
                    InstrData::Slice(s) => vec![s.x],
                    InstrData::FieldAddr(fa) => vec![fa.x],
                    InstrData::IndexAddr(i) => vec![i.x],
                    InstrData::Extract(e) => vec![e.tuple],
                    InstrData::Lookup(l) => vec![l.x],
                    _ => Vec::new(),
                };
                next.into_iter()
                    .any(|x| self.value_reachable_from_params(x, fid, tainted_params, seen, depth + 1))
            }
        }
    }

    /// The `*ssa.Alloc` case: a local whose stores decide its taint.
    fn is_alloc_tainted(
        &mut self,
        iid: InstrId,
        fid: FuncId,
        visited: &mut HashSet<Site>,
        depth: u32,
    ) -> bool {
        let vals: Vec<Value> = {
            let f = self.func(fid);
            let me = Value::Instr(iid);
            let mut out = Vec::new();
            for &r in referrers(f, me) {
                match f.instrs.get(r) {
                    InstrData::Store(st) => out.push(st.val),
                    // Arrays and slices: a variadic call's arguments are stored
                    // through `IndexAddr`, which is how upstream reaches the
                    // elements of the slice go/ssa packs them into.
                    InstrData::IndexAddr(_) => {
                        for &ir in referrers(f, Value::Instr(r)) {
                            if let InstrData::Store(st) = f.instrs.get(ir) {
                                out.push(st.val);
                            }
                        }
                    }
                    _ => {}
                }
            }
            out
        };
        vals.into_iter()
            .any(|v| self.is_tainted(v, fid, visited, depth + 1))
    }

    fn is_make_slice_tainted(
        &mut self,
        iid: InstrId,
        fid: FuncId,
        visited: &mut HashSet<Site>,
        depth: u32,
    ) -> bool {
        let vals: Vec<Value> = {
            let f = self.func(fid);
            let me = Value::Instr(iid);
            let mut out = Vec::new();
            for &r in referrers(f, me) {
                match f.instrs.get(r) {
                    InstrData::Store(st) => out.push(st.val),
                    InstrData::Call(c) => {
                        out.extend(c.call.args.iter().copied().filter(|a| *a != me));
                    }
                    _ => {}
                }
            }
            out
        };
        vals.into_iter()
            .any(|v| self.is_tainted(v, fid, visited, depth + 1))
    }
}
