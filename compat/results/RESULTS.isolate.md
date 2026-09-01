# Compatibility report (guff vs golangci-lint)

| Target | guff | golangci | both | P | R | unexpected |
|--------|-----:|---------:|-----:|--:|--:|-----------:|
| isolate-errcheck | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-ineffassign | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-unused | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-govet | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-staticcheck | 11 | 11 | 11 | 100.0% | 100.0% | 0 |
| isolate-misspell | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-dogsled | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-nakedret | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-whitespace | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-gochecknoglobals | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-gochecknoinits | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-godot | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-dupword | 5 | 5 | 5 | 100.0% | 100.0% | 0 |
| isolate-godox | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-nlreturn | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-prealloc | 5 | 5 | 5 | 100.0% | 100.0% | 0 |
| isolate-goconst | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-usestdlibvars | 5 | 5 | 5 | 100.0% | 100.0% | 0 |
| isolate-goprintffuncname | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-nestif | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-lll | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-asciicheck | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-unconvert | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-durationcheck | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-errname | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-copyloopvar | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-nosprintfhostport | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-nilnil | 6 | 6 | 6 | 100.0% | 100.0% | 0 |
| isolate-recvcheck | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-interfacebloat | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-nonamedreturns | 14 | 14 | 14 | 100.0% | 100.0% | 0 |
| isolate-inamedparam | 5 | 5 | 5 | 100.0% | 100.0% | 0 |
| isolate-forbidigo | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-perfsprint | 7 | 7 | 7 | 100.0% | 100.0% | 0 |
| isolate-tagalign | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-modernize | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-wastedassign | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-decorder | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-funlen | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-maintidx | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-forcetypeassert | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-makezero | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-err113 | 6 | 6 | 6 | 100.0% | 100.0% | 0 |
| isolate-errorlint | 6 | 6 | 6 | 100.0% | 100.0% | 0 |
| isolate-predeclared | 9 | 9 | 9 | 100.0% | 100.0% | 0 |
| isolate-noinlineerr | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-bidichk | 9 | 9 | 9 | 100.0% | 100.0% | 0 |
| isolate-containedctx | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-iotamixing | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-asasalint | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-exhaustive | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-exhaustruct | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-funcorder | 6 | 6 | 6 | 100.0% | 100.0% | 0 |
| isolate-embeddedstructfieldcheck | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-mnd | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-cyclop | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-gocyclo | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-gocognit | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-intrange | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-mirror | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-nilerr | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-wrapcheck | 9 | 9 | 9 | 100.0% | 100.0% | 0 |
| isolate-fatcontext | 6 | 6 | 6 | 100.0% | 100.0% | 0 |
| isolate-noctx | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-musttag | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-reassign | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-tagliatelle | 31 | 31 | 31 | 100.0% | 100.0% | 0 |
| isolate-canonicalheader | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-ireturn | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-iface | 5 | 5 | 5 | 100.0% | 100.0% | 0 |
| isolate-varnamelen | 6 | 6 | 6 | 100.0% | 100.0% | 0 |
| isolate-godoclint | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-nilnesserr | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-errchkjson | 5 | 5 | 5 | 100.0% | 100.0% | 0 |
| isolate-bodyclose | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-rowserrcheck | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-sqlclosecheck | 7 | 7 | 7 | 100.0% | 100.0% | 0 |
| isolate-contextcheck | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-wsl | 7 | 7 | 7 | 100.0% | 100.0% | 0 |
| isolate-gocritic | 6 | 6 | 6 | 100.0% | 100.0% | 0 |
| isolate-revive | 7 | 7 | 7 | 100.0% | 100.0% | 0 |
| isolate-gosec | 8 | 8 | 8 | 100.0% | 100.0% | 0 |
| isolate-unparam | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-dupl | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-grouper | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-sloglint | 10 | 10 | 10 | 100.0% | 100.0% | 0 |
| isolate-loggercheck | 6 | 6 | 6 | 100.0% | 100.0% | 0 |
| isolate-thelper | 6 | 6 | 6 | 100.0% | 100.0% | 0 |
| isolate-testpackage | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-paralleltest | 5 | 5 | 5 | 100.0% | 100.0% | 0 |
| isolate-tparallel | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-usetesting | 7 | 7 | 7 | 100.0% | 100.0% | 0 |
| isolate-gocheckcompilerdirectives | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-gochecksumtype | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-gosmopolitan | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-unqueryvet | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-testableexamples | 5 | 5 | 5 | 100.0% | 100.0% | 0 |
| isolate-gomoddirectives | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-goheader | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-importas | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-depguard | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-protogetter | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-gomodguard | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-testifylint | 3 | 3 | 3 | 100.0% | 100.0% | 0 |
| isolate-exptostd | 4 | 4 | 4 | 100.0% | 100.0% | 0 |
| isolate-zerologlint | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-spancheck | 6 | 6 | 6 | 100.0% | 100.0% | 0 |
| isolate-promlinter | 5 | 5 | 5 | 100.0% | 100.0% | 0 |
| isolate-ginkgolinter | 8 | 8 | 8 | 100.0% | 100.0% | 0 |
| isolate-clickhouselint | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-arangolint | 2 | 2 | 2 | 100.0% | 100.0% | 0 |
| isolate-nolintlint | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-gomodguard_v2 | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-wsl_v5 | 6 | 6 | 6 | 100.0% | 100.0% | 0 |
| isolate-golines | 1 | 1 | 1 | 100.0% | 100.0% | 0 |
| isolate-swaggo | 1 | 1 | 1 | 100.0% | 100.0% | 0 |

