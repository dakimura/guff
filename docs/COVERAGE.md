# チェック単位カバレッジ台帳（COMPAT-HARDENING Phase 0）

> 自動生成: `./compat/coverage.py inventory && ./compat/coverage.py observe && ./compat/coverage.py report`。
> 手で編集しない。計画は [`COMPAT-HARDENING.md`](COMPAT-HARDENING.md)。

**`never` = どのテストでも一度も発火していない = 完全未検証。**
recall バグがあっても既存のどのゲートにも現れない。ここが Phase 3 のターゲットリスト。

| 状態 | 意味 | 件数 | 割合 |
|------|------|-----:|-----:|
| `fired` | golden / isolate / OSS / regress の実行で発火した | 310 | 56.6% |
| `unit-only` | Rust 単体テストが ID に言及するのみ（静的スキャン。golangci-lint との突合なし） | 105 | 19.2% |
| `never` | **どこでも発火していない** | 133 | 24.3% |
| — | **合計** | **548** | 100.0% |

## linter 別

| linter | checks | fired | unit-only | never |
|--------|-------:|------:|----------:|------:|
| arangolint | 1 | 1 | 0 | 0 |
| asasalint | 1 | 1 | 0 | 0 |
| asciicheck | 1 | 1 | 0 | 0 |
| bidichk | 1 | 1 | 0 | 0 |
| bodyclose | 1 | 1 | 0 | 0 |
| canonicalheader | 1 | 1 | 0 | 0 |
| clickhouselint | 1 | 1 | 0 | 0 |
| containedctx | 1 | 1 | 0 | 0 |
| contextcheck | 1 | 1 | 0 | 0 |
| copyloopvar | 1 | 1 | 0 | 0 |
| cyclop | 1 | 1 | 0 | 0 |
| decorder | 1 | 1 | 0 | 0 |
| depguard | 1 | 1 | 0 | 0 |
| dogsled | 1 | 1 | 0 | 0 |
| dupl | 1 | 1 | 0 | 0 |
| dupword | 1 | 1 | 0 | 0 |
| durationcheck | 1 | 1 | 0 | 0 |
| embeddedstructfieldcheck | 1 | 1 | 0 | 0 |
| err113 | 1 | 1 | 0 | 0 |
| errcheck | 1 | 1 | 0 | 0 |
| errchkjson | 1 | 1 | 0 | 0 |
| errname | 1 | 1 | 0 | 0 |
| errorlint | 1 | 1 | 0 | 0 |
| exhaustive | 1 | 1 | 0 | 0 |
| exhaustruct | 1 | 1 | 0 | 0 |
| exptostd | 1 | 1 | 0 | 0 |
| fatcontext | 1 | 1 | 0 | 0 |
| forbidigo | 1 | 1 | 0 | 0 |
| forcetypeassert | 1 | 1 | 0 | 0 |
| funcorder | 1 | 1 | 0 | 0 |
| funlen | 1 | 1 | 0 | 0 |
| gci | 1 | 1 | 0 | 0 |
| ginkgolinter | 1 | 1 | 0 | 0 |
| gocheckcompilerdirectives | 1 | 1 | 0 | 0 |
| gochecknoglobals | 1 | 1 | 0 | 0 |
| gochecknoinits | 1 | 1 | 0 | 0 |
| gochecksumtype | 1 | 1 | 0 | 0 |
| gocognit | 1 | 1 | 0 | 0 |
| goconst | 1 | 1 | 0 | 0 |
| gocritic | 107 | 106 | 0 | 1 |
| gocyclo | 1 | 1 | 0 | 0 |
| godoclint | 1 | 1 | 0 | 0 |
| godot | 1 | 1 | 0 | 0 |
| godox | 1 | 1 | 0 | 0 |
| gofmt | 1 | 1 | 0 | 0 |
| gofumpt | 1 | 1 | 0 | 0 |
| goheader | 1 | 1 | 0 | 0 |
| goimports | 1 | 1 | 0 | 0 |
| golines | 1 | 0 | 1 | 0 |
| gomoddirectives | 1 | 1 | 0 | 0 |
| gomodguard | 1 | 1 | 0 | 0 |
| gomodguard_v2 | 1 | 1 | 0 | 0 |
| goprintffuncname | 1 | 1 | 0 | 0 |
| gosec | 35 | 17 | 18 | 0 |
| gosmopolitan | 1 | 1 | 0 | 0 |
| govet | 30 | 12 | 2 | 16 |
| grouper | 1 | 1 | 0 | 0 |
| iface | 1 | 1 | 0 | 0 |
| importas | 1 | 1 | 0 | 0 |
| inamedparam | 1 | 1 | 0 | 0 |
| ineffassign | 1 | 1 | 0 | 0 |
| interfacebloat | 1 | 1 | 0 | 0 |
| intrange | 1 | 1 | 0 | 0 |
| iotamixing | 1 | 1 | 0 | 0 |
| ireturn | 1 | 1 | 0 | 0 |
| lll | 1 | 1 | 0 | 0 |
| loggercheck | 1 | 1 | 0 | 0 |
| maintidx | 1 | 1 | 0 | 0 |
| makezero | 1 | 1 | 0 | 0 |
| mirror | 1 | 1 | 0 | 0 |
| misspell | 1 | 1 | 0 | 0 |
| mnd | 1 | 1 | 0 | 0 |
| modernize | 1 | 1 | 0 | 0 |
| musttag | 1 | 1 | 0 | 0 |
| nakedret | 1 | 1 | 0 | 0 |
| nestif | 1 | 1 | 0 | 0 |
| nilerr | 1 | 1 | 0 | 0 |
| nilnesserr | 1 | 1 | 0 | 0 |
| nilnil | 1 | 1 | 0 | 0 |
| nlreturn | 1 | 1 | 0 | 0 |
| noctx | 1 | 1 | 0 | 0 |
| noinlineerr | 1 | 1 | 0 | 0 |
| nolintlint | 1 | 1 | 0 | 0 |
| nonamedreturns | 1 | 1 | 0 | 0 |
| nosprintfhostport | 1 | 1 | 0 | 0 |
| paralleltest | 1 | 1 | 0 | 0 |
| perfsprint | 1 | 1 | 0 | 0 |
| prealloc | 1 | 1 | 0 | 0 |
| predeclared | 1 | 1 | 0 | 0 |
| promlinter | 1 | 1 | 0 | 0 |
| protogetter | 1 | 1 | 0 | 0 |
| reassign | 1 | 1 | 0 | 0 |
| recvcheck | 1 | 1 | 0 | 0 |
| revive | 100 | 16 | 83 | 1 |
| rowserrcheck | 1 | 1 | 0 | 0 |
| sloglint | 1 | 1 | 0 | 0 |
| spancheck | 1 | 1 | 0 | 0 |
| sqlclosecheck | 1 | 1 | 0 | 0 |
| staticcheck | 161 | 46 | 1 | 114 |
| swaggo | 1 | 0 | 0 | 1 |
| tagalign | 1 | 1 | 0 | 0 |
| tagliatelle | 1 | 1 | 0 | 0 |
| testableexamples | 1 | 1 | 0 | 0 |
| testifylint | 1 | 1 | 0 | 0 |
| testpackage | 1 | 1 | 0 | 0 |
| thelper | 1 | 1 | 0 | 0 |
| tparallel | 1 | 1 | 0 | 0 |
| unconvert | 1 | 1 | 0 | 0 |
| unparam | 1 | 1 | 0 | 0 |
| unqueryvet | 1 | 1 | 0 | 0 |
| unused | 1 | 1 | 0 | 0 |
| usestdlibvars | 1 | 1 | 0 | 0 |
| usetesting | 1 | 1 | 0 | 0 |
| varnamelen | 1 | 1 | 0 | 0 |
| wastedassign | 1 | 1 | 0 | 0 |
| whitespace | 1 | 1 | 0 | 0 |
| wrapcheck | 1 | 1 | 0 | 0 |
| wsl | 1 | 1 | 0 | 0 |
| wsl_v5 | 1 | 1 | 0 | 0 |
| zerologlint | 1 | 1 | 0 | 0 |

