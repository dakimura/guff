# チェック単位カバレッジ台帳（COMPAT-HARDENING Phase 0）

> 自動生成: `./compat/coverage.py inventory && ./compat/coverage.py observe && ./compat/coverage.py report`。
> 手で編集しない。計画は [`COMPAT-HARDENING.md`](COMPAT-HARDENING.md)。

**`never` = どのテストでも一度も発火していない = 完全未検証。**
recall バグがあっても既存のどのゲートにも現れない。ここが Phase 3 のターゲットリスト。

| 状態 | 意味 | 件数 | 割合 |
|------|------|-----:|-----:|
| `fired` | golden / isolate / OSS / regress の実行で発火した | 517 | 94.5% |
| `unit-only` | Rust 単体テストが ID に言及するのみ（静的スキャン。golangci-lint との突合なし） | 21 | 3.8% |
| `never` | **どこでも発火していない** | 9 | 1.6% |
| — | **合計** | **547** | 100.0% |

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
| govet | 30 | 28 | 0 | 2 |
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
| revive | 100 | 97 | 2 | 1 |
| rowserrcheck | 1 | 1 | 0 | 0 |
| sloglint | 1 | 1 | 0 | 0 |
| spancheck | 1 | 1 | 0 | 0 |
| sqlclosecheck | 1 | 1 | 0 | 0 |
| staticcheck | 160 | 156 | 0 | 4 |
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

## 一度も発火していない check（9 件）

- **gocritic** (1): `gocritic/whyNoLint`
- **govet** (2): `govet/cgocall`, `govet/framepointer`
- **revive** (1): `revive/time-naming`
- **staticcheck** (4): `S1030`, `SA1011`, `SA1027`, `SA3000`
- **swaggo** (1): `swaggo`

## インベントリ外の check ID（1 件）

実行結果には出たが、インベントリ抽出が拾えていない ID。抽出器のバグか、
guff が宣言していない名前を描画している。

`SA9010`

## 集計の元データ

- 走査した実行アーティファクト: `{'golden': 538, 'isolate': 7830, 'oss': 800, 'regress': 1565}`
- インベントリ: 547 checks / 114 linters
- `unit` は Rust テストソースの静的スキャン（下限値）。ID に言及していることの証明であって、
  golangci-lint と突き合わせている証明ではない。
