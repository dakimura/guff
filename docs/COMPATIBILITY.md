# guff ↔ golangci-lint 互換性マトリクス

> このファイルは **R23（互換性マトリクスの公開）** の成果物です。
> 「どの linter・どの設定キー・どの出力フォーマットが対応済/部分/未対応か」を一覧にして、
> guff が「golangci-lint 互換の高速 linter」を名乗る根拠を可視化します。
>
> - **互換ピン**: golangci-lint **2.12.2**（`guff version` / `GOLANGCI_LINT_COMPAT`）。guff 自体の SemVer とは独立。
> - 対象: **golangci-lint v2**（linter 一覧の出典: <https://golangci-lint.run/docs/linters/> / キャプチャ 2026-07-17）。
> - 実装の一次情報は各クレートのコードと [`DEVELOPMENT.md`](DEVELOPMENT.md) §3。細かな DEFERRED は
>   各 analyzer のコード内 `// DEFERRED:` を参照。
> - 差分の**実測**（precision/recall）は `compat/` ハーネス（[`DEVELOPMENT.md`](DEVELOPMENT.md) §8 R21）で継続検証。

## 凡例

| 記号 | 意味 |
|------|------|
| ✅ | **対応** — 検出を実装済みで golangci-lint 相当の指摘が出る |
| 🟡 | **部分対応** — 実装済みだが既知のパリティ差あり（AST 近似 / サブセット / whole-program 未 / 一部設定 DEFERRED など）。詳細はコード内 `// DEFERRED:` |
| ❌ | **未対応** — 未実装（理由を併記） |

---

## 1. Linter 互換性（golangci-lint v2 全 114 linter）

**サマリ: 114 / 114 実装（✅ 97 + 🟡 17）。golangci-lint v2 全 linter 対応。**

