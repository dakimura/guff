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
./compat/run.sh --oss --tier pr,nightly   # run this before every push (~3 min)
./compat/run.sh --oss --name consul       # one target, no fixture/local warm-up

# Same repos, but every linter enabled (discovery tier — expect diffs)
./compat/run.sh --oss --tier pr --all-linters

# Ad-hoc OSS bug hunt (extra repos in corpus/hunt.json; not a CI gate)
./compat/hunt.sh
./compat/hunt.sh --name cobra

# Refresh allowlists from current diffs (merges; review before committing)
./compat/run.sh --oss --tier pr --update-allowlist
./compat/run.sh --isolate --update-allowlist

# Check-level golden gate (exact match incl. column/severity, no allowlist)
./compat/golden/run.sh
./compat/golden/regen.sh gocritic

# Do both tools analyze the same .go files? (build tags / tests / vendor)
./compat/filesets.sh --tier pr
./compat/filesets.sh --isolate

# Check-level coverage ledger — which checks have never fired in any test?
./compat/coverage.py all

# Input-shape ledger — which *shape of input* has no gated target at all?
./corpus/shapes.py check --offline

# Go-stdlib ground truth for the checks that just call the stdlib (SA100x)
./compat/oracles/regen.sh

# Mutate the golden fixtures and check the two tools still agree
./compat/fuzz.py --case gocritic -n 200

# Shrink a disagreement (or an ill-typed package) to a minimal reproducer
./compat/reduce.py --dir corpus/cache/controller-runtime \
  --config corpus/cache/controller-runtime/.golangci.yml \
  --packages ./pkg/controller/priorityqueue/... \
  --guff-stderr 'does not implement' -o /tmp/reduced

# Did *upstream* change? (pin vs latest release, on the 81 golden cases)
./compat/drift.py
./compat/drift.py --candidate 2.11.4      # or against any specific version
```

> **`run.sh` 系のゲートは「合格しているが、ほとんど何も比較していない」**という測定結果があります。
> isolate は 114 linter で合計 178 findings しか比較しておらず、キーは column も severity も
> 見ていません。計画策定時（2026-08-07）は 548 check のうち 133 がどのテストでも
> 一度も発火していませんでした。これを check 単位で潰していくのが `golden/` で、
> 最初のケース（gocritic）は載せた時点で 44 件のバグを出し、
> 直近の gosec は **52 件中 0 件一致**から始まりました
> （golangci が severity を付ける唯一の linter が gosec で、guff は付けていなかった）。
> 2026-08-12 時点で 547 check 中 `fired` 543 / `unit-only` 1 / `never` 3
> （残る 3 件は §6「恒久的に観測できない」側）。**この数字だけを見ないこと**: check が
> 発火したかと、*どんな形の入力で*発火したかは別問題で、後者は
> [`../corpus/shapes.py`](../corpus/shapes.py) が測る。改善計画は
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
| `all_linters.py` / `allowlists-all/` | `--all-linters` config rewrite + its own (empty) allowlist |
| `isolate/` | Per-linter isolate fixtures + configs ([README](isolate/README.md)) |
| `golden/` | Check-level goldens, exact match, no allowlist ([README](golden/README.md)) |
| `oracles/` | Go-stdlib ground truth for the `gostd` ports ([README](oracles/README.md)) |
| `health.py` | Panic / ill-typed gate — failures that never reach the set-diff |
| `baselines/` | Ill-typed package counts per target (panics are never baselined) |
| `filesets.py` / `filesets.sh` | Do both tools analyze the same `.go` files? |
| `repos.txt` | Deprecated stub — use [`../corpus/repos.json`](../corpus/repos.json) |
| `tests/` | Harness unit tests (`test_normalize.py`, `test_isolate.py`) |
| `results/RESULTS.md` | Latest checked-in report snapshot |
| `results/RESULTS.isolate.md` | Latest isolate report snapshot |
| `coverage.py` | Check-level coverage ledger → [`../docs/COVERAGE.md`](../docs/COVERAGE.md) |
| `coverage/` | Ledger data (`inventory.json` / `observed.json`, committed) |
| `fuzz.py` | Mutate golden fixtures, report where the two tools stop agreeing |
| `reduce.py` | Delta-debug a disagreement down to a minimal reproducer |
| `gospans/` | go/ast helper both of the above use: removable spans, mutation sites |
| `drift.py` | Upstream drift: the pinned golangci-lint vs a newer one, on the goldens |
| `pins.json` | The pinned golangci-lint version, and the two tools it shells out to |
| `drift-ledger.json` | Upstream drift that has been reviewed (written by `drift.py --update`) |

## Fuzzing and minimizing (Phase 6)

These two are a pair, and neither is a CI gate: they are for **finding** bugs
and then making them cheap to read, which the gates above cannot do.

`fuzz.py` mutates a golden fixture — parenthesizing an expression, inserting a
comment, appending `//nolint`, swapping two statements, turning `x := v` into
`var x = v` — and then asks whether guff and golangci-lint still agree on the
mutant. A mutation only has to **compile**; it does not have to preserve
findings, because the comparison is between the two tools on the same input, not
between the mutant and the original. That is what makes the mutations cheap to
write and safe to be aggressive with.

