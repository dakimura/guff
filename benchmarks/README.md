# Benchmarks (R11)

Wall-clock harness comparing **guff** and **golangci-lint** on the golangci-lint
v2 `standard` preset (staticcheck / govet / errcheck / ineffassign / unused).

## Quick start

```bash
# Release binary (recommended)
cargo build --release -p guff-lint

# Offline smoke (fixture only)
./benchmarks/smoke.sh

# Default: fixture + synthetic multi-package corpus (`local/`)
./benchmarks/run.sh

# Optional OSS clones (may FAIL on guff until SSA R17)
./benchmarks/run.sh --oss
```

Requires `go`, `python3`, and (for comparison) `golangci-lint` on `PATH`.
Set `SKIP_GOLANGCI=1` to time guff alone.

## Layout

| Path | Role |
|------|------|
| `run.sh` | Main harness (cold/warm, median summary) |
| `smoke.sh` | CI/manual smoke (`--smoke --quick`) |
| `standard.yml` | Shared enable-set for both tools |
| `fixture/` | Tiny module with intentional findings |
| `local/` | ~3k LOC synthetic multi-package corpus (SSA-safe dialect) |
| `gen_local.sh` | Regenerate `local/` |
| `repos.txt` | Optional OSS list for `--oss` |
| `results/RESULTS.md` | Checked-in snapshot of a recent run |

Raw `results/*.tsv` / timestamped `.md` are gitignored; refresh `RESULTS.md`
after a full run:

```bash
BENCH_SAMPLES=3 ./benchmarks/run.sh
cp benchmarks/results/<latest>.md benchmarks/results/RESULTS.md
```

## Notes

- Cold = empty tool cache dir; warm = reuse the same cache after the cold samples.
- Both tools are forced to `--issues-exit-code 0` so findings do not skew exit codes.
- guff still loads/typechecks on cache hits (facts / load skip is DEFERRED; see R10).
- Avoid `switch` / `++` / `--` in synthetic corpora until SSA R17.
