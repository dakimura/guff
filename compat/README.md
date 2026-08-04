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
```

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
