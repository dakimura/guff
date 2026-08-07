# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| isolate-staticcheck | 11 | 11 | 11 | 100.0% | 100.0% | 0 |

Precision = |intersection| / |guff|; Recall = |intersection| / |golangci|. `unexpected` counts diffs not covered by the allowlist (`compat/allowlists/`).

## isolate-staticcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| staticcheck | 11 | 11 | 11 | 100.0% | 100.0% |
