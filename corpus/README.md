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
| `hunt` | Nothing — run by hand ([`hunt.json`](hunt.json), `compat/hunt.sh`) | 41 repos; a discovery tier that is *expected* to carry open diffs |

moby/moby is excluded: public tree has no root `go.mod` (Docker-image builds only).

## Adoption rules

- Checkout must have a **golangci-lint v2** config (`version: "2"`).
- Upstream CI pin need not be exactly v2.12.2 — we always run v2.12.2.
- prometheus stays in [`regress/`](../regress/).

### Host requirements beyond the Go toolchain

Most targets need only Go and `golangci-lint`. One does not, and a target that
needs more says so in its own `corpus/hunt.json` entry through an `env` map,
which `compat/hunt.sh` applies to **both** tools and to the module warm-up. The
point of putting it there rather than in a shell is that a bare
`./compat/hunt.sh --name <target>` cannot quietly measure the collapsed answer
and have it read as a regression.

| target | needs | why |
|---|---|---|
| photoprism/photoprism | `brew install libtensorflow vips` | `github.com/wamuir/graft/tensorflow` `#include`s `tensorflow/c/c_api.h`, and `github.com/davidbyttow/govips/v2/vips` asks pkg-config for `vips`. Without them 28 of 113 packages do not type-check, golangci-lint's whole report collapses to 3 typecheck findings, and guff — which does not collapse — reports 447: `guff 447 / golangci 3 / both 0`, a comparison of nothing. The entry's `env` carries `CGO_CFLAGS=-I/opt/homebrew/include` and `CGO_LDFLAGS=-L/opt/homebrew/lib`, because clang on macOS does not search Homebrew's include directory by default. `go build ./...` exits 0 with them and fails without. `compat/run.sh` has no `env` support, so promoting this target out of the hunt tier needs that first. |

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
| lightningnetwork/lnd | yes (`.golangci.yml`, 337 lines) | **Neither tool can run it.** Measured 2026-09-08 at `v0.21.2-beta`. The config declares a `linters.settings.custom` module plugin `ll` — a custom line-length linter that skips `S`-suffixed log lines — and a stock golangci-lint 2.12.2 refuses to start: `build linters: plugin(ll): plugin "ll" not found` (rc=3). **guff refuses with the same message** (rc=2), which is the parity `compat/reject/cases/custom-module-plugin-missing` already pins, so nothing here needs fixing. The plugin is not an accident of packaging: its source is in the repo (`tools/linters/ll.go`) and the Makefile builds a bespoke binary for it — `lint-native` runs `go tool .../golangci-lint custom` and then `./tools/custom-gcl run`. Same shape as pulumi/pulumi. **The repository itself is measurable**: with the `custom:` block removed by hand, `./build/...` gives golangci **10** / guff **10** / both **10** — an exact match — so this becomes adoptable if `corpus/patch_unlimited_issues.py` ever strips `type: module` entries the way it already strips `.so` goplugins (it deliberately does not: the guard is `has_path and not is_module`). Two further things a future adoption would have to settle, both measured here: `issues.new-from-rev: 03eab4db…` names a commit a `--depth 1` clone does not have, and golangci does **not** error on that — it silently reports everything, so the Diff processor is a no-op rather than a failure; and the tree is multi-module (13 `go.mod` files; the root module lists 159 packages), so `./tor/...` from the root is `directory prefix tor does not contain main module`. |
| grafana/tempo | yes (`.golangci.yml`) | **No longer a valid exclusion — the cause was found and fixed.** It was never a loader *cost*: `guff-build`'s `go/build`-style header scanner accepted only a double-quoted import path, and `bytedance/sonic`'s asm2asm-generated files write theirs backquoted (``import (\n\t`github.com/bytedance/sonic/loader`\n)``), so `skip_import_spec` returned the slice it was given and `parse_go_file_info` spun on it forever — an infinite loop, not a slow one. The five-line sonic probe that used to take guff >180s now takes **0.30s** against golangci's 0.24s, same finding. Re-measured 2026-09-15 at `v3.0.3`: guff **398** / golangci **398** / both **393**, with one ill-typed package on guff's side (`modules/frontend`) that carries three of the five gcl-only findings. Back in `corpus/hunt.json` as an open target. |
| gofiber/fiber | **yes** (307 lines) | **No longer a valid exclusion** — adopt or restate. See `candidates-100.md`. |
| ethereum/go-ethereum | **yes** (96 lines) | **No longer a valid exclusion** on v2 grounds; 234MB and build tags are the remaining question. |
| harness/harness | yes on `main` (14KB), **not on any tag** | Two independent reasons, measured 2026-09-02. (1) **No tag carries a v2 config.** The newest tag is `v3.3.0` (2025-08-14); the config was upgraded to `version: "2"` on 2025-10-17 (`92cb4098f`), and no tag has been cut in the 12 months since while `main` stays active. The candidate row's `ref: v2.28.2` is worse still — that tag is *old Drone* (`module github.com/drone/drone`, 5.0MB, no config at all); the survey read `_config` from the default branch but took the ref from the releases API, and in this repo those are two different codebases. (2) **`./...` on `main` measures only `typecheck`.** `web/dist.go` embeds `dist/*`, an npm build output absent from a checkout, so both tools report exactly one finding — and because a typecheck issue deletes every other issue in the run, ~40 enabled linters over 430 packages measure nothing. Adopting that would add a target that cannot fail. `./registry/...` alone does produce a real set (159 packages, 94 findings across goconst/gosec/govet/noctx/gocritic), so a scoped, SHA-pinned adoption is possible if the tag convention is ever relaxed — `prepare.sh` would need `git fetch --depth 1 origin <sha>` in its fresh-clone path first (a shallow clone of the default branch cannot check out a non-tip SHA: `fatal: unable to read tree`). |
| ollama/ollama | yes (`.golangci.yaml`) | **`./...` measures only `typecheck`**, measured 2026-09-06 at `v0.33.1`. `app/ui/app.go` has `//go:embed app/dist`, the React SPA's build output — and `dist` is in `.gitignore`, so it exists in no tag and no clone. `go list ./...` fails with `pattern app/dist: no matching files found`, golangci-lint's entire report becomes that one `typecheck` finding (a typecheck issue deletes every other issue in the run), and guff reports 15 — 14 of them `nolintlint` "directive is unused", the shadow of linters that never ran. Same shape as harness/harness. **The repository itself is measurable**: stubbing the embed by hand (`app/ui/app/dist/index.html`) loads **104 packages** and gives golangci **0** / guff **10**, so a `prepare` hook in the corpus schema would make this adoptable — and those 10 are worth chasing when it is (8 `unused`, 1 `staticcheck` SA1003, 1 `intrange`; the `unused` ones appear only at `./...` scale and vanish when the same packages are linted alone). |
| open-telemetry/opentelemetry-collector | yes | **The harness cannot reach the code.** 100 `go.mod` files and no `go.work`, so `./...` at the checkout root lists exactly one package — `go.opentelemetry.io/collector/internal/statusutil` — and both tools agree on zero findings for it. A submodule cannot be named from the root either: `go list ./pdata/...` is `pattern ./pdata/...: directory prefix pdata does not contain main module or its selected dependencies`. Upstream lints it as 100 separate runs (`make golint` → `for-all-target` → `cd $module && golangci-lint run`), and a corpus entry is one invocation from the checkout root: `name/url/ref/packages/tier/timeout[/config][/build_tags]` has no field naming a module directory. Adopting it as `./...` would add a target that cannot fail. Measured 2026-09-04 at `v0.159.0`. Reachable if the schema ever grows a module-directory field. |
| kubevirt/kubevirt | yes (`.golangci.yml`, 30 linters) | **Linux only, and `./...` does not even start.** Measured 2026-09-15 at `v1.9.0`. `cmd/container-disk-v2alpha` holds a `main.c` and nothing but `_test.go` files, so `go list` refuses the whole pattern with *C source files not allowed when not using cgo or SWIG: main.c* — with `CGO_ENABLED=1`, so it is a property of the repo and not of this host. kubevirt never lints `./...` either: `hack/golangci-lint.sh` reads 105 paths out of `hack/linter/lint-paths.txt`. Measured against those, **39 of the 105 paths fail `go build` on darwin, covering 85 of their 166 packages** — `cmd/virt-api`, `cmd/virt-controller`, `cmd/virt-operator`, `pkg/instancetype`, `pkg/network/driver/netlink` and six `tests/` directories among them. Two representative causes: the vendored `github.com/containernetworking/plugins/pkg/ns` ships only `ns_linux.go` (*build constraints exclude all Go files*), and `pkg/safepath` calls `syscall.Getxattr`, `touchat` and `mknodat`. golangci-lint's whole report then collapses to **1 typecheck finding** while guff keeps going and reports 130 `mnd` findings: guff 130 / golangci 1 / both 0, with 8 packages ill-typed on guff's side — the milvus and dagger shape. The 66 buildable paths are scattered across `cmd/`, `pkg/` and `tests/`, so there is no subtree to scope to, and any subset would be a package set neither the repo nor upstream uses. |
| inspektor-gadget/inspektor-gadget | yes (`.golangci.yml`, 2 linters) | **Linux only, and there is no clean subtree to scope to.** Measured 2026-09-07 at `v0.55.1`. `pkg/utils/host` (3 files) and `pkg/symbolizer/symtab` (2 files) carry `//go:build linux` on *every* file, so `go list` reports `GoFiles=0` for both on darwin — and **33 files import the first directly**. golangci-lint's whole report becomes **1 issue, and it is `typecheck`** (`could not import .../pkg/utils/host`), which deletes every other issue, so 2 linters over 252 packages measure nothing; guff meanwhile reports 1 `gofumpt` finding, so the pair reads **guff 1 / golangci 1 / both 0 — P=0%, R=0%** on a target whose intended answer is neither. Of the 252 packages listed, **128 are tainted** (the two above, 79 linux-only `gadgets/*/test/**` packages, and everything reaching them) and **124 are clean**, but they are scattered across six top-level directories: `./pkg/...` collapses to the same single typecheck because `pkg/process-helpers` is inside it, so unlike harness/harness there is no `./registry/...` to scope to and the `packages` field takes a pattern, not a list of 124. This is the cri-o / tetragon shape — **platform-bound and permanent on darwin**, unlike SigNoz/signoz, which is toolchain-version-bound. Measurable as-is on a Linux host. Counting the excluded packages needs care: **`go list ./...` does not enumerate a package whose files are all build-excluded**, so the two above are absent from its output and a naive taint walk over it reports 173 clean instead of 124. |
| dagger/dagger | yes (`.golangci.yml`, 24 linters) | **Upstream's report is a race, not a set — there is nothing to be compatible with.** Measured 2026-09-08 and 2026-09-10 at `v0.21.9`. 228 doc snippets under `docs/**` import `dagger/my-module/internal/dagger`, a module path that exists on no platform, so **244 of 481 packages are ill-typed** here (228 of them on any host). golangci-lint's `loadingPackage.analyze` runs a package's actions in an errgroup and calls the **run-wide** `cancel()` as soon as one of them fails with `IllTypedError` (`runner_checker.go` returns it for every `pkg.IllTyped`), so every package that has not started yet returns at `case <-ctx.Done()`. What gets printed is whichever ill-typed packages won the race — and a `typecheck` issue deletes every other issue in the run. Four `./...` runs gave **2, 4, 2 and 8** issues, and the adoption run had recorded a fifth set of 6; the four smaller sets share **no file at all**. A 12-package minimal repro (plus one clean package with two staticcheck findings) gave **8 different sets in 8 runs** — `{bad8} {bad1} {bad1,bad11} {bad2} {bad6,bad8} {bad4,bad6} {bad6} {bad2,bad5,bad6}` — and `--concurrency=1` does not settle it (`bad9 bad9 bad7 bad10`), so it is the iteration order of the package set rather than the parallelism. **The line is at two**: with exactly one ill-typed package the same repro prints the same single issue 6 times out of 6, which is why harness, ollama, SigNoz and inspektor-gadget below can be excluded on a *stated* "the whole report is one typecheck finding" — each collapses at one point. dagger has 186. **Scoping away from `docs/` does not rescue it on darwin**: the other 16 ill-typed packages are all Linux-only — `util/layercopy` alone taints 8, because `cleanRel` is defined in `dest_linux.go` (`//go:build linux`) while its caller `filter.go` carries no tag, and `engine/engineutil` wants `unix.OpenTree`, `network/netinst` wants `syscall.Mount`, `cmd/engine` and `engine/server` pull containerd's overlay snapshotter. On a Linux host a docs-free `packages` pattern *would* measure, and the entry schema has that field, so this is **re-adoptable there** rather than permanently dead. Unlike cri-o/tetragon this is not platform-bound (the dominant cause is host-independent) and unlike SigNoz it is not toolchain-bound; it is the first target excluded because the *reference itself* is nondeterministic. Adopting it is what found the SSA builder panics on ill-typed composite literals (COMPAT-HARDENING 続き 277). |
| milvus-io/milvus | yes (`.golangci.yml`, 13 linters) | **cgo wants milvus's own C++ core, and the report that survives is a race.** Measured 2026-09-14 at `v3.0.0`. `internal/util/cgo/errors.go` opens with `#cgo pkg-config: milvus_core` under **no build tag**, and `milvus_core.pc` is produced by building `internal/core` with CMake — it is in no package manager — so `pkg-config --cflags milvus_core` fails on this host and **24 of the root module's 347 packages are ill-typed**: every cgo binding (`analyzecgowrapper`, `segcore`, `indexcgowrapper`, `initcore`, `cgoconverter`, `util/cgo` itself) and everything that imports one (`internal/proxy`, `internal/querynodev2` and its three subpackages, `internal/datanode/index`, `cmd/tools/migration/*`). A `typecheck` issue deletes every other issue in the run, so golangci-lint's whole report is a handful of typecheck findings — and with more than one ill-typed package **which** findings is the cancel race of COMPAT-HARDENING 続き 277. Three `./...` runs with a fresh cache each gave `{util/cgo/logging/logging_benchmark_test.go:1}`, `{util/cgo/errors.go:9, util/cgo/futures.go:28}` and `{util/cgo/logging/logging_benchmark_test.go:1}`; the adoption run had recorded the middle one. guff analyses the ill-typed packages and reports 20 findings (gosec 11 / govet 4 / staticcheck 3 / revive 2) that nothing can be compared against: **guff 20 / golangci 2 / both 0**. **No subtree to scope to**: `pkg/` and `client/` are separate modules, so `./...` from the root never reaches them and `go list ./pkg/...` answers "main module does not contain package" — and the cgo-tainted packages are spread across `internal/`. Unlike SigNoz this is not toolchain-bound and unlike cri-o not platform-bound: it is **dependency-bound**, and re-adoptable on a machine where milvus's core has been built and installed (their own CI builds it). |
| SigNoz/signoz | yes (`.golangci.yml`, 15 linters) | **The host toolchain cannot build it, and that is the whole of it.** Measured 2026-09-06 at `v0.139.0`. `go.mod` says `go 1.25.7` with no `toolchain` line, and `GOTOOLCHAIN=auto` only ever *upgrades*, so the build runs on this host's **go1.26.5**. Its dependency `github.com/bytedance/sonic v1.14.1` declares `rt.GoMapIterator` in exactly three files and **go1.26 selects none of them**: `map_legacy.go` is `// +build !go1.24`, and `map_nosiwss_go124.go` / `map_siwss_go124.go` are `//go:build go1.24 && !go1.26 && [!]goexperiment.swissmap`. sonic v1.14.1 predates Go 1.26 and has no file for it. `go build ./...` therefore fails with `internal/rt/stubs.go:33:22: undefined: GoMapIterator`, and golangci-lint's whole report becomes **1 issue, and it is `typecheck`** — a typecheck finding deletes every other issue in the run, so 15 linters over 356 packages would measure nothing. The collapse looks like harness and ollama but the **cause is different, and so is the fix**: `go list ./...` here succeeds (356 packages) and the repository is not at fault. `GOTOOLCHAIN=go1.25.7 go build ./...` **exits 0 with no output** — the code is fine, the pinned dependency simply does not support the installed compiler. So unlike cri-o and tetragon (platform-bound, permanent on darwin) this is **toolchain-version-bound**: it measures cleanly on a Go 1.24/1.25 host, and a sonic bump makes it measurable here. It is excluded rather than adopted because a target whose finding set depends on which Go happens to be installed records the toolchain, not the compatibility — and the entry schema (`name/url/ref/packages/tier/timeout[/config][/build_tags]`) has no field to pin `GOTOOLCHAIN` per target. Re-check when signoz moves off sonic v1.14.1. |

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
