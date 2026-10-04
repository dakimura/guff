# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| fixture | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| local | 108 | 108 | 108 | 100.0% | 100.0% | 0 |
| gin | 9 | 10 | 9 | 100.0% | 90.0% | 0 |
| caddy | 0 | 6 | 0 | 100.0% | 0.0% | 0 |
| helm | 76 | 80 | 76 | 100.0% | 95.0% | 0 |
| k9s | 636 | 597 | 493 | 77.5% | 82.6% | 0 |
| cobra | 157 | 152 | 149 | 94.9% | 98.0% | 0 |
| go-client | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| consul | 257 | 260 | 255 | 99.2% | 98.1% | 0 |
| grafana | 0 | 126 | 0 | 100.0% | 0.0% | 0 |
| containerd | 2 | 2 | 2 | 100.0% | 100.0% | 0 |

Precision = |intersection| / |guff|; Recall = |intersection| / |golangci|. `unexpected` counts diffs not covered by the allowlist (`compat/allowlists/`).

## fixture

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errcheck | 2 | 2 | 2 | 100.0% | 100.0% |
| ineffassign | 1 | 1 | 1 | 100.0% | 100.0% |
| unused | 1 | 1 | 1 | 100.0% | 100.0% |

## local

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errcheck | 12 | 12 | 12 | 100.0% | 100.0% |
| ineffassign | 12 | 12 | 12 | 100.0% | 100.0% |
| staticcheck | 72 | 72 | 72 | 100.0% | 100.0% |
| unused | 12 | 12 | 12 | 100.0% | 100.0% |

## gin

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gofumpt | 0 | 1 | 0 | 100.0% | 0.0% |
| gosec | 2 | 2 | 2 | 100.0% | 100.0% |
| govet | 7 | 7 | 7 | 100.0% | 100.0% |

### Allowed known diffs (1)
- golangci-only: `binding/json_test.go:134:gofumpt:File is not properly formatted`

## caddy

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gofumpt | 0 | 5 | 0 | 100.0% | 0.0% |
| staticcheck | 0 | 1 | 0 | 100.0% | 0.0% |

### Allowed known diffs (6)
- golangci-only: `caddytest/caddytest.go:341:staticcheck:(net.Dialer).DualStack has been deprecated since Go 1.12: Fast Fallback is enabled by default. To disable, set FallbackDelay to a negative value.`
- golangci-only: `modules/caddyhttp/fileserver/matcher.go:625:gofumpt:File is not properly formatted`
- golangci-only: `modules/caddyhttp/server.go:638:gofumpt:File is not properly formatted`
- golangci-only: `modules/caddytls/fileloader.go:120:gofumpt:File is not properly formatted`
- golangci-only: `modules/caddytls/folderloader.go:178:gofumpt:File is not properly formatted`
- golangci-only: `modules/caddytls/pemloader.go:92:gofumpt:File is not properly formatted`

## helm

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| modernize | 76 | 79 | 76 | 100.0% | 96.2% |
| staticcheck | 0 | 1 | 0 | 100.0% | 0.0% |

### Allowed known diffs (4)
- golangci-only: `internal/chart/v3/loader/load.go:126:modernize:stringscut: strings.SplitN call can be simplified using strings.Cut`
- golangci-only: `pkg/chart/v2/loader/load.go:156:modernize:stringscut: strings.SplitN call can be simplified using strings.Cut`
- golangci-only: `pkg/kube/client_test.go:81:staticcheck:(k8s.io/apimachinery/pkg/apis/meta/v1.ObjectMeta).SelfLink is deprecated: selfLink is a legacy read-only field that is no longer populated by the system. +optional`
- golangci-only: `pkg/registry/client.go:234:modernize:stringscut: strings.Split call can be simplified using strings.Cut`

## k9s

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errcheck | 1 | 1 | 1 | 100.0% | 100.0% |
| goconst | 626 | 587 | 483 | 77.2% | 82.3% |
| gosec | 7 | 7 | 7 | 100.0% | 100.0% |
| govet | 1 | 1 | 1 | 100.0% | 100.0% |
| intrange | 1 | 1 | 1 | 100.0% | 100.0% |

### Allowed known diffs (247)
- guff-only: `internal/config/scans_test.go:27:goconst:string `ns-2` has 8 occurrences, make it a constant`
- guff-only: `internal/config/scans_test.go:33:goconst:string `ns-1` has 5 occurrences, make it a constant`
- guff-only: `internal/dao/registry.go:209:goconst:string `xrays` has 3 occurrences, make it a constant`
- guff-only: `internal/dao/registry.go:210:goconst:string `XRays` has 3 occurrences, make it a constant`
- guff-only: `internal/dao/registry.go:211:goconst:string `xray` has 4 occurrences, make it a constant`
- guff-only: `internal/dao/registry.go:241:goconst:string `delete` has 6 occurrences, make it a constant`
- guff-only: `internal/dao/registry.go:279:goconst:string `helm` has 3 occurrences, but such constant `helmCat` already exists`
- guff-only: `internal/dao/registry.go:297:goconst:string `Rules` has 3 occurrences, make it a constant`
- … and 239 more (see `compat/allowlists/`)

