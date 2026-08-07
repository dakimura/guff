# Compatibility harness (R21)

Compare **guff** and **golangci-lint** finding sets on the same corpus and
config. Keys are normalized to `relpath:line:linter:message`. Per-linter
precision/recall is reported; known mismatches live under `allowlists/`.
Unexpected diffs fail the run (CI gate).

## Quick start

```bash
cargo build --release -p guff-lint

# CI / offline smoke (fixture only; requires golangci-lint on PATH)
./compat/smoke.sh

# Default: fixture + benchmarks/local (standard.yml)
./compat/run.sh

# Per-linter isolate (one linter enabled at a time — see isolate/)
./compat/run.sh --isolate --smoke
./compat/run.sh --isolate
./compat/run.sh --isolate --linter errcheck

# OSS targets from corpus/repos.json — each repo's real v2 .golangci.yml
./compat/run.sh --oss --tier pr
./compat/run.sh --oss --tier nightly

# Ad-hoc OSS bug hunt (extra repos in corpus/hunt.json; not a CI gate)
./compat/hunt.sh
./compat/hunt.sh --name cobra

# Refresh allowlists from current diffs (merges; review before committing)
./compat/run.sh --oss --tier pr --update-allowlist
./compat/run.sh --isolate --update-allowlist

# Check-level coverage ledger — which checks have never fired in any test?
./compat/coverage.py all
```

> **これらのゲートは「合格しているが、ほとんど何も比較していない」**という測定結果があります。
> isolate は 114 linter で合計 178 findings しか比較しておらず、548 check のうち 222 は
> どのテストでも一度も発火していません。改善計画は
> [`../docs/COMPAT-HARDENING.md`](../docs/COMPAT-HARDENING.md)、現状値は
> [`../docs/COVERAGE.md`](../docs/COVERAGE.md)。

## Layout

| Path | Role |
|------|------|
| `run.sh` | Main harness |
| `smoke.sh` | Fixture-only CI entrypoint |
| `normalize.py` | JSON → keys, diff, markdown/JSON report |
| `standard.yml` | Shared enable-set for fixture/local only |
| `allowlists/` | Per-target accepted diffs (`_default.txt`, `<name>.txt`) |
| `isolate/` | Per-linter isolate fixtures + configs ([README](isolate/README.md)) |
| `repos.txt` | Deprecated stub — use [`../corpus/repos.json`](../corpus/repos.json) |
| `tests/` | Harness unit tests (`test_normalize.py`, `test_isolate.py`) |
| `results/RESULTS.md` | Latest checked-in report snapshot |
| `results/RESULTS.isolate.md` | Latest isolate report snapshot |
| `coverage.py` | Check-level coverage ledger → [`../docs/COVERAGE.md`](../docs/COVERAGE.md) |
| `coverage/` | Ledger data (`inventory.json` / `observed.json`, committed) |

OSS inventory, tiers, and clone/warm live in [`../corpus/`](../corpus/).

## Notes

- golangci-lint is invoked with `--path-mode abs`; both sides are relativized
  to the target module root.
- OSS runs patch configs to `max-issues-per-linter: 0` / `max-same-issues: 0`
  (and pass the same flags to golangci-lint) so identical-message truncation
  cannot rotate finding sets.
- Light message canonicalization covers known errcheck / unused phrasing
  differences; everything else must match or be allowlisted.
- guff diagnostic paths use the full `compiled_go_files` path (not basename)
  so multi-package modules compare cleanly.
- Isolate mode (`--isolate`) enables exactly one linter per fixture; see
  [`isolate/README.md`](isolate/README.md).

## OSS finding-set fixes (2026-08)

Keying the typecheck dep graph by import path (see
[`../regress/README.md`](../regress/README.md)) made a third more code
well-typed, and analyzers that had been silently skipping those packages started
running on them. That exposed eleven pre-existing bugs across `pr`, `nightly`
and `weekly` — all of them guff bugs, none of them allowlisted. All eight OSS
targets are back to `P = R = 100%`.