Precision = |intersection| / |guff|; Recall = |intersection| / |golangci|. `unexpected` counts diffs not covered by the allowlist (`compat/allowlists/`).

## isolate-errcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errcheck | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-ineffassign

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| ineffassign | 2 | 2 | 2 | 100.0% | 100.0% |

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
| staticcheck | 11 | 11 | 11 | 100.0% | 100.0% |

## isolate-misspell

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| misspell | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-dogsled

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| dogsled | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-nakedret

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nakedret | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-whitespace

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| whitespace | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-gochecknoglobals

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gochecknoglobals | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-gochecknoinits

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gochecknoinits | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-godot

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| godot | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-dupword

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| dupword | 5 | 5 | 5 | 100.0% | 100.0% |

## isolate-godox

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| godox | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-nlreturn

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nlreturn | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-prealloc

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| prealloc | 5 | 5 | 5 | 100.0% | 100.0% |

## isolate-goconst

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| goconst | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-usestdlibvars

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| usestdlibvars | 5 | 5 | 5 | 100.0% | 100.0% |

## isolate-goprintffuncname

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| goprintffuncname | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-nestif

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nestif | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-lll

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| lll | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-asciicheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| asciicheck | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-unconvert

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| unconvert | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-durationcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| durationcheck | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-errname

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errname | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-copyloopvar

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| copyloopvar | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-nosprintfhostport

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nosprintfhostport | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-nilnil

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nilnil | 6 | 6 | 6 | 100.0% | 100.0% |

## isolate-recvcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| recvcheck | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-interfacebloat

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| interfacebloat | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-nonamedreturns

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nonamedreturns | 14 | 14 | 14 | 100.0% | 100.0% |

## isolate-inamedparam

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| inamedparam | 5 | 5 | 5 | 100.0% | 100.0% |

## isolate-forbidigo

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| forbidigo | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-perfsprint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| perfsprint | 7 | 7 | 7 | 100.0% | 100.0% |

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
| wastedassign | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-decorder

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| decorder | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-funlen

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| funlen | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-maintidx

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| maintidx | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-forcetypeassert

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| forcetypeassert | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-makezero

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| makezero | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-err113

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| err113 | 6 | 6 | 6 | 100.0% | 100.0% |

## isolate-errorlint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errorlint | 6 | 6 | 6 | 100.0% | 100.0% |

## isolate-predeclared

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| predeclared | 9 | 9 | 9 | 100.0% | 100.0% |

## isolate-noinlineerr

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| noinlineerr | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-bidichk

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| bidichk | 9 | 9 | 9 | 100.0% | 100.0% |

