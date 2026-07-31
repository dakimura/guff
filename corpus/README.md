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
| `cache/` | Clone root (gitignored) |

## Tiers

| Tier | When | Repos |
|------|------|-------|
| `pr` | PR CI (`compat` oss-pr) | gin, caddy, helm |
| `nightly` | Nightly showcase | consul, grafana (`./pkg/...`), containerd (`./pkg/...`) |
| `weekly` | Defined only (no CI yet) | vault (`./helper/...`), kubernetes (apimachinery + `hack/golangci.yaml`) |

moby/moby is excluded: public tree has no root `go.mod` (Docker-image builds only).

## Adoption rules

- Checkout must have a **golangci-lint v2** config (`version: "2"`).
- Upstream CI pin need not be exactly v2.12.2 — we always run v2.12.2.
- Excluded for now: fiber, hugo, etcd, terraform (no confirmed v2 in corpus),
  istio, cockroach, go-ethereum (size / build tags). prometheus stays in
  [`regress/`](../regress/).

OSS inventory, tiers, and clone/warm live in [`../corpus/`](../corpus/).

Issue caps: OSS configs are patched to unlimited `max-issues-per-linter` /
`max-same-issues` (defaults otherwise truncate identical messages and rotate
finding sets). See [`../corpus/patch_unlimited_issues.py`](../corpus/patch_unlimited_issues.py).

## Quick start

```bash
./corpus/prepare.sh --tier pr          # clone + validate + warm
./compat/run.sh --oss --tier pr        # finding-set gate (own config)
./benchmarks/run.sh --oss --tier pr --quick
```

`prepare.sh` prints TSV to stdout (`name dir config packages timeout tier`);
progress goes to stderr.
