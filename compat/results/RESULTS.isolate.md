# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| isolate-errcheck | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-ineffassign | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-unused | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-govet | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-staticcheck | 11 | 11 | 10 | 90.9% | 90.9% | 0 |
| isolate-misspell | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-dogsled | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-nakedret | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-whitespace | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-gochecknoglobals | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-gochecknoinits | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-godot | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-dupword | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-godox | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-nlreturn | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-prealloc | 0 | 0 | 0 | 100.0% | 100.0% | 0 |
| isolate-goconst | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-usestdlibvars | 0 | 0 | 0 | 100.0% | 100.0% | 0 |
| isolate-goprintffuncname | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-nestif | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-lll | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-asciicheck | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-unconvert | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-durationcheck | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-errname | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-copyloopvar | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-nosprintfhostport | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-nilnil | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-recvcheck | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-interfacebloat | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-nonamedreturns | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-inamedparam | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-forbidigo | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-perfsprint | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-tagalign | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-modernize | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-wastedassign | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-decorder | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-funlen | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-maintidx | 0 | 0 | 0 | 100.0% | 100.0% | 0 |
| isolate-forcetypeassert | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-makezero | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-err113 | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-errorlint | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-predeclared | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-noinlineerr | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-bidichk | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-containedctx | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-iotamixing | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-asasalint | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-exhaustive | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-exhaustruct | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-funcorder | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-embeddedstructfieldcheck | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-mnd | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-cyclop | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-gocyclo | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-gocognit | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-intrange | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-mirror | 0 | 0 | 0 | 100.0% | 100.0% | 0 |
| isolate-nilerr | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-wrapcheck | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-fatcontext | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-noctx | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-musttag | 0 | 0 | 0 | 100.0% | 100.0% | 0 |
| isolate-reassign | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-tagliatelle | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-canonicalheader | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-ireturn | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-iface | 0 | 0 | 0 | 100.0% | 100.0% | 0 |
| isolate-varnamelen | 0 | 0 | 0 | 100.0% | 100.0% | 0 |
| isolate-godoclint | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-nilnesserr | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-errchkjson | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-bodyclose | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-rowserrcheck | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-sqlclosecheck | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-contextcheck | 0 | 0 | 0 | 100.0% | 100.0% | 0 |
| isolate-wsl | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-gocritic | 6 | 6 | 6 | 100.0% | 100.0% | 0 |
| isolate-revive | 7 | 7 | 7 | 100.0% | 100.0% | 0 |
| isolate-gosec | 7 | 7 | 7 | 100.0% | 100.0% | 0 |
| isolate-unparam | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-dupl | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-grouper | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-sloglint | 0 | 0 | 0 | 100.0% | 100.0% | 0 |
| isolate-loggercheck | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-thelper | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-testpackage | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-paralleltest | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-tparallel | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-usetesting | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-gocheckcompilerdirectives | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-gochecksumtype | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-gosmopolitan | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-unqueryvet | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-testableexamples | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-gomoddirectives | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-goheader | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-importas | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-depguard | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-protogetter | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-gomodguard | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-testifylint | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-exptostd | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-zerologlint | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-spancheck | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-promlinter | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-ginkgolinter | 6 | 6 | 6 | 100.0% | 100.0% | 0 |
| isolate-clickhouselint | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-arangolint | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-nolintlint | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-gomodguard_v2 | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-wsl_v5 | 3 | 3 | 3 | 100.0% | 100.0% | 0 |

Precision = |intersection| / |guff|; Recall = |intersection| / |golangci|. `unexpected` counts diffs not covered by the allowlist (`compat/allowlists/`).

## isolate-errcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errcheck | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-ineffassign

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| ineffassign | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-unused

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| unused | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-govet

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| govet | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-staticcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| staticcheck | 11 | 11 | 10 | 90.9% | 90.9% |

### Allowed known diffs (2)
- guff-only: `bad.go:81:staticcheck:possible nil pointer dereference`
- golangci-only: `bad.go:72:staticcheck:could remove embedded field "meta" from selector`

