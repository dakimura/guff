# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| fixture | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| local | 108 | 108 | 108 | 100.0% | 100.0% | 0 |
| gin | 9 | 9 | 9 | 100.0% | 100.0% | 0 |
| caddy | 0 | 0 | 0 | 100.0% | 100.0% | 0 |
| helm | 5 | 5 | 5 | 100.0% | 100.0% | 0 |
| consul | 255 | 255 | 255 | 100.0% | 100.0% | 0 |
| grafana | 0 | 0 | 0 | 100.0% | 100.0% | 0 |
| containerd | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
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

## consul

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| govet | 18 | 18 | 18 | 100.0% | 100.0% |
| staticcheck | 237 | 237 | 237 | 100.0% | 100.0% |

## grafana

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|

## containerd

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gosec | 1 | 1 | 1 | 100.0% | 100.0% |

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
