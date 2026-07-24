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
system cache and hybrid default, `full` currently gates at **~57s / ~10.7 GiB**
peak (`baseline.full.json`) after dropping double-held dependency ASTs in the
hybrid seed. Further memory reduction is still desirable on 24GB laptops; the
profile exists so we can gate regressions while chasing it.
`REGRESS_ISOLATE_GOCACHE=1` is available but **not** recommended under 64GB RAM.

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
