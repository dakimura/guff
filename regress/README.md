# Prometheus regression harness (local, 24GB-safe)

Gate **guff** against a checked-in baseline on a local [Prometheus](https://github.com/prometheus/prometheus)
checkout, using **prometheus’s own** `.golangci.yml` (not `compat/standard.yml`).

Tracks three things and fails only when they get **worse** than `baseline.json`:

1. **Wall clock** (cold guff cache / `--no-cache`)
2. **Peak RSS** (`/usr/bin/time` + live RSS kill limit)
3. **Finding-set vs golangci-lint** (`guff_only` / `golangci_only` must not grow; `both` must not shrink)

Absolute parity with golangci-lint is **not** required. On the default package set,
golangci-lint may report far fewer (even zero) findings than guff under the same
prometheus config — the gate still fails if `guff_only` grows or `both` shrinks.

### Known remaining diffs (prometheus `./tsdb/...`)

| Source | Notes |
|--------|--------|
| **gofumpt** | guff shells out to system `gofumpt` (often newer than golangci’s embedded `mvdan.cc/gofumpt`). e.g. v0.10.0 flags files that golangci’s v0.9.2 leaves alone. Pin `gofumpt@v0.9.2` for closer parity, or treat formatter noise as expected. |
| **staticcheck SA5011** | Rare `:0` / empty-path diagnostics from unmapped SSA positions — DEFERRED. |

## 24GB-safe defaults

Full `./...` with an empty `GOCACHE` and high concurrency previously peaked **>40GB**
and OOM-killed the host. Defaults therefore target a ~24GB laptop:

| Knob | Default | Why |
|------|---------|-----|
| `REGRESS_PACKAGES` | `./tsdb/...` | R25 scale subtree; fits ~24GB with the other caps |
| `-j` / `REGRESS_JOBS` | auto (omit) | Use `available_parallelism`; pin `1` if RSS climbs |
| `RAYON_NUM_THREADS` | auto (omit) | Same; pin `2` on tight hosts |
| `GOCACHE` | system (warm) | Empty GOCACHE recompiles deps and blows RAM |
| `REGRESS_RSS_LIMIT_BYTES` | 12 GiB | Live kill before OS jetsams Cursor |

On a large machine you may widen the corpus:

```bash
REGRESS_PACKAGES='./...' REGRESS_RSS_LIMIT_BYTES=$((40*1024*1024*1024)) ./regress/run.sh --update-baseline
```

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

# Ongoing gate:
./regress/run.sh

# Offline unit tests (no prometheus needed):
python3 -m unittest discover -s regress/tests
```

## Layout

| Path | Role |
|------|------|
| `run.sh` | Main entry |
| `measure.py` | `/usr/bin/time` wrapper + RSS/wall parser + RSS watchdog |
| `gate.py` | Baseline compare / update |
| `baseline.json` | Checked-in thresholds |
| `tests/` | Offline unit tests |
| `results/` | Per-run artifacts (`RESULTS.md` snapshot) |

Reuse: [`compat/normalize.py`](../compat/normalize.py) for JSON → `relpath:line:linter:message` keys.

## Tolerances

Default (in `baseline.json` → `tolerances`):

| Key | Default | Meaning |
|-----|--------:|---------|
| `wall_seconds_ratio` | 1.25 | Fail if wall > baseline × ratio |
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
| `REGRESS_PACKAGES` | Package patterns (default `./tsdb/...`) |
| `REGRESS_JOBS` | guff `-j` (default: omit → available_parallelism) |
| `REGRESS_RAYON_THREADS` | `RAYON_NUM_THREADS` (default: omit → rayon default) |
| `REGRESS_ISOLATE_GOCACHE` | `1` = fresh empty GOCACHE (memory-heavy) |
| `REGRESS_RSS_LIMIT_BYTES` | Live RSS kill limit (default 12 GiB) |
| `REGRESS_TIMEOUT` | CLI timeout (default `15m`) |
