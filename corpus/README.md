# OSS corpus (own-config)

Shared checkout list for [`compat/`](../compat/) (finding-set parity) and
[`benchmarks/`](../benchmarks/) (wall-clock SCOREBOARD).

Both harnesses run **guff** and **golangci-lint v2.12.2** against each repo's
**real** `.golangci.yml` / `.golangci.yaml` (or an explicit `config` override).
Fixture / synthetic targets still use `compat/standard.yml` /
`benchmarks/standard.yml`.

## Layout

| Path | Role |
|------|------|
| `repos.json` | Pinned name / url / ref / packages / tier / timeout [/ config] |
| `select.py` | Tier/name filter → TSV |
| `prepare.sh` | Shallow clone, **v2 config check**, `go mod download` + `go list` warm |
| `shapes.py` | Input-shape ledger + gate ([Phase 5](../docs/COMPAT-HARDENING.md)) |
| `shapes.json` | Generated ledger — `./corpus/shapes.py probe` |
| `cache/` | Clone root (gitignored) |

## Tiers

| Tier | When | Repos |
|------|------|-------|
| `pr` | PR CI (`compat` oss-pr) | gin, caddy, helm, k9s, cobra |
| `nightly` | Nightly showcase | consul, grafana (`./pkg/... ./apps/advisor/...`), containerd (`./pkg/...`) |
| `weekly` | Defined only (no CI yet) | vault (`./helper/...`), kubernetes (apimachinery + `hack/golangci.yaml`) |

moby/moby is excluded: public tree has no root `go.mod` (Docker-image builds only).

## Adoption rules

- Checkout must have a **golangci-lint v2** config (`version: "2"`).
- Upstream CI pin need not be exactly v2.12.2 — we always run v2.12.2.
- Excluded for now: fiber, hugo, etcd, terraform (no confirmed v2 in corpus),
  istio, cockroach, go-ethereum (size / build tags). prometheus stays in
  [`regress/`](../regress/).

## What a target is *for* (Phase 5)

A repo earns its place by covering a **shape of input** the others do not, and
that claim is measured rather than asserted: `shapes.py` counts each shape over
the target's real package pattern, which is the only set the compat gate ever
sees. The distinction bites — the grafana checkout contains 47 `go.mod` files,
but `./pkg/...` alone analyzes exactly one module, so grafana did not cover
"multi-module" until `./apps/advisor/...` was added to the pattern.

`shapes.py check` fails when no gated target covers a required shape, so
re-scoping or deleting a target can no longer drop one silently. Shapes we
decided against live in the script's `EXCLUDED` map with the measurement behind
the decision (cgo needs a C toolchain in CI; golangci-lint emits nothing at all
for `.s` files; non-ASCII identifiers exist in no mainstream Go repo and are
covered by `compat/golden/cases/nonascii` instead).

Only `pr` and `nightly` count as covering a shape: `weekly` is defined but no
job runs it, and a gate nobody runs cannot notice a regression.

The three most recent additions and what each one bought:

| Target | Shape it added | What it found |
|--------|----------------|---------------|
| k9s | A config that names a linter in **both** `enable` and `disable`, and `gocritic.enabled-tags` (the only corpus repo that sets it) | 4 bugs — see COMPAT-HARDENING §4 |
| cobra | `go 1.15`, the oldest directive in the corpus (every other target is ≥ 1.24) | 1 bug (`%-36[1]s`) |
| grafana `./apps/advisor/...` | One run spanning two modules of a `go.work` workspace | — |

OSS inventory, tiers, and clone/warm live in [`../corpus/`](../corpus/).

Issue caps: OSS configs are patched to unlimited `max-issues-per-linter` /
`max-same-issues` (defaults otherwise truncate identical messages and rotate
finding sets). See [`../corpus/patch_unlimited_issues.py`](../corpus/patch_unlimited_issues.py).

## Quick start

```bash
./corpus/prepare.sh --tier pr          # clone + validate + warm
./compat/run.sh --oss --tier pr        # finding-set gate (own config)
./benchmarks/run.sh --oss --tier pr --quick

./corpus/shapes.py probe               # re-measure -> corpus/shapes.json
./corpus/shapes.py report              # markdown table of the ledger
./corpus/shapes.py check --offline     # gate: every required shape covered
```

`prepare.sh` prints TSV to stdout (`name dir config packages timeout tier`);
progress goes to stderr.