## cobra

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| goconst | 156 | 151 | 148 | 94.9% | 98.0% |
| gosec | 1 | 1 | 1 | 100.0% | 100.0% |

### Allowed known diffs (11)
- guff-only: `command.go:1192:goconst:string `true` has 6 occurrences, make it a constant`
- guff-only: `completions.go:802:goconst:string `bash` has 6 occurrences, make it a constant`
- guff-only: `completions.go:837:goconst:string `zsh` has 6 occurrences, make it a constant`
- guff-only: `completions.go:876:goconst:string `fish` has 6 occurrences, make it a constant`
- guff-only: `completions.go:901:goconst:string `powershell` has 6 occurrences, make it a constant`
- guff-only: `completions_test.go:2548:goconst:string `bash` has 6 occurrences, make it a constant`
- guff-only: `completions_test.go:3879:goconst:string `true` has 6 occurrences, make it a constant`
- guff-only: `shell_completions.go:39:goconst:string `true` has 6 occurrences, make it a constant`
- … and 3 more (see `compat/allowlists/`)

## go-client

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| goconst | 1 | 1 | 1 | 100.0% | 100.0% |

## consul

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gofmt | 0 | 1 | 0 | 100.0% | 0.0% |
| govet | 18 | 18 | 18 | 100.0% | 100.0% |
| staticcheck | 239 | 241 | 237 | 99.2% | 98.3% |

### Allowed known diffs (7)
- guff-only: `agent/event_endpoint_test.go:115:staticcheck:err refers to the result of a failed type assertion and is a zero value, not the value that was being type-asserted`
- guff-only: `agent/http_test.go:1728:staticcheck:err refers to the result of a failed type assertion and is a zero value, not the value that was being type-asserted`
- golangci-only: `agent/agent.go:3191:staticcheck:this comparison is always true`
- golangci-only: `agent/catalog_endpoint_test.go:1148:staticcheck:this value of req is never used`
- golangci-only: `agent/checks/check.go:1131:staticcheck:this comparison is never true`
- golangci-only: `agent/consul/state/config_entry.go:526:staticcheck:this comparison is always true`
- golangci-only: `agent/proxycfg/state_test.go:3768:gofmt:File is not properly formatted`

## grafana

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| exhaustive | 0 | 1 | 0 | 100.0% | 0.0% |
| goimports | 0 | 4 | 0 | 100.0% | 0.0% |
| gosec | 0 | 2 | 0 | 100.0% | 0.0% |
| staticcheck | 0 | 119 | 0 | 100.0% | 0.0% |

### Allowed known diffs (126)
- golangci-only: `pkg/api/annotations.go:145:staticcheck:(github.com/grafana/grafana/pkg/services/annotations.Item).DashboardID is deprecated: Use DashboardUID and OrgID instead`
- golangci-only: `pkg/api/annotations.go:420:staticcheck:(github.com/grafana/grafana/pkg/services/annotations.DeleteParams).DashboardID is deprecated: Use DashboardUID and OrgID instead`
- golangci-only: `pkg/api/annotations.go:44:staticcheck:(github.com/grafana/grafana/pkg/services/annotations.ItemQuery).DashboardID is deprecated: Use DashboardUID and OrgID instead`
- golangci-only: `pkg/api/annotations_test.go:331:staticcheck:(github.com/grafana/grafana/pkg/services/annotations.Item).DashboardID is deprecated: Use DashboardUID and OrgID instead`
- golangci-only: `pkg/api/annotations_test.go:332:staticcheck:(github.com/grafana/grafana/pkg/services/annotations.Item).DashboardID is deprecated: Use DashboardUID and OrgID instead`
- golangci-only: `pkg/api/annotations_test.go:336:staticcheck:(github.com/grafana/grafana/pkg/services/dashboards.Dashboard).FolderID is deprecated: use FolderUID instead`
- golangci-only: `pkg/api/annotations_test.go:338:staticcheck:(github.com/grafana/grafana/pkg/services/folder.Folder).ID is deprecated: use UID instead`
- golangci-only: `pkg/api/annotations_test.go:369:staticcheck:(github.com/grafana/grafana/pkg/services/annotations.Item).DashboardID is deprecated: Use DashboardUID and OrgID instead`
- … and 118 more (see `compat/allowlists/`)

## containerd

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gosec | 1 | 1 | 1 | 100.0% | 100.0% |
| modernize | 1 | 1 | 1 | 100.0% | 100.0% |
