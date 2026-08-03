# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| fixture | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| local | 108 | 108 | 108 | 100.0% | 100.0% | 0 |
| vault | 160 | 161 | 160 | 100.0% | 99.4% | 0 |
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

## vault

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errcheck | 23 | 23 | 23 | 100.0% | 100.0% |
| govet | 63 | 63 | 63 | 100.0% | 100.0% |
| ineffassign | 2 | 2 | 2 | 100.0% | 100.0% |
| staticcheck | 68 | 69 | 68 | 100.0% | 98.6% |
| unused | 4 | 4 | 4 | 100.0% | 100.0% |

### Allowed known diffs (1)
- golangci-only: `helper/testhelpers/testhelpers.go:414:staticcheck:possible nil pointer dereference`

## kubernetes

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| govet | 5 | 5 | 5 | 100.0% | 100.0% |
