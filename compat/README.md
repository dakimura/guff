# Compatibility harness (R21)

Compare **guff** and **golangci-lint** finding sets on the same corpus and
config. Keys are normalized to `relpath:line:linter:message`. Per-linter
precision/recall is reported; known mismatches live in `allowlist.txt`.
Unexpected diffs fail the run (CI gate).

## Quick start

```bash
cargo build --release -p guff-lint

# CI / offline smoke (fixture only; requires golangci-lint on PATH)
./compat/smoke.sh

# Default: fixture + benchmarks/local
./compat/run.sh

# Optional OSS clones from repos.txt
./compat/run.sh --oss

# Refresh allowlist from current diffs (review before committing)
./compat/run.sh --smoke --update-allowlist
```

## Layout

| Path | Role |
|------|------|
| `run.sh` | Main harness |
| `smoke.sh` | Fixture-only CI entrypoint |
| `normalize.py` | JSON → keys, diff, markdown/JSON report |
| `standard.yml` | Shared enable-set (standard five) |
| `allowlist.txt` | Accepted `guff-only` / `golangci-only` keys |
| `repos.txt` | Optional OSS list for `--oss` |
| `tests/test_normalize.py` | Harness unit tests |
| `results/RESULTS.md` | Latest checked-in report snapshot |

## Notes

- golangci-lint is invoked with `--path-mode abs`; both sides are relativized
  to the target module root.
- Light message canonicalization covers known errcheck / unused phrasing
  differences; everything else must match or be allowlisted.
- guff diagnostic paths use the full `compiled_go_files` path (not basename)
  so multi-package modules compare cleanly.