## isolate-misspell

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| misspell | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-dogsled

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| dogsled | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-nakedret

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nakedret | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-whitespace

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| whitespace | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-gochecknoglobals

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gochecknoglobals | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-gochecknoinits

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gochecknoinits | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-godot

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| godot | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-dupword

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| dupword | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-godox

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| godox | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-nlreturn

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nlreturn | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-prealloc

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|

## isolate-goconst

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| goconst | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-usestdlibvars

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|

## isolate-goprintffuncname

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| goprintffuncname | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-nestif

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nestif | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-lll

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| lll | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-asciicheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| asciicheck | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-unconvert

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| unconvert | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-durationcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| durationcheck | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-errname

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errname | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-copyloopvar

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| copyloopvar | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-nosprintfhostport

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nosprintfhostport | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-nilnil

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nilnil | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-recvcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| recvcheck | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-interfacebloat

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| interfacebloat | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-nonamedreturns

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nonamedreturns | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-inamedparam

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| inamedparam | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-forbidigo

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| forbidigo | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-perfsprint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| perfsprint | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-tagalign

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| tagalign | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-modernize

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| modernize | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-wastedassign

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| wastedassign | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-decorder

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| decorder | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-funlen

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| funlen | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-maintidx

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|

## isolate-forcetypeassert

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| forcetypeassert | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-makezero

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| makezero | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-err113

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| err113 | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-errorlint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errorlint | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-predeclared

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| predeclared | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-noinlineerr

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| noinlineerr | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-bidichk

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| bidichk | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-containedctx

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| containedctx | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-iotamixing

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| iotamixing | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-asasalint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| asasalint | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-exhaustive

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| exhaustive | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-exhaustruct

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| exhaustruct | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-funcorder

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| funcorder | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-embeddedstructfieldcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| embeddedstructfieldcheck | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-mnd

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| mnd | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-cyclop

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| cyclop | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-gocyclo

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gocyclo | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-gocognit

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gocognit | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-intrange

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| intrange | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-mirror

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|

## isolate-nilerr

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nilerr | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-wrapcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| wrapcheck | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-fatcontext

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| fatcontext | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-noctx

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| noctx | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-musttag

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|

## isolate-reassign

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| reassign | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-tagliatelle

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| tagliatelle | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-canonicalheader

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| canonicalheader | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-ireturn

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| ireturn | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-iface

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|

## isolate-varnamelen

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|

## isolate-godoclint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| godoclint | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-nilnesserr

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nilnesserr | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-errchkjson

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errchkjson | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-bodyclose

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| bodyclose | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-rowserrcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| rowserrcheck | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-sqlclosecheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| sqlclosecheck | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-contextcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|

## isolate-wsl

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| wsl | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-gocritic

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gocritic | 6 | 6 | 6 | 100.0% | 100.0% |

## isolate-revive

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| revive | 7 | 7 | 7 | 100.0% | 100.0% |

## isolate-gosec

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gosec | 7 | 7 | 7 | 100.0% | 100.0% |

## isolate-unparam

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| unparam | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-dupl

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| dupl | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-grouper

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| grouper | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-sloglint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|

## isolate-loggercheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| loggercheck | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-thelper

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| thelper | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-testpackage

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| testpackage | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-paralleltest

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| paralleltest | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-tparallel

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| tparallel | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-usetesting

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| usetesting | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-gocheckcompilerdirectives

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gocheckcompilerdirectives | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-gochecksumtype

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gochecksumtype | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-gosmopolitan

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gosmopolitan | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-unqueryvet

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| unqueryvet | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-testableexamples

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| testableexamples | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-gomoddirectives

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gomoddirectives | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-goheader

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| goheader | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-importas

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| importas | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-depguard

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| depguard | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-protogetter

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| protogetter | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-gomodguard

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gomodguard | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-testifylint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| testifylint | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-exptostd

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| exptostd | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-zerologlint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| zerologlint | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-spancheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| spancheck | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-promlinter

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| promlinter | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-ginkgolinter

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| ginkgolinter | 6 | 6 | 6 | 100.0% | 100.0% |

## isolate-clickhouselint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| clickhouselint | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-arangolint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| arangolint | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-nolintlint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nolintlint | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-gomodguard_v2

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gomodguard_v2 | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-wsl_v5

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| wsl_v5 | 3 | 3 | 3 | 100.0% | 100.0% |