`reduce.py` takes a disagreement — from the fuzzer, from `hunt.sh`, or from an
ill-typed package in a corpus repo — and shrinks it by delta debugging, using
`gospans` so that one edit can be a whole declaration, one interface method, one
composite-literal element, or a function body replaced by `panic(...)`.

The rule that makes the reducer's output trustworthy is that it never accepts an
edit the real Go toolchain rejects:

```
go build (or `go vet`, for cases whose findings are in _test.go) accepts it
        AND guff still misbehaves
```

Without it, minimizing "guff says `Manager has no field or method GetCache`"
converges on deleting `GetCache` — a perfect reproducer of a broken file and no
evidence of a guff bug at all. Pass `--build-cmd 'go vet ./pkg/...'` when the
behaviour lives in test files; `go build` does not type-check them.

Measured on the two the 2026-08-12 session left blocked: controller-runtime's
`pkg/controller/priorityqueue` went from 2.6 MB across 349 files to 4 KB in
107 seconds (775 oracle runs), and the shape that came out was twenty lines.

### Ask what the reproduction is a function of

The first pass is not over files at all. `manager.Manager has no field or method
GetCache` reproduced under `./pkg/...` and not under `./pkg/metrics/filters/...`
— same bytes, same config, different answer — because a package that is a *root*
is loaded differently from the same package as a *dependency*. No amount of
deleting source can express that, and two and a half hours of file-level ddmin
had got 349 files to 155 without ever naming the cause.

So `reduce.py` ddmins the **root package set** before it touches a file:
`./pkg/...` expands to 64 packages and comes back with 3, in minutes. Three
packages is small enough to read directly — and it also shrinks the oracle for
every pass after it. `--no-reduce-roots` turns it off.

The general form is worth keeping: **measure what the reproduction is a function
of before assuming it is the source.**

OSS inventory, tiers, and clone/warm live in [`../corpus/`](../corpus/).

## Which tiers run where

| Gate | Trigger | Targets |
|------|---------|---------|
| `smoke` (+ golden) | every PR and push | fixture, 7 golden cases |
| `isolate` | every PR and push | 114 per-linter fixtures |
| `oss-pr` | every PR and push | gin, caddy, helm |
| `oss-nightly` | **push to main only** | consul, grafana, containerd |

`oss-nightly` exists because the nightly tier used to run nowhere anyone read.
consul carries 255 of the corpus's compared findings, and on 2026-08-09 it was
found with six extra findings that `results/RESULTS.md` had been reporting as
`P = R = 100%` — with no way to date them. A tier that runs on every push to
main dates the next one to a commit.

The tier is not on pull requests (cold GHA corpus, ~30 min), so **run it locally
before pushing**: `./compat/run.sh --oss --tier pr,nightly`.

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
- `run.sh` also gates two failures that never reach the set-diff, because the
  findings were never produced: a panicking analyzer unwinds its worker, and an
  ill-typed package is skipped whole. Panics always fail; ill-typed counts may
  shrink but not grow (`health.py`, `baselines/health.json`). Both were found
  passing at P = R = 100% on all eight OSS targets.
- `filesets.sh` compares the *input*: it runs both tools with a `goheader`
  template that cannot match, so each reports once per analyzed file. Blind
  spot: goheader ignores files whose first comment is a `//go:` directive, so
  those are invisible to the probe on both sides.

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
