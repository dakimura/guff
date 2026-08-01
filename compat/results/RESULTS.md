# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| fixture | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| local | 108 | 108 | 108 | 100.0% | 100.0% | 0 |
| gin | 14 | 9 | 9 | 64.3% | 100.0% | 0 |
| caddy | 3 | 0 | 0 | 0.0% | 100.0% | 0 |
| helm | 6 | 5 | 5 | 83.3% | 100.0% | 0 |

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
| testifylint | 5 | 0 | 0 | 0.0% | 100.0% |

### Allowed known diffs (5)
- guff-only: `binding/json_test.go:53:testifylint:time-compare: equality-based assertion on time.Time can be flaky`
- guff-only: `context_test.go:2898:testifylint:time-compare: equality-based assertion on time.Time can be flaky`
- guff-only: `context_test.go:3147:testifylint:time-compare: equality-based assertion on time.Time can be flaky`
- guff-only: `context_test.go:511:testifylint:time-compare: equality-based assertion on time.Time can be flaky`
- guff-only: `context_test.go:871:testifylint:formatter: do not use non-string value as first element (msg) of msgAndArgs`

## caddy

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gosec | 3 | 0 | 0 | 0.0% | 100.0% |

### Allowed known diffs (3)
- guff-only: `modules/caddyhttp/reverseproxy/httptransport.go:791:gosec:G402: TLS InsecureSkipVerify may be set to true.`
- guff-only: `modules/caddyhttp/reverseproxy/selectionpolicies.go:666:gosec:G124: http.Cookie missing or has insecure Secure, HttpOnly, or SameSite attribute`
- guff-only: `modules/caddytls/capools.go:624:gosec:G402: TLS InsecureSkipVerify may be set to true.`

## helm

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| modernize | 5 | 5 | 5 | 100.0% | 100.0% |
| testifylint | 1 | 0 | 0 | 0.0% | 100.0% |

### Allowed known diffs (1)
- guff-only: `pkg/action/upgrade_test.go:417:testifylint:contains: invalid usage of req.Contains, use req.Subset for multi elements assertion`
