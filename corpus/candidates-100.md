# Corpus expansion candidates (27 → 100)

Surveyed 244 well-known Go repositories on 2026-08-27; **152** carry a
golangci-lint **v2** config (`version: "2"`). 73 of them are selected here to take
the corpus from 27 to 100 targets.

Selection is **not** by popularity. Picks were made greedily to maximise linter /
settings keys the corpus has never exercised, with repository size as tie-breaker.

| | keys |
|---|---:|
| exercised by the current 27 targets | 87 |
| exercised after adding all 73 | **121** |
| present anywhere in the 152 surveyed v2 repos | 121 |

## Read this before adding all 73

The greedy pass closed the entire key gap with its **first 8 picks**. The other
65 add **zero** new linter keys.

That is not an argument for dropping them, but it does change what they are for:

- **Group A** buys *linter coverage* — configuration guff has never been run against.
- **Group B** buys *code-shape volume* — new Go written by other people, for linters
  the corpus already enables. Historically this is where the parity bugs actually
  live (see `docs/SESSION-LOG.md`); a linter is rarely wrong everywhere, it is wrong
  on a shape nobody in the corpus happened to write.

So Group A is the cheap, high-value half and should land first. Group B is a
compute-cost decision: `open-policy-agent/opa` (2992MB), `gravitational/teleport`
(1512MB) and `weaviate/weaviate` (1124MB) alone will dominate a full-corpus run.

Machine-readable in `corpus/repos.json` schema: [`candidates-100.json`](candidates-100.json).
Strip the `_`-prefixed keys before merging. Every `ref` is that repository's latest
release tag as of 2026-08-27 and should be reviewed before pinning.

## Group A — closes the linter-coverage gap

The single largest gap is **`rowserrcheck`**: enabled by 28 of the 152 surveyed
repositories, exercised by none of the current 27 targets, and already declared
partial in `docs/COMPARE.md` (AST approximation; SSA / ctrlflow parity deferred).
`nestif` (10 repos), `gochecknoinits` (9), `grouper` (9), `protogetter` (9),
`musttag` (8), `containedctx` (7) and `gocognit` (7) follow.