## isolate-containedctx

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| containedctx | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-iotamixing

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| iotamixing | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-asasalint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| asasalint | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-exhaustive

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| exhaustive | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-exhaustruct

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| exhaustruct | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-funcorder

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| funcorder | 6 | 6 | 6 | 100.0% | 100.0% |

## isolate-embeddedstructfieldcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| embeddedstructfieldcheck | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-mnd

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| mnd | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-cyclop

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| cyclop | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-gocyclo

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gocyclo | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-gocognit

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gocognit | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-intrange

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| intrange | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-mirror

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| mirror | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-nilerr

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nilerr | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-wrapcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| wrapcheck | 9 | 9 | 9 | 100.0% | 100.0% |

## isolate-fatcontext

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| fatcontext | 6 | 6 | 6 | 100.0% | 100.0% |

## isolate-noctx

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| noctx | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-musttag

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| musttag | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-reassign

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| reassign | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-tagliatelle

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| tagliatelle | 31 | 31 | 31 | 100.0% | 100.0% |

## isolate-canonicalheader

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| canonicalheader | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-ireturn

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| ireturn | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-iface

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| iface | 5 | 5 | 5 | 100.0% | 100.0% |

## isolate-varnamelen

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| varnamelen | 6 | 6 | 6 | 100.0% | 100.0% |

## isolate-godoclint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| godoclint | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-nilnesserr

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| nilnesserr | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-errchkjson

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| errchkjson | 5 | 5 | 5 | 100.0% | 100.0% |

## isolate-bodyclose

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| bodyclose | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-rowserrcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| rowserrcheck | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-sqlclosecheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| sqlclosecheck | 7 | 7 | 7 | 100.0% | 100.0% |

## isolate-contextcheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| contextcheck | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-wsl

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| wsl | 7 | 7 | 7 | 100.0% | 100.0% |

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
| gosec | 8 | 8 | 8 | 100.0% | 100.0% |

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
| sloglint | 10 | 10 | 10 | 100.0% | 100.0% |

## isolate-loggercheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| loggercheck | 6 | 6 | 6 | 100.0% | 100.0% |

## isolate-thelper

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| thelper | 6 | 6 | 6 | 100.0% | 100.0% |

## isolate-testpackage

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| testpackage | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-paralleltest

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| paralleltest | 5 | 5 | 5 | 100.0% | 100.0% |

## isolate-tparallel

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| tparallel | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-usetesting

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| usetesting | 7 | 7 | 7 | 100.0% | 100.0% |

## isolate-gocheckcompilerdirectives

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gocheckcompilerdirectives | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-gochecksumtype

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gochecksumtype | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-gosmopolitan

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gosmopolitan | 3 | 3 | 3 | 100.0% | 100.0% |

## isolate-unqueryvet

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| unqueryvet | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-testableexamples

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| testableexamples | 5 | 5 | 5 | 100.0% | 100.0% |

## isolate-gomoddirectives

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| gomoddirectives | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-goheader

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| goheader | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-importas

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| importas | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-depguard

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| depguard | 3 | 3 | 3 | 100.0% | 100.0% |

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
| exptostd | 4 | 4 | 4 | 100.0% | 100.0% |

## isolate-zerologlint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| zerologlint | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-spancheck

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| spancheck | 6 | 6 | 6 | 100.0% | 100.0% |

## isolate-promlinter

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| promlinter | 5 | 5 | 5 | 100.0% | 100.0% |

## isolate-ginkgolinter

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| ginkgolinter | 8 | 8 | 8 | 100.0% | 100.0% |

## isolate-clickhouselint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| clickhouselint | 2 | 2 | 2 | 100.0% | 100.0% |

## isolate-arangolint

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| arangolint | 2 | 2 | 2 | 100.0% | 100.0% |

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
| wsl_v5 | 6 | 6 | 6 | 100.0% | 100.0% |

## isolate-golines

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| golines | 1 | 1 | 1 | 100.0% | 100.0% |

## isolate-swaggo

| Linter | guff | golangci | both | P | R |
|--------|-----:|---------:|-----:|--:|--:|
| swaggo | 1 | 1 | 1 | 100.0% | 100.0% |
