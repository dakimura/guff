# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| fixture | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| local | 108 | 108 | 108 | 100.0% | 100.0% | 0 |
| consul | 258 | 255 | 255 | 98.8% | 100.0% | 0 |
| grafana | 0 | 0 | 0 | 100.0% | 100.0% | 0 |
| containerd | 1 | 1 | 1 | 100.0% | 100.0% | 0 |

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

## consul

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| govet | 18 | 18 | 18 | 100.0% | 100.0% |
| staticcheck | 240 | 237 | 237 | 98.8% | 100.0% |

### Allowed known diffs (3)
- guff-only: `agent/consul/catalog_endpoint.go:280:staticcheck:possible nil pointer dereference`
- guff-only: `agent/event_endpoint_test.go:115:staticcheck:err refers to the result of a failed type assertion and is a zero value, not the value that was being type-asserted`
- guff-only: `agent/http_test.go:1728:staticcheck:err refers to the result of a failed type assertion and is a zero value, not the value that was being type-asserted`

## grafana

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|

## containerd

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gosec | 1 | 1 | 1 | 100.0% | 100.0% |
