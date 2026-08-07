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
| kubernetes | `gocritic` `deprecatedComment` | The message lacked the `deprecatedComment: ` checker prefix golangci-lint emits, so the target's own `exclusions.rules` regex could not match it. **Most other gocritic messages still lack their prefix** — they can only ever be guff-only, so this is worth a sweep. |
| kubernetes | `QF1010` on `(*testing.B).Fatal` | The `(*log.Logger).Print*` arm matched on method name alone; upstream's pattern names the receiver type. |
| vault | `unused` missing `const bucketCount` | honnef groups const specs with `astutil.GroupSpecs` (consecutive lines), not per `const (…)` block, so a doc comment splits the group and an exported member no longer keeps its neighbour alive. |