| linter | guff | 備考 |
|--------|:----:|------|
| arangolint | ✅ | |
| asasalint | ✅ | |
| asciicheck | ✅ | |
| bidichk | ✅ | |
| bodyclose | 🟡 | AST 近似。SSA 完全パリティは DEFERRED（→ R13/R17） |
| canonicalheader | ✅ | |
| clickhouselint | ✅ | |
| containedctx | ✅ | |
| contextcheck | 🟡 | SSA + パッケージ内 facts。HTTP handler / cross-pkg facts 完全パリティは DEFERRED |
| copyloopvar | ✅ | |
| cyclop | ✅ | |
| decorder | ✅ | |
| depguard | 🟡 | パッケージ import 判定は対応。allowed modules/domains 等の一部設定は DEFERRED |
| dogsled | ✅ | |
| dupl | ✅ | `dupl.threshold` 配線済み |
| dupword | ✅ | |
| durationcheck | ✅ | |
| embeddedstructfieldcheck | ✅ | |
| err113 | ✅ | |
| errcheck | ✅ | excludes / blank / assert 対応 |
| errchkjson | ✅ | |
| errname | ✅ | |
| errorlint | 🟡 | comparison / type assertion 対応。errorf は既定オフ。allowed-errors は上流の 64 行の表を `(センチネル, それを返した関数)` の対で引く移植（2026-08-12） |
| exhaustive | ✅ | |
| exhaustruct | 🟡 | 検出は対応。`//exhaustruct:ignore` コメントディレクティブは DEFERRED |
| exptostd | ✅ | |
| fatcontext | ✅ | |
| forbidigo | ✅ | |
| forcetypeassert | ✅ | |
| funcorder | ✅ | |
| funlen | ✅ | |
| ginkgolinter | ✅ | |
| gocheckcompilerdirectives | ✅ | |
| gochecknoglobals | ✅ | |
| gochecknoinits | ✅ | |
| gochecksumtype | ✅ | |
| gocognit | 🟡 | 計測・閾値は対応。`gocognit:ignore` コメント除外は DEFERRED |
| goconst | ✅ | |
| gocritic | ✅ | |
| gocyclo | 🟡 | 計測・閾値は対応。`gocyclo:ignore` コメント除外は DEFERRED |
| godoclint | ✅ | strict / extra rules は一部 DEFERRED |
| godot | ✅ | |
| godox | ✅ | |
| goheader | ✅ | |
| gomoddirectives | 🟡 | replace/retract/exclude 判定は対応。version constraints 等は DEFERRED |
| gomodguard | 🟡 | v2 で deprecated（`gomodguard_v2` の別名）。allow/block は対応、一部設定 DEFERRED |
| gomodguard_v2 | 🟡 | `gomodguard` と同一 analyzer を駆動 |
| goprintffuncname | ✅ | |
| gosec | 🟡 | 主要ルール対応。G113 / G115–G118 / G201–G202 / G304–G305 / G307 / G601 等は DEFERRED |
| gosmopolitan | ✅ | |
| govet | 🟡 | 上流 46 pass のうち **30 を実装**。printf は引数個数・型照合まで `go vet` 一致。28 pass は `compat/golden/cases/govet` で位置・文言まで完全一致（cgocall / framepointer は環境依存で golden に載せられない）。未実装: appends / asmdecl / atomicalign / deepequalerrors / fieldalignment / findcall / hostport / httpmux / nilness / reflectvaluecompare / shadow / sortslice / stdversion / testinggoroutine / unusedwrite / waitgroup |
| grouper | ✅ | |
| iface | ✅ | |
| importas | ✅ | |
| inamedparam | ✅ | |
| ineffassign | ✅ | gordonklaus CFG + generated 除外 |
| interfacebloat | ✅ | |
| intrange | ✅ | |
| iotamixing | ✅ | |
| ireturn | ✅ | |
| lll | ✅ | |
| loggercheck | ✅ | |
| maintidx | ✅ | |
| makezero | ✅ | |
| mirror | ✅ | |
| misspell | ✅ | locale / ignore-words / extra-words / mode 配線済み |
| mnd | ✅ | |
| modernize | ✅ | |
| musttag | ✅ | |
| nakedret | ✅ | |
| nestif | ✅ | |
| nilerr | ✅ | SSA。`//lint:ignore nilerr`（commentmap）は DEFERRED（`//nolint` は runner 層） |
| nilnesserr | ✅ | SSA nilness 事実走査。variadic は flat Call args でメッセージ分類（go/ssa Alloc+Slice 完全一致は DEFERRED） |
| nilnil | ✅ | |
| nlreturn | ✅ | |
| noctx | ✅ | |
| noinlineerr | ✅ | |
| nolintlint | ✅ | `--enable nolintlint`。5 種の診断すべて（先頭空白 / 不正形式 / `require-specific` / `require-explanation` + `allow-no-explanation` / unused）|
| nonamedreturns | ✅ | |
| nosprintfhostport | ✅ | |
| paralleltest | ✅ | |
| perfsprint | ✅ | fiximports は DEFERRED |
| prealloc | ✅ | |
| predeclared | ✅ | |
| promlinter | ✅ | |
| protogetter | ✅ | |
| reassign | ✅ | |
| recvcheck | ✅ | |
| revive | 🟡 | golint-default 23 + extended 76 = **99 rules**（`multiline-if-init` は上流 v1.15.0 に無いため `enable-all-rules` の集合外）。未実装 rule あり。99 rule を `compat/golden/cases/revive/` で有効化し、うち 91 が実際に発火して位置・メッセージまで突合済み |
| rowserrcheck | 🟡 | AST 近似。SSA 完全パリティは DEFERRED |
| sloglint | ✅ | |
| spancheck | 🟡 | AST 近似（`defer End` / 関数内 `End`）。x/tools ctrlflow 完全パリティは DEFERRED |
| sqlclosecheck | 🟡 | AST 近似。SSA 完全パリティは DEFERRED |
| staticcheck | ✅ | 167 analyzers（S* 37 + SA* 100 + ST* 18 + QF* 12） |
| tagalign | 🟡 | 整列チェック対応。StrictStyle は DEFERRED |
| tagliatelle | ✅ | |
| testableexamples | ✅ | |
| testifylint | ✅ | |
| testpackage | ✅ | |
| thelper | ✅ | |
| tparallel | ✅ | |
| unconvert | ✅ | |
| unparam | ✅ | |
| unqueryvet | ✅ | |
| unused | 🟡 | 単一パッケージ（型・定数・メソッド・const グループ）。whole-program 版は DEFERRED |
| usestdlibvars | ✅ | |
| usetesting | ✅ | |
| varnamelen | ✅ | |
| wastedassign | ✅ | NaiveForm SSA（有効時のみ別ビルド） |
| whitespace | ✅ | |
| wrapcheck | ✅ | |
| wsl | 🟡 | v2 で deprecated（`wsl_v5` の別名）。完全パリティは DEFERRED |
| wsl_v5 | 🟡 | 完全パリティは DEFERRED |
| zerologlint | ✅ | SSA（buildir）。ネストしたヘルパ経由の dispatch 追跡は部分対応 |