## 一度も発火していない check（133 件）

- **gocritic** (1): `gocritic/whyNoLint`
- **govet** (16): `govet/buildtag`, `govet/cgocall`, `govet/defers`, `govet/directive`, `govet/framepointer`, `govet/httpresponse`, `govet/ifaceassert`, `govet/nilfunc`, `govet/shift`, `govet/sigchanyzer`, `govet/stringintconv`, `govet/testpass`, `govet/timeformat`, `govet/unmarshal`, `govet/unsafeptr`, `govet/unusedresult`
- **revive** (1): `revive/time-naming`
- **staticcheck** (114): `QF1004`, `QF1005`, `QF1007`, `QF1009`, `S1001`, `S1004`, `S1005`, `S1006`, `S1008`, `S1010`, `S1011`, `S1016`, `S1017`, `S1018`, `S1019`, `S1020`, `S1023`, `S1025`, `S1029`, `S1030`, `S1031`, `S1032`, `S1033`, `S1035`, `S1036`, `S1037`, `S1038`, `S1040`, `SA1001`, `SA1002`, `SA1003`, `SA1004`, `SA1005`, `SA1006`, `SA1007`, `SA1008`, `SA1010`, `SA1011`, `SA1013`, `SA1014`, `SA1015`, `SA1016`, `SA1017`, `SA1018`, `SA1020`, `SA1021`, `SA1023`, `SA1024`, `SA1025`, `SA1027`, `SA1028`, `SA1029`, `SA1030`, `SA1031`, `SA1032`, `SA2000`, `SA2001`, `SA2002`, `SA2003`, `SA3000`, `SA3001`, `SA4001`, `SA4003`, `SA4008`, `SA4011`, `SA4012`, `SA4013`, `SA4015`, `SA4016`, `SA4018`, `SA4020`, `SA4021`, `SA4022`, `SA4024`, `SA4025`, `SA4026`, `SA4028`, `SA4029`, `SA4030`, `SA4031`, `SA5000`, `SA5001`, `SA5002`, `SA5003`, `SA5004`, `SA5005`, `SA5007`, `SA5008`, `SA5010`, `SA6001`, `SA6002`, `SA6003`, `SA6005`, `SA6006`, `SA9001`, `SA9002`, `SA9004`, `SA9006`, `SA9007`, `SA9009`, `SA9010`, `ST1000`, `ST1001`, `ST1003`, `ST1008`, `ST1011`, `ST1012`, `ST1015`, `ST1016`, `ST1017`, `ST1018`, `ST1020`, `ST1021`, `ST1022`
- **swaggo** (1): `swaggo`

## 集計の元データ

- 走査した実行アーティファクト: `{'golden': 19, 'isolate': 6461, 'oss': 662, 'regress': 1492}`
- インベントリ: 548 checks / 114 linters
- `unit` は Rust テストソースの静的スキャン（下限値）。ID に言及していることの証明であって、
  golangci-lint と突き合わせている証明ではない。
