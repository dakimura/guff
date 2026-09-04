mod support;

use std::sync::Arc;

use guff_analysis::SettingsBag;
use guff_runner::RunnerOptions;
use guff_style::{
    arangolint, asasalint, asciicheck, bidichk, canonicalheader, clickhouselint, containedctx,
    copyloopvar, cyclop, decorder, dogsled, embeddedstructfieldcheck, exhaustive, exhaustruct,
    exptostd, forbidigo, funcorder, funlen, gocheckcompilerdirectives, gochecknoglobals,
    gochecknoinits, gochecksumtype, gocognit, goconst, gocritic, gocyclo, goheader, goprintffuncname,
    gosec, gosmopolitan, grouper, iface, inamedparam, interfacebloat, intrange, iotamixing, ireturn,
    lll, loggercheck, maintidx, mnd, modernize, musttag, nakedret, nestif, nlreturn, noinlineerr,
    nonamedreturns, nosprintfhostport, paralleltest, perfsprint, prealloc, predeclared, ginkgolinter,
    promlinter, protogetter, reassign, recvcheck, sloglint, spancheck, tagalign, tagliatelle, testableexamples,
    testifylint, testpackage, thelper, tparallel, unconvert, unparam, unqueryvet, usestdlibvars,
    usetesting, varnamelen, wastedassign, whitespace, wsl, wsl_v5, zerologlint,
};

#[test]
fn gosec_flags_weak_crypto_rand_unsafe_and_blocklist_imports() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec", "bad.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    for needle in [
        "G101:",
        "G102:",
        "G103:",
        "G104:",
        "G106:",
        "G107:",
        "G108:",
        "G109:",
        "G110:",
        "G111:",
        "G112:",
        "G114:",
        "G122:",
        "G124:",
        "G203:",
        "G204:",
        "G301:",
        "G302:",
        "G303:",
        "G306:",
        "G401:",
        "G402:",
        "G403:",
        "G404:",
        "G405:",
        "G406:",
        "G501:",
        "G502:",
        "G503:",
        "G504:",
        "G505:",
        "G506:",
        "G507:",
        "G703:",
    ] {
        assert!(
            messages.iter().any(|m| m.contains(needle)),
            "missing {needle} in {messages:?}"
        );
    }
}

/// G602 is the only SSA analyzer among guff's gosec rules and the only one
/// bad.go does not reach, so it gets its own fixture. The forms that go
/// through a re-slice (`s = s[:2]` then `s[4]`) are the ones the golden case
/// found missing: guff lowers `make` to a single MakeSlice where go/ssa emits
/// `Alloc` + `Slice`, and the bounds walk used to stop there.
#[test]
fn gosec_g602_tracks_bounds_through_reslices() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/g602", "g602.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    let bounds = messages
        .iter()
        .filter(|m| m.contains("G602: slice bounds out of range"))
        .count();
    let index = messages
        .iter()
        .filter(|m| m.contains("G602: slice index out of range"))
        .count();
    assert_eq!((bounds, index), (1, 2), "{messages:?}");
}

/// Where G602 gets a capacity from when guff's SSA does not spell the slice the
/// way upstream reads it.
///
/// Upstream only ever learns a bound from an `Alloc` of a fixed-size array.
/// guff builds no such array for a variadic call — it passes the tail through
/// individually — and none for `make([]T, constN)`, which it lowers to one
/// `MakeSlice`. So G602 never looked inside a variadic callee at all, and the
/// authelia diff that started this (`m[key] = pairs[i+1]` in a `FuncDict`
/// helper) was one of eight shapes out of twenty-four that upstream reported
/// and guff did not.
///
/// Asserted as the **set of report positions**: every finding here carries the
/// identical message, so `any(contains("G602"))` — or even a count — is true of
/// any subset. Measured against golangci-lint 2.12.2 (gosec v2.26.1).
#[test]
fn gosec_g602_learns_a_bound_from_variadic_calls_and_makeslice() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/g602_variadic", "g602_variadic.go");
    let fset = pkg.fset.clone().expect("fixture has a FileSet");
    let mut got: Vec<(i64, i64)> = support::run_analyzer_diagnostics(gosec(), &pkg)
        .into_iter()
        .filter(|d| d.message.contains("G602:"))
        .map(|d| {
            let p = fset.position(guff::position::Pos(d.pos as i64));
            (p.line, p.column)
        })
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            // the two that already worked: the array is in this function
            (26, 15),
            (39, 15),
            // …and the two that reach a callee through a slice value
            (51, 15),
            (61, 15),
            // make([]any, 2) handed to a function: guff's MakeSlice stands
            // where go/ssa's Alloc+Slice stands, and the walk has to accept it
            (80, 15),
            // the variadic tail, whose capacity is now read off the call site
            (95, 15),
            (109, 15),
            // the guard *around* the access, which is what decides whether the
            // `ifs` map records `i < p` or `i+1 < p`
            (126, 16),
            (137, 15),
            (144, 60),
            (155, 15),
            // a method: the receiver is args[0] and params[0] alike
            (166, 65),
            // two call sites, and the shorter one is what makes it bad
            (171, 59),
            // the tail re-sliced before being indexed
            (180, 13),
        ],
        "G602 report positions"
    );
}

/// G115 is the other SSA analyzer (gosec `conversion_overflow.go` +
/// `range_analyzer.go`). The fixture marks every conversion `// FINDING` or
/// `// silent`, and those marks are gated against golangci-lint 2.12.2 by
/// `compat/golden/cases/gosec`; this test pins the same finding set — as a
/// multiset of messages, since a rule that stopped bounding values would keep
/// the count of *some* pairs and change others.
#[test]
fn gosec_g115_reports_only_unbounded_conversions() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/g115", "g115.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    let count = |needle: &str| messages.iter().filter(|m| m.as_str() == needle).count();

    assert_eq!(
        (
            count("G115: integer overflow conversion int -> int32"),
            count("G115: integer overflow conversion int64 -> int32"),
            count("G115: integer overflow conversion int -> uint8"),
            count("G115: integer overflow conversion int -> uint32"),
            count("G115: integer overflow conversion uint64 -> int"),
        ),
        (4, 2, 6, 2, 1),
        "{messages:?}"
    );
    // Nothing else: every other conversion in the fixture is bounded, and the
    // fixture's `// silent` marks say which.
    assert_eq!(
        messages.iter().filter(|m| m.starts_with("G115:")).count(),
        15,
        "{messages:?}"
    );
}

/// G118 is the third SSA analyzer. It is one id over three checks; the fixture
/// marks every `context.With…` call and every `go` statement `// FINDING` or
/// `// silent`, and `compat/golden/cases/gosec` gates those marks against
/// golangci-lint 2.12.2.
#[test]
fn gosec_g118_reports_only_uncalled_cancels_and_detached_goroutines() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/g118", "g118.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    let count = |needle: &str| messages.iter().filter(|m| m.as_str() == needle).count();

    assert_eq!(
        (
            count(
                "G118: context cancellation function returned by \
                 WithCancel/WithTimeout/WithDeadline is not called"
            ),
            count(
                "G118: Goroutine uses context.Background/TODO while request-scoped \
                 context is available"
            ),
            count("G118: Long-running loop performs calls without a ctx.Done() cancellation guard"),
        ),
        (6, 2, 1),
        "{messages:?}"
    );
    // Nothing else: every other case in the fixture is one of the escapes the
    // walk has to recognise, and the `// silent` marks say which.
    assert_eq!(
        messages.iter().filter(|m| m.starts_with("G118:")).count(),
        9,
        "{messages:?}"
    );
}

/// G123 is the fourth SSA analyzer: an inventory of `tls.Config` field stores
/// rather than a dataflow. The fixture marks every config `// FINDING` or
/// `// silent`, gated by `compat/golden/cases/gosec`.
#[test]
fn gosec_g123_reports_verifypeer_without_a_resumption_guard() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/g123", "g123.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    assert_eq!(
        messages.iter().filter(|m| m.starts_with("G123:")).count(),
        5,
        "{messages:?}"
    );
}

/// G102 over a fixture that is about *where the address comes from*.
///
/// `GetIdentStringValues` follows an identifier to its declaration and reads
/// the string literals there — one hop, literals only, and the parser's
/// file-scoped resolution. syncthing `cmd/infra/strelaypoolsrv` declares
/// `listen = ":80"` and hands the variable to two listeners; a rule that only
/// reads a literal at the call sees neither.
#[test]
fn gosec_g102_resolves_the_address_through_its_declaration() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/g102", "g102.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    let g102 = messages.iter().filter(|m| m.starts_with("G102: ")).count();
    // Counted: twelve calls, five findings. Four of the silent ones are
    // addresses that do not match, and three are declarations the resolution
    // does not read (no initializer, a call, a parameter).
    assert_eq!(g102, 5, "{messages:?}");
}

/// G402's cipher-suite half, and its "first finding wins" rule.
#[test]
fn gosec_g402_names_the_first_cipher_off_the_list() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/g402", "g402.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    let mut g402: Vec<&str> = messages
        .iter()
        .filter(|m| m.starts_with("G402: "))
        .map(|m| m.as_str())
        .collect();
    g402.sort_unstable();
    // Counted and spelled out: eight `tls.Config` literals, three findings.
    // `BothBad` and `BothBadReversed` set two bad fields each and report one
    // apiece — whichever comes first — because `Match` returns on the first.
    assert_eq!(
        g402,
        vec![
            "G402: TLS Bad Cipher Suite: TLS_ECDHE_RSA_WITH_AES_128_CBC_SHA",
            "G402: TLS Bad Cipher Suite: TLS_RSA_WITH_AES_128_CBC_SHA",
            "G402: TLS InsecureSkipVerify set to true.",
        ],
        "{messages:?}"
    );
}

/// G122 over a fixture that is about *which callbacks are found*.
///
/// The callback argument is resolved as an SSA value upstream, so a function
/// named at the call site and a local holding one are the same thing, while a
/// call result, a struct field and the caller's own parameter resolve to
/// nothing. Findings are deduped by the sink's position, so the same callback
/// reached from three walks is one finding. authelia passes `fixCoveragePath`
/// to `filepath.Walk` by name, and a `FuncLit`-only rule sees none of it.
#[test]
fn gosec_g122_resolves_named_callbacks_and_dedupes_by_sink() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/g122", "g122.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    let g122 = messages.iter().filter(|m| m.starts_with("G122: ")).count();
    // Counted: eleven walks, four findings. Two inline literals, one named
    // callback (reached three times, deduped to one) and one reached only
    // through a local. The other five callbacks are silent, and each is silent
    // for its own reason — see the fixture.
    assert_eq!(g122, 4, "{messages:?}");
}

/// G304 over one fixture whose every function is one file-reading call.
///
/// "The argument is a variable" is one of five branches. The other four are the
/// reason for the count: a `Clean`ed path is trusted inline or through a
/// variable, a `Join` is judged by its own arguments, a literal base plus a
/// cleaned name is a trusted pair, and the two side maps are filled in AST
/// visit order so a call before its assignment is still reported.
#[test]
fn gosec_g304_trusts_cleaned_and_constant_paths() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/g304", "g304.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    let g304 = messages.iter().filter(|m| m.starts_with("G304: ")).count();
    // Counted: 22 functions, one call each, and 10 of them are findings.
    // `any(contains(…))` passes with all 22 reported.
    assert_eq!(g304, 10, "{messages:?}");
}

/// G117 over one fixture whose every function is one marshal call, marked
/// `// fires` or `// silent`.
///
/// The rule is a call rule with four separate ways to stay silent (a custom
/// marshaler around the call, a marshaler method on the type, a field that is
/// not serialized or not string-ish, and a literal that passes a call result),
/// so a port that implements only the field-name match reports every silent
/// shape here.
#[test]
fn gosec_g117_reports_only_fields_that_are_actually_serialized() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/g117", "g117.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    let mut g117: Vec<&str> = messages
        .iter()
        .filter(|m| m.starts_with("G117: "))
        .map(|m| m.as_str())
        .collect();
    g117.sort_unstable();
    // Counted and spelled out: the fixture has 24 marshal calls and exactly 14
    // of them are findings. `any(contains(…))` passes with all 24 reported.
    assert_eq!(
        g117,
        vec![
            "G117: Marshaled struct field \"Password\" (JSON key \"Password\") matches secret pattern",
            "G117: Marshaled struct field \"Password\" (JSON key \"Password\") matches secret pattern",
            "G117: Marshaled struct field \"Password\" (JSON key \"pass\") matches secret pattern",
            "G117: Marshaled struct field \"Password\" (JSON key \"password\") matches secret pattern",
            "G117: Marshaled struct field \"Password\" (JSON key \"password\") matches secret pattern",
            "G117: Marshaled struct field \"Password\" (JSON key \"password\") matches secret pattern",
            "G117: Marshaled struct field \"Password\" (JSON key \"password\") matches secret pattern",
            "G117: Marshaled struct field \"Password\" (JSON key \"password\") matches secret pattern",
            "G117: Marshaled struct field \"Password\" (JSON key \"password\") matches secret pattern",
            "G117: Marshaled struct field \"Password\" (JSON key \"password\") matches secret pattern",
            "G117: Marshaled struct field \"Password\" (TOML key \"Password\") matches secret pattern",
            "G117: Marshaled struct field \"Password\" (XML key \"Password\") matches secret pattern",
            "G117: Marshaled struct field \"Password\" (YAML key \"Password\") matches secret pattern",
            "G117: Marshaled struct field \"Secret\" (JSON key \"harmless\") matches secret pattern",
        ],
        "{messages:?}"
    );
}

/// The taint engine — G702 / G703 / G705 / G706 / G710 — over one fixture whose
/// every function is marked `// fires` or `// silent`.
///
/// The counts alone would pass on a rule that reports everything, so the
/// fixture is built the other way round: each firing shape is paired with the
/// nearest silent one (a sanitizer on the same source, a constant in the
/// argument the sink actually checks, a second assignment that kills the
/// taint), and `compat/golden/cases/gosec` pins every line and column of both
/// halves against golangci-lint 2.12.2.
#[test]
fn gosec_taint_rules_report_only_reachable_sources() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/g7xx", "g7xx.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    let count = |id: &str| {
        messages
            .iter()
            .filter(|m| m.starts_with(&format!("{id}: ")))
            .count()
    };
    assert_eq!(
        (
            count("G702"),
            count("G703"),
            count("G705"),
            count("G706"),
            count("G710")
        ),
        (7, 5, 8, 5, 3),
        "{messages:?}"
    );
}

/// G120 is the engine's smallest configuration: one source, one sink, no
/// sanitizers, and `CheckArgs: [0]` names the receiver rather than an argument.
///
/// The three silent shapes are the rule: `ParseForm`, `FormValue` and
/// `PostFormValue` are *not* sinks, because the standard library already caps
/// the body for them. A port that treats "parses a form" as the sink reports
/// all four.
#[test]
fn gosec_g120_reports_only_parse_multipart_form_on_an_outside_request() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/g7xx", "g7xx.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    let g120 = messages.iter().filter(|m| m.starts_with("G120: ")).count();
    // Counted: seven calls, three findings. The four silent ones are the other
    // three form accessors plus a request built locally, which is not a source.
    assert_eq!(g120, 3, "{messages:?}");
}

/// CHA resolves a call through a func-typed value to every address-taken bare
/// function with an **identical signature**, and `CallCommon.Signature()` is
/// the *core* type of the called value. Keyed by the named func type instead,
/// the dispatch has no callees at all: every handler reached only through such
/// a variable then looks like an entry point, and a source-typed parameter of
/// an entry point is auto-tainted. authelia dispatches nine OAuth2 consent
/// handlers through one `handlerAuthorizationConsent` variable and three of
/// them reported a G710 upstream does not make.
#[test]
fn gosec_g710_resolves_a_dispatch_through_a_named_func_type() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/g7xx", "g7xx.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    let g710: Vec<&String> = messages.iter().filter(|m| m.starts_with("G710: ")).collect();
    // Counted over the whole fixture: the six functions reached through a
    // dispatch with a clean argument are silent, and only the one whose call
    // site passes a request value is reported. Three G710 in total — the two
    // that predate the dispatch shapes, plus `g710ViaNamedTainted`.
    //
    // Two of the six are reached through a `MakeClosure` rather than by name:
    // `g710Box.redirect` (a method value, which compiles to a bound thunk) and
    // the capturing literal `g710MakeRedirector` returns. A reachability walk
    // that does not follow the closure's function operand leaves both out of
    // the call graph, and an absent node is an entry point whose source-typed
    // parameters are auto-tainted — this count goes to 5.
    assert_eq!(g710.len(), 3, "{messages:?}");
}

/// G705's two shapes that no other taint rule has, stated as the thing that
/// breaks if either is dropped.
///
/// A `Receiver` sink on an interface is an SSA **invoke**: there is no static
/// callee, so a matcher that only asks `static_callee` finds nothing and
/// `w.Write(tainted)` — the most direct XSS there is — reports nothing.
/// `ArgTypeGuards` are the opposite failure: without them `fmt.Fprintf` is a
/// sink wherever it is called, and every `Fprintf(os.Stderr, …)` of a request
/// value in a web server becomes a finding.
#[test]
fn gosec_g705_needs_the_invoke_sink_and_the_writer_guard() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/g7xx", "g7xx.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    let g705: Vec<&String> = messages.iter().filter(|m| m.starts_with("G705: ")).collect();
    // Eight firing shapes; the ten silent ones are what the count is really
    // pinning — four are guarded `fmt` / `io` calls a missing guard would turn
    // into findings, and two are sources that belong to the *other* taint rules
    // and would fire here if the five tables were collapsed into one.
    assert_eq!(g705.len(), 8, "{messages:?}");
}

/// The half of the taint engine that is easiest to lose: a `string` parameter
/// is not a source of anything, and only the call graph can say it carries a
/// request. gosec's graph is `cha.CallGraph`, whose node set is
/// `ssautil.AllFunctions` — package-level functions, methods of *exported*
/// types, and methods of types that reach `RuntimeTypes` by being converted to
/// an interface. The fixture holds the same three lines twice, on an unexported
/// type that is boxed and one that is not.
#[test]
fn gosec_taint_crosses_a_call_only_when_the_caller_is_in_the_call_graph() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/g7xx", "g7xx.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    let g706: Vec<&String> = messages.iter().filter(|m| m.starts_with("G706: ")).collect();
    // Five, not six: `plainRec.plainLog` is the twin of `boxedRec.boxedLog` and
    // its caller is not a node, so its `id` never learns where it came from.
    assert_eq!(g706.len(), 5, "{messages:?}");
}

#[test]
fn gosec_allows_strong_crypto() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/ok", "ok.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    // Three G304, and they are correct: golangci-lint reports the same three,
    // and the golden records them. `ok.go` is silent for the rules its shapes
    // are *about*, and three of those shapes need a non-constant path to exist
    // at all — a `filepath.Walk` callback's `path` (G122), a config file name
    // reaching a helper (G703), a `filepath.Join(dir, …)` fed to a callback
    // (G703). Making them constants would delete what they test.
    let (g304, other): (Vec<&String>, Vec<&String>) =
        messages.iter().partition(|m| m.starts_with("G304: "));
    assert!(other.is_empty(), "{other:?}");
    assert_eq!(g304.len(), 3, "{messages:?}");
}

/// G404 matches by syntax, not by the callee's declaring package.
///
/// gosec's `GetCallInfo` names the receiver: `rand.Int()` gives `("rand",
/// "Int")` and resolves through the file's imports, while `s.r.Int()` gives
/// the receiver's type string `"*math/rand.Rand"`, which matches no rule.
/// coredns wraps `math/rand` in exactly that shape, and resolving the callee's
/// package instead made every method on the wrapper a finding. `Perm` and
/// `Shuffle` are not on gosec's list at all.
#[test]
fn gosec_g404_matches_only_package_qualified_calls() {
    let ok = support::typecheck_fixture("gosec", "example.com/gosec/ok", "ok.go");
    let messages = support::run_analyzer(gosec(), &ok);
    assert!(
        !messages.iter().any(|m| m.starts_with("G404:")),
        "methods on *rand.Rand, rand.Perm and rand.Shuffle are not G404: {messages:?}"
    );

    let bad = support::typecheck_fixture("gosec", "example.com/gosec/bad", "bad.go");
    let bad_messages = support::run_analyzer(gosec(), &bad);
    assert!(
        bad_messages.iter().any(|m| m.starts_with("G404:")),
        "package-qualified rand.Intn is still a finding: {bad_messages:?}"
    );
}

#[test]
fn gosec_severity_and_confidence_filter_findings() {
    use guff_style::GosecOptions;

    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/scores", "scores.go");

    let run = |opts: GosecOptions| {
        let mut bag = SettingsBag::new();
        bag.insert("gosec", opts);
        support::run_analyzer_with_settings(
            gosec(),
            &pkg,
            &RunnerOptions {
                settings: Arc::new(bag),
                ..RunnerOptions::default()
            },
        )
    };

    let unfiltered = run(GosecOptions::default());
    assert!(
        unfiltered.iter().any(|m| m.contains("G101:")),
        "default (no threshold) should report G101: {unfiltered:?}"
    );
    assert!(
        unfiltered.iter().any(|m| m.contains("G401:")),
        "default should report G401: {unfiltered:?}"
    );

    // G101 is Low confidence, G401 is High — `confidence: medium` keeps only G401.
    let medium = run(GosecOptions {
        confidence: "medium".into(),
        ..Default::default()
    });
    assert!(
        !medium.iter().any(|m| m.contains("G101:")),
        "confidence=medium should drop Low-confidence G101: {medium:?}"
    );
    assert!(
        medium.iter().any(|m| m.contains("G401:")),
        "confidence=medium should keep High-confidence G401: {medium:?}"
    );

    // G401 is Medium severity, G101 is High — `severity: high` keeps only G101.
    let high_sev = run(GosecOptions {
        severity: "high".into(),
        ..Default::default()
    });
    assert!(
        high_sev.iter().any(|m| m.contains("G101:")),
        "severity=high should keep High-severity G101: {high_sev:?}"
    );
    assert!(
        !high_sev.iter().any(|m| m.contains("G401:")),
        "severity=high should drop Medium-severity G401: {high_sev:?}"
    );

    // An unrecognized threshold disables filtering (upstream `convertToScore` → -1).
    let bogus = run(GosecOptions {
        confidence: "bogus".into(),
        ..Default::default()
    });
    assert!(
        bogus.iter().any(|m| m.contains("G101:")),
        "unknown confidence must not filter: {bogus:?}"
    );
}

#[test]
fn gosec_g101_pattern_override_replaces_default_names() {
    use guff_style::{G101Options, GosecOptions};

    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/scores2", "scores.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "gosec",
        GosecOptions {
            g101: G101Options {
                pattern: "(?i)example".into(),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        gosec(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    let g101: Vec<&String> = messages.iter().filter(|m| m.contains("G101:")).collect();
    // Only `exampleValue` matches `(?i)example`; the `...Password...` const no
    // longer does, because `pattern` replaces the default name list.
    assert_eq!(g101.len(), 1, "expected exactly one G101: {messages:?}");
}

/// G101's gate is zxcvbn's dictionary-based entropy estimate, not the length
/// or the character mix: a credential spelled out of English words scores low
/// however long it is, and one that isn't scores high at half the length.
#[test]
fn gosec_g101_entropy_follows_zxcvbn() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/entropy", "entropy.go");
    let messages = support::run_analyzer(gosec(), &pkg);
    let g101: Vec<&String> = messages.iter().filter(|m| m.contains("G101:")).collect();
    assert_eq!(
        g101.len(),
        4,
        "the four non-word credentials, and only those: {messages:?}"
    );
}

/// Every `#nosec` shape, and the report position of all four AST nodes G101
/// fires on. Both halves had no fixture at all, and both were wrong.
///
/// The expected set is what golangci-lint 2.12.2 (gosec v2.26.1) prints for
/// this file, measured 2026-09-01. It is asserted as a **set of positions**,
/// not with `any(contains("G101"))`: the two defects this file exists for are
/// a finding that should not be there (line 71) and two findings at the wrong
/// column (79/86 at the `&creds{` type, 128 at the `password` operand), and a
/// message-substring assertion is true of all of them.
#[test]
fn gosec_nosec_directive_scope_and_report_positions() {
    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/nosec", "nosec.go");
    let fset = pkg.fset.clone().expect("fixture has a FileSet");
    let mut got: Vec<(i64, i64)> = support::run_analyzer_diagnostics(gosec(), &pkg)
        .into_iter()
        .filter(|d| d.message.contains("G101:"))
        .map(|d| {
            let p = fset.position(guff::position::Pos(d.pos as i64));
            (p.line, p.column)
        })
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            // ValueSpec: `const secretConst`, `var secretVar`, and the two
            // whose directive names G102 rather than G101.
            (27, 7),
            (31, 5),
            (36, 7),
            (44, 5),
            // AssignStmt.
            (49, 2),
            // CompositeLit: `CompositeLit.Pos()` is the type expression, so
            // the column is the `c` of `creds`, not the `{` five over.
            (79, 10),
            (86, 10),
            (113, 10),
            // BinaryExpr: `X.Pos()`, not the `==`.
            (128, 9),
        ],
        "G101 report positions"
    );
}