| Target | Finding | Upstream behaviour guff was missing |
|--------|---------|-------------------------------------|
| helm | `govet` printf on `os.FileMode` | `type_has_method` matched `TypeData::Named` without `unalias_readonly`, so an **alias** (`os.FileMode` = `io/fs.FileMode`) lost its `String()` and `%s` was "wrong type". |
| grafana | `govet` / `SA5009` arg count | `f(format, args...)` passes an opaque slice. Upstream's `argCanBeChecked` bails on the final argument of a spread call, and staticcheck bails when `irutil.Vararg` cannot recover the operands. `CallCommon` now records `ellipsis`. |
| consul, grafana | `SA5011` ×14 after `panic` | `panic(x)` was emitted as an ordinary call, leaving a fallthrough edge, so the non-nil successor no longer dominated the deref. It is now the `Panic` terminator + unreachable block, as in go/ssa. |
| grafana | `SA5011` ×2 across switch cases | Upstream's IR is SSI: a branch renames every live value in each successor it solely precedes. A check and a deref in *different* successor regions are different `ir.Value`s, and SA5011 is pure value identity. `separated_by_branch` models that. |
| grafana | `SA4005` on `s.Frame.Fields[0].Labels = l` | The store target was tested with `refers_to`, which only asks whether the receiver appears *somewhere* in the subtree. A pointer field leaves the receiver's copy, so the write is observable. |
| grafana | `ineffassign` ×5 on `continue walk` | `BranchStack::index_for` ignored the label and always took the innermost loop; the label was also not consumed by the loop carrying it, so nested loops inherited it. |
| grafana | `prealloc` ×7 | Rewritten as a faithful port of alexkohler/prealloc v1.1.0 — see the module docs. The old approximation missed the `hasReturn`/`hasGoto`/`hasBranch` gates, block-nesting levels, chan/`iter.Seq` range bounds, and the package-wide visitor. |
| consul | `SA1026` on `json.MarshalIndent` | Upstream's rule table is `Marshal` + `(*Encoder).Encode` only. |
| consul | `unparam` ×2 | `dummyImpl`: a function whose entry block immediately returns constants is skipped, so `func f(p *T) error { return nil }` never reports its parameters. |
| kubernetes | `gocritic` `dupBranchBody` ×2 | The if-statement text used for branch comparison dropped the init statement, so `if err := f(a); …` and `if err := f(b); …` compared equal. |
| kubernetes | `gocritic` `deprecatedComment` | The message lacked the `deprecatedComment: ` checker prefix golangci-lint emits, so the target's own `exclusions.rules` regex could not match it. (The rest of gocritic was swept the same way — see below.) |
| kubernetes | `QF1010` on `(*testing.B).Fatal` | The `(*log.Logger).Print*` arm matched on method name alone; upstream's pattern names the receiver type. |
| vault | `unused` missing `const bucketCount` | honnef groups const specs with `astutil.GroupSpecs` (consecutive lines), not per `const (…)` block, so a doc comment splits the group and an exported member no longer keeps its neighbour alive. |

## gocritic message sweep (2026-08)

The `deprecatedComment` prefix fix above was the visible tip of a structural
gap: golangci-lint renders every go-critic warning as
`fmt.Sprintf("%s: %s", checkerName, warning)`, and ~150 of guff's ~170 messages
had no prefix. A message without it can only ever be guff-only, and a target's
own `exclusions.rules` regexes — which are written against the prefixed form —
silently stop matching.

The sweep was driven off ground truth rather than reading code: the 104-checker
fixture in `crates/guff-style/tests/testdata/gocritic/` was run through
golangci-lint 2.12 with `gocritic.enable-all` and diffed message-for-message
against guff. That took the fixture from **15/156 to 156/156 exact matches** and
turned up a dozen divergences the prefix gap had been hiding:

