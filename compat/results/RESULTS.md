# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| fixture | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| local | 108 | 108 | 108 | 100.0% | 100.0% | 0 |
| gin | 36 | 9 | 7 | 19.4% | 77.8% | 0 |
| caddy | 70 | 0 | 0 | 0.0% | 100.0% | 0 |
| helm | 5 | 5 | 5 | 100.0% | 100.0% | 0 |

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
| gofmt | 1 | 0 | 0 | 0.0% | 100.0% |
| gofumpt | 1 | 0 | 0 | 0.0% | 100.0% |
| gosec | 0 | 2 | 0 | 100.0% | 0.0% |
| govet | 9 | 7 | 7 | 77.8% | 100.0% |
| ineffassign | 1 | 0 | 0 | 0.0% | 100.0% |
| nolintlint | 24 | 0 | 0 | 0.0% | 100.0% |

### Allowed known diffs (31)
- guff-only: `binding/binding_nomsgpack.go:74:gofmt:File is not properly formatted`
- guff-only: `binding/binding_nomsgpack.go:74:gofumpt:File is not properly formatted`
- guff-only: `binding/binding_test.go:1131:govet:struct field tag "form:idx" not compatible with reflect.StructTag.Get: bad syntax for struct tag value`
- guff-only: `binding/binding_test.go:1140:govet:struct field tag "form:name" not compatible with reflect.StructTag.Get: bad syntax for struct tag value`
- guff-only: `codec/json/json.go:18:ineffassign:ineffectual assignment to API`
- guff-only: `context.go:1389:nolintlint:directive `//nolint: errcheck` is unused for linter "errcheck"`
- guff-only: `context.go:801:nolintlint:directive `//nolint: errcheck` is unused for linter "errcheck"`
- guff-only: `context.go:820:nolintlint:directive `//nolint: errcheck` is unused for linter "errcheck"`
- … and 23 more (see `compat/allowlists/`)

## caddy

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errcheck | 3 | 0 | 0 | 0.0% | 100.0% |
| gci | 8 | 0 | 0 | 0.0% | 100.0% |
| gofmt | 2 | 0 | 0 | 0.0% | 100.0% |
| gofumpt | 7 | 0 | 0 | 0.0% | 100.0% |
| goimports | 2 | 0 | 0 | 0.0% | 100.0% |
| govet | 8 | 0 | 0 | 0.0% | 100.0% |
| ineffassign | 3 | 0 | 0 | 0.0% | 100.0% |
| staticcheck | 27 | 0 | 0 | 0.0% | 100.0% |
| wastedassign | 10 | 0 | 0 | 0.0% | 100.0% |

### Allowed known diffs (70)
- guff-only: `admin.go:1378:govet:struct field repeats json tag "-"`
- guff-only: `caddyconfig/caddyfile/importgraph.go:101:staticcheck:should merge variable declaration with assignment on next line`
- guff-only: `caddyconfig/caddyfile/parse.go:638:staticcheck:should merge variable declaration with assignment on next line`
- guff-only: `caddyconfig/httpcaddyfile/builtins.go:498:staticcheck:possible nil pointer dereference`
- guff-only: `caddyconfig/httpcaddyfile/builtins.go:499:staticcheck:possible nil pointer dereference`
- guff-only: `caddyconfig/httpcaddyfile/options.go:134:ineffassign:ineffectual assignment to directiveOrder`
- guff-only: `caddyconfig/httpcaddyfile/options.go:142:ineffassign:ineffectual assignment to directiveOrder`
- guff-only: `caddyconfig/httpcaddyfile/options.go:174:ineffassign:ineffectual assignment to directiveOrder`
- … and 62 more (see `compat/allowlists/`)

## helm

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| modernize | 5 | 5 | 5 | 100.0% | 100.0% |