#[test]
fn gosec_respects_includes_excludes() {
    use guff_style::GosecOptions;

    let pkg = support::typecheck_fixture("gosec", "example.com/gosec/settings", "settings.go");
    let all = support::run_analyzer(gosec(), &pkg);
    assert!(
        all.iter().any(|m| m.contains("G501:")),
        "default should flag import: {all:?}"
    );
    assert!(
        all.iter().any(|m| m.contains("G401:")),
        "default should flag md5.New: {all:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "gosec",
        GosecOptions {
            includes: vec!["G501".into()],
            ..Default::default()
        },
    );
    let only_import = support::run_analyzer_with_settings(
        gosec(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        only_import.iter().any(|m| m.contains("G501:")),
        "{only_import:?}"
    );
    assert!(
        !only_import.iter().any(|m| m.contains("G401:")),
        "includes=[G501] should skip call rules: {only_import:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "gosec",
        GosecOptions {
            excludes: vec!["G501".into()],
            ..Default::default()
        },
    );
    let no_import = support::run_analyzer_with_settings(
        gosec(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !no_import.iter().any(|m| m.contains("G501:")),
        "{no_import:?}"
    );
    assert!(
        no_import.iter().any(|m| m.contains("G401:")),
        "{no_import:?}"
    );
}

#[test]
fn grouper_flags_ungrouped_and_multiple_decls() {
    use guff_style::GrouperOptions;

    let pkg = support::typecheck_fixture("grouper", "example.com/grouper", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert("grouper", GrouperOptions::enabled());
    let messages = support::run_analyzer_with_settings(
        grouper(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("should only use a single 'import' declaration")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("should only use grouped 'import' declarations")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("should only use a single global 'const' declaration")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("should only use grouped global 'const' declarations")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("should only use a single global 'var' declaration")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("should only use a single global 'type' declaration")),
        "{messages:?}"
    );
}

#[test]
fn grouper_allows_single_grouped_decls() {
    use guff_style::GrouperOptions;

    let pkg = support::typecheck_fixture("grouper", "example.com/grouper/ok", "ok.go");
    let mut bag = SettingsBag::new();
    bag.insert("grouper", GrouperOptions::enabled());
    let messages = support::run_analyzer_with_settings(
        grouper(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn grouper_default_is_noop() {
    let pkg = support::typecheck_fixture("grouper", "example.com/grouper/settings", "settings.go");
    let messages = support::run_analyzer(grouper(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn grouper_respects_partial_settings() {
    use guff_style::GrouperOptions;

    let pkg = support::typecheck_fixture("grouper", "example.com/grouper/settings", "settings.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "grouper",
        GrouperOptions {
            import_require_grouping: true,
            const_require_grouping: true,
            ..GrouperOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        grouper(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("should only use grouped 'import' declarations")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("should only use grouped global 'const' declarations")),
        "{messages:?}"
    );
}

#[test]
fn ireturn_flags_named_interfaces() {
    let pkg = support::typecheck_fixture("ireturn", "example.com/ireturn", "bad.go");
    let messages = support::run_analyzer(ireturn(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("NewDoer returns interface") && m.contains("Doer")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("NewFooer returns interface") && m.contains("Fooer")),
        "{messages:?}"
    );
}

#[test]
fn ireturn_allows_defaults() {
    let pkg = support::typecheck_fixture("ireturn", "example.com/ireturn/ok", "ok.go");
    let messages = support::run_analyzer(ireturn(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn ireturn_respects_reject_empty() {
    use guff_style::IreturnOptions;

    let pkg =
        support::typecheck_fixture("ireturn", "example.com/ireturn/settings", "settings.go");

    // Defaults allow empty → no diagnostic for interface{}.
    let defaults = support::run_analyzer(ireturn(), &pkg);
    assert!(
        defaults.iter().any(|m| m.contains("ReturnsLocal returns interface")),
        "{defaults:?}"
    );
    assert!(
        !defaults.iter().any(|m| m.contains("ReturnsEmpty")),
        "{defaults:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "ireturn",
        IreturnOptions {
            allow: vec![],
            reject: vec!["empty".to_string()],
        },
    );
    let messages = support::run_analyzer_with_settings(
        ireturn(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("ReturnsEmpty returns interface (interface{})")),
        "{messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("ReturnsLocal")),
        "{messages:?}"
    );
}

#[test]
fn iotamixing_flags_mixed_const_block() {
    let pkg = support::typecheck_fixture("iotamixing", "example.com/iotamixing", "bad.go");
    let messages = support::run_analyzer(iotamixing(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("iota mixing. keep iotas in separate blocks to consts with r-val")),
        "{messages:?}"
    );
}

#[test]
fn iotamixing_allows_separated_blocks() {
    let pkg = support::typecheck_fixture("iotamixing", "example.com/iotamixing/ok", "ok.go");
    let messages = support::run_analyzer(iotamixing(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn iotamixing_report_individual() {
    use guff_style::IotamixingOptions;

    let pkg =
        support::typecheck_fixture("iotamixing", "example.com/iotamixing/settings", "settings.go");

    let block = support::run_analyzer(iotamixing(), &pkg);
    assert_eq!(block.len(), 1, "{block:?}");
    assert!(
        block[0].contains("iota mixing. keep iotas in separate blocks to consts with r-val"),
        "{block:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "iotamixing",
        IotamixingOptions {
            report_individual: true,
        },
    );
    let individual = support::run_analyzer_with_settings(
        iotamixing(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        individual
            .iter()
            .any(|m| m.contains("Above is a const with r-val")),
        "{individual:?}"
    );
    assert!(
        individual
            .iter()
            .any(|m| m.contains("Between is a const with r-val")),
        "{individual:?}"
    );
    assert!(
        individual
            .iter()
            .any(|m| m.contains("Below is a const with r-val")),
        "{individual:?}"
    );
    assert_eq!(individual.len(), 3, "{individual:?}");
}

#[test]
fn decorder_flags_multiple_decls_order_and_late_init() {
    let pkg = support::typecheck_fixture("decorder", "example.com/decorder", "bad.go");
    let messages = support::run_analyzer(decorder(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("multiple \"type\" declarations are not allowed")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("multiple \"const\" declarations are not allowed")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("multiple \"var\" declarations are not allowed")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("init func must be the first function in file")),
        "{messages:?}"
    );
}

#[test]
fn decorder_allows_grouped_decls_with_init_first() {
    let pkg = support::typecheck_fixture("decorder", "example.com/decorder/ok", "ok.go");
    let messages = support::run_analyzer(decorder(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn decorder_respects_disable_flags() {
    use guff_style::DecorderOptions;

    let pkg =
        support::typecheck_fixture("decorder", "example.com/decorder/settings", "settings.go");

    // Enabled (upstream defaults via run_analyzer): order violations.
    let flagged = support::run_analyzer(decorder(), &pkg);
    assert!(
        flagged
            .iter()
            .any(|m| m.contains("type must not be placed after func")),
        "{flagged:?}"
    );

    // Golangci defaults: all checks off → silent.
    let mut bag = SettingsBag::new();
    bag.insert("decorder", DecorderOptions::default());
    let silent = support::run_analyzer_with_settings(
        decorder(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(silent.is_empty(), "unexpected diagnostics: {silent:?}");
}

#[test]
fn tagliatelle_flags_non_camel_json_yaml() {
    let pkg = support::typecheck_fixture("tagliatelle", "example.com/tagliatelle", "bad.go");
    let messages = support::run_analyzer(tagliatelle(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("json(camel): got 'ID' want 'id'")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("json(camel): got 'UserID' want 'userId'")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("json(camel): got 'CommonServiceItem' want 'commonServiceItem'")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("yaml(camel): got 'Value' want 'value'")),
        "{messages:?}"
    );
    // A digit belongs to the word it follows and never opens one. Splitting at
    // the digit produced `Name-2` / `foo_2_bar` / `h_2_c` — fiber's
    // `header:"Name2"` and `header:"Class2"` are the first of those. `header`
    // carries no rule in this fixture: golangci-lint's wrapper defaults it.
    for want in [
        "json(camel): got 'Name2' want 'name2'",
        "json(camel): got 'Foo2Bar' want 'foo2Bar'",
        "json(camel): got 'H2C' want 'h2C'",
        "json(camel): got 'A1B2' want 'a1B2'",
        "json(camel): got 'HTTP2Server' want 'http2Server'",
        "header(header): got 'Foo2Bar' want 'Foo2-Bar'",
        "header(header): got 'H2C' want 'H2-C'",
    ] {
        assert!(
            messages.iter().any(|m| m.contains(want)),
            "missing `{want}`: {messages:?}"
        );
    }
    // `Name2` is already the header convention, so it is not a finding at all.
    assert!(
        messages.iter().all(|m| !m.contains("header(header): got 'Name2'")),
        "`Name2` is already correct for the header case: {messages:?}"
    );
}

#[test]
fn tagliatelle_allows_camel_tags() {
    let pkg =
        support::typecheck_fixture("tagliatelle", "example.com/tagliatelle/ok", "ok.go");
    let messages = support::run_analyzer(tagliatelle(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn tagliatelle_respects_rules_and_ignored_fields() {
    use guff_style::TagliatelleOptions;
    use std::collections::HashMap;

    let pkg = support::typecheck_fixture(
        "tagliatelle",
        "example.com/tagliatelle/settings",
        "settings.go",
    );

    // Defaults (camel): UserID and Name and SkipMe are wrong.
    let flagged = support::run_analyzer(tagliatelle(), &pkg);
    assert!(
        flagged.iter().any(|m| m.contains("UserID")),
        "{flagged:?}"
    );
    assert!(flagged.iter().any(|m| m.contains("Name")), "{flagged:?}");
    assert!(
        flagged.iter().any(|m| m.contains("SkipMe")),
        "{flagged:?}"
    );

    // snake + use-field-name + ignore SkipMe.
    let mut bag = SettingsBag::new();
    bag.insert(
        "tagliatelle",
        TagliatelleOptions {
            rules: HashMap::from([("json".into(), "snake".into())]),
            extended_rules: HashMap::new(),
            use_field_name: true,
            ignored_fields: vec!["SkipMe".into()],
            ignore: false,
        },
    );
    let with_settings = support::run_analyzer_with_settings(
        tagliatelle(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        with_settings
            .iter()
            .any(|m| m.contains("json(snake): got 'UserID' want 'user_id'")),
        "{with_settings:?}"
    );
    assert!(
        with_settings
            .iter()
            .any(|m| m.contains("json(snake): got 'Name' want 'name'")),
        "{with_settings:?}"
    );
    assert!(
        !with_settings.iter().any(|m| m.contains("SkipMe")),
        "SkipMe should be ignored: {with_settings:?}"
    );
}

#[test]
fn recvcheck_flags_mixed_receivers() {
    let pkg = support::typecheck_fixture("recvcheck", "example.com/recvcheck", "bad.go");
    let messages = support::run_analyzer(recvcheck(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("the methods of \"RPC\"")
                && m.contains("pointer receiver and non-pointer receiver")),
        "{messages:?}"
    );
    // `Period` too: a pointer `UnmarshalJSON` is not on the exclusion list that
    // golangci-lint 2.12.2 pins, so it counts towards the mix.
    assert!(
        messages.iter().any(|m| m.contains("the methods of \"Period\"")),
        "{messages:?}"
    );
    assert_eq!(messages.len(), 2, "{messages:?}");
}

#[test]
fn recvcheck_allows_consistent_and_builtin_marshal() {
    let pkg = support::typecheck_fixture("recvcheck", "example.com/recvcheck/ok", "ok.go");
    let messages = support::run_analyzer(recvcheck(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn recvcheck_respects_disable_builtin_and_exclusions() {
    use guff_style::RecvcheckOptions;

    let pkg =
        support::typecheck_fixture("recvcheck", "example.com/recvcheck/settings", "settings.go");

    // Default: UnmarshalJSON excluded → only SQL mixed receivers.
    let flagged = support::run_analyzer(recvcheck(), &pkg);
    assert!(
        flagged.iter().any(|m| m.contains("the methods of \"SQL\"")),
        "{flagged:?}"
    );
    assert!(
        !flagged.iter().any(|m| m.contains("JSON")),
        "UnmarshalJSON should be built-in excluded: {flagged:?}"
    );

    // disable-builtin: JSON also flagged.
    let mut bag = SettingsBag::new();
    bag.insert(
        "recvcheck",
        RecvcheckOptions {
            disable_builtin: true,
            exclusions: Vec::new(),
        },
    );
    let with_disabled = support::run_analyzer_with_settings(
        recvcheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        with_disabled
            .iter()
            .any(|m| m.contains("the methods of \"JSON\"")),
        "{with_disabled:?}"
    );
    assert!(
        with_disabled
            .iter()
            .any(|m| m.contains("the methods of \"SQL\"")),
        "{with_disabled:?}"
    );

    // exclusions: SQL.Value → SQL clean; JSON still excluded by builtin.
    let mut bag = SettingsBag::new();
    bag.insert(
        "recvcheck",
        RecvcheckOptions {
            disable_builtin: false,
            exclusions: vec!["SQL.Value".into()],
        },
    );
    let with_excl = support::run_analyzer_with_settings(
        recvcheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        with_excl.is_empty(),
        "SQL.Value exclusion + builtin Unmarshal should clear all: {with_excl:?}"
    );
}

#[test]
fn iface_flags_identical_interfaces_by_default() {
    let pkg = support::typecheck_fixture("iface", "example.com/iface", "bad.go");
    let messages = support::run_analyzer(iface(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface 'Pinger'") && m.contains("Healthcheck")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface 'Healthcheck'") && m.contains("Pinger")),
        "{messages:?}"
    );
    // Default enable is identical only — Granter unused must not be reported.
    assert!(
        !messages.iter().any(|m| m.contains("Granter")),
        "unused should be off by default: {messages:?}"
    );
    assert_eq!(messages.len(), 2, "{messages:?}");
}

#[test]
fn iface_allows_distinct_and_used_interfaces() {
    let pkg = support::typecheck_fixture("iface", "example.com/iface/ok", "ok.go");
    let messages = support::run_analyzer(iface(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn iface_respects_enable_unused_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::IfaceOptions;

    let pkg = support::typecheck_fixture("iface", "example.com/iface/settings", "settings.go");

    // Default: identical only (Alpha/Beta).
    let flagged = support::run_analyzer(iface(), &pkg);
    assert!(
        flagged.iter().any(|m| m.contains("interface 'Alpha'")),
        "{flagged:?}"
    );
    assert!(
        !flagged.iter().any(|m| m.contains("Orphan")),
        "{flagged:?}"
    );

    // enable unused only: Orphan, not Alpha/Beta identical.
    let mut bag = SettingsBag::new();
    bag.insert(
        "iface",
        IfaceOptions {
            enable: vec!["unused".into()],
            unused_exclude: Vec::new(),
        },
    );
    let messages = support::run_analyzer_with_settings(
        iface(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface 'Orphan'") && m.contains("not used")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("interface 'Alpha'")),
        "identical-only interfaces are also unused: {messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("identical methods") || m.contains("redundancy")),
        "{messages:?}"
    );
}

#[test]
fn thelper_flags_begin_first_name() {
    let pkg = support::typecheck_fixture("thelper", "example.com/thelper", "bad.go");
    let messages = support::run_analyzer(thelper(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("test helper function should start from t.Helper()")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("parameter *testing.T should be the first")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("parameter *testing.T should have name t")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("test helper function should start from b.Helper()")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("parameter *testing.B should have name b")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("test helper function should start from tb.Helper()")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("parameter testing.TB should have name tb")),
        "{messages:?}"
    );
    // anotherCheck is also called from check → not filtered.
    assert!(
        messages
            .iter()
            .any(|m| m.contains("test helper function should start from t.Helper()")),
        "{messages:?}"
    );
    // Anonymous subtest / Test* entry points should not appear alone as false positives.
    assert!(
        !messages.iter().any(|m| m.contains("TestSomething")),
        "{messages:?}"
    );
}

#[test]
fn thelper_allows_valid_helpers_and_filtered_subtests() {
    let pkg = support::typecheck_fixture("thelper", "example.com/thelper/ok", "ok.go");
    let messages = support::run_analyzer(thelper(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn thelper_respects_kind_settings() {
    use guff_style::{ThelperKindOptions, ThelperOptions};

    let pkg = support::typecheck_fixture("thelper", "example.com/thelper/settings", "settings.go");

    // Default: begin reports helperWithoutHelper.
    let flagged = support::run_analyzer(thelper(), &pkg);
    assert!(
        flagged
            .iter()
            .any(|m| m.contains("test helper function should start from t.Helper()")),
        "{flagged:?}"
    );

    // begin off, name on: only wrong name.
    let mut bag = SettingsBag::new();
    bag.insert(
        "thelper",
        ThelperOptions {
            test: ThelperKindOptions {
                first: false,
                name: true,
                begin: false,
            },
            ..ThelperOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        thelper(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("parameter *testing.T should have name t")),
        "{messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("test helper function should start from t.Helper()")),
        "{messages:?}"
    );
}

#[test]
fn copyloopvar_flags_redundant_copies() {
    let pkg = support::typecheck_fixture("copyloopvar", "example.com/copyloopvar", "bad.go");
    let messages = support::run_analyzer(copyloopvar(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("for") && m.contains("\"i\"")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("for") && m.contains("\"v\"")),
        "{messages:?}"
    );
}

#[test]
fn asasalint_flags_slice_any_as_variadic_any() {
    let pkg = support::typecheck_fixture("asasalint", "example.com/asasalint", "bad.go");
    let messages = support::run_analyzer(asasalint(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("pass []any as any to func A")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("pass []any as any to func errMsg")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("pass []any as any to func B")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("pass []any as any to func Err")),
        "{messages:?}"
    );
    assert!(messages.len() >= 4, "{messages:?}");
}

#[test]
fn asasalint_allows_spread_and_builtin_exclusions() {
    let pkg = support::typecheck_fixture("asasalint", "example.com/asasalint/ok", "ok.go");
    let messages = support::run_analyzer(asasalint(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn asasalint_respects_exclude_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::AsasalintOptions;

    let pkg = support::typecheck_fixture(
        "asasalint",
        "example.com/asasalint/settings",
        "settings.go",
    );

    // Default: Append is reported.
    let flagged = support::run_analyzer(asasalint(), &pkg);
    assert!(
        flagged
            .iter()
            .any(|m| m.contains("pass []any as any to func Append")),
        "{flagged:?}"
    );

    // With exclude: Append is silenced.
    let mut bag = SettingsBag::new();
    bag.insert(
        "asasalint",
        AsasalintOptions {
            exclude: vec!["Append".into()],
            use_builtin_exclusions: false,
        },
    );
    let messages = support::run_analyzer_with_settings(
        asasalint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn reassign_flags_other_package_err_and_eof() {
    let pkg = support::typecheck_fixture("reassign", "example.com/reassign", "bad.go");
    let messages = support::run_analyzer(reassign(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("reassigning variable ErrB in other package b")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("reassigning variable EOF in other package io")),
        "{messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("NotErr")),
        "NotErr should not match default pattern: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("ErrSt")),
        "struct field should not be reported: {messages:?}"
    );
    assert_eq!(messages.len(), 2, "{messages:?}");
}

#[test]
fn reassign_allows_local_and_non_matching() {
    let pkg = support::typecheck_fixture("reassign", "example.com/reassign/ok", "ok.go");
    let messages = support::run_analyzer(reassign(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn reassign_respects_patterns_settings() {
    use guff_style::ReassignOptions;

    let pkg = support::typecheck_fixture("reassign", "example.com/reassign/settings", "settings.go");

    // Default: only ErrB.
    let flagged = support::run_analyzer(reassign(), &pkg);
    assert!(
        flagged
            .iter()
            .any(|m| m.contains("reassigning variable ErrB")),
        "{flagged:?}"
    );
    assert!(
        !flagged.iter().any(|m| m.contains("NotErr")),
        "{flagged:?}"
    );

    // patterns: [".*"] → both.
    let mut bag = SettingsBag::new();
    bag.insert(
        "reassign",
        ReassignOptions {
            patterns: vec![".*".into()],
        },
    );
    let messages = support::run_analyzer_with_settings(
        reassign(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("reassigning variable ErrB")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("reassigning variable NotErr")),
        "{messages:?}"
    );
    assert_eq!(messages.len(), 2, "{messages:?}");
}

#[test]
fn interfacebloat_flags_large_interface() {
    let pkg =
        support::typecheck_fixture("interfacebloat", "example.com/interfacebloat", "bad.go");
    let messages = support::run_analyzer(interfacebloat(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(
        messages[0].contains("the interface has more than 10 methods: 11"),
        "{messages:?}"
    );
}

#[test]
fn interfacebloat_allows_interfaces_within_limit() {
    let pkg =
        support::typecheck_fixture("interfacebloat", "example.com/interfacebloat/ok", "ok.go");
    let messages = support::run_analyzer(interfacebloat(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn interfacebloat_respects_custom_max() {
    use guff_style::InterfacebloatOptions;

    let pkg = support::typecheck_fixture(
        "interfacebloat",
        "example.com/interfacebloat/settings",
        "settings.go",
    );

    // Default max (10): three-method interface is fine.
    assert!(support::run_analyzer(interfacebloat(), &pkg).is_empty());

    // max = 2: the three-method interface is now flagged.
    let mut bag = SettingsBag::new();
    bag.insert("interfacebloat", InterfacebloatOptions { max: 2 });
    let flagged = support::run_analyzer_with_settings(
        interfacebloat(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    assert!(
        flagged[0].contains("the interface has more than 2 methods: 3"),
        "{flagged:?}"
    );
}

#[test]
fn embeddedstructfieldcheck_flags_order_and_spacing() {
    let pkg = support::typecheck_fixture(
        "embeddedstructfieldcheck",
        "example.com/embeddedstructfieldcheck",
        "bad.go",
    );
    let messages = support::run_analyzer(embeddedstructfieldcheck(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("there must be an empty line separating embedded fields")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("embedded fields should be listed before regular fields")),
        "{messages:?}"
    );
    assert!(
        messages.len() >= 4,
        "expected order + spacing reports: {messages:?}"
    );
}

#[test]
fn embeddedstructfieldcheck_allows_sorted_with_blank_line() {
    let pkg = support::typecheck_fixture(
        "embeddedstructfieldcheck",
        "example.com/embeddedstructfieldcheck/ok",
        "ok.go",
    );
    let messages = support::run_analyzer(embeddedstructfieldcheck(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn embeddedstructfieldcheck_empty_line_respects_field_doc() {
    // Upstream comments-empty-line: Field.Doc must be used so a blank line
    // between embedded and a documented regular field is accepted.
    let pkg = support::typecheck_fixture(
        "embeddedstructfieldcheck",
        "example.com/embeddedstructfieldcheck/comments",
        "comments.go",
    );
    let messages = support::run_analyzer(embeddedstructfieldcheck(), &pkg);
    assert!(
        !messages.iter().any(|m| m.contains("ValidStructWithSingleLineComments")
            || (m.contains("there must be an empty line") && m.contains("time.Time") && !m.contains("version"))),
        "valid blank+doc case must not flag: {messages:?}"
    );
    let spacing: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("there must be an empty line separating embedded fields"))
        .collect();
    assert_eq!(
        spacing.len(),
        2,
        "expected two missing-blank reports (single + multi-line doc): {messages:?}"
    );
}

#[test]
fn embeddedstructfieldcheck_respects_settings() {
    use guff_style::EmbeddedstructfieldcheckOptions;

    let pkg = support::typecheck_fixture(
        "embeddedstructfieldcheck",
        "example.com/embeddedstructfieldcheck/settings",
        "settings.go",
    );

    // Defaults: empty-line on → missing blank line flagged; forbid-mutex off → mutex OK.
    let default_msgs = support::run_analyzer(embeddedstructfieldcheck(), &pkg);
    assert!(
        default_msgs
            .iter()
            .any(|m| m.contains("there must be an empty line separating embedded fields")),
        "{default_msgs:?}"
    );
    assert!(
        !default_msgs.iter().any(|m| m.contains("should not be embedded")),
        "mutex should be allowed by default: {default_msgs:?}"
    );

    // empty-line off + forbid-mutex on.
    let mut bag = SettingsBag::new();
    bag.insert(
        "embeddedstructfieldcheck",
        EmbeddedstructfieldcheckOptions {
            empty_line: false,
            forbid_mutex: true,
        },
    );
    let flagged = support::run_analyzer_with_settings(
        embeddedstructfieldcheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !flagged
            .iter()
            .any(|m| m.contains("there must be an empty line")),
        "empty-line:false should skip spacing: {flagged:?}"
    );
    assert!(
        flagged
            .iter()
            .any(|m| m.contains("sync.Mutex should not be embedded")),
        "{flagged:?}"
    );
    assert!(
        flagged
            .iter()
            .any(|m| m.contains("sync.RWMutex should not be embedded")),
        "{flagged:?}"
    );
    assert_eq!(flagged.len(), 3, "{flagged:?}"); // Mutex, *Mutex, RWMutex
}

#[test]
fn gochecksumtype_flags_incomplete_and_bad_decl() {
    let pkg = support::typecheck_fixture("gochecksumtype", "example.com/gochecksumtype", "bad.go");
    let messages = support::run_analyzer(gochecksumtype(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("type 'One' is not an interface")),
        "{messages:?}"
    );
    let missing: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("exhaustiveness check failed") && m.contains("missing cases for Two"))
        .collect();
    assert!(
        missing.len() >= 2,
        "expected ≥2 incomplete switches (no default + panic default): {messages:?}"
    );
}

#[test]
fn gochecksumtype_allows_exhaustive() {
    let pkg =
        support::typecheck_fixture("gochecksumtype", "example.com/gochecksumtype/ok", "ok.go");
    let messages = support::run_analyzer(gochecksumtype(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn gochecksumtype_respects_settings() {
    use guff_style::GochecksumtypeOptions;

    let pkg = support::typecheck_fixture(
        "gochecksumtype",
        "example.com/gochecksumtype/settings",
        "settings.go",
    );

    // Default: non-panic default satisfies exhaustiveness.
    let default_msgs = support::run_analyzer(gochecksumtype(), &pkg);
    assert!(
        default_msgs.is_empty(),
        "default should allow non-panic default: {default_msgs:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "gochecksumtype",
        GochecksumtypeOptions {
            default_signifies_exhaustive: false,
            include_shared_interfaces: false,
        },
    );
    let flagged = support::run_analyzer_with_settings(
        gochecksumtype(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        flagged
            .iter()
            .any(|m| m.contains("exhaustiveness check failed") && m.contains("missing cases for Two")),
        "{flagged:?}"
    );
}

#[test]
fn inamedparam_flags_unnamed_interface_params() {
    let pkg = support::typecheck_fixture("inamedparam", "example.com/inamedparam", "bad.go");
    let messages = support::run_analyzer(inamedparam(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface method SingleParam must have named param for type context.Context")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface method WithoutName must have named param for type context.Context")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface method WithoutName must have named param for type int")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface method WithoutName must have named param for type bool")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface method WithoutName must have all named params")),
        "{messages:?}"
    );
    assert_eq!(messages.len(), 5, "{messages:?}");
}

#[test]
fn inamedparam_allows_named_params() {
    let pkg =
        support::typecheck_fixture("inamedparam", "example.com/inamedparam/ok", "ok.go");
    let messages = support::run_analyzer(inamedparam(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn inamedparam_respects_skip_single_param() {
    use guff_style::InamedparamOptions;

    let pkg = support::typecheck_fixture(
        "inamedparam",
        "example.com/inamedparam/settings",
        "settings.go",
    );

    // Default: single unnamed param is flagged.
    let flagged = support::run_analyzer(inamedparam(), &pkg);
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    assert!(
        flagged[0].contains("interface method Run must have named param for type context.Context"),
        "{flagged:?}"
    );

    // skip-single-param: true → no report.
    let mut bag = SettingsBag::new();
    bag.insert(
        "inamedparam",
        InamedparamOptions {
            skip_single_param: true,
        },
    );
    let skipped = support::run_analyzer_with_settings(
        inamedparam(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(skipped.is_empty(), "unexpected diagnostics: {skipped:?}");
}

#[test]
fn arangolint_flags_missing_allow_implicit_and_query_concatenation() {
    let pkg = support::typecheck_fixture("arangolint", "example.com/arangolint", "bad.go");
    let messages = support::run_analyzer(arangolint(), &pkg);
    let missing = messages
        .iter()
        .filter(|m| m.contains("missing AllowImplicit option"))
        .count();
    assert_eq!(missing, 2, "{messages:?}");
    let concat = messages
        .iter()
        .filter(|m| m.contains("query string uses concatenation instead of bind variables"))
        .count();
    assert_eq!(concat, 3, "{messages:?}");
}

#[test]
fn arangolint_allows_explicit_options_and_static_queries() {
    let pkg = support::typecheck_fixture("arangolint", "example.com/arangolint/ok", "ok.go");
    let messages = support::run_analyzer(arangolint(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn clickhouselint_flags_missing_err_and_batch_close() {
    let pkg = support::typecheck_fixture("clickhouselint", "example.com/clickhouselint", "bad.go");
    let messages = support::run_analyzer(clickhouselint(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Err() must be checked after")),
        "missing rows.Err diagnostic: {messages:?}"
    );
    let batch = messages
        .iter()
        .filter(|m| m.contains("must be closed defensively"))
        .count();
    assert!(batch >= 2, "expected ≥2 missing Close diagnostics: {messages:?}");
    assert!(
        messages
            .iter()
            .any(|m| m.contains("assigned to blank identifier")),
        "missing blank Batch diagnostic: {messages:?}"
    );
}

#[test]
fn clickhouselint_allows_err_defer_close_and_return() {
    let pkg =
        support::typecheck_fixture("clickhouselint", "example.com/clickhouselint/ok", "ok.go");
    let messages = support::run_analyzer(clickhouselint(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn wsl_v5_flags_cuddle_and_err_whitespace() {
    let pkg = support::typecheck_fixture("wsl_v5", "example.com/wsl_v5", "bad.go");
    let messages = support::run_analyzer(wsl_v5(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("too many statements above if")),
        "expected too-many-statements if: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("no shared variables above if")),
        "expected no-shared if: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("never cuddle decl")),
        "expected never cuddle decl: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("unnecessary whitespace (err)")),
        "expected err whitespace: {messages:?}"
    );
}

#[test]
fn wsl_v5_allows_proper_spacing() {
    let pkg = support::typecheck_fixture("wsl_v5", "example.com/wsl_v5/ok", "ok.go");
    let messages = support::run_analyzer(wsl_v5(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn wsl_v5_respects_cuddle_max_statements() {
    use guff_style::WslV5Options;

    let pkg = support::typecheck_fixture("wsl_v5", "example.com/wsl_v5/settings", "settings.go");
    assert!(
        !support::run_analyzer(wsl_v5(), &pkg).is_empty(),
        "default cuddle-max=1 should flag settings.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "wsl_v5",
        WslV5Options {
            cuddle_max_statements: 2,
            ..WslV5Options::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        wsl_v5(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "cuddle-max-statements=2 should allow two shared assigns: {messages:?}"
    );
}

#[test]
fn containedctx_flags_context_fields() {
    let pkg = support::typecheck_fixture("containedctx", "example.com/containedctx", "bad.go");
    let messages = support::run_analyzer(containedctx(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m.contains("found a struct that contains a context.Context field")),
        "{messages:?}"
    );
}

#[test]
fn canonicalheader_flags_non_canonical_keys() {
    let pkg =
        support::typecheck_fixture("canonicalheader", "example.com/canonicalheader", "bad.go");
    let messages = support::run_analyzer(canonicalheader(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("non-canonical header \"Test-Header\"")
                || m.contains("non-canonical header \"Test-HEader\"")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("non-canonical header \"Raw-STRING-Literal\"")
                || m.contains("instead use: \"Raw-String-Literal\"")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("non-canonical header \"testHeaderValue\"")
                || m.contains("instead use: \"Testheadervalue\"")),
        "{messages:?}"
    );
    // `etag` is **not** reported. Upstream's `canonicalHeaderKey` returns
    // `isWellKnown` when the MIME-canonical form is in the initialism table,
    // and its caller returns on that — so the table only ever suppresses:
    //
    //     if argValue == headerKeyCanonical || isWellKnown { return }
    //
    // This test used to assert the opposite, and no tier could see it: the
    // isolate fixture reached the linter through `content-type` alone.
    assert!(
        !messages.iter().any(|m| m.contains("\"etag\"")),
        "etag canonicalizes into the initialism table, so upstream is silent: {messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("\"www-authenticate\"")),
        "{messages:?}"
    );
    assert!(messages.len() >= 6, "{messages:?}");
}

#[test]
fn canonicalheader_allows_canonical_keys() {
    let pkg =
        support::typecheck_fixture("canonicalheader", "example.com/canonicalheader/ok", "ok.go");
    let messages = support::run_analyzer(canonicalheader(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn containedctx_allows_non_context_fields() {
    let pkg =
        support::typecheck_fixture("containedctx", "example.com/containedctx/ok", "ok.go");
    let messages = support::run_analyzer(containedctx(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn spancheck_flags_unassigned_and_missing_end() {
    let pkg = support::typecheck_fixture("spancheck", "example.com/spancheck", "bad.go");
    let messages = support::run_analyzer(spancheck(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("span is unassigned, probable memory leak")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains(".End is not called on all paths")),
        "{messages:?}"
    );
}

#[test]
fn spancheck_allows_end_calls() {
    let pkg = support::typecheck_fixture("spancheck", "example.com/spancheck/ok", "ok.go");
    assert!(
        support::run_analyzer(spancheck(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(spancheck(), &pkg)
    );
}

#[test]
fn nonamedreturns_flags_named_returns() {
    let pkg = support::typecheck_fixture("nonamedreturns", "example.com/nonamedreturns", "bad.go");
    let messages = support::run_analyzer(nonamedreturns(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("named return \"i\" with type \"int\" found")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("named return \"err\" with type \"error\" found")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("named return \"a\" with type \"int\" found")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("named return \"b\" with type \"string\" found")),
        "{messages:?}"
    );
    assert_eq!(messages.len(), 5, "{messages:?}");
}

#[test]
fn nonamedreturns_allows_unnamed_and_error_in_defer() {
    let pkg =
        support::typecheck_fixture("nonamedreturns", "example.com/nonamedreturns/ok", "ok.go");
    let messages = support::run_analyzer(nonamedreturns(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn nonamedreturns_report_error_in_defer() {
    use guff_style::NonamedreturnsOptions;

    let pkg = support::typecheck_fixture(
        "nonamedreturns",
        "example.com/nonamedreturns/report",
        "report_error.go",
    );

    // Default: error-in-defer exemption applies → only named int flagged.
    let flagged = support::run_analyzer(nonamedreturns(), &pkg);
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    assert!(
        flagged[0].contains("named return \"i\" with type \"int\" found"),
        "{flagged:?}"
    );

    // report-error-in-defer: true → both flagged.
    let mut bag = SettingsBag::new();
    bag.insert(
        "nonamedreturns",
        NonamedreturnsOptions {
            report_error_in_defer: true,
            allow_unused_named_returns: false,
        },
    );
    let reported = support::run_analyzer_with_settings(
        nonamedreturns(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert_eq!(reported.len(), 2, "{reported:?}");
    assert!(
        reported
            .iter()
            .any(|m| m.contains("named return \"err\" with type \"error\" found")),
        "{reported:?}"
    );
}

#[test]
fn nonamedreturns_allow_unused_named_returns() {
    use guff_style::NonamedreturnsOptions;

    let pkg = support::typecheck_fixture(
        "nonamedreturns",
        "example.com/nonamedreturns/allow",
        "allow_unused.go",
    );

    // Default: all named returns (except underscore) are flagged.
    let flagged = support::run_analyzer(nonamedreturns(), &pkg);
    assert!(
        flagged
            .iter()
            .any(|m| m.contains("named return \"sum\" with type \"int\" found")),
        "{flagged:?}"
    );

    // allow-unused: only referenced / naked-return cases.
    let mut bag = SettingsBag::new();
    bag.insert(
        "nonamedreturns",
        NonamedreturnsOptions {
            report_error_in_defer: false,
            allow_unused_named_returns: true,
        },
    );
    let messages = support::run_analyzer_with_settings(
        nonamedreturns(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages.iter().any(|m| m.contains(
            "named return \"sum\" with type \"int\" must not be referenced or used by a naked return"
        )),
        "{messages:?}"
    );
}

#[test]
fn testpackage_flags_same_package_tests() {
    let pkg = support::typecheck_fixture(
        "testpackage",
        "example.com/testpackage",
        "bad_test.go",
    );
    let messages = support::run_analyzer(testpackage(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(
        messages[0].contains("package should be `testpackage_test` instead of `testpackage`"),
        "{messages:?}"
    );
}

#[test]
fn testpackage_allows_external_test_package() {
    let pkg = support::typecheck_fixture(
        "testpackage",
        "example.com/testpackage_test",
        "ok_test.go",
    );
    let messages = support::run_analyzer(testpackage(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn testpackage_skips_internal_test_by_default() {
    let pkg = support::typecheck_fixture(
        "testpackage",
        "example.com/testpackage/internal",
        "internal_test.go",
    );
    let messages = support::run_analyzer(testpackage(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn testpackage_allows_main_by_default() {
    let pkg = support::typecheck_fixture(
        "testpackage",
        "example.com/testpackage/main",
        "main_test.go",
    );
    let messages = support::run_analyzer(testpackage(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn testpackage_respects_allow_packages_settings() {
    use guff_style::TestpackageOptions;

    let pkg = support::typecheck_fixture(
        "testpackage",
        "example.com/testpackage/settings",
        "settings_test.go",
    );

    // Default allow-packages is only `main` → flagged.
    let flagged = support::run_analyzer(testpackage(), &pkg);
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    assert!(
        flagged[0].contains("package should be `allowed_test` instead of `allowed`"),
        "{flagged:?}"
    );

    // Custom allow-packages includes `allowed` → clean.
    let mut bag = SettingsBag::new();
    bag.insert(
        "testpackage",
        TestpackageOptions {
            skip_regexp: r"(export|internal)_test\.go".into(),
            allow_packages: vec!["allowed".into()],
        },
    );
    let allowed = support::run_analyzer_with_settings(
        testpackage(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(allowed.is_empty(), "unexpected diagnostics: {allowed:?}");
}

#[test]
fn paralleltest_flags_missing_parallel() {
    let pkg = support::typecheck_fixture(
        "paralleltest",
        "example.com/paralleltest",
        "bad_test.go",
    );
    let messages = support::run_analyzer(paralleltest(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Function TestMissingParallel missing the call to method parallel")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains(
            "Range statement for test TestRangeMissingParallel missing the call to method parallel in test Run"
        )),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains(
            "Function TestSubtestsMissingParallel missing the call to method parallel in the test run"
        )),
        "{messages:?}"
    );
}

#[test]
fn paralleltest_allows_valid_parallel_usage() {
    let pkg = support::typecheck_fixture(
        "paralleltest",
        "example.com/paralleltest/ok",
        "ok_test.go",
    );
    let messages = support::run_analyzer(paralleltest(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn paralleltest_respects_settings() {
    use guff_style::ParalleltestOptions;

    let pkg = support::typecheck_fixture(
        "paralleltest",
        "example.com/paralleltest/settings",
        "settings_test.go",
    );

    // Default: missing parallel is flagged; cleanup not checked.
    let flagged = support::run_analyzer(paralleltest(), &pkg);
    assert!(
        flagged
            .iter()
            .any(|m| m.contains("Function TestMissingButIgnored missing the call to method parallel")),
        "{flagged:?}"
    );
    assert!(
        !flagged.iter().any(|m| m.contains("uses defer with t.Parallel")),
        "{flagged:?}"
    );

    // ignore-missing + check-cleanup.
    let mut bag = SettingsBag::new();
    bag.insert(
        "paralleltest",
        ParalleltestOptions {
            ignore_missing: true,
            ignore_missing_subtests: false,
            check_cleanup: true,
        },
    );
    let messages = support::run_analyzer_with_settings(
        paralleltest(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("Function TestMissingButIgnored missing the call to method parallel")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Function TestCleanupDefer uses defer with t.Parallel")),
        "{messages:?}"
    );
}

#[test]
fn tparallel_flags_mismatched_parallel_and_defer() {
    let pkg = support::typecheck_fixture("tparallel", "example.com/tparallel", "bad_test.go");
    let messages = support::run_analyzer(tparallel(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains(
            "Test_Func1 should call t.Parallel on the top level as well as its subtests"
        )),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Test_Func2's subtests should call t.Parallel")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Test_Cleanup1 should use t.Cleanup instead of defer")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains(
            "Test_Table1 should call t.Parallel on the top level as well as its subtests"
        )),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Test_Table1 should use t.Cleanup instead of defer")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Test_Table2's subtests should call t.Parallel")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains(
            "Test_NamedSub should call t.Parallel on the top level as well as its subtests"
        )),
        "{messages:?}"
    );
}

#[test]
fn tparallel_allows_consistent_parallel_usage() {
    let pkg =
        support::typecheck_fixture("tparallel", "example.com/tparallel/ok", "ok_test.go");
    let messages = support::run_analyzer(tparallel(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn intrange_flags_classic_for_and_range_len() {
    let pkg = support::typecheck_fixture("intrange", "example.com/intrange", "bad.go");
    let messages = support::run_analyzer(intrange(), &pkg);
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("for loop can be changed to use an integer range"))
            .count()
            >= 8,
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("for loop can be changed to `i := range s`")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("for loop can be changed to `range s`")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("returned by a function or method")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("not part of the loop's scope")),
        "{messages:?}"
    );
}

#[test]
fn intrange_allows_already_modern_or_unsafe_loops() {
    let pkg = support::typecheck_fixture("intrange", "example.com/intrange/ok", "ok.go");
    let messages = support::run_analyzer(intrange(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn gochecknoinits_flags_init_functions() {
    let pkg = support::typecheck_fixture("gochecknoinits", "example.com/gochecknoinits", "bad.go");
    let messages = support::run_analyzer(gochecknoinits(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages.iter().all(|m| m.contains("`init` function")),
        "{messages:?}"
    );
}

#[test]
fn gochecknoinits_allows_methods_and_other_names() {
    let pkg =
        support::typecheck_fixture("gochecknoinits", "example.com/gochecknoinits/ok", "ok.go");
    assert!(support::run_analyzer(gochecknoinits(), &pkg).is_empty());
}

#[test]
fn gochecknoglobals_flags_package_level_vars() {
    let pkg =
        support::typecheck_fixture("gochecknoglobals", "example.com/gochecknoglobals", "bad.go");
    let messages = support::run_analyzer(gochecknoglobals(), &pkg);
    for name in ["myVar", "myVar1", "myVar2", "Version", "version22", "theVar"] {
        assert!(
            messages
                .iter()
                .any(|m| m.contains(&format!("{name} is a global variable"))),
            "missing {name}: {messages:?}"
        );
    }
    assert_eq!(messages.len(), 6, "{messages:?}");
}

#[test]
fn gochecknoglobals_allows_exceptions() {
    let pkg = support::typecheck_fixture(
        "gochecknoglobals",
        "example.com/gochecknoglobals/ok",
        "ok.go",
    );
    let messages = support::run_analyzer(gochecknoglobals(), &pkg);
    assert!(
        messages.is_empty(),
        "unexpected diagnostics: {messages:?}"
    );
}

#[test]
fn gocheckcompilerdirectives_flags_space_and_unknown() {
    let pkg = support::typecheck_fixture(
        "gocheckcompilerdirectives",
        "example.com/gocheckcompilerdirectives",
        "bad.go",
    );
    let messages = support::run_analyzer(gocheckcompilerdirectives(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("compiler directive contains space: // go:embed")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("compiler directive contains space: //    go:embed")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("compiler directive unrecognized: //go:genrate")),
        "{messages:?}"
    );
    assert_eq!(messages.len(), 3, "{messages:?}");
}

#[test]
fn gocheckcompilerdirectives_allows_valid_directives() {
    let pkg = support::typecheck_fixture(
        "gocheckcompilerdirectives",
        "example.com/gocheckcompilerdirectives/ok",
        "ok.go",
    );
    let messages = support::run_analyzer(gocheckcompilerdirectives(), &pkg);
    assert!(
        messages.is_empty(),
        "unexpected diagnostics: {messages:?}"
    );
}

#[test]
fn forbidigo_flags_default_print_patterns() {
    let pkg = support::typecheck_fixture("forbidigo", "example.com/forbidigo", "bad.go");
    let messages = support::run_analyzer(forbidigo(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("use of `fmt.Println` forbidden")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("use of `fmt.Printf` forbidden")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("use of `print` forbidden")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("use of `println` forbidden")),
        "{messages:?}"
    );
    assert_eq!(messages.len(), 4, "{messages:?}");
}

#[test]
fn forbidigo_allows_sprintf() {
    let pkg = support::typecheck_fixture("forbidigo", "example.com/forbidigo/ok", "ok.go");
    let messages = support::run_analyzer(forbidigo(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn forbidigo_respects_custom_forbid_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::{ForbidigoOptions, ForbidigoPattern};

    let pkg = support::typecheck_fixture("forbidigo", "example.com/forbidigo/custom", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "forbidigo",
        ForbidigoOptions {
            forbid: vec![ForbidigoPattern {
                pattern: r"^fmt\.Print.*$".into(),
                pkg: String::new(),
                msg: "Do not commit print statements.".into(),
            }],
            exclude_godoc_examples: true,
            analyze_types: false,
        },
    );
    let messages = support::run_analyzer_with_settings(
        forbidigo(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    // Exact text, not `contains`: upstream renders the configured message with
    // `%q` — `fmt.Sprintf(" because %q", a.customMsg)` in forbidigo's
    // `UsedIssue.Details` — so it is double-quoted, not backticked. guff used
    // backticks, which the 2026-08-17 field report caught as a cosmetic but
    // real output difference (issue F). A `contains` assertion cannot see it.
    assert!(
        messages.iter().any(|m| m
            == r#"use of `fmt.Println` forbidden because "Do not commit print statements.""#),
        "{messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("`print`")),
        "builtin print should not match custom fmt-only pattern: {messages:?}"
    );
}

/// The other half of `UsedIssue.Details`: with no custom message upstream falls
/// back to ``" by pattern `%s`"`` — backticks there, and only there. Pinned so
/// the `%q` fix above cannot be over-applied to this branch.
#[test]
fn forbidigo_pattern_explanation_keeps_backticks() {
    let pkg = support::typecheck_fixture("forbidigo", "example.com/forbidigo", "bad.go");
    let messages = support::run_analyzer(forbidigo(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m == "use of `fmt.Println` forbidden by pattern `^(fmt\\.Print(|f|ln)|print|println)$`"),
        "{messages:?}"
    );
}

#[test]
fn bidichk_flags_dangerous_unicode_in_source() {
    let pkg = support::typecheck_fixture("bidichk", "example.com/bidichk", "bad.go");
    let messages = support::run_analyzer(bidichk(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("RIGHT-TO-LEFT-OVERRIDE")),
        "{messages:?}"
    );
    assert!(!messages.is_empty(), "{messages:?}");
}

#[test]
fn bidichk_allows_clean_source() {
    let pkg = support::typecheck_fixture("bidichk/ok", "example.com/bidichk/ok", "ok.go");
    assert!(support::run_analyzer(bidichk(), &pkg).is_empty());
}

#[test]
fn bidichk_respects_disallowed_runes_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::BidichkOptions;

    let pkg = support::typecheck_fixture("bidichk/settings", "example.com/bidichk/settings", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "bidichk",
        BidichkOptions {
            disallowed_runes: vec!["LEFT-TO-RIGHT-OVERRIDE".into()],
        },
    );
    let messages = support::run_analyzer_with_settings(
        bidichk(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("LEFT-TO-RIGHT-OVERRIDE")),
        "{messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("RIGHT-TO-LEFT-OVERRIDE")),
        "RLO should be skipped when only LRO is enabled: {messages:?}"
    );
}

#[test]
fn copyloopvar_allows_alias_copies() {
    let pkg = support::typecheck_fixture("copyloopvar", "example.com/copyloopvar/ok", "ok.go");
    assert!(support::run_analyzer(copyloopvar(), &pkg).is_empty());
}

#[test]
fn usetesting_flags_os_mkdirtemp_and_createtemp() {
    let pkg = support::typecheck_fixture("usetesting", "example.com/usetesting", "bad.go");
    let messages = support::run_analyzer(usetesting(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("os.MkdirTemp") && m.contains("t.TempDir")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("os.CreateTemp") && m.contains("t.TempDir")),
        "{messages:?}"
    );
}

#[test]
fn usetesting_allows_testing_helpers() {
    let pkg = support::typecheck_fixture("usetesting", "example.com/usetesting/ok", "ok.go");
    assert!(support::run_analyzer(usetesting(), &pkg).is_empty());
}

#[test]
fn usestdlibvars_flags_http_literals() {
    let pkg = support::typecheck_fixture("usestdlibvars", "example.com/usestdlibvars", "bad.go");
    let messages = support::run_analyzer(usestdlibvars(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("http.MethodGet")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("http.StatusNotFound")),
        "{messages:?}"
    );
}

#[test]
fn usestdlibvars_allows_stdlib_constants() {
    let pkg = support::typecheck_fixture("usestdlibvars", "example.com/usestdlibvars/ok", "ok.go");
    assert!(support::run_analyzer(usestdlibvars(), &pkg).is_empty());
}

#[test]
fn copyloopvar_check_alias_flags_renames() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::CopyloopvarOptions;

    let pkg = support::typecheck_fixture("copyloopvar", "example.com/copyloopvar/ok", "ok.go");
    assert!(
        support::run_analyzer(copyloopvar(), &pkg).is_empty(),
        "default check-alias=false should allow alias copies"
    );

    let mut bag = SettingsBag::new();
    bag.insert("copyloopvar", CopyloopvarOptions { check_alias: true });
    let messages = support::run_analyzer_with_settings(
        copyloopvar(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("for") && m.contains("\"i\"")),
        "check-alias=true should flag alias copies: {messages:?}"
    );
}

#[test]
fn usetesting_respects_os_setenv_and_temp_dir() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::UsetestingOptions;

    let pkg = support::typecheck_fixture(
        "usetesting",
        "example.com/usetesting/settings",
        "settings_extra.go",
    );
    // golangci-lint's defaults, not ldez/usetesting's: `os-setenv` is on and
    // `os-temp-dir` is off.
    let defaults = support::run_analyzer(usetesting(), &pkg);
    assert!(
        defaults.iter().any(|m| m.contains("os.Setenv()")),
        "os-setenv defaults to on: {defaults:?}"
    );
    assert!(
        !defaults.iter().any(|m| m.contains("os.TempDir()")),
        "os-temp-dir defaults to off: {defaults:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "usetesting",
        UsetestingOptions {
            os_setenv: true,
            os_temp_dir: true,
            ..UsetestingOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        usetesting(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("os.Setenv") && m.contains("t.Setenv")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("os.TempDir") && m.contains("t.TempDir")),
        "{messages:?}"
    );
}

#[test]
fn usetesting_respects_os_mkdir_temp_off() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::UsetestingOptions;

    let pkg = support::typecheck_fixture("usetesting", "example.com/usetesting", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "usetesting",
        UsetestingOptions {
            os_mkdir_temp: false,
            os_create_temp: false,
            ..UsetestingOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        usetesting(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "os-mkdir-temp/os-create-temp=false should suppress bad.go: {messages:?}"
    );
}

#[test]
fn usestdlibvars_respects_http_toggles_off() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::UsestdlibvarsOptions;

    let pkg = support::typecheck_fixture("usestdlibvars", "example.com/usestdlibvars", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "usestdlibvars",
        UsestdlibvarsOptions {
            http_method: false,
            http_status_code: false,
            ..UsestdlibvarsOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        usestdlibvars(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "http toggles off should suppress bad.go: {messages:?}"
    );
}

#[test]
fn usestdlibvars_optional_tables_default_off() {
    let pkg = support::typecheck_fixture(
        "usestdlibvars",
        "example.com/usestdlibvars/optional",
        "optional_bad.go",
    );
    let messages = support::run_analyzer(usestdlibvars(), &pkg);
    assert!(
        messages.is_empty(),
        "optional tables default off: {messages:?}"
    );
}

#[test]
fn usestdlibvars_optional_tables_when_enabled() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::UsestdlibvarsOptions;

    let pkg = support::typecheck_fixture(
        "usestdlibvars",
        "example.com/usestdlibvars/optional",
        "optional_bad.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "usestdlibvars",
        UsestdlibvarsOptions {
            http_method: false,
            http_status_code: false,
            time_weekday: true,
            time_month: true,
            time_layout: true,
            crypto_hash: true,
            default_rpc_path: true,
            sql_isolation_level: true,
            tls_signature_scheme: true,
            constant_kind: true,
            time_date_month: true,
        },
    );
    let messages = support::run_analyzer_with_settings(
        usestdlibvars(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    for needle in [
        "\"Monday\" can be replaced by time.Monday.String()",
        "\"January\" can be replaced by time.January.String()",
        "\"2006-01-02\" can be replaced by time.DateOnly",
        "\"SHA-256\" can be replaced by crypto.SHA256.String()",
        "\"/_goRPC_\" can be replaced by rpc.DefaultRPCPath",
        "\"Read Committed\" can be replaced by sql.LevelReadCommitted.String()",
        "\"PSSWithSHA256\" can be replaced by tls.PSSWithSHA256.String()",
        "\"Bool\" can be replaced by constant.Bool.String()",
        "\"1\" can be replaced by time.January",
    ] {
        assert!(
            messages.iter().any(|m| m.contains(needle)),
            "missing {needle} in {messages:?}"
        );
    }
}

#[test]
fn usestdlibvars_optional_ok_clean() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::UsestdlibvarsOptions;

    let pkg = support::typecheck_fixture(
        "usestdlibvars",
        "example.com/usestdlibvars/optional_ok",
        "optional_ok.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "usestdlibvars",
        UsestdlibvarsOptions {
            http_method: false,
            http_status_code: false,
            time_weekday: true,
            time_month: true,
            time_layout: true,
            crypto_hash: true,
            default_rpc_path: true,
            sql_isolation_level: true,
            tls_signature_scheme: true,
            constant_kind: true,
            time_date_month: true,
        },
    );
    let messages = support::run_analyzer_with_settings(
        usestdlibvars(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "stdlib constants should be clean: {messages:?}"
    );
}

#[test]
fn perfsprint_flags_fmt_shortcuts() {
    let pkg = support::typecheck_fixture("perfsprint", "example.com/perfsprint", "bad.go");
    let messages = support::run_analyzer(perfsprint(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("string-format") && m.contains("fmt.Sprintf")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("error-format") && m.contains("errors.New")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("bool-format") && m.contains("FormatBool")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("integer-format") && m.contains("Itoa")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("hex-format") && m.contains("EncodeToString")),
        "{messages:?}"
    );
}

#[test]
fn perfsprint_sprintf1_governs_the_one_argument_form() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::PerfsprintOptions;

    // Upstream carries the flag on the *arm* that recognizes a one-argument
    // `fmt.Sprintf` — `case calledObj == fmtSprintfObj && len(call.Args) == 1 &&
    // n.strFormat.sprintf1` — so with it off no arm matches and the call is
    // passed over. guff gated it at the report instead, and with an *or*
    // (`string_format || sprintf1`), so leaving `string-format` at its default
    // kept the finding alive however `sprintf1` was set. velero writes
    // `sprintf1: false` and got it anyway.
    let pkg = support::typecheck_fixture("perfsprint", "example.com/perfsprint/sprintf1", "bad.go");

    let one_arg = |messages: &[String]| -> usize {
        messages
            .iter()
            .filter(|m| m.contains("string-format") && m.contains("just using the string"))
            .count()
    };

    let on = support::run_analyzer(perfsprint(), &pkg);
    assert_eq!(one_arg(&on), 4, "{on:?}");

    let mut bag = SettingsBag::new();
    bag.insert(
        "perfsprint",
        PerfsprintOptions {
            sprintf1: false,
            ..PerfsprintOptions::default()
        },
    );
    let off = support::run_analyzer_with_settings(
        perfsprint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    // The two-argument forms (`fmt.Sprintf("%s", s)` and friends) keep their
    // findings — a different arm — and only `fmt.Sprintf("hello")` falls
    // silent.
    assert_eq!(one_arg(&off), 3, "{off:?}");
}

/// `hex-format` is two upstream cases, not one, and the fix differs between
/// them.
///
/// catenacyber/perfsprint has `case isArray && …` — which refuses anything but
/// an identifier ("Doesn't support array literals") and appends `[:]` — and
/// `case isSlice && …`, which takes any expression and appends nothing. guff
/// had a single "is this a byte sequence" predicate whose bool was read as "is
/// it an array", so a `[]byte` inherited the array rules in both directions:
/// `fmt.Sprintf("%x", hasher.Sum(nil))` went unreported (a call is not an
/// identifier), and a `[]byte` that *was* an identifier was rewritten to
/// `hex.EncodeToString(b[:])`. A non-byte element fell through the same
/// predicate as an array, so `%x` on a `[]int` was reported.
///
/// The `[:]` half is invisible to every finding-set comparison — the golden
/// key carries the message, not the replacement — so the edits are asserted
/// here. Measured against golangci-lint 2.12.2.
#[test]
fn perfsprint_hex_format_separates_the_slice_case_from_the_array_case() {
    let pkg = support::typecheck_fixture("perfsprint", "example.com/perfsprint/hex", "hex.go");
    let fset = pkg.fset.clone().expect("fixture has a FileSet");
    let mut got: Vec<(i64, bool)> = support::run_analyzer_diagnostics(perfsprint(), &pkg)
        .into_iter()
        .filter(|d| d.message.contains("hex.EncodeToString"))
        .map(|d| {
            let line = fset.position(guff::position::Pos(d.pos as i64)).line;
            let slices = d.suggested_fixes[0]
                .text_edits
                .iter()
                .any(|e| e.new_text == "[:]");
            (line, slices)
        })
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            // a call, a field and a composite literal are all fine for a slice…
            (37, false),
            (39, false),
            (41, false),
            (43, false),
            // …and only the array case appends `[:]`
            (45, true),
        ],
        "(line, fix appends `[:]`)"
    );
}

#[test]
fn perfsprint_allows_complex_fmt() {
    let pkg = support::typecheck_fixture("perfsprint", "example.com/perfsprint/ok", "ok.go");
    assert!(support::run_analyzer(perfsprint(), &pkg).is_empty());
}

#[test]
fn perfsprint_flags_concat_loop() {
    let pkg = support::typecheck_fixture(
        "perfsprint",
        "example.com/perfsprint/concat",
        "concat_loop_bad.go",
    );
    let messages = support::run_analyzer(perfsprint(), &pkg);
    let concat: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("concat-loop"))
        .collect();
    assert!(
        concat.len() >= 8,
        "expected several concat-loop diagnostics, got {}: {messages:?}",
        concat.len()
    );
    assert!(
        concat
            .iter()
            .all(|m| m.contains("string concatenation in a loop")),
        "{concat:?}"
    );
}

#[test]
fn perfsprint_concat_loop_allows_local_and_other_ops() {
    let pkg = support::typecheck_fixture(
        "perfsprint",
        "example.com/perfsprint/concat_ok",
        "concat_loop_ok.go",
    );
    let messages = support::run_analyzer(perfsprint(), &pkg);
    assert!(
        !messages.iter().any(|m| m.contains("concat-loop")),
        "default loop-other-ops=false should skip otherOps cases; locals should be ignored: {messages:?}"
    );
}

#[test]
fn perfsprint_concat_loop_respects_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::PerfsprintOptions;

    let bad = support::typecheck_fixture(
        "perfsprint",
        "example.com/perfsprint/concat_settings",
        "concat_loop_bad.go",
    );
    let ok = support::typecheck_fixture(
        "perfsprint",
        "example.com/perfsprint/concat_ok_settings",
        "concat_loop_ok.go",
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "perfsprint",
        PerfsprintOptions {
            concat_loop: false,
            ..PerfsprintOptions::default()
        },
    );
    let disabled = support::run_analyzer_with_settings(
        perfsprint(),
        &bad,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !disabled.iter().any(|m| m.contains("concat-loop")),
        "concat-loop=false should suppress: {disabled:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "perfsprint",
        PerfsprintOptions {
            loop_other_ops: true,
            ..PerfsprintOptions::default()
        },
    );
    let with_other = support::run_analyzer_with_settings(
        perfsprint(),
        &ok,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        with_other.iter().any(|m| m.contains("concat-loop")),
        "loop-other-ops=true should report otherOps concat loops: {with_other:?}"
    );
}

#[test]
fn goconst_flags_repeated_strings() {
    let pkg = support::typecheck_fixture("goconst", "example.com/goconst", "bad.go");
    let messages = support::run_analyzer(goconst(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("needconst") && m.contains("3 occurrences")),
        "{messages:?}"
    );
}

#[test]
fn goconst_flags_strings_in_call_composite_lit() {
    // Nested CompositeLit args must still count when ignore-calls is on
    // (golangci default). Regression from cobra OSS hunt.
    let pkg = support::typecheck_fixture(
        "goconst/call_composite",
        "example.com/goconst/call_composite",
        "bad.go",
    );
    let messages = support::run_analyzer(goconst(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("nested") && m.contains("3 occurrences")),
        "{messages:?}"
    );
}

#[test]
fn goconst_ignores_direct_call_string_args_by_default() {
    let pkg = support::typecheck_fixture(
        "goconst/call_direct",
        "example.com/goconst/call_direct",
        "ok.go",
    );
    assert!(
        support::run_analyzer(goconst(), &pkg).is_empty(),
        "direct call args should be ignored with ignore-calls default"
    );
}

#[test]
fn goconst_ignores_package_var_initializers() {
    // Upstream does not count `var x = "s"` toward occurrences.
    let pkg = support::typecheck_fixture(
        "goconst/var_init",
        "example.com/goconst/var_init",
        "ok.go",
    );
    let messages = support::run_analyzer(goconst(), &pkg);
    assert!(
        messages.is_empty(),
        "var initializer must not inflate count: {messages:?}"
    );
}

#[test]
fn goconst_filters_numeric_looking_strings_by_default_range() {
    // golangci defaults NumberMin=NumberMax=3; ProcessResults drops any
    // ParseInt-able string outside that range even when ParseNumbers is off.
    let pkg = support::typecheck_fixture(
        "goconst/numeric_str",
        "example.com/goconst/numeric_str",
        "ok.go",
    );
    let messages = support::run_analyzer(goconst(), &pkg);
    assert!(
        messages.is_empty(),
        "numeric-looking strings outside min/max must be filtered: {messages:?}"
    );
}

#[test]
fn goconst_allows_below_threshold() {
    let pkg = support::typecheck_fixture("goconst", "example.com/goconst/ok", "ok.go");
    assert!(support::run_analyzer(goconst(), &pkg).is_empty());
}

#[test]
fn goconst_flags_repeated_numbers() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GoconstOptions;

    let pkg =
        support::typecheck_fixture("goconst", "example.com/goconst/numbers", "numbers_bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "goconst",
        GoconstOptions {
            numbers: true,
            number_min: 0,
            number_max: 0,
            ..GoconstOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        goconst(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("`100`") && m.contains("3 occurrences")),
        "{messages:?}"
    );
}

#[test]
fn goconst_numbers_respect_range_and_threshold() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GoconstOptions;

    let pkg =
        support::typecheck_fixture("goconst", "example.com/goconst/numbers_ok", "numbers_ok.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "goconst",
        GoconstOptions {
            numbers: true,
            number_min: 0,
            number_max: 0,
            ..GoconstOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        goconst(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "numbers_ok.go should stay clean: {messages:?}"
    );
}

#[test]
fn goconst_match_constant_reports_existing_const() {
    let pkg = support::typecheck_fixture(
        "goconst",
        "example.com/goconst/match",
        "match_constant_bad.go",
    );
    let messages = support::run_analyzer(goconst(), &pkg);
    assert!(
        messages.iter().any(|m| {
            m.contains("repeated value")
                && m.contains("3 occurrences")
                && m.contains("ExistingConst")
        }),
        "{messages:?}"
    );
}

#[test]
fn goconst_match_constant_allows_below_threshold() {
    let pkg = support::typecheck_fixture(
        "goconst",
        "example.com/goconst/match_ok",
        "match_constant_ok.go",
    );
    assert!(support::run_analyzer(goconst(), &pkg).is_empty());
}

#[test]
fn goconst_find_duplicates_reports_duplicate_consts() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GoconstOptions;

    let pkg = support::typecheck_fixture(
        "goconst",
        "example.com/goconst/find_dup",
        "find_duplicates_bad.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "goconst",
        GoconstOptions {
            find_duplicates: true,
            match_constant: false,
            ..GoconstOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        goconst(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.iter().any(|m| {
            m.contains("This constant is a duplicate of `DuplicateConst1`")
                && m.contains("find_duplicates_bad.go")
        }),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| { m.contains("This constant is a duplicate of `GroupedDuplicateConst1`") }),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| { m.contains("This constant is a duplicate of `ScopedDuplicateConst1`") }),
        "{messages:?}"
    );
}

#[test]
fn goconst_find_duplicates_default_off() {
    let pkg = support::typecheck_fixture(
        "goconst",
        "example.com/goconst/find_dup_default",
        "find_duplicates_bad.go",
    );
    let messages = support::run_analyzer(goconst(), &pkg);
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("This constant is a duplicate")),
        "find-duplicates defaults to false: {messages:?}"
    );
}

#[test]
fn goconst_find_duplicates_allows_unique_consts() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GoconstOptions;

    let pkg = support::typecheck_fixture(
        "goconst",
        "example.com/goconst/find_dup_ok",
        "find_duplicates_ok.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "goconst",
        GoconstOptions {
            find_duplicates: true,
            match_constant: false,
            ..GoconstOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        goconst(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "find_duplicates_ok.go should stay clean: {messages:?}"
    );
}

#[test]
fn dogsled_flags_too_many_blanks() {
    let pkg = support::typecheck_fixture("dogsled", "example.com/dogsled", "bad.go");
    let messages = support::run_analyzer(dogsled(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("declaration has 3 blank identifiers")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("declaration has 4 blank identifiers")),
        "{messages:?}"
    );
}

#[test]
fn dogsled_allows_two_or_fewer_blanks() {
    let pkg = support::typecheck_fixture("dogsled", "example.com/dogsled/ok", "ok.go");
    assert!(support::run_analyzer(dogsled(), &pkg).is_empty());
}

#[test]
fn asciicheck_flags_non_ascii_idents() {
    let pkg = support::typecheck_fixture("asciicheck", "example.com/asciicheck", "bad.go");
    let messages = support::run_analyzer(asciicheck(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("TéstFunc") && m.contains("non-ASCII")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("téstConst") && m.contains("non-ASCII")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("téstParam") && m.contains("non-ASCII")),
        "{messages:?}"
    );
}

#[test]
fn asciicheck_allows_ascii_idents() {
    let pkg = support::typecheck_fixture("asciicheck", "example.com/asciicheck/ok", "ok.go");
    assert!(support::run_analyzer(asciicheck(), &pkg).is_empty());
}

#[test]
fn goprintffuncname_flags_missing_f_suffix() {
    let pkg =
        support::typecheck_fixture("goprintffuncname", "example.com/goprintffuncname", "bad.go");
    let messages = support::run_analyzer(goprintffuncname(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("prinfLikeFunc") && m.contains("prinfLikeFuncf")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("prinfLikeFuncAny") && m.contains("should be named")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("prinfLikeFuncWithExtraArgs")),
        "{messages:?}"
    );
}

#[test]
fn goprintffuncname_allows_correct_names() {
    let pkg = support::typecheck_fixture(
        "goprintffuncname",
        "example.com/goprintffuncname/ok",
        "ok.go",
    );
    assert!(support::run_analyzer(goprintffuncname(), &pkg).is_empty());
}

#[test]
fn funlen_flags_too_many_statements() {
    let pkg = support::typecheck_fixture("funlen", "example.com/funlen", "bad.go");
    let messages = support::run_analyzer(funlen(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("TooManyStatements") && m.contains("too many statements")),
        "{messages:?}"
    );
}

#[test]
fn funlen_allows_short_functions() {
    let pkg = support::typecheck_fixture("funlen", "example.com/funlen/ok", "ok.go");
    assert!(support::run_analyzer(funlen(), &pkg).is_empty());
}

#[test]
fn gocyclo_flags_high_complexity() {
    let pkg = support::typecheck_fixture("gocyclo", "example.com/gocyclo", "bad.go");
    let messages = support::run_analyzer(gocyclo(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("HighComplexity") && m.contains("cyclomatic complexity")),
        "{messages:?}"
    );
}

#[test]
fn maintidx_flags_low_maintainability() {
    let pkg = support::typecheck_fixture("maintidx", "example.com/maintidx", "bad.go");
    let messages = support::run_analyzer(maintidx(), &pkg);
    assert!(
        messages.iter().any(|m| {
            m.contains("Function name: under20")
                && m.contains("Maintainability Index:")
                && m.contains("Cyclomatic Complexity:")
        }),
        "{messages:?}"
    );
    let mi_msg = messages
        .iter()
        .find(|m| m.contains("under20"))
        .expect("under20 diagnostic");
    let mi: i32 = mi_msg
        .rsplit("Maintainability Index: ")
        .next()
        .and_then(|s| s.trim().parse().ok())
        .expect("parse MI");
    assert!(mi < 20, "expected MI < 20, got {mi} from {mi_msg}");
}

#[test]
fn maintidx_allows_simple_functions() {
    let pkg = support::typecheck_fixture("maintidx", "example.com/maintidx/ok", "ok.go");
    assert!(support::run_analyzer(maintidx(), &pkg).is_empty());
}

#[test]
fn maintidx_respects_under_setting() {
    use guff_style::MaintidxOptions;

    let pkg = support::typecheck_fixture("maintidx", "example.com/maintidx/settings", "settings.go");
    assert!(
        support::run_analyzer(maintidx(), &pkg).is_empty(),
        "default under=20 should allow medium()"
    );

    let mut bag = SettingsBag::new();
    bag.insert("maintidx", MaintidxOptions { under: 100 });
    let messages = support::run_analyzer_with_settings(
        maintidx(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Function name: medium") && m.contains("Maintainability Index:")),
        "{messages:?}"
    );
}

#[test]
fn gocyclo_allows_low_complexity() {
    let pkg = support::typecheck_fixture("gocyclo", "example.com/gocyclo/ok", "ok.go");
    assert!(support::run_analyzer(gocyclo(), &pkg).is_empty());
}

#[test]
fn lll_flags_long_lines() {
    let pkg = support::typecheck_fixture("lll", "example.com/lll", "bad.go");
    let messages = support::run_analyzer(lll(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("characters long") && m.contains("120")),
        "{messages:?}"
    );
}

#[test]
fn lll_allows_short_lines() {
    let pkg = support::typecheck_fixture("lll", "example.com/lll/ok", "ok.go");
    assert!(support::run_analyzer(lll(), &pkg).is_empty());
}

#[test]
fn gocognit_flags_high_cognitive_complexity() {
    let pkg = support::typecheck_fixture("gocognit", "example.com/gocognit", "bad.go");
    let messages = support::run_analyzer(gocognit(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("HighCognitive") && m.contains("cognitive complexity")),
        "{messages:?}"
    );
}

#[test]
fn gocognit_allows_low_cognitive_complexity() {
    let pkg = support::typecheck_fixture("gocognit", "example.com/gocognit/ok", "ok.go");
    assert!(support::run_analyzer(gocognit(), &pkg).is_empty());
}

#[test]
fn nestif_flags_deep_nesting() {
    let pkg = support::typecheck_fixture("nestif", "example.com/nestif", "bad.go");
    let messages = support::run_analyzer(nestif(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("complex nested blocks") && m.contains("if a")),
        "{messages:?}"
    );
}

#[test]
fn nestif_allows_shallow_nesting() {
    let pkg = support::typecheck_fixture("nestif", "example.com/nestif/ok", "ok.go");
    assert!(support::run_analyzer(nestif(), &pkg).is_empty());
}

#[test]
fn cyclop_flags_high_complexity() {
    let pkg = support::typecheck_fixture("cyclop", "example.com/cyclop", "bad.go");
    let messages = support::run_analyzer(cyclop(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("HighComplexity") && m.contains("cyclomatic complexity")),
        "{messages:?}"
    );
}

#[test]
fn cyclop_allows_low_complexity() {
    let pkg = support::typecheck_fixture("cyclop", "example.com/cyclop/ok", "ok.go");
    assert!(support::run_analyzer(cyclop(), &pkg).is_empty());
}

#[test]
fn nakedret_flags_long_naked_returns() {
    let pkg = support::typecheck_fixture("nakedret", "example.com/nakedret", "bad.go");
    let messages = support::run_analyzer(nakedret(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("naked return") && m.contains("LongNamed")),
        "{messages:?}"
    );
}

#[test]
fn nakedret_allows_short_or_explicit() {
    let pkg = support::typecheck_fixture("nakedret", "example.com/nakedret/ok", "ok.go");
    assert!(support::run_analyzer(nakedret(), &pkg).is_empty());
}

#[test]
fn nosprintfhostport_flags_host_port_sprintf() {
    let pkg = support::typecheck_fixture(
        "nosprintfhostport",
        "example.com/nosprintfhostport",
        "bad.go",
    );
    let messages = support::run_analyzer(nosprintfhostport(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("net.JoinHostPort") && m.contains("fmt.Sprintf")),
        "{messages:?}"
    );
    assert!(
        messages.len() >= 2,
        "expected both host:port and auth URL hits, got {messages:?}"
    );
}

#[test]
fn nosprintfhostport_allows_safe_sprintf() {
    let pkg = support::typecheck_fixture(
        "nosprintfhostport",
        "example.com/nosprintfhostport/ok",
        "ok.go",
    );
    assert!(support::run_analyzer(nosprintfhostport(), &pkg).is_empty());
}

#[test]
fn predeclared_flags_shadowed_identifiers() {
    let pkg = support::typecheck_fixture("predeclared", "example.com/predeclared", "bad.go");
    let messages = support::run_analyzer(predeclared(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("function len") && m.contains("predeclared")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("variable error") && m.contains("predeclared")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("variable true") && m.contains("predeclared")),
        "{messages:?}"
    );
}

#[test]
fn predeclared_allows_non_shadowing_names() {
    let pkg = support::typecheck_fixture("predeclared", "example.com/predeclared/ok", "ok.go");
    assert!(support::run_analyzer(predeclared(), &pkg).is_empty());
}

#[test]
fn whitespace_flags_leading_and_trailing() {
    let pkg = support::typecheck_fixture("whitespace", "example.com/whitespace", "bad.go");
    let messages = support::run_analyzer(whitespace(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("unnecessary leading newline")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("unnecessary trailing newline")),
        "{messages:?}"
    );
}

#[test]
fn whitespace_allows_tight_blocks() {
    let pkg = support::typecheck_fixture("whitespace", "example.com/whitespace/ok", "ok.go");
    assert!(support::run_analyzer(whitespace(), &pkg).is_empty());
}

#[test]
fn whitespace_allows_leading_comment_without_blank() {
    let pkg = support::typecheck_fixture(
        "whitespace",
        "example.com/whitespace/commentok",
        "comment_ok.go",
    );
    let messages = support::run_analyzer(whitespace(), &pkg);
    assert!(
        messages.is_empty(),
        "comment after {{ is not a leading blank: {messages:?}"
    );
}

#[test]
fn whitespace_multi_if_requires_leading_newline_when_enabled() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::WhitespaceOptions;

    let pkg = support::typecheck_fixture(
        "whitespace",
        "example.com/whitespace/multiif",
        "multi_if_bad.go",
    );
    assert!(
        support::run_analyzer(whitespace(), &pkg).is_empty(),
        "multi-if off should not flag multi_if_bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "whitespace",
        WhitespaceOptions {
            multi_if: true,
            ..WhitespaceOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        whitespace(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("multi-line statement should be followed by a newline")),
        "multi-if=true should flag multi_if_bad.go: {messages:?}"
    );

    let ok_pkg = support::typecheck_fixture(
        "whitespace",
        "example.com/whitespace/multiif/ok",
        "multi_if_ok.go",
    );
    let mut ok_bag = SettingsBag::new();
    ok_bag.insert(
        "whitespace",
        WhitespaceOptions {
            multi_if: true,
            ..WhitespaceOptions::default()
        },
    );
    let ok_messages = support::run_analyzer_with_settings(
        whitespace(),
        &ok_pkg,
        &RunnerOptions {
            settings: Arc::new(ok_bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        ok_messages.is_empty(),
        "multi-if=true should allow multi_if_ok.go: {ok_messages:?}"
    );
}

#[test]
fn whitespace_multi_func_requires_leading_newline_when_enabled() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::WhitespaceOptions;

    let pkg = support::typecheck_fixture(
        "whitespace",
        "example.com/whitespace/multifunc",
        "multi_func_bad.go",
    );
    assert!(
        support::run_analyzer(whitespace(), &pkg).is_empty(),
        "multi-func off should not flag multi_func_bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "whitespace",
        WhitespaceOptions {
            multi_func: true,
            ..WhitespaceOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        whitespace(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("multi-line statement should be followed by a newline")),
        "multi-func=true should flag multi_func_bad.go: {messages:?}"
    );

    let ok_pkg = support::typecheck_fixture(
        "whitespace",
        "example.com/whitespace/multifunc/ok",
        "multi_func_ok.go",
    );
    let mut ok_bag = SettingsBag::new();
    ok_bag.insert(
        "whitespace",
        WhitespaceOptions {
            multi_func: true,
            ..WhitespaceOptions::default()
        },
    );
    let ok_messages = support::run_analyzer_with_settings(
        whitespace(),
        &ok_pkg,
        &RunnerOptions {
            settings: Arc::new(ok_bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        ok_messages.is_empty(),
        "multi-func=true should allow multi_func_ok.go: {ok_messages:?}"
    );
}

#[test]
fn nlreturn_flags_missing_blank_before_return() {
    let pkg = support::typecheck_fixture("nlreturn", "example.com/nlreturn", "bad.go");
    let messages = support::run_analyzer(nlreturn(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("return with no blank line before")),
        "{messages:?}"
    );
}

#[test]
fn nlreturn_allows_alone_or_blanked_returns() {
    let pkg = support::typecheck_fixture("nlreturn", "example.com/nlreturn/ok", "ok.go");
    assert!(support::run_analyzer(nlreturn(), &pkg).is_empty());
}

#[test]
fn mnd_flags_magic_numbers() {
    let pkg = support::typecheck_fixture("mnd", "example.com/mnd", "bad.go");
    let messages = support::run_analyzer(mnd(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Magic number") && m.contains("<condition>")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Magic number") && m.contains("<argument>")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Magic number") && m.contains("<return>")),
        "{messages:?}"
    );
}

#[test]
fn mnd_allows_ignored_literals() {
    let pkg = support::typecheck_fixture("mnd", "example.com/mnd/ok", "ok.go");
    assert!(support::run_analyzer(mnd(), &pkg).is_empty());
}

#[test]
fn prealloc_flags_range_append() {
    let pkg = support::typecheck_fixture("prealloc", "example.com/prealloc", "bad.go");
    let messages = support::run_analyzer(prealloc(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Consider preallocating dest with capacity len(source)")),
        "{messages:?}"
    );
}

#[test]
fn prealloc_allows_make_capacity() {
    let pkg = support::typecheck_fixture("prealloc", "example.com/prealloc/ok", "ok.go");
    assert!(support::run_analyzer(prealloc(), &pkg).is_empty());
}

#[test]
fn tagalign_flags_misaligned_tags() {
    let pkg = support::typecheck_fixture("tagalign", "example.com/tagalign", "bad.go");
    let messages = support::run_analyzer(tagalign(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("tag is not aligned")),
        "{messages:?}"
    );
}

#[test]
fn tagalign_allows_aligned_sorted_tags() {
    let pkg = support::typecheck_fixture("tagalign", "example.com/tagalign/ok", "ok.go");
    assert!(support::run_analyzer(tagalign(), &pkg).is_empty());
}

#[test]
fn wsl_flags_cuddle_violations() {
    let pkg = support::typecheck_fixture("wsl", "example.com/wsl", "bad.go");
    let messages = support::run_analyzer(wsl(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("if statements should only be cuddled with assignments")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("used in the if statement itself")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("only one cuddle assignment allowed before if")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("declarations should never be cuddled")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("assignments should only be cuddled with other assignments")),
        "{messages:?}"
    );
}

#[test]
fn wsl_allows_proper_spacing() {
    let pkg = support::typecheck_fixture("wsl", "example.com/wsl/ok", "ok.go");
    let messages = support::run_analyzer(wsl(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn gocyclo_respects_min_complexity_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GocycloOptions;

    let pkg = support::typecheck_fixture("gocyclo", "example.com/gocyclo", "bad.go");
    assert!(
        !support::run_analyzer(gocyclo(), &pkg).is_empty(),
        "default min-complexity=30 should flag bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert("gocyclo", GocycloOptions { min_complexity: 50 });
    let messages = support::run_analyzer_with_settings(
        gocyclo(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "min-complexity=50 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn dogsled_respects_max_blank_identifiers_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::DogsledOptions;

    let pkg = support::typecheck_fixture("dogsled", "example.com/dogsled", "bad.go");
    assert!(
        !support::run_analyzer(dogsled(), &pkg).is_empty(),
        "default max-blank-identifiers=2 should flag bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "dogsled",
        DogsledOptions {
            max_blank_identifiers: 4,
        },
    );
    let messages = support::run_analyzer_with_settings(
        dogsled(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "max-blank-identifiers=4 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn funlen_ignore_comments_subtracts_comment_lines() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::FunlenOptions;

    // The production typecheck parses without comments, so this only passes
    // if funlen re-parses the file to see them.
    let pkg = support::typecheck_fixture("funlen", "example.com/funlen/comments", "comments.go");

    let run = |opts: FunlenOptions| {
        let mut bag = SettingsBag::new();
        bag.insert("funlen", opts);
        support::run_analyzer_with_settings(
            funlen(),
            &pkg,
            &RunnerOptions {
                settings: Arc::new(bag),
                ..RunnerOptions::default()
            },
        )
    };

    // 36 body lines, 12 of them comments → 24 with ignore-comments.
    let counted = run(FunlenOptions {
        lines: 30,
        statements: 200,
        ignore_comments: false,
    });
    assert!(
        counted.iter().any(|m| m.contains("(36 > 30)")),
        "without ignore-comments all 36 lines count: {counted:?}"
    );

    let ignored = run(FunlenOptions {
        lines: 30,
        statements: 200,
        ignore_comments: true,
    });
    assert!(
        ignored.is_empty(),
        "ignore-comments should drop the 12 comment lines to 24: {ignored:?}"
    );

    let ignored_tight = run(FunlenOptions {
        lines: 20,
        statements: 200,
        ignore_comments: true,
    });
    assert!(
        ignored_tight.iter().any(|m| m.contains("(24 > 20)")),
        "expected the comment-subtracted count: {ignored_tight:?}"
    );
}

#[test]
fn funlen_ignore_comments_works_in_a_file_that_has_a_package_doc() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::FunlenOptions;

    // The re-parse used to be guarded on `file.comments.is_empty()`, on the
    // theory that a non-empty list means the file was parsed with comments. It
    // does not: the production typecheck keeps the package doc comment and
    // drops the rest, so every file with a doc comment — which is most of
    // them — took the fast path and subtracted nothing.
    //
    // Measured on k6 `internal/log/cloud/cloud.go`: `file_comments=1`, no
    // re-parse, `Listen` reported at 105 lines where upstream counts 105 - 28.
    // The same file without a doc comment passed, which is why the existing
    // fixture never caught it.
    let pkg = support::typecheck_fixture("funlen", "example.com/funlen/docfile", "docfile.go");

    let run = |opts: FunlenOptions| {
        let mut bag = SettingsBag::new();
        bag.insert("funlen", opts);
        support::run_analyzer_with_settings(
            funlen(),
            &pkg,
            &RunnerOptions {
                settings: Arc::new(bag),
                ..RunnerOptions::default()
            },
        )
    };

    // 36 body lines, 12 of them comments → 24.
    let counted = run(FunlenOptions {
        lines: 30,
        statements: 200,
        ignore_comments: false,
    });
    assert!(
        counted.iter().any(|m| m.contains("(36 > 30)")),
        "without ignore-comments all 36 lines count: {counted:?}"
    );

    let ignored = run(FunlenOptions {
        lines: 30,
        statements: 200,
        ignore_comments: true,
    });
    assert!(
        ignored.is_empty(),
        "the doc comment must not stop the re-parse: {ignored:?}"
    );

    let ignored_tight = run(FunlenOptions {
        lines: 20,
        statements: 200,
        ignore_comments: true,
    });
    assert!(
        ignored_tight.iter().any(|m| m.contains("(24 > 20)")),
        "expected the comment-subtracted count: {ignored_tight:?}"
    );
}

#[test]
fn funlen_respects_statements_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::FunlenOptions;

    let pkg = support::typecheck_fixture("funlen", "example.com/funlen", "bad.go");
    assert!(
        !support::run_analyzer(funlen(), &pkg).is_empty(),
        "default statements=40 should flag bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "funlen",
        FunlenOptions {
            statements: 50,
            ..FunlenOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        funlen(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "statements=50 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn cyclop_respects_max_complexity_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::CyclopOptions;

    let pkg = support::typecheck_fixture("cyclop", "example.com/cyclop", "bad.go");
    assert!(
        !support::run_analyzer(cyclop(), &pkg).is_empty(),
        "default max-complexity=10 should flag bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "cyclop",
        CyclopOptions {
            max_complexity: 20,
            ..CyclopOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        cyclop(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "max-complexity=20 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn lll_respects_line_length_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::LllOptions;

    let pkg = support::typecheck_fixture("lll", "example.com/lll", "bad.go");
    assert!(
        !support::run_analyzer(lll(), &pkg).is_empty(),
        "default line-length=120 should flag bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "lll",
        LllOptions {
            line_length: 200,
            ..LllOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        lll(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "line-length=200 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn nakedret_respects_max_func_lines_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::NakedretOptions;

    let pkg = support::typecheck_fixture("nakedret", "example.com/nakedret", "bad.go");
    assert!(
        !support::run_analyzer(nakedret(), &pkg).is_empty(),
        "default max-func-lines=30 should flag bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "nakedret",
        NakedretOptions {
            max_func_lines: 50,
            ..NakedretOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        nakedret(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "max-func-lines=50 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn nlreturn_respects_block_size_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::NlreturnOptions;

    let pkg = support::typecheck_fixture("nlreturn", "example.com/nlreturn", "bad.go");
    assert!(
        !support::run_analyzer(nlreturn(), &pkg).is_empty(),
        "default block-size=1 should flag bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert("nlreturn", NlreturnOptions { block_size: 10 });
    let messages = support::run_analyzer_with_settings(
        nlreturn(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "block-size=10 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn cyclop_respects_package_average_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::CyclopOptions;

    let pkg = support::typecheck_fixture("cyclop", "example.com/cyclop/pkgavg", "pkgavg_bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "cyclop",
        CyclopOptions {
            max_complexity: 20,
            package_average: 5.0,
            ..CyclopOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        cyclop(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("average complexity for the package")),
        "package-average=5 should flag pkgavg_bad.go: {messages:?}"
    );
}

#[test]
fn nakedret_skips_test_files_when_configured() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::NakedretOptions;

    let pkg = support::typecheck_with_deps(
        "example.com/nakedret/test",
        &support::testdata("nakedret/bad_test.go"),
        &[],
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "nakedret",
        NakedretOptions {
            skip_test_files: true,
            ..NakedretOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        nakedret(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "skip-test-files should ignore bad_test.go: {messages:?}"
    );
}

#[test]
fn perfsprint_respects_disabled_integer_format() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::PerfsprintOptions;

    let pkg = support::typecheck_fixture("perfsprint", "example.com/perfsprint", "bad.go");
    assert!(
        support::run_analyzer(perfsprint(), &pkg)
            .iter()
            .any(|m| m.contains("integer-format")),
        "default should flag integer-format"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "perfsprint",
        PerfsprintOptions {
            integer_format: false,
            bool_format: false,
            hex_format: false,
            ..PerfsprintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        perfsprint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !messages.iter().any(|m| m.contains("integer-format")),
        "integer-format=false should suppress integer diagnostics: {messages:?}"
    );
}

#[test]
fn perfsprint_err_error_off_by_default() {
    let pkg = support::typecheck_fixture(
        "perfsprint",
        "example.com/perfsprint/err_error",
        "err_error.go",
    );
    let messages = support::run_analyzer(perfsprint(), &pkg);
    assert!(
        !messages.iter().any(|m| m.contains(".Error()")),
        "err-error defaults to false: {messages:?}"
    );
}

#[test]
fn perfsprint_err_error_when_enabled() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::PerfsprintOptions;

    let pkg = support::typecheck_fixture(
        "perfsprint",
        "example.com/perfsprint/err_error_on",
        "err_error.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "perfsprint",
        PerfsprintOptions {
            err_error: true,
            ..PerfsprintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        perfsprint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("error-format") && m.contains("err.Error()")),
        "err-error=true should suggest err.Error(): {messages:?}"
    );
    assert_eq!(
        messages.iter().filter(|m| m.contains(".Error()")).count(),
        3,
        "expected Sprint/Sprintf %s/%v: {messages:?}"
    );
}

#[test]
fn perfsprint_int_conversion_when_disabled() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::PerfsprintOptions;

    let pkg = support::typecheck_fixture(
        "perfsprint",
        "example.com/perfsprint/int_conv",
        "int_conversion.go",
    );

    // Default (int-conversion=true): cast-requiring and non-cast types.
    let default_msgs = support::run_analyzer(perfsprint(), &pkg);
    assert!(
        default_msgs.iter().any(|m| m.contains("Itoa")),
        "{default_msgs:?}"
    );
    assert!(
        default_msgs.iter().any(|m| m.contains("FormatUint")),
        "{default_msgs:?}"
    );
    assert_eq!(
        default_msgs
            .iter()
            .filter(|m| m.contains("integer-format"))
            .count(),
        5,
        "int/int8/int64/uint/uint64: {default_msgs:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "perfsprint",
        PerfsprintOptions {
            int_conversion: false,
            ..PerfsprintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        perfsprint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    // int and int64/uint64 need no cast — still flagged.
    assert!(
        messages.iter().any(|m| m.contains("strconv.Itoa")),
        "plain int should still flag: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("FormatInt")),
        "int64 should still flag: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("FormatUint") && !m.contains("uint64(")),
        "uint64 should still flag without cast: {messages:?}"
    );
    // int8 / uint require casts — suppressed.
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("integer-format"))
            .count(),
        3,
        "int-conversion=false should keep only int/int64/uint64: {messages:?}"
    );
}

#[test]
fn goconst_respects_min_occurrences_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GoconstOptions;

    let pkg = support::typecheck_fixture("goconst", "example.com/goconst", "bad.go");
    assert!(!support::run_analyzer(goconst(), &pkg).is_empty());

    let mut bag = SettingsBag::new();
    bag.insert(
        "goconst",
        GoconstOptions {
            min_occurrences: 10,
            ..GoconstOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        goconst(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "min-occurrences=10 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn predeclared_respects_ignore_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::PredeclaredOptions;

    let pkg = support::typecheck_fixture("predeclared", "example.com/predeclared", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "predeclared",
        PredeclaredOptions {
            ignore: vec!["len".into(), "error".into(), "true".into()],
            ..PredeclaredOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        predeclared(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "ignore list should suppress bad.go: {messages:?}"
    );
}

#[test]
fn mnd_respects_disabled_argument_check() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::MndOptions;

    let pkg = support::typecheck_fixture("mnd", "example.com/mnd", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "mnd",
        MndOptions {
            checks: vec!["case".into(), "condition".into()],
            ..MndOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        mnd(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !messages.iter().any(|m| m.contains("<argument>")),
        "disabled argument check should suppress call args: {messages:?}"
    );
}

#[test]
fn prealloc_for_loops_computes_trip_counts() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::PreallocOptions;

    let pkg =
        support::typecheck_fixture("prealloc", "example.com/prealloc/forloops", "forloops.go");
    // `for-loops` is off by default, so nothing here is reported.
    assert!(
        support::run_analyzer(prealloc(), &pkg).is_empty(),
        "three-clause loops must be silent with for-loops off"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "prealloc",
        PreallocOptions {
            for_loops: true,
            ..PreallocOptions::default()
        },
    );
    let mut messages = support::run_analyzer_with_settings(
        prealloc(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    messages.sort();
    // Verbatim golangci-lint 2.12 output for this file with `for-loops: true`.
    let mut want = vec![
        "Consider preallocating out with capacity n",
        "Consider preallocating out with capacity n + 1",
        "Consider preallocating out with capacity n",
        "Consider preallocating out with capacity n/2 + 1",
        "Consider preallocating out with capacity n/k + 1",
        "Consider preallocating out with capacity min(a, b, c)",
        "Consider preallocating out with capacity max(n, m)",
        "Consider preallocating out with capacity n",
        "Consider preallocating out with capacity 3 * n",
        "Consider preallocating out",
    ];
    want.sort();
    assert_eq!(messages, want);
}

#[test]
fn prealloc_respects_range_loops_off() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::PreallocOptions;

    let pkg = support::typecheck_fixture("prealloc", "example.com/prealloc", "bad.go");
    assert!(!support::run_analyzer(prealloc(), &pkg).is_empty());

    let mut bag = SettingsBag::new();
    bag.insert(
        "prealloc",
        PreallocOptions {
            range_loops: false,
            ..PreallocOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        prealloc(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "range-loops=false should suppress bad.go: {messages:?}"
    );
}

#[test]
fn tagalign_respects_align_off() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::TagalignOptions;

    let pkg = support::typecheck_fixture("tagalign", "example.com/tagalign", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "tagalign",
        TagalignOptions {
            align: false,
            sort: false,
            ..TagalignOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        tagalign(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "align=false sort=false should suppress bad.go: {messages:?}"
    );
}

#[test]
fn wsl_respects_allow_assign_and_anything() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::WslOptions;

    let pkg = support::typecheck_fixture("wsl", "example.com/wsl", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "wsl",
        WslOptions {
            allow_assign_and_anything: true,
            ..WslOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        wsl(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("assignments should only be cuddled")),
        "allow-assign-and-anything should suppress assign cuddling: {messages:?}"
    );
}

#[test]
fn unconvert_flags_identity_conversions() {
    let pkg = support::typecheck_fixture("unconvert", "example.com/unconvert", "bad.go");
    let messages = support::run_analyzer(unconvert(), &pkg);
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("unnecessary conversion"))
            .count()
            >= 2,
        "expected identity conversions on int and ID: {messages:?}"
    );
}

#[test]
fn unconvert_allows_real_conversions() {
    let pkg = support::typecheck_fixture("unconvert", "example.com/unconvert/ok", "ok.go");
    assert!(support::run_analyzer(unconvert(), &pkg).is_empty());
}

#[test]
fn unconvert_skips_float_by_default() {
    let pkg = support::typecheck_fixture("unconvert", "example.com/unconvert/fast", "fast_math.go");
    assert!(
        support::run_analyzer(unconvert(), &pkg).is_empty(),
        "float/complex identity conversions must stay when fast-math is off"
    );
}

#[test]
fn unconvert_fast_math_flags_float() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::UnconvertOptions;

    let pkg = support::typecheck_fixture("unconvert", "example.com/unconvert/fast", "fast_math.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "unconvert",
        UnconvertOptions {
            fast_math: true,
            ..UnconvertOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        unconvert(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("unnecessary conversion"))
            .count()
            >= 2,
        "fast-math should flag float/complex identity conversions: {messages:?}"
    );
}

#[test]
fn exhaustruct_flags_missing_fields() {
    let pkg = support::typecheck_fixture("exhaustruct", "example.com/exhaustruct", "bad.go");
    let messages = support::run_analyzer(exhaustruct(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("missing field Y")),
        "expected missing Y: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("missing fields") && m.contains("X") && m.contains("Y")),
        "expected missing X, Y on empty lit: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("<anonymous>") && m.contains("missing field B")),
        "expected anonymous missing B: {messages:?}"
    );
}

#[test]
fn exhaustruct_allows_complete_and_optional() {
    let pkg = support::typecheck_fixture("exhaustruct", "example.com/exhaustruct/ok", "ok.go");
    let messages = support::run_analyzer(exhaustruct(), &pkg);
    assert!(
        messages.is_empty(),
        "expected no diagnostics (optional Z + error return): {messages:?}"
    );
}

#[test]
fn exhaustruct_include_filters_types() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::ExhaustructOptions;

    let pkg = support::typecheck_fixture(
        "exhaustruct",
        "example.com/exhaustruct/include",
        "include.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "exhaustruct",
        ExhaustructOptions {
            include: vec![r".*\.Included$".into()],
            ..ExhaustructOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        exhaustruct(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Included") && m.contains("missing")),
        "include should flag Included: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("Other")),
        "Other must be skipped by include filter: {messages:?}"
    );
}

#[test]
fn exhaustruct_allow_empty_declarations() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::ExhaustructOptions;

    let pkg = support::typecheck_fixture(
        "exhaustruct",
        "example.com/exhaustruct/emptydecl",
        "empty_decl.go",
    );
    assert!(
        !support::run_analyzer(exhaustruct(), &pkg).is_empty(),
        "empty decls must be flagged by default"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "exhaustruct",
        ExhaustructOptions {
            allow_empty_declarations: true,
            ..ExhaustructOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        exhaustruct(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "allow-empty-declarations should silence var/:= empties: {messages:?}"
    );
}

#[test]
fn exhaustive_flags_missing_cases() {
    let pkg = support::typecheck_fixture("exhaustive", "example.com/exhaustive", "bad.go");
    let messages = support::run_analyzer(exhaustive(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("missing cases") && m.contains("C")),
        "expected missing C: {messages:?}"
    );
}

#[test]
fn exhaustive_allows_complete_switch() {
    let pkg = support::typecheck_fixture("exhaustive", "example.com/exhaustive/ok", "ok.go");
    let messages = support::run_analyzer(exhaustive(), &pkg);
    assert!(
        messages.is_empty(),
        "expected no diagnostics for complete switches: {messages:?}"
    );
}

#[test]
fn exhaustive_default_signifies_exhaustive() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::ExhaustiveOptions;

    let pkg =
        support::typecheck_fixture("exhaustive", "example.com/exhaustive/def", "default_ok.go");
    // Default off: missing Green/Blue.
    let messages = support::run_analyzer(exhaustive(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("missing cases")),
        "default alone should not satisfy exhaustiveness: {messages:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "exhaustive",
        ExhaustiveOptions {
            default_signifies_exhaustive: true,
            ..ExhaustiveOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        exhaustive(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "default-signifies-exhaustive should silence: {messages:?}"
    );
}

#[test]
fn exhaustive_checks_map_literals_when_enabled() {
    use guff_style::ExhaustiveOptions;

    let pkg = support::typecheck_fixture("exhaustive", "example.com/exhaustive/map", "map.go");

    // Map checks are opt-in, matching upstream and golangci-lint defaults.
    let defaults = support::run_analyzer(exhaustive(), &pkg);
    assert!(
        defaults.is_empty(),
        "default settings should not check maps: {defaults:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "exhaustive",
        ExhaustiveOptions {
            check_switch: false,
            check_map: true,
            ..ExhaustiveOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        exhaustive(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(
        messages[0].contains("missing keys in map of key type exhaustive.Direction")
            && messages[0].contains("exhaustive.South")
            && messages[0].contains("exhaustive.West"),
        "{messages:?}"
    );
}

#[test]
fn musttag_flags_missing_json_tags() {
    let pkg = support::typecheck_fixture("musttag", "example.com/musttag", "bad.go");
    let messages = support::run_analyzer(musttag(), &pkg);
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("annotated with the `json` tag"))
            .count()
            >= 2,
        "expected Marshal + Unmarshal diagnostics: {messages:?}"
    );
}

#[test]
fn musttag_allows_tagged_structs() {
    let pkg = support::typecheck_fixture("musttag", "example.com/musttag/ok", "ok.go");
    let messages = support::run_analyzer(musttag(), &pkg);
    assert!(
        messages.is_empty(),
        "expected no diagnostics for tagged structs: {messages:?}"
    );
}

#[test]
fn musttag_custom_functions_from_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::{MusttagFunc, MusttagOptions};

    let pkg = support::typecheck_fixture("musttag", "example.com/musttag", "custom.go");
    assert!(
        support::run_analyzer(musttag(), &pkg).is_empty(),
        "custom DecodeYAML is not a builtin"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "musttag",
        MusttagOptions {
            functions: vec![MusttagFunc {
                name: "example.com/musttag.DecodeYAML".into(),
                tag: "yaml".into(),
                arg_pos: 1,
            }],
        },
    );
    let messages = support::run_analyzer_with_settings(
        musttag(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.iter().any(|m| m.contains("`yaml` tag")),
        "custom function should require yaml tags: {messages:?}"
    );
}

#[test]
fn loggercheck_flags_odd_kv_pairs() {
    let pkg = support::typecheck_fixture("loggercheck", "example.com/loggercheck", "bad.go");
    let messages = support::run_analyzer(loggercheck(), &pkg);
    let odd = messages
        .iter()
        .filter(|m| m.contains("odd number of arguments"))
        .count();
    assert!(
        odd >= 5,
        "expected multiple odd-kv diagnostics, got {odd}: {messages:?}"
    );
}

#[test]
fn loggercheck_allows_even_kv_pairs() {
    let pkg = support::typecheck_fixture("loggercheck", "example.com/loggercheck/ok", "ok.go");
    let messages = support::run_analyzer(loggercheck(), &pkg);
    assert!(
        messages.is_empty(),
        "expected no diagnostics for even kv pairs: {messages:?}"
    );
}

#[test]
fn loggercheck_custom_rules_from_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::LoggercheckOptions;

    let pkg = support::typecheck_fixture("loggercheck", "example.com/loggercheck", "custom.go");
    assert!(
        support::run_analyzer(loggercheck(), &pkg).is_empty(),
        "MyLog is not a builtin logger"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "loggercheck",
        LoggercheckOptions {
            rules: vec!["example.com/loggercheck.MyLog".into()],
            ..LoggercheckOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        loggercheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("odd number of arguments"))
            .count()
            >= 2,
        "custom rule should flag odd kv: {messages:?}"
    );
}

#[test]
fn loggercheck_disable_slog_skips_diagnostics() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::LoggercheckOptions;

    let pkg = support::typecheck_fixture("loggercheck", "example.com/loggercheck", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "loggercheck",
        LoggercheckOptions {
            slog: false,
            ..LoggercheckOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        loggercheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "slog=false should skip slog calls: {messages:?}"
    );
}

#[test]
fn loggercheck_require_string_key_and_noprintflike() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::LoggercheckOptions;

    let pkg = support::typecheck_fixture("loggercheck", "example.com/loggercheck", "settings.go");
    assert!(
        support::run_analyzer(loggercheck(), &pkg).is_empty(),
        "defaults should not flag requirestringkey/noprintflike"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "loggercheck",
        LoggercheckOptions {
            require_string_key: true,
            no_printf_like: true,
            ..LoggercheckOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        loggercheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("inlined constant strings")),
        "require-string-key: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("format specifier")),
        "no-printf-like: {messages:?}"
    );
}

#[test]
fn sloglint_flags_mixed_args_by_default() {
    let pkg = support::typecheck_fixture("sloglint", "example.com/sloglint", "bad.go");
    let messages = support::run_analyzer(sloglint(), &pkg);
    let mixed = messages
        .iter()
        .filter(|m| m.contains("should not be mixed"))
        .count();
    assert!(
        mixed >= 3,
        "expected mixed-args diagnostics, got {mixed}: {messages:?}"
    );
}

#[test]
fn sloglint_allows_pure_kv_or_attrs() {
    let pkg = support::typecheck_fixture("sloglint", "example.com/sloglint/ok", "ok.go");
    let messages = support::run_analyzer(sloglint(), &pkg);
    assert!(
        messages.is_empty(),
        "expected no diagnostics for clean slog usage: {messages:?}"
    );
}

#[test]
fn sloglint_settings_static_msg_forbidden_keys_and_attr_only() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::SloglintOptions;

    let pkg = support::typecheck_fixture("sloglint", "example.com/sloglint", "settings.go");
    assert!(
        support::run_analyzer(sloglint(), &pkg).is_empty(),
        "defaults should only enforce no-mixed-args"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "sloglint",
        SloglintOptions {
            no_mixed_args: false,
            attr_only: true,
            static_msg: true,
            msg_style: Some("lowercased".into()),
            no_global: Some("default".into()),
            forbidden_keys: vec!["time".into(), "level".into()],
            no_raw_keys: true,
            allowed_keys: vec!["user_id".into()],
            ..SloglintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        sloglint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("default logger should not be used")),
        "no-global: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("string literal or a constant")),
        "static-msg: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("message should be lowercased")),
        "msg-style: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("forbidden") && m.contains("time")),
        "forbidden-keys: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("key-value pairs should not be used")),
        "attr-only: {messages:?}"
    );
}

#[test]
fn testifylint_flags_common_anti_patterns() {
    let pkg = support::typecheck_fixture("testifylint", "example.com/testifylint", "bad.go");
    let messages = support::run_analyzer(testifylint(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("bool-compare")),
        "bool-compare: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("compares")),
        "compares: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("empty")),
        "empty: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("error-nil")),
        "error-nil: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("nil-compare")),
        "nil-compare: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("len")),
        "len: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("float-compare")),
        "float-compare: {messages:?}"
    );
    // `zero` must stay *off*: golangci-lint 2.12 vendors testifylint v1.6.4,
    // which does not ship that checker, so `IMPLEMENTED` deliberately omits it
    // (see the comment there). `bad.go:50` has `assert.True(t, ts.IsZero())`,
    // which `check_zero` would flag, so this asserts the gate rather than the
    // absence of a test case — enabling `zero` by default would show up here
    // instead of as `guff_only` findings against golangci-lint.
    assert!(
        !messages.iter().any(|m| m.starts_with("zero:")),
        "zero must stay disabled to match golangci-lint: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("negative-positive")),
        "negative-positive: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("useless-assert")),
        "useless-assert: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("contains")),
        "contains: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("equal-values")),
        "equal-values: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("regexp")),
        "regexp: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("error-is-as")),
        "error-is-as: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("encoded-compare")),
        "encoded-compare: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("expected-actual")),
        "expected-actual: {messages:?}"
    );
    // time-compare / zero are omitted under defaults to match golangci 2.12
    // (vendors testifylint v1.6.4 which does not ship them).
    assert!(
        !messages.iter().any(|m| m.starts_with("time-compare:")),
        "time-compare should stay disabled by default: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("formatter")),
        "formatter: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("suite-extra-assert-call")),
        "suite-extra-assert-call: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("suite-dont-use-pkg")),
        "suite-dont-use-pkg: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("suite-subtest-run")),
        "suite-subtest-run: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("suite-method-signature")),
        "suite-method-signature: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("suite-broken-parallel")),
        "suite-broken-parallel: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("require-error")),
        "require-error: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("go-require") && m.contains("require must only")),
        "go-require goroutine: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("go-require") && m.contains("FailNow")),
        "go-require FailNow: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("go-require") && m.contains("helperWithRequire")),
        "go-require nested helper: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("go-require") && m.contains("http handlers")),
        "go-require http handler: {messages:?}"
    );
    assert!(
        messages.iter().all(|m| !m.contains("suite-thelper")),
        "suite-thelper should be off by default: {messages:?}"
    );
}

#[test]
fn testifylint_allows_idiomatic_assertions() {
    let pkg = support::typecheck_fixture("testifylint", "example.com/testifylint/ok", "ok.go");
    let messages = support::run_analyzer(testifylint(), &pkg);
    assert!(
        messages.is_empty(),
        "expected no diagnostics for idiomatic testify usage: {messages:?}"
    );
}

#[test]
fn testifylint_flags_blank_imports() {
    let pkg =
        support::typecheck_fixture("testifylint", "example.com/testifylint/blank", "blank.go");
    let messages = support::run_analyzer(testifylint(), &pkg);
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("blank-import"))
            .count()
            >= 2,
        "blank-import: {messages:?}"
    );
}

/// `mock-expect` is off unless a config names it.
///
/// testifylint v1.6.4 — what golangci-lint 2.12.2 vendors — has no such
/// checker: its registry stops at `useless-assert`. jaeger v2.20.0, which
/// golangci-lint reports as entirely clean, came back with 354 of them.
#[test]
fn testifylint_mock_expect_is_off_by_default() {
    let pkg = support::typecheck_fixture(
        "testifylint",
        "example.com/testifylint/mockexpect",
        "mock_expect.go",
    );
    let messages = support::run_analyzer(testifylint(), &pkg);
    assert!(
        !messages.iter().any(|m| m.contains("mock-expect")),
        "the pinned testifylint has no mock-expect checker: {messages:?}"
    );
}

#[test]
fn testifylint_flags_mock_expect() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::TestifylintOptions;

    let pkg = support::typecheck_fixture(
        "testifylint",
        "example.com/testifylint/mockexpect",
        "mock_expect.go",
    );
    // Ahead of the pin: reachable only when named. See `AHEAD_OF_PIN`.
    let mut bag = SettingsBag::new();
    bag.insert(
        "testifylint",
        TestifylintOptions {
            disable_all: true,
            enable: vec!["mock-expect".into()],
            ..TestifylintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        testifylint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    let mock_msgs: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("mock-expect"))
        .collect();
    assert!(
        mock_msgs
            .iter()
            .any(|m| m.contains("u.EXPECT().CreateUser")),
        "CreateUser: {mock_msgs:?}"
    );
    assert!(
        mock_msgs.iter().any(|m| m.contains("u.EXPECT().Void")),
        "Void: {mock_msgs:?}"
    );
    assert!(
        mock_msgs
            .iter()
            .any(|m| m.contains("u.EXPECT().CountUsers")),
        "CountUsers: {mock_msgs:?}"
    );
    assert!(
        mock_msgs.iter().any(|m| m.contains("u.EXPECT().Variadic")),
        "Variadic: {mock_msgs:?}"
    );
    assert!(
        mock_msgs
            .iter()
            .any(|m| m.contains("holder.user.EXPECT().Void")),
        "holder.user: {mock_msgs:?}"
    );
    assert!(
        mock_msgs
            .iter()
            .any(|m| m.contains("mockFrom(u).EXPECT().Void")),
        "mockFrom: {mock_msgs:?}"
    );
    // Ignored cases must not report.
    assert!(
        mock_msgs.iter().all(|m| !m.contains("DoesNotExist")),
        "ignored DoesNotExist: {mock_msgs:?}"
    );
    assert!(
        mock_msgs.len() >= 10,
        "expected many mock-expect hits, got {}: {mock_msgs:?}",
        mock_msgs.len()
    );
}

#[test]
fn testifylint_disable_all_then_enable_subset() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::TestifylintOptions;

    let pkg = support::typecheck_fixture("testifylint", "example.com/testifylint", "settings.go");
    let all = support::run_analyzer(testifylint(), &pkg);
    assert!(
        all.iter().any(|m| m.contains("bool-compare")),
        "defaults should flag bool-compare: {all:?}"
    );
    assert!(
        all.iter().any(|m| m.contains("empty")),
        "defaults should flag empty: {all:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "testifylint",
        TestifylintOptions {
            disable_all: true,
            enable: vec!["bool-compare".into()],
            ..TestifylintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        testifylint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.iter().any(|m| m.contains("bool-compare")),
        "enabled bool-compare: {messages:?}"
    );
    assert!(
        messages.iter().all(|m| !m.contains("empty:")),
        "empty should be disabled: {messages:?}"
    );
}

#[test]
fn testifylint_suite_thelper_when_enabled() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::TestifylintOptions;

    let pkg = support::typecheck_fixture("testifylint", "example.com/testifylint", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "testifylint",
        TestifylintOptions {
            disable_all: true,
            enable: vec!["suite-thelper".into()],
            ..TestifylintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        testifylint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("suite-thelper") && m.contains("s.T().Helper()")),
        "suite-thelper: {messages:?}"
    );
}

#[test]
fn testifylint_require_error_fn_pattern() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::TestifylintOptions;

    let pkg = support::typecheck_fixture("testifylint", "example.com/testifylint", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "testifylint",
        TestifylintOptions {
            disable_all: true,
            enable: vec!["require-error".into()],
            require_error_fn_pattern: Some("^NoError$".into()),
            ..TestifylintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        testifylint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.iter().any(|m| m.contains("require-error")),
        "fn-pattern NoError should still flag: {messages:?}"
    );

    let mut bag_all = SettingsBag::new();
    bag_all.insert(
        "testifylint",
        TestifylintOptions {
            disable_all: true,
            enable: vec!["require-error".into()],
            require_error_fn_pattern: Some("^DoesNotMatch$".into()),
            ..TestifylintOptions::default()
        },
    );
    let none = support::run_analyzer_with_settings(
        testifylint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag_all),
            ..RunnerOptions::default()
        },
    );
    assert!(
        none.iter().all(|m| !m.contains("require-error")),
        "non-matching fn-pattern should suppress: {none:?}"
    );
}

#[test]
fn testifylint_go_require_ignore_http_handlers() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::TestifylintOptions;

    let pkg = support::typecheck_fixture("testifylint", "example.com/testifylint", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "testifylint",
        TestifylintOptions {
            disable_all: true,
            enable: vec!["go-require".into()],
            go_require_ignore_http_handlers: true,
            ..TestifylintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        testifylint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("go-require") && m.contains("require must only")),
        "goroutine require still flagged: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .all(|m| !(m.contains("go-require") && m.contains("http handlers"))),
        "http handlers should be ignored: {messages:?}"
    );
}

#[test]
fn exptostd_flags_exp_maps() {
    let pkg = support::typecheck_fixture("exptostd", "example.com/exptostd", "bad_maps.go");
    let messages = support::run_analyzer(exptostd(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("golang.org/x/exp/maps.Clone()") && m.contains("maps.Clone()")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("golang.org/x/exp/maps.Clear()") && m.contains("clear()")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m
                .contains("Import statement 'golang.org/x/exp/maps' may be replaced by 'maps'")),
        "{messages:?}"
    );
}

#[test]
fn exptostd_flags_exp_slices_import_only_when_fully_replaceable() {
    let pkg = support::typecheck_fixture("exptostd", "example.com/exptostd", "bad_slices.go");
    let messages = support::run_analyzer(exptostd(), &pkg);
    assert!(
        messages.iter().any(|m| m
            .contains("Import statement 'golang.org/x/exp/slices' may be replaced by 'slices'")),
        "{messages:?}"
    );
    // Upstream reports only the import when every slices call is 1:1 replaceable.
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("golang.org/x/exp/slices.Equal()")),
        "per-call slices diagnostics should be omitted: {messages:?}"
    );
}

#[test]
fn exptostd_flags_exp_constraints() {
    let pkg = support::typecheck_fixture("exptostd", "example.com/exptostd", "bad_constraints.go");
    let messages = support::run_analyzer(exptostd(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m
                .contains("golang.org/x/exp/constraints.Ordered can be replaced by cmp.Ordered")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m
            .contains("Import statement 'golang.org/x/exp/constraints' may be replaced by 'cmp'")),
        "{messages:?}"
    );
}

#[test]
fn exptostd_allows_non_exp_maps() {
    let pkg = support::typecheck_fixture("exptostd", "example.com/exptostd/ok", "ok.go");
    assert!(support::run_analyzer(exptostd(), &pkg).is_empty());
}

#[test]
fn modernize_flags_common_patterns() {
    let pkg = support::typecheck_fixture("modernize", "example.com/modernize", "bad.go");
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface{} can be replaced by any")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("for loop can be modernized using range")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("if/else statement can be modernized using min")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("fmt.Appendf") || m.contains("Appendf")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("sort.Slice can be modernized using slices.Sort")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("copying variable is unneeded")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("HasPrefix + TrimPrefix can be simplified to CutPrefix")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Loop can be simplified using slices.Contains")),
        "{messages:?}"
    );
}

/// minmax's **pattern 2**: `v := x` immediately above `if v > y { v = y }`,
/// with no else. Upstream words it "if statement", where pattern 1 says
/// "if/else statement", so the two are told apart by that word alone.
#[test]
fn modernize_minmax_matches_the_assignment_above_the_if() {
    let pkg = support::typecheck_fixture("modernize", "example.com/modernize/minmax", "minmax.go");
    let messages = support::run_analyzer(modernize(), &pkg);
    let mut got: Vec<&String> = messages.iter().collect();
    got.sort();
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.starts_with("if statement can be modernized using"))
            .count(),
        6,
        "{got:?}"
    );
    // The if/else control still fires, and the three silent shapes stay silent:
    // a different variable above, operands unrelated to it, and floats.
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.starts_with("if/else statement can be modernized using"))
            .count(),
        1,
        "{got:?}"
    );
    // Seven in total: six pattern-2 findings and the one if/else control.
    assert_eq!(messages.len(), 7, "{got:?}");
}

#[test]
fn modernize_rangeint_skips_mutated_limits() {
    let pkg =
        support::typecheck_fixture("modernize", "example.com/modernize/rangeint", "rangeint.go");
    let messages = support::run_analyzer(modernize(), &pkg);
    let hits: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("for loop can be modernized using range"))
        .collect();
    // param/const/local/len, plus the two `for i = 0` loops whose index is not
    // read after the loop. `nopeIndexReadAfterLoop` returns its index, so a
    // range loop — which would leave it holding limit-1 — is not offered.
    // Then three whose *body* never reads the index: two `:=` (the fix drops
    // the declaration) and one `=` (it does not). Whether the index survives is
    // a property of the fix, not of the finding — all nine report identically.
    assert_eq!(
        hits.len(),
        9,
        "expected 9 rangeint hits, got {} {messages:?}",
        hits.len()
    );
    for bad in ["k", "incLimit", "addrLimit", "chks", "outer"] {
        assert!(
            !messages
                .iter()
                .any(|m| m.contains(&format!("range over {bad}"))),
            "mutated/addr-taken limit {bad:?} must be skipped: {messages:?}"
        );
    }
    // Upstream (x/tools modernize) always says "range over int".
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("range over int"))
            .count()
            >= 4,
        "expected ≥4 range-over-int diagnostics: {messages:?}"
    );
}

/// The four functions upstream looks up are `strings.Split`, `strings.Fields`,
/// `bytes.Split` and `bytes.Fields` — `bytes` grew `SplitSeq`/`FieldsSeq` in
/// the same release. guff had only the `strings` half.
#[test]
fn modernize_flags_stringsseq() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/stringsseq",
        "stringsseq.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    let mut seq: Vec<&str> = messages
        .iter()
        .filter(|m| m.contains("SplitSeq") || m.contains("FieldsSeq"))
        .map(|m| m.as_str())
        .collect();
    seq.sort_unstable();
    // Counted: two `Split`s, two `Fields`, and a `SplitN` that is not one of
    // the four.
    assert_eq!(
        seq,
        vec![
            "Ranging over FieldsSeq is more efficient",
            "Ranging over FieldsSeq is more efficient",
            "Ranging over SplitSeq is more efficient",
            "Ranging over SplitSeq is more efficient",
        ],
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_waitgroupgo() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/waitgroupgo",
        "waitgroupgo.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("WaitGroup.Go")),
        "{messages:?}"
    );
}

/// The destination can be a call result — syncthing writes `w.Header()[k] = v`.
/// That one went unreported not because the shape was rejected but because the
/// fix text could not be built: rendering the tree by hand had no case for a
/// call with no arguments, and a failure there drops the diagnostic.
#[test]
fn modernize_flags_mapsloop() {
    let pkg =
        support::typecheck_fixture("modernize", "example.com/modernize/mapsloop", "mapsloop.go");
    let messages = support::run_analyzer(modernize(), &pkg);
    // Counted: two loops, two findings — a plain map variable and a call
    // result.
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("Replace m[k]=v loop with maps.Copy"))
            .count(),
        2,
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_slicesbackward() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/slicesbackward",
        "slicesbackward.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("slices.Backward")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_reflecttypefor() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/reflecttypefor",
        "reflecttypefor.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    let hits: Vec<_> = messages.iter().filter(|m| m.contains("TypeFor")).collect();
    assert_eq!(
        hits.len(),
        2,
        "expected 2 TypeFor hits (concrete + Elem; interface arg skipped), got {} {messages:?}",
        hits.len()
    );
}

#[test]
fn modernize_flags_reflecttypeassert() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/reflecttypeassert",
        "reflecttypeassert.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    let hits: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("can be simplified using reflect.TypeAssert"))
        .collect();
    assert_eq!(
        hits.len(),
        6,
        "expected exactly 6 reflecttypeassert hits (6 positive / negatives skipped), got {} {messages:?}",
        hits.len()
    );
    assert!(
        hits.iter().any(|m| m.contains("Interface().(string)")),
        "{messages:?}"
    );
    assert!(
        hits.iter().any(|m| m.contains("Interface().(payload)")),
        "{messages:?}"
    );
    assert!(
        hits.iter().any(|m| m.contains("Interface().(io.Reader)")),
        "{messages:?}"
    );
    assert!(
        hits.iter().any(|m| m.contains("Interface().(int)")),
        "{messages:?}"
    );
    assert!(
        hits.iter().any(|m| m.contains("Interface().(error)")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_atomictypes() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/atomictypes",
        "atomictypes.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    let hits: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("may be simplified using atomic."))
        .collect();
    // goodLocal (x Int32), goodShadowAlias (x Int32), goodField (X.x Int32 + Z.y Int64)
    assert_eq!(
        hits.len(),
        4,
        "expected exactly 4 atomictypes hits, got {} {messages:?}",
        hits.len()
    );
    assert_eq!(
        hits.iter().filter(|m| m.contains("atomic.Int32")).count(),
        3,
        "{messages:?}"
    );
    assert!(
        hits.iter().any(|m| m.contains("atomic.Int64")),
        "{messages:?}"
    );
    // Negatives must not be reported.
    assert!(
        !messages.iter().any(|m| m.contains("var x2 ")),
        "init-assigned var must be skipped: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("var z ")),
        "unsynchronized load must be skipped: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("var n ")),
        "a field named in an elided `[]*P{{…}}` literal must be skipped: {messages:?}"
    );
}

/// `atomictypes` keeps only local vars once the package has files it cannot see.
///
/// Upstream: `if !isLocal(v) && len(pass.IgnoredFiles) > 0 { continue }`. A
/// package-level var or a struct field can be used from a build-excluded file,
/// so the rewrite the analyzer proposes might not compile there; a local var
/// cannot be. coredns's `plugin/forward` and `plugin/grpc` each carry a
/// `//go:build gofuzz` file, which is what makes their `robin uint32` fields
/// silent upstream — guff reported both.
#[test]
fn modernize_atomictypes_drops_non_locals_when_the_package_has_ignored_files() {
    let names = |pkg: &std::sync::Arc<guff_packages::Package>| -> Vec<String> {
        support::run_analyzer(modernize(), pkg)
            .into_iter()
            .filter(|m| m.contains("may be simplified using atomic."))
            .collect()
    };

    let clean = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/ignoredfiles",
        "ignoredfiles.go",
    );
    let all = names(&clean);
    assert_eq!(all.len(), 3, "field, package var and local var: {all:?}");

    let with_ignored = support::typecheck_fixture_with_ignored_files(
        "modernize",
        "example.com/modernize/ignoredfiles",
        "ignoredfiles.go",
        &["ignoredfiles_gofuzz.go"],
    );
    let kept = names(&with_ignored);
    assert_eq!(
        kept.len(),
        1,
        "only the local var survives an ignored file: {kept:?}"
    );
    assert!(kept[0].contains("var n uint32"), "{kept:?}");
}

/// The goroutine's own call has to go, or the fix does not parse.
///
/// `go func() {...}()` becomes `wg.Go(func() {...})`: the `(` of the trailing
/// `()` is deleted so the `)` closes `wg.Go(`. guff was leaving it, writing
/// `wg.Go(func() {...}()` — a syntax error. Both spellings report the same
/// diagnostic, so only the edits can tell them apart.
#[test]
fn modernize_waitgroupgo_fix_removes_the_goroutine_call_parens() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/waitgroupgo",
        "waitgroupgo.go",
    );
    let mut checked = 0;
    for d in support::run_analyzer_diagnostics(modernize(), &pkg) {
        if !d.message.contains("WaitGroup.Go") {
            continue;
        }
        let edits = &d.suggested_fixes[0].text_edits;
        assert!(
            edits.iter().any(|e| e.new_text.ends_with(".Go(")),
            "{edits:?}"
        );
        // A one-byte deletion spanning `(`: lparen..rparen of the go call.
        assert!(
            edits
                .iter()
                .any(|e| e.new_text.is_empty() && e.end == e.pos + 1),
            "the trailing `()` must lose its `(`: {edits:?}"
        );
        checked += 1;
    }
    assert_eq!(checked, 1, "one goroutine in the fixture");
}

/// An index the body never reads must not survive into the range clause.
///
/// `for i := 0; i < n; i++` with no use of `i` becomes `for range n` upstream,
/// not `for i := range n` — the latter is `declared and not used` and does not
/// compile. Nothing in the finding says which one guff writes, so only the edit
/// text can pin it (COMPAT-HARDENING, `compat/fix/`).
#[test]
fn modernize_rangeint_drops_an_index_the_body_never_reads() {
    let pkg =
        support::typecheck_fixture("modernize", "example.com/modernize/rangeint", "rangeint.go");
    let mut headers: Vec<String> = Vec::new();
    for d in support::run_analyzer_diagnostics(modernize(), &pkg) {
        if !d.message.contains("range over int") {
            continue;
        }
        // The loop header is the last edit; any import edits come first.
        let edit = d.suggested_fixes[0]
            .text_edits
            .last()
            .expect("the fix rewrites the loop header");
        headers.push(edit.new_text.clone());
    }

    // `indexUnused` and `indexShadowedInBody`. The second is why resolution is
    // by object: the inner `i := "inner"` would read as a use by name.
    assert_eq!(
        headers.iter().filter(|h| *h == "for range n").count(),
        2,
        "{headers:?}"
    );
    // `for i = 0` has no declaration to drop, so `assignIndexUnusedInBody`
    // keeps its index even though the body never reads it.
    assert!(
        headers.iter().any(|h| h == "for i = range n"),
        "{headers:?}"
    );
    assert!(
        headers.iter().any(|h| h == "for i := range n"),
        "a read index still binds: {headers:?}"
    );
}

/// The `found := false` / `found = true` shape must keep the source's own
/// assignment token.
///
/// Upstream replaces only the previous assignment's right-hand side, so `:=`
/// survives. guff rewrote from the left-hand side and spelled `=`, turning
/// `found := false` into `found = slices.Contains(...)` — `undefined: found`,
/// a `--fix` that does not compile.
#[test]
fn modernize_slicescontains_keeps_the_assignment_token() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/slicescontains",
        "slicescontains.go",
    );
    let mut found = 0;
    for d in support::run_analyzer_diagnostics(modernize(), &pkg) {
        if !d.message.contains("Loop can be simplified using slices.") {
            continue;
        }
        let edits = &d.suggested_fixes[0].text_edits;
        // The bool-accumulator shape is the one that rewrites an rhs to a bare
        // `slices.Contains(...)` with no assignment token of its own.
        if let Some(e) = edits.iter().find(|e| {
            e.new_text.starts_with("slices.Contains(") || e.new_text.starts_with("!slices.Contains(")
        }) {
            assert!(
                !e.new_text.contains('='),
                "the fix must not spell the assignment itself: {e:?}"
            );
            found += 1;
        }
    }
    assert!(found > 0, "the fixture has the bool-accumulator shape");
}

/// A fix that names a package has to add the import, and has to name the
/// package the way *this file* can.
///
/// `mapsloop.go` does not import `maps` at all, so the fix must carry the
/// import edit or `--fix` writes a file that does not compile. That is not
/// visible in any finding-set comparison: the message says `maps.Copy` either
/// way (COMPAT-HARDENING, `compat/fix/`).
#[test]
fn modernize_mapsloop_fix_adds_the_maps_import() {
    let pkg =
        support::typecheck_fixture("modernize", "example.com/modernize/mapsloop", "mapsloop.go");
    let mut replacements = Vec::new();
    for d in support::run_analyzer_diagnostics(modernize(), &pkg) {
        if !d.message.contains("maps.Copy") {
            continue;
        }
        let edits = &d.suggested_fixes[0].text_edits;
        assert_eq!(edits.len(), 2, "import edit + replacement: {edits:?}");
        // The import edit comes first and is an insertion.
        assert_eq!(edits[0].pos, edits[0].end);
        assert_eq!(edits[0].new_text, "import \"maps\"\n\n");
        replacements.push(edits[1].new_text.clone());
    }
    replacements.sort();
    // The second one names a call, which is the shape the hand-written printer
    // could not render — and a fix it cannot write is a finding it does not
    // make.
    assert_eq!(
        replacements,
        vec![
            "maps.Copy(dst, src)".to_string(),
            "maps.Copy(w.Header(), r.header)".to_string(),
        ]
    );
}

/// The prefix comes from the declaration's imports, not from the call site's
/// alias.
///
/// `atomictypes.go` imports `sync/atomic` twice — once plainly and once as
/// `myatomic` — and one function reaches the package through the alias. guff
/// used to copy the alias into the *declaration*, writing
/// `var x myatomic.Int32`; upstream writes `atomic.Int32`, because the name it
/// picks is the one `AddImport` finds in scope at the declaration, and the
/// plain import is the first spec in the file.
#[test]
fn modernize_atomictypes_fix_names_the_package_the_declaration_can_see() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/atomictypes",
        "atomictypes.go",
    );
    let mut type_edits: Vec<String> = Vec::new();
    for d in support::run_analyzer_diagnostics(modernize(), &pkg) {
        if !d.message.contains("may be simplified using atomic.") {
            continue;
        }
        for edit in &d.suggested_fixes[0].text_edits {
            assert!(
                !edit.new_text.starts_with("import "),
                "sync/atomic is already imported; nothing to add: {edit:?}"
            );
            if edit.new_text.starts_with("atomic.") || edit.new_text.starts_with("myatomic.") {
                type_edits.push(edit.new_text.clone());
            }
        }
    }
    assert!(!type_edits.is_empty(), "the fixture reports something");
    assert!(
        type_edits.iter().all(|t| t.starts_with("atomic.")),
        "the alias must not reach the declaration: {type_edits:?}"
    );
}

/// omitzero offers **two** fixes, and they deliberately conflict.
///
/// Upstream reports a deletion and a replacement over the same span, so
/// golangci-lint's fixer sees an overlap and drops every modernize edit in the
/// file — a user running `--fix` gets nothing here. guff emitted only the
/// `omitzero` half, which had nothing to conflict with, so it silently rewrote
/// the tag: `omitempty` -> `omitzero` is a change to what the encoder puts on
/// the wire, made without asking.
///
/// The spans are asserted relative to the tag literal (the diagnostic's own
/// Pos), because that is where the old code was wrong in the other direction:
/// it replaced the *whole literal* rather than the `,omitempty` run inside it.
#[test]
fn modernize_omitzero_offers_a_removal_and_a_replacement() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/omitzeroshapes",
        "omitzero_shapes.go",
    );
    let mut spans: Vec<(u32, u32, String)> = Vec::new();
    for d in support::run_analyzer_diagnostics(modernize(), &pkg) {
        if !d.message.contains("Omitempty has no effect") {
            continue;
        }
        assert_eq!(
            d.suggested_fixes.len(),
            2,
            "both alternatives are reported: {:?}",
            d.suggested_fixes
        );
        assert_eq!(d.suggested_fixes[0].message, "Remove redundant omitempty tag");
        assert_eq!(
            d.suggested_fixes[1].message,
            "Replace omitempty with omitzero (behavior change)"
        );
        for fix in &d.suggested_fixes {
            let edit = &fix.text_edits[0];
            spans.push((edit.pos - d.pos, edit.end - edit.pos, edit.new_text.clone()));
        }
    }

    // Offsets from the start of the tag literal; lengths in source bytes.
    // Confirmed against golangci-lint 2.12.2 on the same fixture.
    assert_eq!(
        spans,
        vec![
            // `"json:\"value,omitempty\""` — the escaped quote is two source
            // bytes for one value byte, so the run starts at 13, not 11.
            (13, 10, String::new()),
            (13, 10, ",omitzero".to_string()),
            // `` `json:",omitempty"` `` — json carries nothing else, so the
            // removal takes the literal whole, backquotes included.
            (0, 19, String::new()),
            (7, 10, ",omitzero".to_string()),
            // Another key follows, so only the json tag goes — and the regex's
            // trailing `\s?` takes the space with it.
            (1, 18, String::new()),
            (7, 10, ",omitzero".to_string()),
        ],
        "upstream's spans, per omitzero.go and astutil.RangeInStringLiteral"
    );
}

/// `omitzero` is off for a package that carries a kubebuilder marker.
///
/// kubebuilder has its own interpretation of the tag (go.dev/issue/76649), so
/// upstream returns before reporting — for the whole package, not just the
/// fields near a marker. dapr's `pkg/apis/**` are CRD types of exactly this
/// shape: 24 findings golangci-lint does not make.
///
/// The marker lives in a comment, and the production load parses without
/// `PARSE_COMMENTS`, so the check re-reads the retained source. The fixture
/// keeps the marker on a doc comment two fields away from the tags it silences.
#[test]
fn modernize_omitzero_is_off_for_a_kubebuilder_package() {
    let with_marker = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/omitzerokubebuilder",
        "omitzero_kubebuilder.go",
    );
    let silenced = support::run_analyzer(modernize(), &with_marker);
    assert!(
        !silenced.iter().any(|m| m.contains("Omitempty")),
        "a kubebuilder package reports no omitzero at all: {silenced:?}"
    );

    let plain = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/omitzeroplain",
        "omitzero_plain.go",
    );
    let reported = support::run_analyzer(modernize(), &plain);
    assert_eq!(
        reported
            .iter()
            .filter(|m| m.contains("Omitempty has no effect on nested struct fields"))
            .count(),
        2,
        "the same shape without a marker keeps both: {reported:?}"
    );
}

#[test]
fn modernize_flags_testingcontext() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/testingcontext",
        "testingcontext.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("t.Context")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_unsafefuncs() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/unsafefuncs",
        "unsafefuncs.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages.iter().filter(|m| m.contains("unsafe.Add")).count() >= 2,
        "{messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("namedUP")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_importcomment() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/importcomment",
        "importcomment.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("canonical import path comment")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_stringscut() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/stringscut",
        "stringscut.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("strings.Cut"))
            .count()
            >= 2,
        "{messages:?}"
    );
    assert!(
        messages.iter().filter(|m| m.contains("bytes.Cut")).count() >= 2,
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_newexpr() {
    let pkg =
        support::typecheck_fixture("modernize", "example.com/modernize/newexpr", "newexpr.go");
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("intVar can be an inlinable wrapper around new(expr)")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("varOf can be an inlinable wrapper around new(expr)")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("alreadyAnnotated can be an inlinable wrapper around new(expr)")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("call of intVar(x) can be simplified to new(x)")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("call of stringVar(x) can be simplified to new(x)")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("call of varOf(x) can be simplified to new(x)")),
        "{messages:?}"
    );
    // Untyped int → int64 parameter must not rewrite.
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("call of int64Var(x) can be simplified to new(x)")),
        "{messages:?}"
    );
    // Variadic must not be flagged as a new-like wrapper.
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("variadic can be an inlinable wrapper")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_errorsastype() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/errorsastype",
        "errorsastype.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    let as_type: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("errors.As can be simplified using AsType"))
        .collect();
    assert!(
        as_type.len() >= 9,
        "expected >=9 AsType suggestions, got {} {messages:?}",
        as_type.len()
    );
    assert!(
        as_type.iter().any(|m| m.contains("AsType[*os.PathError]")),
        "{messages:?}"
    );
    assert!(
        as_type.iter().any(|m| m.contains("AsType[*os.LinkError]")),
        "{messages:?}"
    );
    assert!(
        as_type.iter().any(|m| m.contains("AsType[FooError]")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_stringsbuilder() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/stringsbuilder",
        "stringsbuilder.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    let hits: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("using string += string in a loop is inefficient"))
        .collect();
    assert_eq!(
        hits.len(),
        4,
        "expected exactly 4 stringsbuilder hits (4 positive / 5 negative), got {} {messages:?}",
        hits.len()
    );
}

#[test]
fn modernize_flags_slicesdelete() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/slicesdelete",
        "slicesdelete.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    let hits: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("Replace append with slices.Delete"))
        .collect();
    assert_eq!(
        hits.len(),
        8,
        "expected exactly 8 slicesdelete hits, got {} {messages:?}",
        hits.len()
    );
}

#[test]
fn modernize_flags_bloop() {
    let pkg = support::typecheck_fixture("modernize", "example.com/modernize/bloop", "bloop.go");
    let messages = support::run_analyzer(modernize(), &pkg);
    let hits: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("b.N can be modernized using b.Loop()"))
        .collect();
    assert_eq!(
        hits.len(),
        4,
        "expected exactly 4 bloop hits (A/C/D/E), got {} {messages:?}",
        hits.len()
    );
}

#[test]
fn modernize_flags_stditerators() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/stditerators",
        "stditerators.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    let struct_hits = messages
        .iter()
        .filter(|m| m.contains("NumFields/Field loop can simplified using Struct.Fields iteration"))
        .count();
    assert_eq!(
        struct_hits, 2,
        "expected 2 Struct hits (C-style + range), got {struct_hits}: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Len/At loop can simplified using Tuple.Variables iteration")),
        "expected a Tuple hit, got {messages:?}"
    );
    // extraUse / plainSlice must not be flagged.
    let total = messages
        .iter()
        .filter(|m| m.contains("loop can simplified using"))
        .count();
    assert_eq!(
        total, 3,
        "expected exactly 3 stditerators hits, got {total}: {messages:?}"
    );
}

#[test]
fn modernize_flags_slicescontains_variants() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/slicescontains",
        "slicescontains.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("slices.Contains") && !m.contains("ContainsFunc"))
            .count()
            >= 4,
        "expected Contains variants, got {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("slices.ContainsFunc")),
        "expected ContainsFunc, got {messages:?}"
    );
}

#[test]
fn modernize_flags_stringscutprefix_pattern2_and_bytes() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/stringscutprefix",
        "stringscutprefix.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("TrimPrefix can be simplified to CutPrefix")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("TrimSuffix can be simplified to CutSuffix")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("HasPrefix + TrimPrefix can be simplified to CutPrefix"))
            .count()
            >= 1,
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("HasSuffix + TrimSuffix can be simplified to CutSuffix")),
        "{messages:?}"
    );
}

#[test]
fn modernize_allows_modern_code() {
    let pkg = support::typecheck_fixture("modernize", "example.com/modernize/ok", "ok.go");
    assert!(
        support::run_analyzer(modernize(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(modernize(), &pkg)
    );
}

#[test]
fn modernize_flags_obsolete_plusbuild() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/plusbuild",
        "plusbuild.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("+build line is no longer needed")),
        "{messages:?}"
    );
}

#[test]
fn modernize_disable_skips_checkers() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::ModernizeOptions;

    let pkg = support::typecheck_fixture("modernize", "example.com/modernize", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "modernize",
        ModernizeOptions {
            disable: vec!["any".into(), "rangeint".into(), "minmax".into()],
        },
    );
    let messages = support::run_analyzer_with_settings(
        modernize(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("interface{} can be replaced by any")),
        "{messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("for loop can be modernized using range")),
        "{messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("if/else statement can be modernized using min")),
        "{messages:?}"
    );
}

#[test]
fn gocritic_flags_common_patterns() {
    let pkg = support::typecheck_fixture("gocritic", "example.com/gocritic", "bad.go");
    let messages = support::run_analyzer(gocritic(), &pkg);
    let expect = [
        "else if cond",
        "rewrite switch statement to if",
        "default` case as first or as last",
        "switch true",
        "always true",
        "always false",
        "can be len(",
        "could simplify",
        "*new(bool)",
        "append result not assigned",
        "is duplicated",
        "should not be capitalized",
        "exitAfterDefer: log.Fatal will exit, and `defer os.Remove(name)` will not run",
        "rewrite if-else to switch",
        "can re-write as",
        "flagDeref: immediate deref in *flag.Bool(\"b\", false, \"docs\")",
        "no-op append",
        "suspicious Join",
        "probably meant -1",
        "x++",
        "x *=",
        "dupArg: suspicious duplicated args in strings.Contains(a, a)",
        "both branches in if statement have same body",
        "identical LHS and RHS",
        "contains whitespace",
        "suspicious whitespace",
        "always panics",
        "type switch with assignment",
        "in loop; probably meant",
        "condition is suspicious",
        "replace `",
        "MustCompile",
        "wrapperFunc: use strings.Split method in `strings.SplitN(s, \",\", -1)`",
        "arguments order looks reversed",
        "must go before the",
        "Code generated .* DO NOT EDIT",
        "put a space between",
        "Deprecated: ` (note the casing)",
        "from/to types are identical",
    ];
    for needle in expect {
        assert!(
            messages.iter().any(|m| m.contains(needle)),
            "missing `{needle}` in {messages:?}"
        );
    }
}

#[test]
fn gocritic_allows_clean_code() {
    let pkg = support::typecheck_fixture("gocritic", "example.com/gocritic/ok", "ok.go");
    let messages = support::run_analyzer(gocritic(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn gocritic_disabled_checks_are_skipped() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GocriticOptions;

    let pkg = support::typecheck_fixture("gocritic", "example.com/gocritic", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "gocritic",
        GocriticOptions {
            enable_all: true,
            disabled_checks: vec![
                "appendAssign".into(),
                "ifElseChain".into(),
                "underef".into(),
            ],
            ..GocriticOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        gocritic(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("append result not assigned")),
        "{messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("rewrite if-else to switch")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("else if cond")),
        "{messages:?}"
    );
}

#[test]
fn gocritic_disabled_tags_style_skips_if_else_chain() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GocriticOptions;

    let pkg = support::typecheck_fixture("gocritic", "example.com/gocritic", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "gocritic",
        GocriticOptions {
            disabled_tags: vec!["style".into()],
            ..GocriticOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        gocritic(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("rewrite if-else to switch")),
        "ifElseChain is style-tagged; disabled-tags:style must suppress it: {messages:?}"
    );
    // Diagnostic-tagged checks remain on.
    assert!(
        messages
            .iter()
            .any(|m| m.contains("append result not assigned")),
        "{messages:?}"
    );
}

/// Which doc comments `deprecatedComment` is shown, and what each malformed
/// notice is called.
///
/// The checker is `astwalk.WalkerForDocComment`, whose file walk starts at
/// `f.Decls` and reaches a `GenDecl`'s doc *and* every spec inside it, plus
/// every `Field` under a `TypeSpec`'s type. guff walked only the outermost two
/// and additionally visited the *package* doc, which upstream never reaches —
/// six `Deprecated: ` notices on Tekton pipeline's grouped constants and struct
/// fields went unreported, and a `//nolint:gocritic` covering a seventh was
/// called unused.
///
/// Asserted as `(line, message)` pairs. Four of the five messages this checker
/// can produce had no fixture at all before this one, and one of them was
/// wrong: `warnPattern` says ``the proper format is `Deprecated: <text>` ``,
/// not ``the proper format is `Deprecated: ` ``. Measured against
/// golangci-lint 2.12.2 (go-critic v0.14.3).
#[test]
fn gocritic_deprecated_comment_sees_every_declaration_doc() {
    let pkg = support::typecheck_fixture(
        "gocritic",
        "example.com/gocritic/deprecated",
        "deprecated.go",
    );
    let fset = pkg.fset.clone().expect("fixture has a FileSet");
    let paragraph = "deprecatedComment: `Deprecated: ` notices should be in a dedicated paragraph, separated from the rest";
    let pattern = "deprecatedComment: the proper format is `Deprecated: <text>`";
    let casing = "deprecatedComment: use `Deprecated: ` (note the casing) instead of `DEPRECATED: `";
    let mut got: Vec<(i64, String)> = support::run_analyzer_diagnostics(gocritic(), &pkg)
        .into_iter()
        .map(|d| {
            (
                fset.position(guff::position::Pos(d.pos as i64)).line,
                d.message,
            )
        })
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            // an ImportSpec doc, then a GenDecl doc — and the GenDecl one is
            // reported once, though guff's parser hangs it on the lone
            // ValueSpec as well
            (19, paragraph.into()),
            (26, paragraph.into()),
            // a spec inside `const (…)` and one inside `type (…)`
            (31, paragraph.into()),
            (37, paragraph.into()),
            // struct fields, including one nested two types deep…
            (43, paragraph.into()),
            (46, pattern.into()),
            (53, paragraph.into()),
            // …and an interface method
            (60, paragraph.into()),
            // a previous line shorter than the prefix still counts as text
            (73, paragraph.into()),
            // the five messages, one declaration each
            (97, casing.into()),
            (100, "deprecatedComment: use `:` instead of `,` in `Deprecated, `".into()),
            (103, "deprecatedComment: typo in `Deprecatd`; should be `Deprecated`".into()),
            (106, pattern.into()),
            (109, pattern.into()),
            (112, pattern.into()),
            (115, pattern.into()),
            (118, pattern.into()),
            (121, pattern.into()),
            (124, pattern.into()),
            // two problems in one comment: the walk returns after the first
            (128, casing.into()),
        ],
        "deprecatedComment findings"
    );
}

#[test]
fn gocritic_enable_all_extras() {
    // Counts over `extras.go` alone (the golden case also reads bad.go and
    // testfuncs.go, so its numbers are larger).
    const PSW: usize = 6;
    const PFP: usize = 4;
    const RSPRINT: usize = 3;
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GocriticOptions;

    let pkg = support::typecheck_fixture("gocritic", "example.com/gocritic/extras", "extras.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "gocritic",
        GocriticOptions {
            enable_all: true,
            ..GocriticOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        gocritic(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    let expect = [
        "replace `len(s)",
        "replace empty case containing only fallthrough",
        "empty var() block",
        "empty const() block",
        "empty type() block",
        "use new octal literal style, 0o755",
        "returned expr is always nil",
        "consider to change order in expression to p == nil",
        "consider to change order in expression to *p == 10",
        "can rewrite as `defer fmt.Println",
        "consider to move `sideEffectExtra()` before if",
        "shadowing of predeclared identifier: len",
        "shadowing of predeclared identifier: complex64",
        "package is imported 2 times under different aliases",
        "remove commented-out \"os\" import",
        "func(a int, b int) could be replaced with func(a, b int)",
        "\"dir/\" contains a path separator",
        "append all `ns` data while range it",
        "nil check may not be enough, check for len",
        "function argument `withWidth(w)` is duplicated",
        "consider to change `methodFoo.bar` to `f.bar`",
        "copy of xs (512 bytes) can be avoided with &xs",
        "'.com' should probably be '\\.com'",
        "^ applied only to",
        "is duplicated",
        "`\\w` intersects with `_`",
        "cmp func must use xs slice in comparison",
        "use db.Exec() if returned result is not needed",
        "ignoring Query() rows result may lead to a connection leak",
        "rewrite if-else to type switch statement",
        "truncation in comparison",
        "definition of type 'typeDefFirstRecv'",
        "Possible resource leak, 'defer' is called in the 'for' loop",
        "prefer 0x over 0X",
        "don't mix hex literal letter digits casing",
        "invert if cond, replace body with `continue`",
        "may want to add detail/assignee to this TODO/FIXME/BUG comment",
        "silencing go lint doc-comment warnings is unadvised",
        "block doesn't have definitions, can be simply deleted",
        "re-assignment to `err` can be replaced with",
        "http.NoBody should be preferred",
        "utf8.DecodeRuneInString",
        "consider writing single byte rune '\\n' with w.WriteByte('\\n')",
        "bytes.Index(",
        "stringXbytes: can simplify `[]byte(s)` to `s`",
        "stringXbytes: suggestion: len(b) == 0",
        "stringXbytes: suggestion: len(b)",
        "filepath.Join(",
        "stringsCompare: suggestion: a == b",
        "avoid bytes.Repeat",
        "suspicious sort.StringSlice usage",
        "rewrite as for-range so compiler can recognize",
        "preferFprint: fmt.Fprint(w, x) should be preferred to the w.Write([]byte(fmt.Sprint(x)))",
        "preferFprint: suggestion: fmt.Fprintf(w, \"%d\", x)",
        "preferFprint: suggestion: fmt.Fprintln(w, x)",
        "preferStringWriter: w.WriteString(s) should be preferred to the w.Write([]byte(s))",
        "use m.LoadAndDelete to perform load+delete",
        "use errors.New(msg) or fmt.Errorf",
        "use errors.New(f()) or fmt.Errorf",
        "stringConcatSimplify: suggestion: x + y",
        "stringConcatSimplify: suggestion: x + y + z",
        "stringConcatSimplify: suggestion: x + glue + y",
        "sync.OnceFunc(f) result is not used",
        "consider to assign sync.OnceFunc(f) to a variable",
        "consider replacing with strings.EqualFold(x, y)",
        "consider replacing with !strings.EqualFold(x, y)",
        "consider replacing with bytes.EqualFold(xb, yb)",
        "use %q instead of \"%s\" for quoted strings",
        "use t.UnixMilli() instead of",
        "use tp.UnixMicro() instead of",
        "can combine chain of 2 appends into one",
        "defer appendCombineExtra() is placed just before return",
        "s is already string",
        "use w.String() instead",
        "could simplify [](func()) to []func()",
        "shadow of imported package 'filepath'",
        "consider giving a name to these results",
        "include an explanation for nolint directive",
        "is heavy (",
        "each iteration copies",
        "consider `m' to be of non-pointer type",
        "consider `ch' to be of non-pointer type",
        "function has more than 5 results",
        "may want to evaluate evalOrderMutate(&x) before the return statement",
        "label label1 is redundant",
        "change `continue outer` to `break`",
        "Possibly return is missed after the http.Error call",
        "may want to remove commented-out code",
        "don't embed sync.Mutex",
        "don't embed *sync.RWMutex",
        "defer is missing, mutex is unlocked immediately",
        "suspicious unlock, maybe Unlock was intended?",
        "suspicious unlock, maybe RUnlock was intended?",
        "maybe defer rw.Unlock() was intended?",
        "maybe defer rw.RUnlock() was intended?",
        "suspicious reassignment of error from another package",
        "err error is unchecked, maybe intended to check it instead of err2",
        "can simplify `!!x` to `x`",
        "can simplify `!(a >= b)` to `a < b`",
        "can simplify `!x == !y` to `x == y`",
        "can simplify `a > b || a == b` to `a >= b`",
        "can simplify `a < b+1` to `a <= b`",
        "can simplify `a+1 > b` to `a >= b`",
        "can simplify `a >= b+1` to `a > b`",
        "can simplify `!(a >= b+1)` to `a <= b`",
        "can simplify `a > 10 && a < 12` to `a == 11`",
        "can simplify `a < 11 || a > 11` to `a != 11`",
        "can re-write `[0-9]+` as `\\d+`",
        "can re-write `(?:a|b|c)` as `[abc]`",
        "can re-write `foo|fo` as `foo?`",
        "can re-write `axx*y` as `ax+y`",
    ];
    for needle in expect {
        assert!(
            messages.iter().any(|m| m.contains(needle)),
            "missing `{needle}` in {messages:?}"
        );
    }
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("consider writing single byte rune"))
            .count(),
        1,
        "{messages:?}"
    );
    // `unnamedResult` reads its type names off `types.Type`, where every
    // unnamed type answers the empty string — so `(bool, []string)` is two
    // results with *the same* name and does want naming. Reading them off the
    // syntax made those two look different, and four of the twenty measured
    // shapes went unreported: `(bool, []string)`, `([]byte, bool, error)`,
    // `(int, string)`, `(int, string, error)`. fiber carries ten
    // `//nolint:gocritic // unnamedResult` directives on exactly that family.
    //
    // Thirteen: the ten shapes the fixture now says report, plus
    // `unnamedResultExtra`, `tooManyResultsExtra` and `evalOrderExtra`, which
    // are `(float64, float64)` and `(int, int)` shapes of their own.
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("consider giving a name to these results"))
            .count(),
        13,
        "unnamedResult shapes: {messages:?}"
    );
    // `Type.Implements` is `types.Implements`: the method set of the type as
    // written. A `WriteString` or `String` with a pointer receiver is not in
    // the value type's method set, so only the pointer forms report. Counting
    // is the whole test — the value forms are silent, and an `any(contains(…))`
    // over these three messages was already true of the pointer forms.
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("should be preferred to the") && m.contains("WriteString"))
            .count(),
        PSW,
        "preferStringWriter: {messages:?}"
    );
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("preferFprint") || m.contains("fmt.Fprint"))
            .count(),
        PFP,
        "preferFprint: {messages:?}"
    );
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("instead") && m.contains(".String()"))
            .count(),
        RSPRINT,
        "redundantSprint: {messages:?}"
    );
}

#[test]
/// `rangeValCopy` / `rangeExprCopy` take `skipTestFuncs`, default **true**, and
/// are the only two checkers that do. `isUnitTestFunc` is name + signature
/// (`Test` prefix, one `*testing.T`, no results), never the file name — so a
/// benchmark, a `Test…` with a result, and a plain helper are all still
/// reported. k9s had three of these in `internal/render/*_test.go`; the same
/// bytes go through golangci-lint in `compat/golden/cases/gocritic`.
#[test]
fn gocritic_range_copy_skips_unit_test_funcs() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GocriticOptions;

    let pkg = support::typecheck_fixture(
        "gocritic",
        "example.com/gocritic/testfuncs",
        "testfuncs.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "gocritic",
        GocriticOptions {
            enable_all: true,
            ..GocriticOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        gocritic(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    // Exact counts, because the whole point is what is *absent*. Measured
    // against golangci-lint 2.12.2 in compat/golden/cases/gocritic: six
    // rangeValCopy sites, of which the two inside TestRangeValCopySkipped
    // (its body and its `t.Run` closure, pruned with the parent) go
    // unreported; two rangeExprCopy sites, one of them pruned.
    let val_copies = messages
        .iter()
        .filter(|m| m.contains("each iteration copies"))
        .count();
    let expr_copies = messages
        .iter()
        .filter(|m| m.contains("can be avoided with &"))
        .count();
    assert_eq!(
        val_copies, 4,
        "rangeValCopy should skip the two sites in the unit test func: {messages:?}"
    );
    assert_eq!(
        expr_copies, 1,
        "rangeExprCopy should skip the site in the unit test func: {messages:?}"
    );
}

#[test]
fn gocritic_extras_off_by_default() {
    let pkg = support::typecheck_fixture("gocritic", "example.com/gocritic/extras", "extras.go");
    let messages = support::run_analyzer(gocritic(), &pkg);
    assert!(
        !messages.iter().any(|m| {
            m.contains("empty var() block")
                || m.contains("octal literal")
                || m.contains("yoda")
                || m.contains("always nil")
                || m.contains("can rewrite as `defer")
                || m.contains("before if")
                || m.contains("shadowing of predeclared")
                || m.contains("imported 2 times")
                || m.contains("commented-out")
                || m.contains("could be replaced with func(a, b int)")
                || m.contains("path separator")
                || m.contains("append all")
                || m.contains("nil check may not be enough")
                || m.contains("is duplicated")
                || m.contains("consider to change `methodFoo")
                || m.contains("can be avoided with &")
                || m.contains("should probably be")
                || m.contains("applied only to")
                || m.contains("intersects with")
                || m.contains("must use xs slice")
                || m.contains("use db.Exec()")
                || m.contains("connection leak")
                || m.contains("type switch statement")
                || m.contains("truncation in comparison")
                || m.contains("definition of type")
                || m.contains("Possible resource leak")
                || m.contains("prefer 0x over 0X")
                || m.contains("don't mix hex literal")
                || m.contains("invert if cond")
                || m.contains("detail/assignee")
                || m.contains("doc-comment warnings")
                || m.contains("block doesn't have definitions")
                || m.contains("re-assignment to")
                || m.contains("http.NoBody")
                || m.contains("DecodeRuneInString")
                || m.contains("bytes.Index(")
                || m.contains("can simplify `[]byte")
                || m.contains("filepath.Join(")
                || m.contains("strings.Compare")
                || m.contains("bytes.Repeat")
                || m.contains("sort.StringSlice")
                || m.contains("for-range so compiler")
                || m.contains("should be preferred")
                || m.contains("LoadAndDelete")
                || m.contains("errors.New")
                || m.contains("strings.Join")
                || m.contains("sync.OnceFunc")
                || m.contains("EqualFold")
                || m.contains("EqualFold")
                || m.contains("%q instead")
                || m.contains("%#q instead")
                || m.contains("UnixMilli")
                || m.contains("UnixMicro")
                || m.contains("combine chain of")
                || m.contains("just before return")
                || m.contains("already string")
                || m.contains(".String() instead")
                || m.contains("could simplify [](func())")
                || m.contains("shadow of imported")
                || m.contains("giving a name to these results")
                || m.contains("explanation for nolint")
                || m.contains("is heavy (")
                || m.contains("each iteration copies")
                || m.contains("non-pointer type")
                || m.contains("more than 5 results")
                || m.contains("before the return statement")
                || m.contains("label label1 is redundant")
                || m.contains("continue outer")
                || m.contains("return is missed after the http.Error")
                || m.contains("don't embed sync.Mutex")
                || m.contains("don't embed *sync.RWMutex")
        }),
        "extras should be off by default: {messages:?}"
    );
}

#[test]
fn noinlineerr_flags_inline_error_handling() {
    let pkg = support::typecheck_fixture("noinlineerr", "example.com/noinlineerr", "bad.go");
    let messages = support::run_analyzer(noinlineerr(), &pkg);
    assert_eq!(
        messages.len(),
        3,
        "expected exactly 3 inline-error reports: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .all(|m| m.contains("avoid inline error handling")),
        "{messages:?}"
    );
}

#[test]
fn noinlineerr_allows_plain_assignment() {
    let pkg = support::typecheck_fixture("noinlineerr", "example.com/noinlineerr/ok", "ok.go");
    let messages = support::run_analyzer(noinlineerr(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn testableexamples_flags_missing_output() {
    let pkg = support::typecheck_fixture(
        "testableexamples",
        "example.com/testableexamples",
        "bad_test.go",
    );
    let messages = support::run_analyzer(testableexamples(), &pkg);
    assert_eq!(
        messages.len(),
        1,
        "expected exactly 1 missing-output report: {messages:?}"
    );
    assert!(
        messages[0].contains("missing output for example"),
        "{messages:?}"
    );
}

#[test]
fn testableexamples_allows_examples_with_output() {
    let pkg = support::typecheck_fixture(
        "testableexamples",
        "example.com/testableexamples/ok",
        "ok_test.go",
    );
    let messages = support::run_analyzer(testableexamples(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn testableexamples_flags_whole_file_example_without_output() {
    let pkg = support::typecheck_fixture(
        "testableexamples/whole_bad",
        "example.com/testableexamples/whole_bad",
        "whole_bad_test.go",
    );
    let messages = support::run_analyzer(testableexamples(), &pkg);
    assert_eq!(
        messages.len(),
        1,
        "expected whole-file missing output: {messages:?}"
    );
    assert!(
        messages[0].contains("missing output for example"),
        "{messages:?}"
    );
}

#[test]
fn funcorder_flags_default_violations() {
    let pkg = support::typecheck_fixture("funcorder", "example.com/funcorder", "bad.go");
    let messages = support::run_analyzer(funcorder(), &pkg);
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(
        messages.iter().any(|m| m
            == "constructor \"NewOther\" for struct \"Other\" should be placed after the struct declaration"),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m
            == "unexported method \"lenName\" for struct \"Other\" should be placed after the exported method \"GetName\""),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m
            == "constructor \"NewThird\" for struct \"Third\" should be placed before struct method \"Do\""),
        "{messages:?}"
    );
}

#[test]
fn funcorder_allows_correct_order() {
    let pkg = support::typecheck_fixture("funcorder", "example.com/funcorder/ok", "ok.go");
    assert!(support::run_analyzer(funcorder(), &pkg).is_empty());
}

#[test]
fn funcorder_alphabetical_is_opt_in() {
    use guff_style::FuncorderOptions;

    let pkg = support::typecheck_fixture(
        "funcorder",
        "example.com/funcorder/alpha",
        "alphabetical.go",
    );

    // Default settings: alphabetical off → no diagnostics.
    assert!(
        support::run_analyzer(funcorder(), &pkg).is_empty(),
        "alphabetical should be off by default"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "funcorder",
        FuncorderOptions {
            alphabetical: true,
            ..FuncorderOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        funcorder(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(
        messages.iter().any(|m| m
            == "constructor \"NewAS\" for struct \"S\" should be placed before constructor \"NewBS\""),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m
            == "method \"GoodAfternoon\" for struct \"S\" should be placed before method \"GoodMorning\""),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m
            == "method \"bye\" for struct \"S\" should be placed before method \"hello\""),
        "{messages:?}"
    );
}

#[test]
fn funcorder_function_check_is_opt_in() {
    use guff_style::FuncorderOptions;

    let pkg =
        support::typecheck_fixture("funcorder", "example.com/funcorder/func", "function.go");

    // Default settings: function off → no diagnostics.
    assert!(
        support::run_analyzer(funcorder(), &pkg).is_empty(),
        "function check should be off by default"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "funcorder",
        FuncorderOptions {
            function: true,
            ..FuncorderOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        funcorder(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(
        messages[0],
        "unexported function \"helper\" should be placed after the exported function \"PublicFunc\""
    );
}

#[test]
fn varnamelen_flags_short_names_with_long_scope() {
    let pkg = support::typecheck_fixture("varnamelen", "example.com/varnamelen", "bad.go");
    let messages = support::run_analyzer(varnamelen(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("variable name 'x' is too short")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("variable name 'y' is too short")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("parameter name 'z' is too short")),
        "{messages:?}"
    );
}

#[test]
fn varnamelen_allows_short_distance_and_long_names() {
    let pkg = support::typecheck_fixture("varnamelen", "example.com/varnamelen/ok", "ok.go");
    let messages = support::run_analyzer(varnamelen(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn varnamelen_respects_ignore_names() {
    use guff_style::VarnamelenOptions;

    let pkg =
        support::typecheck_fixture("varnamelen", "example.com/varnamelen/settings", "settings.go");

    let default_msgs = support::run_analyzer(varnamelen(), &pkg);
    assert!(
        default_msgs
            .iter()
            .any(|m| m.contains("variable name 'x' is too short")),
        "{default_msgs:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "varnamelen",
        VarnamelenOptions {
            ignore_names: vec!["x".into()],
            ..VarnamelenOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        varnamelen(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(messages.is_empty(), "{messages:?}");
}

/// An interface method belongs to the type that *implements* the interface.
///
/// Upstream skips a method only when the receiver's own named type is known to
/// implement an interface that requires it. guff also silenced any method whose
/// name and signature matched an interface declared in the package, whoever
/// implemented it — a rule upstream has no counterpart for, and the reason
/// syncthing's `(*stateTracker).getState` went unreported: the interface is
/// implemented by a `folder` that *embeds* the tracker.
#[test]
fn unparam_asks_whether_this_type_implements_the_interface() {
    let pkg =
        support::typecheck_fixture("unparam", "example.com/unparam/ifacename", "ifacename.go");
    let messages = support::run_analyzer(unparam(), &pkg);
    // Counted: two near-identical methods, and only the one whose own type is
    // not asserted to implement the interface is reported.
    assert_eq!(
        messages,
        vec!["(*tracker).getState - result changed is never used".to_string()],
        "{messages:?}"
    );
}

/// "always receives" over call sites that spell the same constant two ways.
///
/// go/ssa's `NewConst` normalizes a zero value, so the constant for `var s
/// string` carries `""` and compares equal to a written `""`. A zero constant
/// that keeps "no value" disagrees with the literal and the parameter goes
/// unreported — authelia's `runCryptoPairGenerate` has four call sites writing
/// `""` and one passing a `var privateKeyLegacyPath string` straight down.
#[test]
fn unparam_sees_a_zero_variable_and_a_written_zero_as_one_constant() {
    let pkg = support::typecheck_fixture("unparam", "example.com/unparam/zeroconst", "zeroconst.go");
    let messages = support::run_analyzer(unparam(), &pkg);
    let mut got: Vec<&str> = messages.iter().map(|m| m.as_str()).collect();
    got.sort_unstable();
    // Counted: five functions, four of them findings. Without the
    // normalization the three mixed-spelling ones answer nothing.
    assert_eq!(
        got,
        vec![
            "mixedBool - on always receives false",
            "mixedInt - n always receives 0",
            "mixedString - legacyPath always receives \"\"",
            "nilPointer - p always receives nil",
        ],
        "{messages:?}"
    );
}

#[test]
fn unparam_flags_unused_parameters() {
    let pkg = support::typecheck_fixture("unparam", "example.com/unparam", "bad.go");
    let messages = support::run_analyzer(unparam(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("example - unused is unused")),
        "{messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("withBlank - _")),
        "underscore params should be skipped: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("stub") || m.contains("discardOnly")),
        "stub / discard-only bodies should be skipped: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("ExportedUnused")),
        "exported funcs skipped by default: {messages:?}"
    );
    // Statements behind a call that cannot return are not in upstream's IR, so
    // a parameter used only there is unused. One assertion per terminator
    // family: the stdlib table, in-package induction, and the call sites that
    // vanish with the block they were written in.
    for func in [
        "afterOsExit",
        "afterSyscallExit",
        "afterGoexit",
        "afterLogFatalf",
        "afterLogPanicln",
        "afterLoggerFatal",
        "afterTestingFatal",
        "afterTestingSkip",
        "afterTestingSkipNow",
        "afterNestedSkip",
        "afterDies",
        "afterDiesTwice",
    ] {
        assert!(
            messages
                .iter()
                .any(|m| m.contains(&format!("{func} - unused is unused"))),
            "{func}: expected a report behind the no-return call: {messages:?}"
        );
    }
    assert!(
        messages
            .iter()
            .any(|m| m.contains(r#"execCmd - cmd always receives "sh""#)),
        "call sites in dead code should not count: {messages:?}"
    );
    // `dummyImpl` is the IR's answer and the only one. Nine `return <expr>`
    // shapes whose value is outside upstream's operand whitelist, so the body
    // is not a stub and the parameter is reported. `retClosure` is scaleway-cli's
    // `deleteServer`; before this, an AST stand-in ran *after* `dummy_impl` and
    // called every one of them a stub.
    for func in [
        "retClosure",
        "retField",
        "retMapIndex",
        "retStringIndex",
        "retMethodValue",
        "retTypeAssert",
        "retFieldAddr",
        "retCall",
        "retConversion",
    ] {
        assert!(
            messages
                .iter()
                .any(|m| m.contains(&format!(r#"{func} - key always receives "K""#))),
            "{func}: not a stub, so its parameter is reported: {messages:?}"
        );
    }
    // The whole set, not a floor: the eleven shapes in `ok.go` that *are* stubs
    // are asserted by `unparam_allows_used_and_intentional_keep` being empty,
    // and this pins that nothing else crept in.
    assert_eq!(messages.len(), UNPARAM_BAD_KEYS, "{messages:?}");
}

/// Every diagnostic `testdata/unparam/bad.go` must produce — the same set
/// `compat/golden/cases/unparam` pins against golangci-lint.
const UNPARAM_BAD_KEYS: usize = 39;

#[test]
fn unparam_reads_variadic_parameters_and_func_literals() {
    // `bad.go` has neither a variadic parameter nor a func literal, so two
    // whole families went unmeasured.
    //
    // Variadic: go/ssa packs the tail into a slice, so upstream's
    // `call.Args[pos]` is a nil constant exactly when no caller fills it and an
    // `*ssa.Slice` — never constant — as soon as one does. guff does not build
    // that slice (`guff_ssa::builder::call`), and skipped the parameter
    // outright rather than reading the same answer off the argument count and
    // the `ellipsis` flag; `count always receives nil` was unreachable.
    //
    // Literals: `checkFunc` runs the result families over them too, which guff
    // never did, and `signRequiredBy` pins a literal's signature only when the
    // value can be followed back to its function. An assignment to a plain
    // local is not one of those ways — guff's syntactic stand-in treated it as
    // one, so a literal held in a variable was never checked at all.
    let pkg = support::typecheck_fixture("unparam", "example.com/unparam/literal", "literal.go");
    let messages = support::run_analyzer(unparam(), &pkg);
    assert_eq!(messages.len(), 14, "{messages:?}");

    // Four variadic parameters that no caller ever fills, one of them reached
    // through `nil...` and one of them a method (whose SSA argument list
    // carries the receiver and whose AST one does not).
    for want in [
        "neverGiven - count always receives nil",
        "spreadNil - count always receives nil",
        "onlyVariadic - count always receives nil",
        "(*box).tagged - count always receives nil",
    ] {
        assert!(messages.iter().any(|m| m == want), "{want}: {messages:?}");
    }
    // The three that *are* filled say nothing about their variadic parameter.
    for quiet in ["alwaysGiven - count", "spread - count", "mixedGiven - count"] {
        assert!(
            !messages.iter().any(|m| m.starts_with(quiet)),
            "{quiet}: {messages:?}"
        );
    }
    // Two of the fourteen literals are checkable: the capturing one held in a
    // cell another closure captures, and the plain immediately-invoked one.
    // `litDeadIIFE` is the same literal as `litLiveIIFE` in a statement nothing
    // reaches: go/ssa never builds it, so upstream criticises neither its
    // parameters nor its results — not even the unused one that its live twin
    // is reported for. guff builds it regardless, and reported it.
    let mut lits: Vec<&String> = messages.iter().filter(|m| m.contains("$1 -")).collect();
    lits.sort();
    assert_eq!(
        lits,
        vec![
            "litCapturedFreeVar$1 - result 0 (error) is always nil",
            "litLiveIIFE$1 - unused is unused",
        ],
        "{messages:?}"
    );
}

#[test]
fn unparam_allows_used_and_intentional_keep() {
    let pkg = support::typecheck_fixture("unparam", "example.com/unparam/ok", "ok.go");
    assert!(support::run_analyzer(unparam(), &pkg).is_empty());
}

#[test]
fn unparam_respects_check_exported() {
    use guff_style::UnparamOptions;

    let pkg =
        support::typecheck_fixture("unparam", "example.com/unparam/settings", "settings.go");
    assert!(
        support::run_analyzer(unparam(), &pkg).is_empty(),
        "exported func skipped when check-exported is false"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "unparam",
        UnparamOptions {
            check_exported: true,
        },
    );
    let messages = support::run_analyzer_with_settings(
        unparam(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.iter().any(|m| m.contains("Exported - x is unused")),
        "{messages:?}"
    );
}

#[test]
fn gosmopolitan_flags_han_scripts_and_time_local() {
    let pkg = support::typecheck_fixture("gosmopolitan", "example.com/gosmopolitan", "bad.go");
    let messages = support::run_analyzer(gosmopolitan(), &pkg);
    let han = messages
        .iter()
        .filter(|m| m.contains("string literal contains rune in Han script"))
        .count();
    assert_eq!(han, 2, "{messages:?}");
    assert!(
        messages.iter().any(|m| m == "usage of time.Local"),
        "{messages:?}"
    );
}

#[test]
fn gosmopolitan_allows_ascii_and_utc() {
    let pkg = support::typecheck_fixture("gosmopolitan", "example.com/gosmopolitan/ok", "ok.go");
    let messages = support::run_analyzer(gosmopolitan(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn gosmopolitan_respects_allow_time_local_and_escape_hatches() {
    use guff_style::GosmopolitanOptions;

    let pkg = support::typecheck_fixture(
        "gosmopolitan",
        "example.com/gosmopolitan/settings",
        "settings.go",
    );

    // Default: both the Han literal and time.Local are reported.
    let flagged = support::run_analyzer(gosmopolitan(), &pkg);
    assert!(
        flagged
            .iter()
            .any(|m| m.contains("string literal contains rune in Han script")),
        "{flagged:?}"
    );
    assert!(
        flagged.iter().any(|m| m == "usage of time.Local"),
        "{flagged:?}"
    );

    // With allow-time-local + escape-hatch on i18n.T: both silenced.
    let mut bag = SettingsBag::new();
    bag.insert(
        "gosmopolitan",
        GosmopolitanOptions {
            allow_time_local: true,
            escape_hatches: vec!["i18n.T".into()],
            watch_for_scripts: vec!["Han".into()],
        },
    );
    let messages = support::run_analyzer_with_settings(
        gosmopolitan(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn goheader_flags_missing_header() {
    use guff_style::GoheaderOptions;
    use std::collections::HashMap;

    let pkg = support::typecheck_fixture("goheader", "example.com/goheader", "bad.go");
    let mut bag = SettingsBag::new();
    let mut const_values = HashMap::new();
    const_values.insert("COMPANY".into(), "Example Corp".into());
    const_values.insert("YEAR".into(), "2020".into());
    bag.insert(
        "goheader",
        GoheaderOptions {
            template: "Copyright {{ YEAR }} {{ COMPANY }}\nSPDX-License-Identifier: Apache-2.0"
                .into(),
            template_path: String::new(),
            const_values,
            regexp_values: HashMap::new(),
        },
    );
    let messages = support::run_analyzer_with_settings(
        goheader(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.iter().any(|m| m == "Missed header for check"),
        "{messages:?}"
    );
}

#[test]
fn goheader_allows_matching_header() {
    use guff_style::GoheaderOptions;
    use std::collections::HashMap;

    let pkg = support::typecheck_fixture("goheader", "example.com/goheader/ok", "ok.go");
    let mut bag = SettingsBag::new();
    let mut const_values = HashMap::new();
    const_values.insert("COMPANY".into(), "Example Corp".into());
    const_values.insert("YEAR".into(), "2020".into());
    bag.insert(
        "goheader",
        GoheaderOptions {
            template: "Copyright {{ YEAR }} {{ COMPANY }}\nSPDX-License-Identifier: Apache-2.0"
                .into(),
            template_path: String::new(),
            const_values,
            regexp_values: HashMap::new(),
        },
    );
    let messages = support::run_analyzer_with_settings(
        goheader(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn goheader_flags_mismatching_header() {
    use guff_style::GoheaderOptions;
    use std::collections::HashMap;

    let pkg =
        support::typecheck_fixture("goheader", "example.com/goheader/mismatch", "mismatch.go");
    let mut bag = SettingsBag::new();
    let mut const_values = HashMap::new();
    const_values.insert("COMPANY".into(), "Example Corp".into());
    const_values.insert("YEAR".into(), "2020".into());
    bag.insert(
        "goheader",
        GoheaderOptions {
            template: "Copyright {{ YEAR }} {{ COMPANY }}\nSPDX-License-Identifier: Apache-2.0"
                .into(),
            template_path: String::new(),
            const_values,
            regexp_values: HashMap::new(),
        },
    );
    let messages = support::run_analyzer_with_settings(
        goheader(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    // golangci-lint 2.12.2 on this fixture:
    //   mismatch.go:1:14: Expected:2020, Actual: 2019 Wrong Corp
    assert!(
        messages
            .iter()
            .any(|m| m == "Expected:2020, Actual: 2019 Wrong Corp"),
        "{messages:?}"
    );
}

#[test]
fn protogetter_flags_direct_proto_field_reads() {
    let pkg = support::typecheck_fixture("protogetter", "example.com/protogetter", "bad.go");
    let messages = support::run_analyzer(protogetter(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m == "avoid direct access to proto field u.Name, use u.GetName() instead"),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m == "avoid direct access to proto field u.Age, use u.GetAge() instead"),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m
            == "avoid direct access to proto field u.Address.City, use u.GetAddress().GetCity() instead"),
        "{messages:?}"
    );
}

#[test]
fn protogetter_ignores_getters_writes_and_non_proto() {
    let pkg = support::typecheck_fixture("protogetter", "example.com/protogetter/ok", "ok.go");
    let messages = support::run_analyzer(protogetter(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

/// `msg.Field == nil` is filtered when `GetField` returns a non-pointer.
///
/// Upstream's `BinaryExpr` arm: for a `==`/`!=` against `nil` on a proto
/// message field, it asks whether the getter's first result is a pointer. If it
/// is not — a map, a slice — the getter answers the question identically, so
/// there is nothing to rewrite and the position is filtered. If it *is* a
/// pointer, nothing is filtered and the ordinary selector rule reports it.
///
/// The filter is keyed on the **left** operand's position, so `nil == m.Field`
/// filters the `nil` and the field is still reported. dapr writes 80 of the
/// common spelling (`if req.Metadata == nil`).
#[test]
fn protogetter_nil_comparison_follows_the_getter_result_type() {
    let ok = support::typecheck_fixture("protogetter", "example.com/protogetter/ok", "ok.go");
    let ok_messages = support::run_analyzer(protogetter(), &ok);
    assert!(
        ok_messages.is_empty(),
        "a non-pointer getter's nil comparison is filtered: {ok_messages:?}"
    );

    let bad = support::typecheck_fixture("protogetter", "example.com/protogetter", "bad.go");
    let bad_messages = support::run_analyzer(protogetter(), &bad);
    assert!(
        bad_messages.iter().any(|m| m.contains("u.Address")),
        "a pointer getter's nil comparison is still reported: {bad_messages:?}"
    );
    assert!(
        bad_messages.iter().any(|m| m.contains("u.Meta")),
        "`nil == u.Meta` filters the nil, not the field: {bad_messages:?}"
    );
}

#[test]
fn unqueryvet_flags_select_star() {
    let pkg = support::typecheck_fixture("unqueryvet", "example.com/unqueryvet", "bad.go");
    let messages = support::run_analyzer(unqueryvet(), &pkg);
    let select_star: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("avoid SELECT * - explicitly specify needed columns"))
        .collect();
    assert!(
        select_star.len() >= 3,
        "expected ≥3 SELECT * hits: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("avoid SELECT alias.*")),
        "{messages:?}"
    );
}

#[test]
fn unqueryvet_allows_explicit_and_default_patterns() {
    let pkg = support::typecheck_fixture("unqueryvet", "example.com/unqueryvet/ok", "ok.go");
    let messages = support::run_analyzer(unqueryvet(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn unqueryvet_respects_settings() {
    use guff_style::UnqueryvetOptions;

    let pkg = support::typecheck_fixture(
        "unqueryvet",
        "example.com/unqueryvet/settings",
        "settings.go",
    );

    let default_msgs = support::run_analyzer(unqueryvet(), &pkg);
    assert!(
        default_msgs
            .iter()
            .any(|m| m.contains("avoid SELECT alias.*")),
        "{default_msgs:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "unqueryvet",
        UnqueryvetOptions {
            check_aliased_wildcard: false,
            check_subqueries: true,
            allowed_patterns: Vec::new(),
        },
    );
    let flagged = support::run_analyzer_with_settings(
        unqueryvet(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        flagged.is_empty(),
        "aliased wildcard should be disabled: {flagged:?}"
    );
}

#[test]
fn promlinter_flags_bad_metric_names() {
    let pkg = support::typecheck_fixture("promlinter", "example.com/promlinter", "bad.go");
    let messages = support::run_analyzer(promlinter(), &pkg);
    for needle in [
        "_total",
        "no help text",
        "snake_case",
        "non-counter metrics should not have",
        "use base unit",
    ] {
        assert!(
            messages.iter().any(|m| m.contains(needle)),
            "missing {needle:?} in {messages:?}"
        );
    }
    assert!(
        messages.iter().any(|m| m.starts_with("Metric:")),
        "{messages:?}"
    );
}

/// `MetricTypeInName` reports *every* metric type name the metric name carries,
/// not just the metric's own type — golangci-lint builds promlinter v0.3.0
/// against client_golang v1.12.1, whose rule ranges over all of
/// `dto.MetricType_name` and skips only `UNTYPED`. A newer client_golang
/// checkout says the opposite, which is the rule guff had: syncthing's gauge
/// `syncthing_model_folder_summary` then went unreported.
#[test]
fn promlinter_reports_every_type_name_in_a_metric_name() {
    let pkg = support::typecheck_fixture(
        "promlinter",
        "example.com/promlinter/typeinname",
        "typeinname.go",
    );
    let messages = support::run_analyzer(promlinter(), &pkg);
    let mut type_msgs: Vec<&String> = messages
        .iter()
        .filter(|m| m.contains("should not include type"))
        .collect();
    type_msgs.sort();
    // Counted: seven metrics, four of them named after a type and one of those
    // carrying two type names. `any(contains(…))` passes with the three silent
    // shapes reported as well.
    assert_eq!(
        type_msgs
            .iter()
            .map(|m| m.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Metric: folder_summary Error: metric name should not include type 'summary'",
            "Metric: queue_counter_gauge Error: metric name should not include type 'counter'",
            "Metric: queue_counter_gauge Error: metric name should not include type 'gauge'",
            "Metric: queue_gauge Error: metric name should not include type 'gauge'",
            "Metric: queue_histogram_depth Error: metric name should not include type 'histogram'",
        ],
        "{messages:?}"
    );
}

#[test]
fn promlinter_allows_good_metrics() {
    let pkg = support::typecheck_fixture("promlinter", "example.com/promlinter/ok", "ok.go");
    let messages = support::run_analyzer(promlinter(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn promlinter_respects_disabled_linters() {
    use guff_style::PromlinterOptions;

    let pkg = support::typecheck_fixture(
        "promlinter",
        "example.com/promlinter/settings",
        "settings.go",
    );

    let default_msgs = support::run_analyzer(promlinter(), &pkg);
    assert!(
        default_msgs
            .iter()
            .any(|m| m.contains("counter metrics should have")),
        "{default_msgs:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "promlinter",
        PromlinterOptions {
            strict: false,
            disabled_linters: vec!["Counter".to_string()],
        },
    );
    let flagged = support::run_analyzer_with_settings(
        promlinter(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        flagged.is_empty(),
        "Counter check should be disabled: {flagged:?}"
    );
}

#[test]
fn ginkgolinter_flags_common_assertion_mistakes() {
    let pkg = support::typecheck_fixture("ginkgolinter", "example.com/ginkgolinter", "bad.go");
    let messages = support::run_analyzer(ginkgolinter(), &pkg);
    for needle in [
        "wrong length assertion",
        "wrong nil assertion",
        "wrong boolean assertion",
        "missing assertion method",
    ] {
        assert!(
            messages.iter().any(|m| m.contains(needle)),
            "missing {needle:?} in {messages:?}"
        );
    }
    assert!(
        messages.iter().filter(|m| m.contains("wrong length")).count() >= 4,
        "expected ≥4 length reports: {messages:?}"
    );
    // Focus containers are opt-in.
    assert!(
        !messages.iter().any(|m| m.contains("Focus container")),
        "{messages:?}"
    );
}

#[test]
fn ginkgolinter_allows_idiomatic_assertions() {
    let pkg = support::typecheck_fixture("ginkgolinter", "example.com/ginkgolinter/ok", "ok.go");
    let messages = support::run_analyzer(ginkgolinter(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn ginkgolinter_respects_settings() {
    use guff_style::GinkgolinterOptions;

    let pkg = support::typecheck_fixture(
        "ginkgolinter",
        "example.com/ginkgolinter/settings",
        "settings.go",
    );

    let default_msgs = support::run_analyzer(ginkgolinter(), &pkg);
    assert!(
        default_msgs
            .iter()
            .any(|m| m.contains("wrong length assertion")),
        "{default_msgs:?}"
    );
    assert!(
        !default_msgs
            .iter()
            .any(|m| m.contains("Focus container")),
        "{default_msgs:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "ginkgolinter",
        GinkgolinterOptions {
            suppress_len_assertion: true,
            allow_havelen_zero: true,
            forbid_focus_container: true,
            force_expect_to: true,
            ..GinkgolinterOptions::default()
        },
    );
    let flagged = support::run_analyzer_with_settings(
        ginkgolinter(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !flagged.iter().any(|m| m.contains("wrong length")),
        "len checks should be suppressed: {flagged:?}"
    );
    assert!(
        flagged.iter().any(|m| m.contains("Focus container")),
        "{flagged:?}"
    );
    assert!(
        flagged
            .iter()
            .any(|m| m.contains("must not use Expect with Should")),
        "{flagged:?}"
    );
}

#[test]
fn wastedassign_flags_unused_local_assignments() {
    let pkg = support::typecheck_fixture("wastedassign", "example.com/wastedassign", "bad.go");
    let messages = support::run_analyzer(wastedassign(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("assigned to a, but never used afterwards")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("assigned to b, but reassigned without using the value")),
        "{messages:?}"
    );
}

/// A cell whose address is taken is heap-allocated by go/ssa and dropped from
/// `Function.Locals` in `finishBody`; wastedassign only walks `Locals`, so no
/// store to such a cell is a finding however dead it looks. syncthing
/// `cmd/syncthing/perfstats_unix.go` was one over-report of this shape
/// (`runtime.ReadMemStats(&prevMem)`, then `prevMem = curMem` at the tail of
/// the loop, never read).
#[test]
fn wastedassign_ignores_cells_whose_address_escapes() {
    let pkg = support::typecheck_fixture("wastedassign", "example.com/wastedassign", "escapes.go");
    let messages = support::run_analyzer(wastedassign(), &pkg);
    // Counted: the fixture has eight functions with a store that looks dead and
    // only the two controls may be reported. `any(contains(…))` would pass with
    // all six escaping shapes reported as well.
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m.contains("assigned to x, but reassigned without using the value")
                || m.contains("assigned to y, but reassigned without using the value")),
        "only the two controls are reported: {messages:?}"
    );
}

#[test]
fn wastedassign_allows_used_assignments() {
    let pkg = support::typecheck_fixture("wastedassign", "example.com/wastedassign/ok", "ok.go");
    let messages = support::run_analyzer(wastedassign(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn zerologlint_flags_undispatched_events() {
    let pkg = support::typecheck_fixture("zerologlint", "example.com/zerologlint", "bad.go");
    let messages = support::run_analyzer(zerologlint(), &pkg);
    let undispatched = messages
        .iter()
        .filter(|m| m.contains("must be dispatched by Msg or Send method"))
        .count();
    assert!(undispatched >= 4, "expected multiple reports, got {undispatched}: {messages:?}");
}

#[test]
fn zerologlint_allows_dispatched_events() {
    let pkg = support::typecheck_fixture("zerologlint", "example.com/zerologlint/ok", "ok.go");
    let messages = support::run_analyzer(zerologlint(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}
#[test]
fn gocritic_selector_keys_follow_infer_enabled_checks() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GocriticOptions;

    // The fixture holds one finding per tag class; compat/golden/cases/
    // gocritic-* runs the same bytes through golangci-lint.
    let pkg = support::typecheck_fixture("gocritic", "example.com/gocritic/settings", "settings.go");
    let run = |opts: GocriticOptions| -> Vec<String> {
        let mut bag = SettingsBag::new();
        bag.insert("gocritic", opts);
        let mut names: Vec<String> = support::run_analyzer_with_settings(
            gocritic(),
            &pkg,
            &RunnerOptions {
                settings: Arc::new(bag),
                ..RunnerOptions::default()
            },
        )
        .into_iter()
        // The checker name is the message's prefix up to the colon.
        .map(|m| m.split(':').next().unwrap_or("").to_string())
        .collect();
        names.sort();
        names
    };

    // Default: the checkers carrying none of the four opt-in tags.
    assert_eq!(
        run(GocriticOptions::default()),
        vec!["appendAssign", "singleCaseSwitch"]
    );

    // `enabled-tags` is a **union** with the default set, not a filter on it.
    // Read as a filter this is empty: no default-on checker carries an opt-in
    // tag.
    assert_eq!(
        run(GocriticOptions {
            enabled_tags: vec!["performance".into()],
            ..GocriticOptions::default()
        }),
        vec!["appendAssign", "rangeValCopy", "singleCaseSwitch"]
    );

    // `disabled-tags` runs *after* `enabled-checks`, so naming a checker and
    // then disabling its tag leaves it off.
    assert_eq!(
        run(GocriticOptions {
            enabled_checks: vec!["paramTypeCombine".into()],
            disabled_tags: vec!["opinionated".into()],
            ..GocriticOptions::default()
        }),
        vec!["appendAssign", "singleCaseSwitch"]
    );

    // `disable-all` revokes the default set; `enabled-checks` is then the only
    // thing left running.
    assert_eq!(
        run(GocriticOptions {
            disable_all: true,
            enabled_checks: vec!["rangeValCopy".into()],
            ..GocriticOptions::default()
        }),
        vec!["rangeValCopy"]
    );
}