| Area | What was wrong |
|------|----------------|
| checker prefix | `report()` now takes the checker name and formats it centrally, so a new check cannot silently omit it. |
| node rendering | Messages embedding an AST node used a hand-rolled renderer that printed `f(...)` for any call and blanks around every operator. Upstream interpolates nodes with `astfmt` (= `go/printer`), so `node_text` now uses guff's gofmt-exact printer: `defer os.Remove(name)`, `*flag.Bool("b", false, "docs")`, `strings.SplitN(s, ",", -1)`, `a < b+1`. |
| ruleguard `$$` | `indexAlloc` / `preferFilepathJoin` / `preferFprint` / `preferStringWriter` emitted the literal text `$$` instead of the matched expression. |
| ruleguard `Suggest` | A rule with `Suggest` and no `Report` renders as `suggestion: <replacement>`. `stringXbytes`, `stringsCompare`, `stringConcatSimplify` and two `preferFprint` arms had invented their own wording. |
| `docStub` | Reported under the `deprecatedComment` name. |
| `exposedSyncMutex` | Reported at the embedded field; the rule matches a whole `type $x struct{…}` declaration, so upstream reports at the `type` keyword — and does not match grouped `type (…)` blocks at all. |
| `unnamedResult`, `tooManyResultsChecker` | Reported at the function *name*; upstream reports at the `func` keyword. |
| `wrapperFunc` | `Type.Is("sync.WaitGroup")` / `Type.Is("bytes.Buffer")` are exact matches, so a `*sync.WaitGroup` receiver is not reported. guff had widened both to pointers. |
| `sprintfQuotedString` | The `%#q` arm is dead upstream: it is a second `m.Match` with the *same* syntax pattern as the first, and ruleguard keeps one rule per pattern. |
| checker ordering | `issues.uniq-by-line` (on by default) keeps the first gocritic issue per line, and go-critic runs checkers in name order — so `tooManyResultsChecker` beats `unnamedResult` on the same line. guff walks all checkers in one pass, so it now sorts pending findings by checker name before reporting. |

## prealloc `for-loops` (2026-08)

`forLoopCount` — the trip count of a three-clause `for` — is ported, so the
non-default `prealloc.for-loops: true` now produces a capacity instead of a
bare "Consider preallocating x". Verified against golangci-lint 2.12 on
`crates/guff-style/tests/testdata/prealloc/forloops.go`, including the
`min(a, b, c)` / `max(n, m)` folding of `&&` / `||` bounds, `n/k + 1` for a
non-unit step, reversed loops, and flipped comparison operands.

## Worker panics on grafana (2026-08)

Two panics fired inside worker threads on `./pkg/...`. Neither changed the
finding set (grafana is `guff=0 golangci=0`), but a panic unwinds the worker and
would drop a package's findings if that package had any.

- **`function literal has a signature type`** — a type-checker bug, not an SSA
  one. `exprInternal`'s `ParenExpr` arm recursed through `expr_internal`
  instead of `raw_expr`, so a parenthesized inner node never got an
  `Info.Types` entry. `(func(input bool) *bool { return &input })(false)` — the
  shape grafana's code generator emits — therefore reached the SSA builder as a
  signature-less function literal, whose parameters could not be declared, so
  `input` looked like a captured free variable and needed a closure type.
- **`expected Chan, got …`** — a `select` receive whose operand typed Invalid.
  The SSA builder now falls back to the Invalid type rather than panicking, as
  `Builder::type_of` already did. The residual type-checker gap is still open:
  a local bound to the result of a method on an inferred generic type
  (`b := newBroadcasterWithSizes(ctx, ch, …); sub, err := b.Subscribe(…)`) comes
  back Invalid, but only at that one call site — a verbatim copy of the same
  function elsewhere in the package types fine, so it looks order-dependent.
