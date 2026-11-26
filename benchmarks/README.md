# Benchmarks (R11)

Wall-clock harness comparing **guff** and **golangci-lint**.

- **fixture / local**: golangci-lint v2 `standard` preset via `standard.yml`
- **OSS (`--oss`)**: each repo's real golangci-lint **v2** config (own-config),
  via [`../corpus/`](../corpus/)

## Quick start

```bash
# Release binary (recommended)
cargo build --release -p guff-lint

# Offline smoke (fixture only)
./benchmarks/smoke.sh

# Default: fixture + synthetic multi-package corpus (`local/`)
./benchmarks/run.sh

# OSS clones from corpus/repos.json
./benchmarks/run.sh --oss --tier pr --quick
./benchmarks/run.sh --oss --tier pr,nightly
```

Requires `go`, `python3`, and (for comparison) `golangci-lint` on `PATH`.
Set `SKIP_GOLANGCI=1` to time guff alone.

## Layout

| Path | Role |
|------|------|
| `run.sh` | Main harness (cold/warm, median summary) |
| `smoke.sh` | CI/manual smoke (`--smoke --quick`) |
| `standard.yml` | Shared enable-set for fixture/local |
| `fixture/` | Tiny module with intentional findings |
| `local/` | ~3k LOC synthetic multi-package corpus |
| `gen_local.sh` | Regenerate `local/` |
| `repos.txt` | Deprecated stub — use [`../corpus/repos.json`](../corpus/repos.json) |
| `results/RESULTS.md` | Checked-in snapshot of a recent run |
| `results/SCOREBOARD.md` | OSS own-config speedup table (refresh after `--oss`) |

Raw `results/*.tsv` / timestamped `.md` are gitignored except `RESULTS.md` /
`SCOREBOARD.md`.

```bash
BENCH_SAMPLES=3 ./benchmarks/run.sh --oss --tier pr,nightly
# SCOREBOARD.md is written automatically on --oss
```

## Notes

- Cold = empty tool cache dir; warm = reuse the same cache after the cold samples.
- Protocol: GOCACHE warm (`corpus/prepare.sh`); clone/mod download excluded from timing.
- Both tools are forced to `--issues-exit-code 0` so findings do not skew exit codes.
- OSS configs are patched for unlimited issue caps (same as compat) for fair runs.
- Perf hard gate (OSS only): guff non-zero exit, or warm speedup `< 1.0`
  (`golangci_warm / guff_warm`). ≈20x is a SCOREBOARD claim, not a hard fail.
- Avoid `switch` / `++` / `--` in synthetic corpora until SSA R17.