> **共通の DEFERRED**: 各 linter の `SuggestedFix`（`--fix` の完全網羅）と、`//<linter>:ignore` 系の
> コメントディレクティブは linter ごとに順次対応中。詳細はコード内 `// DEFERRED:`。

### 1.1 Formatter（golangci-lint v2 `formatters`）

`guff fmt` サブコマンドおよび `guff run` 時の formatter 診断で対応（システムバイナリ経由）。

| formatter | guff | 備考 |
|-----------|:----:|------|
| gofmt | ✅ | `simplify` / `rewrite-rules` |
| gofumpt | ✅ | `extra-rules` / `module-path` / `-lang`（`run.go` から注入） |
| goimports | ✅ | `local-prefixes` |
| gci | ✅ | `sections` / `custom-order` / `no-lex-order` / `no-inline-comments` / `no-prefix-comments` |
| golines | ✅ | `max-len` / `tab-len` / `shorten-comments` / `reformat-tags` / `chain-split-dots` |
| swaggo | ✅ | `swag fmt` 経由 |

---

## 2. 設定キー互換性（`.golangci.yml` v1 / v2）

`.golangci.{yml,yaml}` / `.guff.{yml,yaml}` を上位ディレクトリまで探索。v1 は `guff migrate` で v2 へ移行。
実 OSS の v2 設定 **52 件** を CI でパース検証（R22）。

| セクション / キー | guff | 備考 |
|-------------------|:----:|------|
| `version` | ✅ | `"2"` で v2 判定 |
| `run.build-tags` | ✅ | load へ配線 |
| `run.tests` | ✅ | |
| `run.go` | ✅ | 型チェッカ言語バージョン + gofumpt `-lang` |
| `run.timeout` | ✅ | 既定 `1m` |
| `run.concurrency` | ✅ | `-j` と同義 |
| `run.issues-exit-code` | ✅ | |
| `run.modules-download-mode` | 🟡 | パースのみ（外部 module 解決は DEFERRED） |
| `linters.default`（standard/fast/all/none） | 🟡 | `standard`==`all`（standard 5 系統）。100+ linter を跨ぐ本来の `all`/`fast` は未 |
| `linters.enable` / `disable` | ✅ | 別名正規化（gosimple→staticcheck ほか） |
| `linters.settings.*` | ✅ | 各 analyzer に配線（キー詳細は §3.3）。一部キーは DEFERRED |
| `linters.exclusions.paths` / `paths-except` | ✅ | |
| `linters.exclusions.rules` | ✅ | linters / path / path-except / text / source。`linters` は linter 名を逐語で比較（analyzer 名では一致しない）、`text` / `source` は大文字小文字を区別。条件 1 個の規則を上流は設定エラーにするが guff は受け入れる |
| `linters.exclusions.presets` | ✅ | comments / std-error-handling / common-false-positives / legacy（上流の 13 規則。camelCase 別名も受ける） |
| `linters.exclusions.warn-unused` | ❌ | 未実効（DEFERRED） |
| `linters.exclusions.generated` | ✅ | `strict`（**`run` の既定**。`config.Loader.Load` が空値を書き換える）/ `lax`（`formatters.exclusions.generated` の既定）/ `disable`。`lax` はパッケージ節の直下のコメントまで見る |
| `formatters.enable` / `settings` / `exclusions` | ✅ | §1.1 |
| `issues.exclude` / `exclude-rules` | ✅ | |
| `issues.exclude-dirs` / `exclude-files` | ✅ | |
| `issues.exclude-use-default` / `-case-sensitive` | ✅ | v2 は既定 exclusion 無し |
| `issues.max-issues-per-linter` / `max-same-issues` | ✅ | 既定 50 / 3 |
| `issues.uniq-by-line` | ✅ | 既定 true |
| `issues.include` | ✅ | 既定 exclusion の打ち消し |
| `issues.new` / `new-from-rev` / `new-from-merge-base` / `new-from-patch` | ✅ | git diff（subprocess）。失敗時は警告してスキップ |
| `issues.whole-files` | ✅ | `new*` と併用。変更ファイル全体の issue を残す |
| `severity.default-severity` / `rules` / `case-sensitive` | ✅ | |
| `output.formats` / `format`（deprecated） | ✅ | §3 |
| `output.print-linter-name` | ✅ | 既定 `true`。text / tab / colored-* に配線 |
| `output.print-issued-lines` | ✅ | 既定 `true`。text / colored-line-number でソース行 + `^` |
| `output.sort-results` | 🟡 | パースのみ（診断は決定的順序で出力） |
| `output.path-prefix` | 🟡 | パースのみ（未実効） |
| `output.show-stats` | 🟡 | パースのみ（未実効） |

