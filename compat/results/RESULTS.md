# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| fixture | 5 | 4 | 4 | 80.0% | 100.0% | 0 |
| local | 492 | 108 | 108 | 22.0% | 100.0% | 0 |

Precision = |intersection| / |guff|; Recall = |intersection| / |golangci|. `unexpected` counts diffs not covered by `allowlist.txt`.

## fixture

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errcheck | 2 | 2 | 2 | 100.0% | 100.0% |
| ineffassign | 1 | 1 | 1 | 100.0% | 100.0% |
| staticcheck | 1 | 0 | 0 | 0.0% | 100.0% |
| unused | 1 | 1 | 1 | 100.0% | 100.0% |

### Allowed known diffs (1)
- guff-only: `pkg/util.go:1:staticcheck:at least one file in a package should have a package comment`

## local

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errcheck | 12 | 12 | 12 | 100.0% | 100.0% |
| ineffassign | 300 | 12 | 12 | 4.0% | 100.0% |
| staticcheck | 168 | 72 | 72 | 42.9% | 100.0% |
| unused | 12 | 12 | 12 | 100.0% | 100.0% |

### Allowed known diffs (384)
- guff-only: `pkg00/f00.go:1:staticcheck:at least one file in a package should have a package comment`
- guff-only: `pkg00/f01.go:1:staticcheck:at least one file in a package should have a package comment`
- guff-only: `pkg00/f02.go:10:ineffassign:ineffectual assignment to sum`
- guff-only: `pkg00/f02.go:12:ineffassign:ineffectual assignment to sum`
- guff-only: `pkg00/f02.go:14:ineffassign:ineffectual assignment to k`
- guff-only: `pkg00/f02.go:1:staticcheck:at least one file in a package should have a package comment`
- guff-only: `pkg00/f02.go:8:ineffassign:ineffectual assignment to sum`
- guff-only: `pkg00/f03.go:10:ineffassign:ineffectual assignment to sum`
- … and 376 more (see `allowlist.txt`)
