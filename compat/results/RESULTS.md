# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| fixture | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| local | 108 | 108 | 108 | 100.0% | 100.0% | 0 |
| gin | 9 | 10 | 9 | 100.0% | 90.0% | 0 |
| caddy | 1 | 6 | 1 | 100.0% | 16.7% | 0 |
| helm | 80 | 80 | 80 | 100.0% | 100.0% | 0 |
| k9s | 636 | 597 | 493 | 77.5% | 82.6% | 0 |
| cobra | 157 | 152 | 149 | 94.9% | 98.0% | 0 |
| go-client | 1 | 1 | 1 | 100.0% | 100.0% | 0 |

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
| staticcheck | 1 | 1 | 1 | 100.0% | 100.0% |

### Allowed known diffs (5)
- golangci-only: `modules/caddyhttp/fileserver/matcher.go:625:gofumpt:File is not properly formatted`
- golangci-only: `modules/caddyhttp/server.go:638:gofumpt:File is not properly formatted`
- golangci-only: `modules/caddytls/fileloader.go:120:gofumpt:File is not properly formatted`
- golangci-only: `modules/caddytls/folderloader.go:178:gofumpt:File is not properly formatted`
- golangci-only: `modules/caddytls/pemloader.go:92:gofumpt:File is not properly formatted`

## helm

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| modernize | 79 | 79 | 79 | 100.0% | 100.0% |
| staticcheck | 1 | 1 | 1 | 100.0% | 100.0% |

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
