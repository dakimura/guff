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
| `pr` | PR CI (`compat` oss-pr) | gin, caddy, helm, k9s, cobra, go-client |
| `nightly` | Nightly showcase | consul, grafana (`./pkg/... ./apps/advisor/...`), containerd (`./pkg/...`) |
| `weekly` | Sunday CI (`compat-weekly` oss-weekly) | controller-runtime, vault (`./helper/...`), kubernetes (apimachinery + `hack/golangci.yaml`) |
| `hunt` | Nothing — run by hand ([`hunt.json`](hunt.json), `compat/hunt.sh`) | 26 repos; a discovery tier that is *expected* to carry open diffs |

moby/moby is excluded: public tree has no root `go.mod` (Docker-image builds only).

## Adoption rules

- Checkout must have a **golangci-lint v2** config (`version: "2"`).
- Upstream CI pin need not be exactly v2.12.2 — we always run v2.12.2.
- prometheus stays in [`regress/`](../regress/).

### Excluded targets

One reason per repository. The old shared "no confirmed v2" note covered
repositories whose real reasons differ, and two of them have since adopted v2 —
a grouped reason cannot expire, so it was never revisited. Re-checked 2026-08-27
against each default branch.

| Repo | v2 config | Excluded because |
|---|---|---|
| gohugoio/hugo | **none** | No `.golangci.yml` / `.golangci.yaml` on the default branch. Nothing to run against. |
| etcd-io/etcd | **none** | Same — no config on the default branch. |
| hashicorp/terraform | **none** | Same — no config on the default branch. |
| istio/istio | **none** | Same — no config on the default branch; 296MB besides. |
| cockroachdb/cockroach | **none** | Same — no config on the default branch; 2.6GB checkout. |
| moby/moby | yes (370 lines) | Public tree has no root `go.mod` (Docker-image builds only). Unrelated to v2. |
| pulumi/pulumi | yes (273 lines) | **Neither tool can run it.** The config declares two `linters.settings.custom` module plugins (`requiredfield`, `noosexit`), and a stock golangci-lint binary refuses to start: `build linters: plugin(requiredfield): plugin "requiredfield" not found`. Measuring compat needs a config both tools accept; this one needs a custom-built binary on each side first. It is what found the guff bug in `compat/reject/cases/custom-module-plugin-missing` (2026-08-29). |
| gofiber/fiber | **yes** (307 lines) | **No longer a valid exclusion** — adopt or restate. See `candidates-100.md`. |
| ethereum/go-ethereum | **yes** (96 lines) | **No longer a valid exclusion** on v2 grounds; 234MB and build tags are the remaining question. |
| harness/harness | yes on `main` (14KB), **not on any tag** | Two independent reasons, measured 2026-09-02. (1) **No tag carries a v2 config.** The newest tag is `v3.3.0` (2025-08-14); the config was upgraded to `version: "2"` on 2025-10-17 (`92cb4098f`), and no tag has been cut in the 12 months since while `main` stays active. The candidate row's `ref: v2.28.2` is worse still — that tag is *old Drone* (`module github.com/drone/drone`, 5.0MB, no config at all); the survey read `_config` from the default branch but took the ref from the releases API, and in this repo those are two different codebases. (2) **`./...` on `main` measures only `typecheck`.** `web/dist.go` embeds `dist/*`, an npm build output absent from a checkout, so both tools report exactly one finding — and because a typecheck issue deletes every other issue in the run, ~40 enabled linters over 430 packages measure nothing. Adopting that would add a target that cannot fail. `./registry/...` alone does produce a real set (159 packages, 94 findings across goconst/gosec/govet/noctx/gocritic), so a scoped, SHA-pinned adoption is possible if the tag convention is ever relaxed — `prepare.sh` would need `git fetch --depth 1 origin <sha>` in its fresh-clone path first (a shallow clone of the default branch cannot check out a non-tip SHA: `fatal: unable to read tree`). |
| open-telemetry/opentelemetry-collector | yes | **The harness cannot reach the code.** 100 `go.mod` files and no `go.work`, so `./...` at the checkout root lists exactly one package — `go.opentelemetry.io/collector/internal/statusutil` — and both tools agree on zero findings for it. A submodule cannot be named from the root either: `go list ./pdata/...` is `pattern ./pdata/...: directory prefix pdata does not contain main module or its selected dependencies`. Upstream lints it as 100 separate runs (`make golint` → `for-all-target` → `cd $module && golangci-lint run`), and a corpus entry is one invocation from the checkout root: `name/url/ref/packages/tier/timeout[/config][/build_tags]` has no field naming a module directory. Adopting it as `./...` would add a target that cannot fail. Measured 2026-09-04 at `v0.159.0`. Reachable if the schema ever grows a module-directory field. |

`candidates-100.md` carries a 152-repository v2 survey and the 27 → 100 expansion
list; re-check this table whenever that survey is refreshed.

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

`pr`, `nightly` and `weekly` count as covering a shape; `hunt` does not. The
rule is that a gate nobody runs cannot notice a regression, so the list follows
CI rather than the inventory: `weekly` joined it on 2026-08-30 when
`.github/workflows/compat-weekly.yml` started running the tier, and `hunt`
stays out because nothing gates it and its targets carry open diffs by design.

The three most recent additions and what each one bought:

| Target | Shape it added | What it found |
|--------|----------------|---------------|
| k9s | A config that names a linter in **both** `enable` and `disable`, and `gocritic.enabled-tags` (the only corpus repo that sets it) | 4 bugs — see COMPAT-HARDENING §4 |
| cobra | `go 1.15`, the oldest directive in the corpus (every other target is ≥ 1.24) | 1 bug (`%-36[1]s`) |
| grafana `./apps/advisor/...` | One run spanning two modules of a `go.work` workspace | — |
| go-client (qdrant) | Eight linter keys no other target enables — `cyclop`, `exhaustruct`, `gochecknoglobals`, `gocognit`, `inamedparam`, `nestif`, `nonamedreturns`, `testpackage` | — (0 diffs on the first run; promoted from `hunt` to `pr` the same day, 2026-08-30) |

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