v1 → v2 移行（`guff migrate`）では、`formatters` へ移った linter（gci/gofmt/gofumpt/goimports/golines/swaggo）と
v2 で削除された linter（deadcode / structcheck / varcheck / golint / interfacer / maligned / scopelint /
exhaustivestruct / exportloopref / ifshort / nosnakecase / tenv / execinquery）を自動で仕分けます。

---

## 3. 出力フォーマット互換性（`--out-format` / `output.formats`）

| フォーマット | guff | 別名 |
|--------------|:----:|------|
| text | ✅ | `line-number`。既定でソース行 + `^`（`print-issued-lines`） |
| colored-line-number | ✅ | `colored`。TTY 時の暗黙デフォルト（golangci 互換） |
| json | ✅ | golangci-lint スキーマ準拠（`{"Issues":[...],"Report":...}`） |
| checkstyle | ✅ | Checkstyle XML |
| sarif | ✅ | SARIF 2.1.0 |
| tab | ✅ | |
| colored-tab | ✅ | |
| github-actions | ✅ | `github`（`::error file=…`） |

- `format:path` / config の `path`（例 `json:report.json`）でファイル書き出し。`stdout` / `stderr` も可。
- 複数フォーマットの同時出力に対応（例: text を stdout、json をファイル）。
- **DEFERRED**: JSON `Report` への warnings 埋め込み・`SuggestedFixes` の JSON 化。

---

## 4. その他の互換性

| 項目 | guff | 備考 |
|------|:----:|------|
| `//nolint` / `//nolint:linter` | ✅ | 同一行・直前行の AST 展開。書式/説明必須は DEFERRED |
| `--fix`（autofix） | 🟡 | SuggestedFix / TextEdit を適用。linter ごとの fix 網羅は継続 |
| 終了コード | ✅ | 0=クリーン / `--issues-exit-code`（既定 1）=指摘あり / 2=エラー |
| キャッシュ | ✅ | パッケージ単位の issues 永続キャッシュ。facts 永続化は DEFERRED（→ R24） |
| 並列実行 | ✅ | action DAG を rayon で並列。型チェックも並列 |
| プリセット | 🟡 | `standard` / `fast` / `all` / `none`（`standard`==`all`） |

### 意図的に一致させていない差分

上流の欠陥に追従すると**真陽性を捨てることになる**ため、guff の方が多く報告する箇所。

| 対象 | 差 | 理由 |
|------|----|------|
| revive `time-equal` / `epoch-naming` | guff は報告し、golangci-lint は報告しない | revive は `importer.Default()`（gc の export data importer）で型検査するが、いまの Go には `.a` が無いので import が全部 invalid になる。`time.Time` かどうかを判定する rule は上流では常に黙る。詳細は [`COMPAT-HARDENING.md`](COMPAT-HARDENING.md) §6 |
| revive `context-keys-type` | 文言が `string` / 上流は `untyped string` | 同じ原因。上流は `context.WithValue` のシグネチャを解決できず、untyped 定数が defaulting されない |

---

_最終更新: 2026-07-17（R23）。linter 一覧は golangci-lint v2 に追随。追加・変更時はこの表と
[`DEVELOPMENT.md`](DEVELOPMENT.md) §3 を同時に更新すること。_
