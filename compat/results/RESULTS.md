# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| fixture | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| local | 108 | 108 | 108 | 100.0% | 100.0% | 0 |
| gin | 9 | 9 | 9 | 100.0% | 100.0% | 0 |
| caddy | 0 | 0 | 0 | 100.0% | 100.0% | 0 |
| helm | 5 | 5 | 5 | 100.0% | 100.0% | 0 |
| k9s | 636 | 636 | 636 | 100.0% | 100.0% | 0 |
| cobra | 157 | 157 | 157 | 100.0% | 100.0% | 0 |
| consul | 257 | 255 | 255 | 99.2% | 100.0% | 0 |
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

## gin

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gosec | 2 | 2 | 2 | 100.0% | 100.0% |
| govet | 7 | 7 | 7 | 100.0% | 100.0% |

## caddy

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|

## helm

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| modernize | 5 | 5 | 5 | 100.0% | 100.0% |

## k9s

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errcheck | 1 | 1 | 1 | 100.0% | 100.0% |
| goconst | 626 | 626 | 626 | 100.0% | 100.0% |
| gosec | 7 | 7 | 7 | 100.0% | 100.0% |
| govet | 1 | 1 | 1 | 100.0% | 100.0% |
| intrange | 1 | 1 | 1 | 100.0% | 100.0% |

## cobra

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| goconst | 156 | 156 | 156 | 100.0% | 100.0% |
| gosec | 1 | 1 | 1 | 100.0% | 100.0% |

## consul

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| govet | 18 | 18 | 18 | 100.0% | 100.0% |
| staticcheck | 239 | 237 | 237 | 99.2% | 100.0% |

### Allowed known diffs (2)
- guff-only: `agent/event_endpoint_test.go:115:staticcheck:err refers to the result of a failed type assertion and is a zero value, not the value that was being type-asserted`
- guff-only: `agent/http_test.go:1728:staticcheck:err refers to the result of a failed type assertion and is a zero value, not the value that was being type-asserted`

## grafana

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|

## containerd

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gosec | 1 | 1 | 1 | 100.0% | 100.0% |
