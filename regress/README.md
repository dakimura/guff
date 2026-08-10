# Prometheus regression harness (local)

Gate **guff** against a checked-in baseline on a local [Prometheus](https://github.com/prometheus/prometheus)
checkout, using **prometheus’s own** `.golangci.yml` (not `compat/standard.yml`).

Tracks three things and fails only when they get **worse** than the profile baseline:

1. **Wall clock** (cold guff cache / `--no-cache`)
2. **Peak RSS** (`/usr/bin/time` + live RSS kill limit)
3. **Finding-set vs golangci-lint** (`guff_only` / `golangci_only` must not grow; `both` must not shrink)

Absolute parity with golangci-lint is **not** required. On the default package set,
golangci-lint may report far fewer (even zero) findings than guff under the same
prometheus config — the gate still fails if `guff_only` grows or `both` shrinks.

### Known remaining diffs (prometheus `./...`)

| Source | Notes |
|--------|--------|
| **finding-set / display** | Full `./...` matches golangci-lint 2.12 on path+Text (`both=20`, `guff_only=0`, `golangci_only=0`): modernize 16 + govet `inline` 4. Default `output.path-mode: rel`. |
| **gofumpt** | `guff run` format checks set `match_golangci` (omit gofumpt ≥v0.10 rules) so diagnostics match golangci-lint’s embedded `mvdan.cc/gofumpt@v0.9.2`. Native `guff fmt` still applies latest v0.10 rules. Override with `GUFF_GOFUMPT_MATCH_GOLANGCI=0` or pin a binary via `GUFF_GOFUMPT_BIN`. |
| **staticcheck SA5011** | `:0` / empty-path diagnostics are suppressed; remaining SSA position gaps DEFERRED. |

## Profiles

| Profile | Packages | Baseline | RSS kill | Notes |
|---------|----------|----------|----------|-------|
| `tsdb` (default) | `./tsdb/...` | `baseline.json` | 12 GiB | Fast local gate on ~24GB laptops |
| `full` | `./...` | `baseline.full.json` | 18 GiB | Whole-repo gate; needs warm system `GOCACHE` |

```bash
./regress/run.sh                          # tsdb
./regress/run.sh --profile full           # full ./...
./regress/run.sh --profile full --update-baseline
```

Shared knobs (override profile defaults):

| Knob | Default | Why |
|------|---------|-----|
| `-j` / `REGRESS_JOBS` | auto (omit) | Use `available_parallelism`; pin `1` if RSS climbs |
| `RAYON_NUM_THREADS` | auto (omit) | Same; pin `2` on tight hosts |
| `GOCACHE` | system (warm) | Empty GOCACHE recompiles deps and blows RAM |
| `REGRESS_PACKAGES` | profile default | Override package set without changing profile files |

Cold `GOCACHE` + high concurrency historically peaked **>40GB**. With a warm
system cache and hybrid default, `full` currently gates at **~2.33s / ~2.73 GiB**
peak (`baseline.full.json`). Further memory reduction is still desirable on 24GB
laptops; the profile exists so we can gate regressions while chasing it.
`REGRESS_ISOLATE_GOCACHE=1` is available but **not** recommended under 64GB RAM.

### Baseline history (`full`)

| Date | wall | peak RSS | Why |
|------|-----:|---------:|-----|
| — | 1.890s | 2,501,869,568 | **Invalid.** Measured while 57 of prometheus's 118 packages were ill-typed and therefore skipped by every analyzer without `run_despite_errors`. |
| 2026-08 | 2.330s | 2,932,523,008 | First baseline measured with the whole corpus actually analyzed. |
| 2026-08-11 | 2.360s | 3,114,582,016 | **Same failure mode as the 1.890s row, one layer down.** `7edba5f` fixed eight type-checker false positives, taking prometheus from 14 ill-typed packages to 8; the 2.330s was measured while `promql/parser`, `scrape`, `tsdb/chunks`, `tsdb/encoding`, `util/zeropool` and `web/api/v1` were still being skipped whole. |

The old number was not a faster guff. `dep_graph` was keyed by package id, so
after `filter_duplicate_packages` renamed a package to `P [P.test]` its
dependents could not resolve `P` and were type-checked ahead of it. The
resulting `Invalid` types marked 57 packages ill-typed, and analyzers silently
skipped them. Keying the graph by import path (`import_path_dep_graph`) made 43
of those packages well-typed, so the analysis suite now runs on roughly a third
more code — the whole `+0.44s / +431 MB` is that extra work, not new overhead.
Per-package seed and type-arena sizes were measured as unchanged across the two
keyings.

Roughly 0.15s of the increase was clawed back first by removing quadratic
lookups in SA4006 / SA4031 (`ExprValueIndex`, `IdentIndex`); the remainder is
spread evenly over ~40 analyzers with no remaining hotspot, so it was accepted
rather than optimized further.

**The `compat` block was never rebaselined**, and it did not have to be. Correct
type info also exposed four pre-existing analyzer false positives (QF1008 ×2,
S1021, SA5011), so the run reported `guff_only=4` against a baseline that still
said `0`; the gate was left failing on purpose rather than accepting them. All
four are now fixed and `guff_only` is back to `0` against the unchanged block:

| FP | Upstream behaviour guff was missing |
|----|------------------------------------|
| **QF1008** ×2 (`discovery/kubernetes/endpointslice_test.go:364,365`) | `extractSelectors` resolves the enclosing path to the file and bails unless the *outermost* `SelectorExpr` on it is the visited one, so a selector nested anywhere inside another selector's operand — here `k8sDiscoveryTest{afterStart: func() { obj.ObjectMeta.Labels = nil }}.Run(t)` — is skipped. guff only checked the immediate parent. |
| **S1021** (`discovery/kubernetes/kubernetes.go:707`) | `hasMultipleAssignments` is a full `ast.Inspect` of the block; guff's hand-rolled statement walker did not descend into `select`, so it missed the second `err = f()` in `retryOnError` and offered a merge that changes behaviour. Report position also moved to the `var` keyword, as upstream reports on the `DeclStmt`. |
| **SA5011** (`web/web_test.go:738`) | honnef's `ir` is SSI: `if resp != nil { … }` gives the branch successor a sigma node and the join below a phi, so a later `resp.StatusCode` is a *different* `ir.Value` than the one `maybeNil` recorded. guff's SSA has no sigma nodes, so `sigma_shadows` now reconstructs which derefs that renaming hides. |

Chasing those exposed two QF1008 false *negatives*, fixed with them:

- `pkg: None` was passed to `lookup_field_or_method`, so **unexported** embedded
  fields never resolved (`r.meta.Labels` was silently missed). This was the
  `golangci-only` entry in `compat/isolate/allowlists/isolate-staticcheck.txt`.
- Only the last segment of a chain interrupted by a call or index was checked,
  missing e.g. `call.Inner.F8().ContinuedInner.F9`'s first segment. golangci-lint's
  default `issues.uniq-by-line: true` hides the second finding on such a line,
  which is why comparing raw output made the miss look intentional — verify
  staticcheck ports against `uniq-by-line: false`.

With both allowlist entries gone, `isolate-staticcheck` is
`guff=11 golangci=11 both=11 P=R=100%`. Known remaining SA5011 gap (not on this
corpus, so it does not move the gate): a deref after `if p == nil { … }` whose nil
branch never mentions `p` is upstream-reportable but suppressed by guff's
`dominance_guard` heuristic.

The three fixes are wall- and RSS-neutral. Measured by interleaving pre- and
post-fix binaries (`GUFF_BIN=… --skip-golangci`, 4 rounds each, first round
discarded as cold): before `2.32 / 2.25 / 2.23s`, after `2.27 / 2.26 / 2.26s`,
peak RSS within 10 MB. `sigma_shadows` walks each nil-check successor's dominator
subtree once per check in `collect_maybe_nil` rather than per deref, so it cannot
go quadratic on functions with many checks.

> A single measurement on a busy host read `2.47s` — inside the 0.15s epsilon but
> only just. `PERF_GUARD` (load > ncpu/4) catches the worst of it; anything within
> ~0.2s of the limit still deserves an interleaved A/B before it is called a
> regression.

### "It is the host" is a claim you have to measure

The `full` gate then sat red from 2026-08-07 to 2026-08-11 — ten sessions, each
concluding the host was busy and the baseline needed retaking, `7edba5f`'s own
commit message included ("clean HEAD measures 2.71-3.10s under the same
conditions, so it is the host, not this change"). None of them checked, and the
claim was wrong: on a quiet machine `4d345bb` — the commit that locked 2.330s —
measures **2.23-2.26s**, i.e. *faster* than when its own baseline was taken. The
host had not degraded at all.

The check costs three minutes and is worth running before blaming a machine:

```bash
git worktree add /tmp/base <commit-that-locked-the-baseline>
(cd /tmp/base && cargo build --release -p guff-lint)     # ~2 min
# interleave, never batch: A B A B A B
for r in 1 2 3; do
  for b in /tmp/base/target/release/guff ./target/release/guff; do
    GUFF_BIN=$b ./regress/run.sh --profile full --skip-golangci | grep -o 'wall=[0-9.]*s'
  done
done
```

If the old binary reproduces its baseline, the regression is real and lives in
the commits since — bisect them the same way (each step is one build plus three
runs). That bisect put the whole 2.24s → 2.46s step on a single commit.

**Rebaseline only with a reason.** A larger number that buys margin is a gate
that has stopped detecting things. Both increases in the table above are the
same reason — guff was analyzing code it had previously skipped — and neither is
overhead to optimize away.

## Prerequisites

- `cargo build --release -p guff-lint`
- `golangci-lint` on `PATH`
- `go`, `python3`, `/usr/bin/time`
- A prometheus checkout via either:
  - repo-root `prometheus/` symlink, or
  - `PROMETHEUS_DIR=/path/to/prometheus`

The harness does **not** clone prometheus. Local-only (no CI workflow).

## Quick start

```bash
cargo build --release -p guff-lint

# First time (or after intentional corpus / metric changes):
./regress/run.sh --update-baseline
./regress/run.sh --profile full --update-baseline

# Ongoing gate:
./regress/run.sh
./regress/run.sh --profile full

# Offline unit tests (no prometheus needed):
python3 -m unittest discover -s regress/tests
```

## Layout

| Path | Role |
|------|------|
| `run.sh` | Main entry (`--profile tsdb|full`) |
| `measure.py` | `/usr/bin/time` wrapper + RSS/wall parser + RSS watchdog |
| `gate.py` | Baseline compare / update |
| `fmt_diff.py` | Task 1a: byte-diff native fmt vs gofmt/gofumpt/goimports/gci |
| `baseline.json` | Checked-in thresholds (`tsdb` profile) |
| `baseline.full.json` | Checked-in thresholds (`full` profile) |
| `tests/` | Offline unit tests |
| `results/` | Per-run artifacts (`RESULTS.md` / `RESULTS.full.md`) |

### Formatter byte-diff harness (PERF_TASKS Task 1)

```bash
cargo build --release -p guff-fmt --bin guff-fmt-native
# Reference tool smoke (no native needed):
./regress/fmt_diff.py --formatter gofmt --self-check --corpus prometheus --limit 100
# Native vs reference (exit 3 while Task 1b–1e stubs remain):
./regress/fmt_diff.py --formatter gofmt --corpus both
./regress/fmt_diff.py --formatter gofumpt --extra --corpus prometheus
```

Reuse: [`compat/normalize.py`](../compat/normalize.py) for JSON → `relpath:line:linter:message` keys.

## Tolerances

Default (in `baseline.json` → `tolerances`):

| Key | Default | Meaning |
|-----|--------:|---------|
| `wall_seconds_ratio` | 1.0 | Fail if wall > baseline × ratio + epsilon |
| `wall_seconds_epsilon` | 0.15 | Absolute seconds of measurement noise allowed |
| `peak_rss_ratio` | 1.20 | Fail if peak RSS > baseline × ratio |
| `max_guff_only_delta` | 0 | Fail if `guff_only` increases |
| `max_golangci_only_delta` | 0 | Fail if `golangci_only` increases |
| `min_both_delta` | 0 | Fail if `both` decreases |

Improvements always pass. Package-set / SHA drift warns but does not fail — re-run
`--update-baseline` when intentional.

`--update-baseline` rewrites the `compat` block too, so it accepts whatever
`guff_only` the run reported. Use it for perf/corpus drift only; for a finding-set
change, edit the block by hand so the acceptance is visible in the diff.

## Env

| Var | Role |
|-----|------|
| `GUFF_BIN` | Override guff binary |
| `GOLANGCI_LINT_BIN` | Override golangci-lint |
| `PROMETHEUS_DIR` | Override corpus path |
| `REGRESS_PROFILE` | `tsdb` or `full` (same as `--profile`) |
| `REGRESS_PACKAGES` | Package patterns (override profile default) |
| `REGRESS_JOBS` | guff `-j` (default: omit → available_parallelism) |
| `REGRESS_RAYON_THREADS` | `RAYON_NUM_THREADS` (default: omit → rayon default) |
| `REGRESS_ISOLATE_GOCACHE` | `1` = fresh empty GOCACHE (memory-heavy) |
| `REGRESS_RSS_LIMIT_BYTES` | Live RSS kill limit (profile default 12 / 18 GiB) |
| `REGRESS_TIMEOUT` | CLI timeout (default `15m`) |
