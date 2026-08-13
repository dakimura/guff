# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| fixture | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| local | 108 | 108 | 108 | 100.0% | 100.0% | 0 |
| controller-runtime | 309 | 300 | 300 | 97.1% | 100.0% | 0 |
| vault | 161 | 161 | 161 | 100.0% | 100.0% | 0 |
| kubernetes | 5 | 5 | 5 | 100.0% | 100.0% | 0 |

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

## controller-runtime

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| bodyclose | 1 | 0 | 0 | 0.0% | 100.0% |
| goconst | 294 | 294 | 294 | 100.0% | 100.0% |
| govet | 5 | 5 | 5 | 100.0% | 100.0% |
| nilerr | 1 | 0 | 0 | 0.0% | 100.0% |
| nolintlint | 6 | 1 | 1 | 16.7% | 100.0% |
| unparam | 2 | 0 | 0 | 0.0% | 100.0% |

### Allowed known diffs (9)
- guff-only: `examples/builtins/validatingwebhook.go:55:unparam:(*podValidator).ValidateUpdate - oldObj is unused`
- guff-only: `examples/tokenreview/tokenreview.go:32:unparam:(*authenticator).Handle - ctx is unused`
- guff-only: `pkg/cluster/cluster_test.go:168:nolintlint:directive `//nolint:staticcheck` is unused for linter "staticcheck"`
- guff-only: `pkg/internal/controller/controller.go:395:nilerr:error is not nil (line 394) but it returns nil`
- guff-only: `pkg/manager/internal.go:264:nolintlint:directive `//nolint:staticcheck` is unused for linter "staticcheck"`
- guff-only: `pkg/manager/manager_test.go:1361:bodyclose:response body must be closed`
- guff-only: `pkg/manager/manager_test.go:1819:nolintlint:directive `//nolint:staticcheck` is unused for linter "staticcheck"`
- guff-only: `pkg/manager/manager_test.go:1994:nolintlint:directive `//nolint:staticcheck` is unused for linter "staticcheck"`
- … and 1 more (see `compat/allowlists/`)

## vault

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errcheck | 23 | 23 | 23 | 100.0% | 100.0% |
| govet | 63 | 63 | 63 | 100.0% | 100.0% |
| ineffassign | 2 | 2 | 2 | 100.0% | 100.0% |
| staticcheck | 69 | 69 | 69 | 100.0% | 100.0% |
| unused | 4 | 4 | 4 | 100.0% | 100.0% |

## kubernetes

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| govet | 5 | 5 | 5 | 100.0% | 100.0% |