| repo | stars | size | ref | new keys |
|---|--:|--:|---|---|
| [cri-o/cri-o](https://github.com/cri-o/cri-o) | 5654 | 197.8MB | `v1.36.4` | `arangolint`, `embeddedstructfieldcheck`, `gochecknoinits`, `gosmopolitan`, `grouper`, `musttag`, `nlreturn`, `protogetter`, `recvcheck`, `rowserrcheck`, `unqueryvet`, `wsl_v5` |
| [qdrant/go-client](https://github.com/qdrant/go-client) | 346 | 1.8MB | `v1.19.0` | `cyclop`, `exhaustruct`, `gochecknoglobals`, `gocognit`, `inamedparam`, `nestif`, `nonamedreturns`, `testpackage` |
| ~~[pulumi/pulumi](https://github.com/pulumi/pulumi)~~ | 25612 | 356.5MB | `v3.259.0` | ~~`custom`, `noosexit`, `paralleltest`, `requiredfield`~~ — **not adoptable**, see below |
| [tektoncd/pipeline](https://github.com/tektoncd/pipeline) | 9046 | 157.3MB | `v1.15.0` | `containedctx`, `interfacebloat`, `maintidx` |
| [gofiber/fiber](https://github.com/gofiber/fiber) | 40091 | 72.2MB | `v3.5.0` | `err113`, `wrapcheck` |
| [scaleway/scaleway-cli](https://github.com/scaleway/scaleway-cli) | 996 | 31.3MB | `v2.61.0` | `funcorder`, `golines` |
| [connectrpc/connect-go](https://github.com/connectrpc/connect-go) | 4051 | 3.6MB | `v1.20.0` | `exhaustruct_v5`, `varnamelen` |
| [velero-io/velero](https://github.com/velero-io/velero) | 10254 | 61.2MB | `v1.18.2` | `wsl` |

### `custom` is not a linter key

pulumi was picked for four "new keys", but `custom` is not a linter — it is the
block that declares **module plugins**, and `noosexit` / `requiredfield` are two
of pulumi's own. A stock golangci-lint binary refuses to start on that config
(`build linters: plugin(requiredfield): plugin "requiredfield" not found`), so
there is no finding set to compare against without building a custom binary on
both sides. Measured 2026-08-29; pulumi is out, and `paralleltest` — the one
real key it carried — is still unexercised.

The survey should not count `custom` as coverage. It did buy one thing on the
way out: guff accepted the same config and linted on, which is now
`compat/reject/cases/custom-module-plugin-missing`.

## Group B — code-shape volume

No new linter keys; selected largest-first.

| repo | stars | size | ref | timeout |
|---|--:|--:|---|---|
| [open-policy-agent/opa](https://github.com/open-policy-agent/opa) | 12164 | 2991.7MB | `v1.19.1` | 60m |
| [gravitational/teleport](https://github.com/gravitational/teleport) | 20852 | 1511.8MB | `v18.10.0` | 60m |
| [weaviate/weaviate](https://github.com/weaviate/weaviate) | 16754 | 1123.7MB | `v1.39.2` | 60m |
| [DataDog/datadog-agent](https://github.com/DataDog/datadog-agent) | 3713 | 944.6MB | `7.82.3` | 60m |
| [aquasecurity/trivy](https://github.com/aquasecurity/trivy) | 37637 | 918.8MB | `v0.74.0` | 60m |
| [open-telemetry/opentelemetry-collector-contrib](https://github.com/open-telemetry/opentelemetry-collector-contrib) | 4888 | 867.7MB | `v0.159.0` | 60m |
| [pingcap/tidb](https://github.com/pingcap/tidb) | 40473 | 738.7MB | `v8.5.7` | 40m |
| [grafana/mimir](https://github.com/grafana/mimir) | 5218 | 642.1MB | `mimir-3.2.0` | 40m |
| [hashicorp/nomad](https://github.com/hashicorp/nomad) | 16837 | 631.9MB | `v2.0.5` | 40m |
| [zitadel/zitadel](https://github.com/zitadel/zitadel) | 14864 | 578.4MB | `v4.17.1` | 40m |
| [k3s-io/k3s](https://github.com/k3s-io/k3s) | 33828 | 577.4MB | `v1.36.3+k3s1` | 40m |
| [cilium/cilium](https://github.com/cilium/cilium) | 25014 | 541.0MB | `v1.20.1` | 40m |
| [elastic/beats](https://github.com/elastic/beats) | 12640 | 512.6MB | `v9.5.2` | 40m |
| [vitessio/vitess](https://github.com/vitessio/vitess) | 21259 | 511.9MB | `v24.0.2` | 40m |
| [ava-labs/avalanchego](https://github.com/ava-labs/avalanchego) | 2358 | 508.5MB | `v1.14.2` | 40m |
| [grafana/loki](https://github.com/grafana/loki) | 28790 | 502.8MB | `v3.7.6` | 40m |
| [cortexproject/cortex](https://github.com/cortexproject/cortex) | 5856 | 402.7MB | `v1.21.1` | 40m |
| [photoprism/photoprism](https://github.com/photoprism/photoprism) | 40114 | 372.8MB | `260728-bbde8f452` | 25m |
| [kubevirt/kubevirt](https://github.com/kubevirt/kubevirt) | 7030 | 368.8MB | `v1.9.0` | 25m |
| [cosmos/cosmos-sdk](https://github.com/cosmos/cosmos-sdk) | 7050 | 365.3MB | `cosmovisor/v1.7.3` | 25m |
| [VictoriaMetrics/VictoriaMetrics](https://github.com/VictoriaMetrics/VictoriaMetrics) | 17597 | 358.5MB | `v1.150.0` | 25m |
| [milvus-io/milvus](https://github.com/milvus-io/milvus) | 45809 | 318.9MB | `v3.0.0` | 25m |
| [opentofu/opentofu](https://github.com/opentofu/opentofu) | 29937 | 282.7MB | `v1.12.6` | 25m |
| [moby/moby](https://github.com/moby/moby) | 72002 | 264.3MB | `docker-v29.7.2` | 25m |
| [grafana/pyroscope](https://github.com/grafana/pyroscope) | 11641 | 246.4MB | `v2.3.0` | 25m |
| [dagger/dagger](https://github.com/dagger/dagger) | 16203 | 238.6MB | `v0.21.9` | 25m |
| [ethereum/go-ethereum](https://github.com/ethereum/go-ethereum) | 51314 | 233.5MB | `v1.17.5` | 25m |
| [kubernetes-sigs/cluster-api](https://github.com/kubernetes-sigs/cluster-api) | 4288 | 232.4MB | `v1.14.0` | 25m |
| [grafana/tempo](https://github.com/grafana/tempo) | 5455 | 207.6MB | `v3.0.3` | 25m |
| [open-policy-agent/gatekeeper](https://github.com/open-policy-agent/gatekeeper) | 4269 | 200.1MB | `v3.23.0` | 25m |
| [cometbft/cometbft](https://github.com/cometbft/cometbft) | 914 | 186.8MB | `v0.40.0` | 25m |
| [podman-container-tools/podman](https://github.com/podman-container-tools/podman) | 32704 | 180.9MB | `v6.1.0` | 25m |
| [argoproj/argo-workflows](https://github.com/argoproj/argo-workflows) | 16939 | 179.9MB | `v4.1.2` | 25m |
| [jesseduffield/lazygit](https://github.com/jesseduffield/lazygit) | 81664 | 153.3MB | `v0.64.1` | 25m |
| [kubernetes/ingress-nginx](https://github.com/kubernetes/ingress-nginx) | 19476 | 147.8MB | `controller-v1.15.1` | 25m |
| [elastic/go-elasticsearch](https://github.com/elastic/go-elasticsearch) | 6063 | 129.2MB | `v9.5.1` | 25m |
| [lightningnetwork/lnd](https://github.com/lightningnetwork/lnd) | 8186 | 127.8MB | `v0.21.2-beta` | 25m |
| [woodpecker-ci/woodpecker](https://github.com/woodpecker-ci/woodpecker) | 7753 | 123.9MB | `v3.18.0` | 25m |
| [ory/hydra](https://github.com/ory/hydra) | 17498 | 123.2MB | `v26.2.0` | 25m |
| [minio/minio](https://github.com/minio/minio) | 61374 | 122.3MB | `RELEASE.2025-10-15T17-29-55Z` | 25m |
| [hashicorp/packer](https://github.com/hashicorp/packer) | 15768 | 117.3MB | `v1.16.0` | 25m |
| [inspektor-gadget/inspektor-gadget](https://github.com/inspektor-gadget/inspektor-gadget) | 2913 | 114.1MB | `v0.55.1` | 25m |
| [SigNoz/signoz](https://github.com/SigNoz/signoz) | 31930 | 110.6MB | `v0.139.0` | 25m |
| [hashicorp/boundary](https://github.com/hashicorp/boundary) | 4056 | 107.3MB | `v0.21.3` | 25m |
| [flipt-io/flipt](https://github.com/flipt-io/flipt) | 4882 | 105.4MB | `v2.11.0` | 25m |
| [argoproj/argo-rollouts](https://github.com/argoproj/argo-rollouts) | 3561 | 104.7MB | `v1.9.1` | 25m |
| [cert-manager/cert-manager](https://github.com/cert-manager/cert-manager) | 14052 | 99.3MB | `v1.21.1` | 15m |
| [apache/dubbo-go](https://github.com/apache/dubbo-go) | 4953 | 97.9MB | `tools/dubbogo-cli/v1.0.1` | 15m |
| [ory/kratos](https://github.com/ory/kratos) | 13849 | 96.1MB | `v26.2.0` | 15m |
| [celestiaorg/celestia-node](https://github.com/celestiaorg/celestia-node) | 996 | 94.6MB | `v0.31.4` | 15m |
| [cilium/tetragon](https://github.com/cilium/tetragon) | 4955 | 92.6MB | `v1.7.1` | 15m |
| [podman-container-tools/buildah](https://github.com/podman-container-tools/buildah) | 8995 | 89.8MB | `v1.45.0` | 15m |
| [ollama/ollama](https://github.com/ollama/ollama) | 179522 | 89.4MB | `v0.33.1` | 15m |
| [tailscale/tailscale](https://github.com/tailscale/tailscale) | 35599 | 84.5MB | `v1.102.3` | 15m |
| [karmada-io/karmada](https://github.com/karmada-io/karmada) | 5576 | 84.2MB | `v1.18.2` | 15m |
| [influxdata/telegraf](https://github.com/influxdata/telegraf) | 17767 | 83.3MB | `v1.39.3` | 15m |
| [grafana/k6](https://github.com/grafana/k6) | 31327 | 82.1MB | `v2.2.0` | 15m |
| [kubernetes-sigs/external-dns](https://github.com/kubernetes-sigs/external-dns) | 9071 | 76.0MB | `v0.22.0` | 15m |
| [kubeshark/kubeshark](https://github.com/kubeshark/kubeshark) | 12059 | 74.9MB | `v53.4.0` | 15m |
| [open-telemetry/opentelemetry-collector](https://github.com/open-telemetry/opentelemetry-collector) | 7456 | 71.5MB | `v0.159.0` | 15m |
| [podman-container-tools/skopeo](https://github.com/podman-container-tools/skopeo) | 11194 | 68.3MB | `v1.24.0` | 15m |
| [moby/buildkit](https://github.com/moby/buildkit) | 10211 | 64.9MB | `v0.32.2` | 15m |
| [cilium/ebpf](https://github.com/cilium/ebpf) | 7933 | 59.4MB | `v0.22.0` | 15m |
| [harness/harness](https://github.com/harness/harness) | 38137 | 58.7MB | `v2.28.2` | 15m |
| [prometheus/alertmanager](https://github.com/prometheus/alertmanager) | 8594 | 56.8MB | `v0.34.0` | 15m |

## Surveyed, v2, not selected

Replacements for any selected target that proves unusable (no root `go.mod`,
build tags, vendored tree, licence).

| repo | stars | size | config |
|---|--:|--:|---|
| [fatedier/frp](https://github.com/fatedier/frp) | 109041 | 31.9MB | `.golangci.yml` |
| [nektos/act](https://github.com/nektos/act) | 71649 | 17.3MB | `.golangci.yml` |
| [usememos/memos](https://github.com/usememos/memos) | 62560 | 38.1MB | `.golangci.yaml` |
| [charmbracelet/bubbletea](https://github.com/charmbracelet/bubbletea) | 44577 | 5.8MB | `.golangci.yml` |
| [juanfont/headscale](https://github.com/juanfont/headscale) | 43206 | 55.1MB | `.golangci.yaml` |
| [go-gorm/gorm](https://github.com/go-gorm/gorm) | 39930 | 4.7MB | `.golangci.yml` |
| [filebrowser/filebrowser](https://github.com/filebrowser/filebrowser) | 35950 | 23.8MB | `.golangci.yml` |
| [labstack/echo](https://github.com/labstack/echo) | 32660 | 6.6MB | `.golangci.yaml` |
| [abiosoft/colima](https://github.com/abiosoft/colima) | 30526 | 3.2MB | `.golangci.yml` |
| [spf13/viper](https://github.com/spf13/viper) | 30447 | 1.6MB | `.golangci.yaml` |
| [go-kratos/kratos](https://github.com/go-kratos/kratos) | 25892 | 9.8MB | `.golangci.yml` |
| [sirupsen/logrus](https://github.com/sirupsen/logrus) | 25750 | 1.4MB | `.golangci.yml` |
| [uber-go/zap](https://github.com/uber-go/zap) | 24644 | 2.0MB | `.golangci.yml` |
| [navidrome/navidrome](https://github.com/navidrome/navidrome) | 23132 | 56.1MB | `.golangci.yml` |
| [lima-vm/lima](https://github.com/lima-vm/lima) | 21764 | 17.8MB | `.golangci.yml` |
| [charmbracelet/vhs](https://github.com/charmbracelet/vhs) | 20740 | 39.5MB | `.golangci.yml` |
| [golangci/golangci-lint](https://github.com/golangci/golangci-lint) | 19319 | 48.5MB | `.golangci.yml` |
| [goreleaser/goreleaser](https://github.com/goreleaser/goreleaser) | 15998 | 28.2MB | `.golangci.yaml` |
| [gotify/server](https://github.com/gotify/server) | 15801 | 4.9MB | `.golangci.yml` |
| [apache/answer](https://github.com/apache/answer) | 15658 | 15.4MB | `.golangci.yaml` |
| [cloudflare/cloudflared](https://github.com/cloudflare/cloudflared) | 15395 | 45.0MB | `.golangci.yaml` |
| [casdoor/casdoor](https://github.com/casdoor/casdoor) | 14276 | 45.9MB | `.golangci.yml` |
| [prometheus/node_exporter](https://github.com/prometheus/node_exporter) | 13719 | 12.9MB | `.golangci.yml` |
| [anchore/grype](https://github.com/anchore/grype) | 12790 | 10.0MB | `.golangci.yaml` |
| [IBM/sarama](https://github.com/IBM/sarama) | 12510 | 12.1MB | `.golangci.yml` |
| [crossplane/crossplane](https://github.com/crossplane/crossplane) | 11979 | 49.5MB | `.golangci.yml` |
| [charmbracelet/lipgloss](https://github.com/charmbracelet/lipgloss) | 11753 | 2.3MB | `.golangci.yml` |
| [bufbuild/buf](https://github.com/bufbuild/buf) | 11389 | 29.8MB | `.golangci.yml` |
| [pressly/goose](https://github.com/pressly/goose) | 11366 | 11.5MB | `.golangci.yaml` |
| [google/osv-scanner](https://github.com/google/osv-scanner) | 10924 | 41.3MB | `.golangci.yaml` |
| [99designs/gqlgen](https://github.com/99designs/gqlgen) | 10753 | 26.2MB | `.golangci.yml` |
| [containerd/nerdctl](https://github.com/containerd/nerdctl) | 10333 | 14.4MB | `.golangci.yml` |
| [miniflux/v2](https://github.com/miniflux/v2) | 9618 | 38.6MB | `.golangci.yml` |
| [anchore/syft](https://github.com/anchore/syft) | 9462 | 25.4MB | `.golangci.yaml` |
| [securego/gosec](https://github.com/securego/gosec) | 8933 | 6.4MB | `.golangci.yml` |
| [metallb/metallb](https://github.com/metallb/metallb) | 8333 | 51.4MB | `.golangci.yml` |
| [cloudwego/kitex](https://github.com/cloudwego/kitex) | 8026 | 14.2MB | `.golangci.yaml` |
| [cloudwego/hertz](https://github.com/cloudwego/hertz) | 7347 | 3.7MB | `.golangci.yaml` |
| [charmbracelet/soft-serve](https://github.com/charmbracelet/soft-serve) | 7202 | 21.9MB | `.golangci.yml` |
| [vektra/mockery](https://github.com/vektra/mockery) | 7154 | 42.4MB | `.golangci.yml` |
| [btcsuite/btcd](https://github.com/btcsuite/btcd) | 6703 | 28.1MB | `.golangci.yml` |
| [open-telemetry/opentelemetry-go](https://github.com/open-telemetry/opentelemetry-go) | 6526 | 31.2MB | `.golangci.yml` |
| [sigstore/cosign](https://github.com/sigstore/cosign) | 6249 | 26.1MB | `.golangci.yml` |
| [kubernetes/kube-state-metrics](https://github.com/kubernetes/kube-state-metrics) | 6184 | 25.2MB | `.golangci.yml` |
| [prometheus/client_golang](https://github.com/prometheus/client_golang) | 6020 | 5.3MB | `.golangci.yml` |
| [ory/keto](https://github.com/ory/keto) | 5390 | 36.3MB | `.golangci.yml` |
| [stern/stern](https://github.com/stern/stern) | 4849 | 0.8MB | `.golangci.yml` |
| [uber-go/nilaway](https://github.com/uber-go/nilaway) | 3900 | 1.6MB | `.golangci.yaml` |
| [charmbracelet/glamour](https://github.com/charmbracelet/glamour) | 3665 | 6.8MB | `.golangci.yml` |
| [ClickHouse/clickhouse-go](https://github.com/ClickHouse/clickhouse-go) | 3335 | 5.9MB | `.golangci.yaml` |
| [vmware-tanzu/sonobuoy](https://github.com/vmware-tanzu/sonobuoy) | 3051 | 30.3MB | `.golangci.yaml` |
| [sigstore/rekor](https://github.com/sigstore/rekor) | 1200 | 17.7MB | `.golangci.yml` |
| [typesense/typesense-go](https://github.com/typesense/typesense-go) | 317 | 0.8MB | `.golangci.yml` |
| [open-feature/go-sdk](https://github.com/open-feature/go-sdk) | 248 | 1.0MB | `.golangci.yml` |
| [netobserv/netobserv-ebpf-agent](https://github.com/netobserv/netobserv-ebpf-agent) | 209 | 45.9MB | `.golangci.yml` |

## Stale exclusions in `corpus/README.md`

`README.md` excludes fiber, hugo, etcd and terraform for "no confirmed v2".
As of this survey **fiber carries a 307-line v2 config and go-ethereum a 96-line
v2 config**; hugo, etcd and terraform have no `.golangci.yml` on their default
branch at all. The exclusion list should be re-stated per repository with its
actual reason (size, build tags, no config) rather than a shared one.

`moby/moby` also carries a v2 config (370 lines) — the existing exclusion reason
(no root `go.mod` in the public tree) still stands and is unrelated to v2.

## Method

```
244 candidate repos  →  gh api repos/{r}/contents/.golangci.{yml,yaml}
                     →  152 with version: "2"
                     →  parse linters.enable / formatters.enable / linters.settings keys
                     →  greedy set-cover against the current corpus's key set
```
`default:` mode across the 152: `none` 73, unset 67, `all` 7, `standard` 5.

