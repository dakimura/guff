# guff 開発ガイド & ロードマップ（唯一の正典）

> このファイルは、以前分かれていた次の 5 つの計画書を **1 本に統合** したものです。
> `MIGRATION.md` / `PRE-LINTER-PLAN.md` / `docs/LINTER-MIGRATION.md` /
> `docs/STATICCHECK-MIGRATION.md` / `docs/ADDING-ANALYZER.md`。
> これらの原文は git 履歴に残っています（`git log -- docs/LINTER-MIGRATION.md` 等）。
> 以後、設計・進捗・残タスクの更新はこの 1 ファイルに集約してください。

---

## 0. このドキュメントの使い方（作業者へ）

- あなたは 1 回のセッションで **1 タスク**だけ進めます（§4.3 の作業サイクル）。
- 迷ったら「既存コードに倣う」。新しい書き方を発明しない。
- 変更したら必ず `cargo build` と `cargo test`（該当クレート）を通す。**テストが赤いまま次へ進まない。**
- 残タスクは §8 のロードマップに **R番号（R1, R2…）** で載っています。着手するときは
  そのタスクの「完了条件（Done when）」と「テスト」を満たしてから完了にする。
- 大きな作業は「後回し（deferral）」してよいが、**必ずコード内に `// DEFERRED:` コメントと、
  §8 の該当タスクへのメモを残す**。黙って省略しない。安全なデフォルト（`false`/`None`/空 `Vec`）を返す。

---

## 1. guff とは / ゴール

**guff は Go 向けの linter を Rust で書き直したもの**です。最終ゴールは:

> **golangci-lint 互換の高速 linter** ——
> 既存の `.golangci.yml` を持つ Go プロジェクトで golangci-lint の代わりに `guff` を実行しても
> 同等の結果が、より速く得られる状態。

現在は golangci-lint v2 の **`standard` プリセット相当**（staticcheck / govet / errcheck /
ineffassign / unused の 5 系統）を、パッケージロード → 型チェック → `go/analysis` 実行まで
**1 本のパイプライン**でこなせます。CLI バイナリ名は `guff`（クレート名 `guff-lint`）。

「golangci-lint 互換」を名乗るために足りていないものは §8 に全部書いてあります。

---

## 2. アーキテクチャ

### 2.1 パイプライン

```
go list / モジュールグラフ (guff-packages)
  → ビルドタグ適用・パッケージ列挙 (guff-build)
  → 型チェック（ソース / export data） (guff-types, guff-exportdata)
  → go/analysis Pass 生成 (guff-analysis)
  → Analyzer を action DAG で実行 (guff-runner)
  → Diagnostic 収集
       ↑
  guff-lint CLI（設定・linter 選択・診断表示）
```

### 2.2 クレート地図

| 層 | クレート | 役割（Go 相当） |
|----|----------|-----------------|
| **CLI** | `guff-lint` (`bin: guff`) | 設定・linter 選択・診断表示・`migrate` |
| **Linters** | `guff-staticcheck`, `guff-govet`, `guff-errcheck`, `guff-ineffassign`, `guff-unused`, `guff-gostaticanalysis`, `guff-error`, `guff-context`, `guff-style`, `guff-comment`, `guff-import`, `guff-misspell`, `guff-dupl`, `guff-revive` | 各 linter の Analyzer 群 |
| **Driver** | `guff-runner` | Analyzer の DAG 実行・パッケージ並列・メモリ管理 |
| **Framework** | `guff-analysis`, `guff-pattern` | `go/analysis`（Pass/Analyzer/inspect/facts/code/callcheck）+ Staticcheck のパターン DSL |
| **SSA** | `guff-ssa` | `go/ssa`（buildir） |
| **Load / types** | `guff-packages`, `guff-build`, `guff-exportdata`, `guff-types`, `guff-constant` | パッケージロード・型検査・export data |
| **AST** | `guff-ast` | `go/token` / `scanner` / `ast` / `parser` |
| **Version** | `guff-version`, `guff-gover`, `guff-goversion`, `guff-types-errors` | Go バージョン・型エラーコード |

依存の流れ（下 → 上）:

```
guff-ast / guff-constant / guff-version*
  → guff-types ← guff-exportdata
  → guff-build → guff-packages
  → guff-ssa / guff-pattern / guff-analysis
  → guff-runner
  → guff-{staticcheck,govet,errcheck,ineffassign,unused,gostaticanalysis,error,context,style,comment,import,misspell,dupl,revive}
  → guff-lint
```

### 2.3 型チェッカのアリーナモデル（`guff-types`）

Go の `cmd/compile/internal/types2` を移植したもの。**Go のポインタは全部 ID に変換**する:

- `*Type` → `TypeId`、オブジェクト → `ObjectId`、`*Scope` → `ScopeId`、`*Package` → `PackageId`
- `nil` → `Option::None`
- 実体は `TypeArena` / `ObjectArena` / `ScopeArena` / `PackageArena` に格納
- `Checker` 構造体（`src/check.rs`）がこの 4 アリーナ + `universe` + `conf: Config` +
  `info: Info` + `ctxt: Context`（ジェネリクスのインスタンスキャッシュ）などを所有する
- 主要ファイル: `expr.rs`（式）, `decl.rs`（宣言）, `typexpr.rs`（型式）, `stmt.rs`（文）,
  `call.rs`（呼び出し）, `builtins.rs`（組込関数）, `index.rs`, `subst.rs`/`instantiate.rs`（ジェネリクス）,
  `check_lookup.rs`, `check_assign.rs`, `errors.rs`/`format.rs`（エラー収集・整形）

### 2.4 解析フレームワーク（`guff-analysis`）

golangci-lint / staticcheck が土台にしている `go/analysis` 相当:

- `Analyzer`（`src/analyzer.rs`）: `name`, `doc`, `url`, `run: RunFn`, `requires`, `fact_types`
- `Pass`（`src/pass.rs`）: analyzer から AST・型情報・他 analyzer の結果・診断出力にアクセスする窓口
  （`pass.files()`, `pass.types_info()`, `pass.result_of::<T>(other())`, `pass.reportf(pos, msg)`）
- `inspect` パス（`src/passes/inspect.rs`）: AST の preorder walk。ほぼ全 analyzer が `requires`
- `code` モジュール: `call_name`, 定数抽出などの補助
- `facts`（`src/facts.rs` + `passes/facts/generated`）: パッケージ間で伝播する事実（generated 判定など）
- `callcheck`（`src/callcheck.rs`）: 関数呼び出し引数検証フレームワーク（SA1000 系で使用）
- `guff-pattern`: 構造パターンマッチ DSL（§5.1）
- `buildir` パス（`guff-ssa`）: SSA/IR を構築（`build_package_for_analysis`）

---

## 3. 現在の状況（正直なスナップショット）

> 最終更新: 2026-07-16。ワークスペース全体 **1900+ tests green**（`guff-revive` extended rules 計 **100 rules** + `linters.settings.revive` / `dupl` / `misspell` / `godot` / `godox` / `dupword` / **`depguard` / `gomoddirectives` / `gomodguard` / `wrapcheck` / `exhaustive` / `musttag` / `loggercheck` / `sloglint` / `testifylint`（28 checkers） / `exptostd` / `modernize` / `gocritic`（58 checkers）** YAML 配線 + stylecheck ST* **15** + quickfix QF* **12** + **`mirror`**）。

### 3.1 型チェッカ（`guff-types`）
- 構造層（全 Type/Object 種別・述語・universe・ジェネリクス subst/instantiate/infer/unify・
  operand・conversions・assignments・typestring）**完了**。
- Checker エンジン本体もほぼ完走（`check_files` 到達、宣言・式・文・呼び出し・組込・ジェネリクス
  end-to-end・importer・unused/dot/blank import・mono・sizes・version）。
- **残**: `initorder.rs`（パッケージ初期化順, Step 34）, `recording.rs`（AST ノード ID が前提, Step 37）,
  `util.rs` 一部（Step 39）, および D01/D02/D03/D04/D07/D10/D13/D16 の未了分（→ §8 R19）。

### 3.2 解析フレームワーク（PRE-LINTER Phase 0–7）
- Phase 0（types 仕上げ）〜Phase 7（E2E smoke）**完了**。
- **残**: Phase 8（gofmt / go/doc 等の付帯ユーティリティ）, PL07（GOCACHE 管理）,
  PL05（ctrlflow）, PL02（go 無し driver）, SSA `RangeStmt`（→ §8 各タスク）。
  PL11（真の並列実行）は **R9 で完了**。

### 3.2.1 SSA（`guff-ssa`, `go/ssa` 移植）
- naive SSA（lift 無し）→ dom/lift/blockopt → Milestone D/E/F 完了。**150 tests green**。
- 型機構（subst/canonizer/typeset/instantiate データモデル）と builder コア（emit・alloc/local・
  param/result spill・selector・assign・複合リテラル各種）まで移植済み。golden 逆アセンブル比較で検証。
- **残**: `methods.rs` とメソッドラッパ（`createWrapper`/`$thunk`/`$bound`）, FromSyntax インスタンス本体の
  subst 適用ビルド, `InstantiateGenerics` オーケストレーション, メソッド呼び出し emit（E25+）,
  そして `RangeStmt`（→ §8 R17）。これらが揃うと IR ベースの linter（SA1015 等）を default で駆動できる。

### 3.3 実装済み linter
| linter | 状態 | 規模 |
|--------|------|------|
| `guff-staticcheck` | ✅ **164 analyzers**（simple S* 37 + staticcheck SA* 100 + stylecheck ST* **15** + quickfix QF* **12**） | ST* 残り IR 依存は **未着手**（→ R16）。`initialisms` / `dot-import-whitelist` / `http-status-code-whitelist` settings 配線済み |
| `guff-govet` | ✅ **29/29** passes（printf は引数個数・型照合まで, `go vet` 一致） | — |
| `guff-errcheck` | ✅（excludes / blank / assert） | `unchecked_call` FW 無しで実装 |
| `guff-ineffassign` | ✅（gordonklaus CFG + generated 除外） | — |
| `guff-unused` | ✅（単一パッケージ; 型・定数・メソッド・const グループ） | whole-program 版は未 |
| `guff-gostaticanalysis` | ✅ **4**（forcetypeassert / nilnil / makezero / **mirror**） | nilerr / nilnesserr は **DEFERRED（SSA → R17）** |
| `guff-error` | ✅ **6**（errname / err113 / durationcheck / errorlint / wrapcheck / errchkjson） | `errchkjson` settings（`check-error-free-encoding` / `report-no-exported`）配線済み。`wrapcheck` settings（`ignore-sigs` / `extra-ignore-sigs` / `ignore-sig-regexps` / `ignore-package-globs` / `ignore-interface-regexps` / `report-internal-errors`）配線済み。rowserrcheck 等は **DEFERRED**（SSA） |
| `guff-context` | ✅ **2**（noctx / fatcontext） | bodyclose / contextcheck / sqlclosecheck 等は **DEFERRED**（SSA → R17） |
| `guff-style` | ✅ **33**（copyloopvar / usetesting / usestdlibvars / perfsprint / goconst / dogsled / asciicheck / goprintffuncname / funlen / gocyclo / lll / gocognit / nestif / cyclop / nakedret / nosprintfhostport / predeclared / whitespace / nlreturn / mnd / prealloc / tagalign / wsl / unconvert / exhaustruct / exhaustive / musttag / loggercheck / sloglint / testifylint / **exptostd** / **modernize** / **gocritic**） | `linters.settings` 配線済み（copyloopvar `check-alias` / usetesting 各フラグ / usestdlibvars `http-method`・`http-status-code` + optional `time-weekday` / `time-month` / `time-layout` / `crypto-hash` / `default-rpc-path` / `sql-isolation-level` / `tls-signature-scheme` / `constant-kind` / `time-date-month` / unconvert `fast-math` / `safe`（safe の親コンテキスト判定は DEFERRED）/ **exhaustruct** `include` / `exclude` / `allow-empty` / `allow-empty-rx` / `allow-empty-returns` / `allow-empty-declarations` / **exhaustive** `check` / `default-signifies-exhaustive` / `default-case-required` / `ignore-enum-members` / `ignore-enum-types` / `package-scope-only`（map チェック・コメントディレクティブは DEFERRED）/ **musttag** `functions`（name / tag / arg-pos；iface whitelist はメソッド名ヒューリスティック）/ **loggercheck** `kitlog`/`klog`/`logr`/`slog`/`zap` / `require-string-key` / `no-printf-like` / `rules`（rulefile・printf 完全パリティは DEFERRED）/ **sloglint** `no-mixed-args`（既定 true）/ `kv-only` / `attr-only` / `no-global` / `context` / `static-msg` / `msg-style` / `no-raw-keys` / `key-naming-case` / `allowed-keys` / `forbidden-keys` / `args-on-sep-lines` / `custom-funcs`（SuggestedFix・discard-handler の Go 1.24 ゲートは DEFERRED）/ **testifylint** `enable-all` / `disable-all` / `enable` / `disable` / `bool-compare.ignore-custom-types` / `expected-actual.pattern` / `time-compare.suppress-calls-pattern` / `formatter.check-format-string` / `require-f-funcs` / `require-string-msg` / `suite-extra-assert-call.mode` / `require-error.fn-pattern` / **`go-require.ignore-http-handlers`**（実装済 checker: blank-import / bool-compare / compares / contains / empty / equal-values / error-is-as / error-nil / expected-actual / float-compare / **formatter** / **go-require** / len / **mock-expect** / negative-positive / nil-compare / regexp / **require-error** / **suite-broken-parallel** / **suite-dont-use-pkg** / **suite-extra-assert-call** / **suite-method-signature** / **suite-subtest-run** / **suite-thelper**（既定オフ）/ **time-compare** / useless-assert / zero / encoded-compare；SuggestedFix・formatter の printf 完全パリティは DEFERRED）/ **exptostd**（設定キー無し；`maps.Keys`/`Values` SuggestedFix は DEFERRED）/ **modernize** `disable`（実装 checker: any / plusbuild / forvar / rangeint / minmax / fmtappendf / omitzero / slicessort / **stringscutprefix** / **slicescontains** / **stringsseq** / **waitgroupgo** / **mapsloop** / **slicesbackward** / **reflecttypefor** / **testingcontext** / **unsafefuncs** / **importcomment** / **stringscut**（Split/SplitN[0]）；atomictypes / newexpr / stringsbuilder 等・mapsloop Insert/Collect/Clone・stringscut Index/Contains は DEFERRED）/ **gocritic** `enable-all` / `disable-all` / `enabled-checks` / `disabled-checks`（実装 checker **58**: default 34 + enable-all extras 24: deferUnlambda / emptyDecl / emptyFallthrough / emptyStringTest / initClause / nilValReturn / octalLiteral / yodaStyleExpr / builtinShadow / builtinShadowDecl / commentedOutImport / dupImport / filepathJoin / paramTypeCombine / rangeAppendAll / weakCond / **dupOption** / **methodExprCall** / **rangeExprCopy** / **regexpPattern** / **sortSlice** / **sqlQuery** / **typeAssertChain** / **badRegexp**；残 enable-all extras は `ruleguard`・per-check settings・SuggestedFix は DEFERRED）/ gocyclo / gocognit / nestif / dogsled / funlen / cyclop（`max-complexity` / `package-average` / `skip-tests`）/ lll / nakedret（`max-func-lines` / `skip-test-files`）/ nlreturn / predeclared / whitespace（`multi-if` / `multi-func`）/ mnd / prealloc / tagalign / wsl / perfsprint（`concat-loop` / `loop-other-ops` / `err-error` / `int-conversion` 含む）/ goconst（`match-constant` / `numbers` / `min` / `max` / `find-duplicates` 含む）の主要キー）。`usestdlibvars` ignore ディレクティブ・SuggestedFix 完全パリティ・`perfsprint` fiximports・wsl 完全パリティ・`exhaustruct` コメントディレクティブは **DEFERRED** |
| `guff-comment` | ✅ **3**（godot / godox / dupword） | `linters.settings` 配線済み（godot scope/exclude/period/capital、godox keywords、dupword keywords/ignore/comments-only）。SuggestedFix・godot `toplevel`/`noinline`・dupword 跨行は **DEFERRED** |
| `guff-import` | ✅ **3**（depguard / gomoddirectives / gomodguard） | `linters.settings` 配線済み（depguard rules / list-mode / files / allow / deny、gomoddirectives replace-local・allow-list・retract/exclude/toolchain/tool/godebug flags、gomodguard + gomodguard_v2 blocked・local-replace）。allowed modules/domains・version constraints・match-type・depguard path placeholders・gomoddirectives ignore/toolchain-pattern 等は **DEFERRED** |
| `guff-misspell` | ✅ **1**（misspell） | `linters.settings.misspell` 配線済み（locale / ignore-words / extra-words / mode=restricted） |
| `guff-dupl` | ✅ **1**（dupl） | `linters.settings.dupl.threshold` YAML 配線済み |
| `guff-revive` | ✅ **1**（revive） | golint-default **23 rules** + extended **77 rules**（計 100）；`linters.settings.revive` YAML 配線済み（rules・arguments・global/per-rule severity・confidence・ignore-generated-header）。prometheus 互換: `context-as-argument` `allowTypesBefore`、`early-return`/`indent-error-flow`/`superfluous-else` の `preserveScope`（+ `allowJump`）実効化済み |

### 3.4 CLI / 設定 / 出力 / 実行（`guff-lint`, `guff-runner`）
現状は「薄いドライバ」。golangci-lint 互換にはほど遠い。**ここが §8 ロードマップの主戦場。**

| 項目 | 現状 | golangci-lint との差（ギャップ） |
|------|------|------------------------------------|
| サブコマンド | `run`, `migrate`, `version`, `linters`, `cache`（clean/status） | `help`/`fmt` 無し |
| run フラグ | `-c`, `--no-config`, `--preset`, `--enable`, `--disable`, `--sequential`, `--issues-exit-code`, `--build-tags`, `--timeout`, `-j/--concurrency`, `--out-format`（`format` / `format:path`）, `--no-cache`, `--fix` | — |
| 設定ファイル | `.golangci.{yml,yaml}` / `.guff.{yml,yaml}` を上位ディレクトリまで探索。v1/v2 の linter 選択 + `issues`/`run`/`severity`/`output` をパース。**v2 `linters.exclusions`**（`paths` / `paths-except` / `rules` / `presets` / `warn-unused`）を `IssueFilter` に折り込み（v2 は既定除外なし；presets で EXC* 相当を展開）。`issues.exclude*` / `exclude-rules` / max-* / severity を後処理で適用。`run.build-tags`・`run.tests` を load に渡す。`run.timeout` を全体タイムアウトに適用（既定 `1m`）。`run.concurrency` / `-j` で rayon ワーカー数（`1` → sequential）。`linters.settings`（errcheck check-blank / check-type-assertions / **exclude-functions** / **disable-default-exclusions**、govet enable/disable、staticcheck checks / **initialisms** / **dot-import-whitelist** / **http-status-code-whitelist**（`stylecheck` キーも merge）、errchkjson check-error-free-encoding / report-no-exported、**wrapcheck** ignore-sigs / extra-ignore-sigs / ignore-sig-regexps / ignore-package-globs / ignore-interface-regexps / report-internal-errors、style/comment/revive/dupl/misspell、**usestdlibvars** HTTP + optional tables、**unconvert** fast-math / safe、**exhaustruct** include / exclude / allow-empty*、**exhaustive** check / default-signifies-exhaustive / default-case-required / ignore-enum-* / package-scope-only、**musttag** functions、**loggercheck** / **sloglint** / **testifylint** enable-all/disable-all/enable/disable / bool-compare.ignore-custom-types / expected-actual.pattern / time-compare.suppress-calls-pattern / formatter.check-format-string/require-f-funcs/require-string-msg / suite-extra-assert-call.mode / require-error.fn-pattern / **go-require.ignore-http-handlers**、**modernize** disable、**gocritic** enable-all/disable-all/enabled-checks/disabled-checks、**depguard / gomoddirectives / gomodguard(+_v2)**）を Pass / 選択に配線。`output.formats`/`format` → `--out-format`（text / colored / json / checkstyle / sarif / tab / github-actions）。R22 の config corpus smoke で Prometheus / Grafana 由来の v2 設定をパース検証。 | `issues.new`/`new-from-rev`（diff 除外）・exclusions `warn-unused` 実効化・`generated` モードは未 |
| プリセット | `standard`/`fast`/`all`/`none`。ただし `standard`==`all`（5 系統）。追加系は `--enable`（forcetypeassert/nilnil/makezero/mirror/errname/err113/durationcheck/errorlint/wrapcheck/errchkjson/noctx/fatcontext/copyloopvar/usetesting/usestdlibvars/perfsprint/goconst/dogsled/asciicheck/goprintffuncname/funlen/gocyclo/lll/gocognit/nestif/cyclop/nakedret/nosprintfhostport/predeclared/whitespace/nlreturn/mnd/prealloc/tagalign/wsl/unconvert/exhaustruct/exhaustive/musttag/loggercheck/sloglint/testifylint/exptostd/modernize/gocritic/godot/godox/dupword/depguard/gomoddirectives/gomodguard/misspell/dupl/revive） | 100+ linter を跨ぐ本来の `all`/`fast`/カテゴリプリセットに未対応 |
| 出力 | `Formatter` 抽象 + `--out-format text`（`line-number` 別名）/ `colored-line-number` / `json` / `checkstyle` / `sarif` / `tab` / `colored-tab` / `github-actions`。`format:path` / config `path` でファイル書き出し（親ディレクトリ作成・`stdout`/`stderr` 特殊値） | — |
| nolint | ✅ `//nolint` / `//nolint:linter`（同一行・直前行の AST 展開）。`nolintlint`（未使用報告）は `--enable nolintlint` | 書式/説明必須（NeedsMachineOnly / NeedsExplanation）は未 |
| キャッシュ | ✅ パッケージ単位の issues 永続キャッシュ（`$GUFF_CACHE` / `$GOLANGCI_LINT_CACHE` / `{UserCacheDir}/guff`）。未変更 pkg は再解析スキップ。`guff cache clean`/`status`、`--no-cache` | facts キャッシュ・ロード/型チェックのスキップは未 |
| 並列 | ✅ action DAG を rayon ウェーブフロントで並列実行。`-j`/`run.concurrency` でワーカー数。`Ident::obj` を `Mutex` 化し `Package: Sync` | ロード/型チェックはまだ逐次。実 OSS の wall-clock は R11 で計測開始（現状 guff は warmup 比で golangci より遅い） |
| ベンチ | ✅ `benchmarks/` ハーネス（cold/warm・同一 `standard.yml`）。`fixture`/`local` で再現可能。`results/RESULTS.md` | 実 OSS は SSA 未実装で FAIL しがち（→ R17）。load スキップ無しで warm 恩恵は小さい |
| 終了コード | 0=クリーン / `--issues-exit-code`（既定 1）=指摘あり / 2=エラー | —（R1 完了） |
| autofix | ✅ `--fix`（SuggestedFix / TextEdit 適用、修正済み診断は出力から除外） | golangci の fix 範囲全体には未 |

---

## 4. 規約（必ず守る）

### 4.1 Go → Rust 機械的翻訳ルール
- 可変長 printf は無い。`check.errorf(pos, Code, "x %s", y)` → `self.error(pos, Code, &format!("x {}", y))`。
- エラーは即時報告せず `self.errors` に**収集**する（型チェッカ）。analyzer は `pass.reportf` で出す。
- 型名を出すときは `guff_types::typestring::type_string(...)` を使う。
- `assert()` → `assert!`、`nil` → `Option::None`、Go ポインタ → ID（`Copy`）。
- 借用チェッカ対策: 再帰前にフィールドをローカルへスナップショットしてから可変借用を取る。

### 4.2 クレート命名
| パターン | 例 | いつ |
|---------|-----|------|
| `guff-<linter名>` | `guff-errcheck`, `guff-revive` | golangci と 1:1 の主要 linter |
| `guff-<upstream名>` | `guff-gostaticanalysis` | 同一 org の小 linter を束ねる |
| `guff-<カテゴリ>` | `guff-style`, `guff-comment` | 小さなスタンドアロン linter 群 |
| `guff-lint` | — | CLI + レジストリ + nolint |
| `guff-fmt` | — | formatter 群（gofmt 等） |

**ルール**: 1 クレート = `analyzers() -> Vec<&'static Analyzer>` を公開。登録は `guff-lint` が行う。

### 4.3 作業サイクル（1 セッション = 1 タスク）
1. 参考にする Go ソースを読む（§9 の clone URL 参照）。
2. 該当クレートに `Analyzer` を定義（`requires` は最小限）。
3. `tests/testdata/<case>/` に小さな fixture（1 診断/1 ファイルが理想）。
4. `cargo test -p guff-<name>`。
5. `guff-lint` レジストリに登録。
6. **§3 の状況表と §8 の該当タスクを更新**（チェックを付ける / 完了メモ）。

---

## 5. 新しい analyzer（linter）の追加手順

参考実装: `crates/guff-analysis/src/passes/printast.rs`, `printf.rs`（型不要/型必要の両例）。
既存 linter は `crates/guff-govet/src/*.rs`（AST 系）と `crates/guff-staticcheck/src/*.rs`（パターン系）。

1. **analyzer を定義**
   - `crates/guff-<crate>/src/<name>.rs` を作る。
   - `fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError>` を書く
     （AST は `pass.files()`、型は `pass.types_info()`、出力は `pass.reportf(pos, msg)`）。
   - `pub fn analyzer() -> &'static Analyzer` を `OnceLock` で公開し、
     `name`/`doc`/`url`/`run as RunFn`/`run_despite_errors`/`requires`（例 `vec![inspect::analyzer()]`）/
     `fact_types` を埋める。`mod.rs`（または crate の `analyzers()`）から公開する。
   - **他 analyzer に依存**するなら `requires` に列挙 → `pass.result_of::<T>(other::analyzer())` で結果取得。
   - **型が必要**なら load mode を `LOAD_SYNTAX` 相当にする（`guff-runner/src/load_mode.rs` の
     `infer_load_mode` は facts を出さない限り AST のみになる点に注意）。
2. **グラフ検証**: `guff_analysis::validate::validate(&[analyzer()])` を単体テストで呼ぶ（requires の循環を検出）。
3. **testdata**: `tests/testdata/<case>/` に `bad.go`（検出されるべき）と `ok.go`（検出されない）を置く。
   標準ライブラリを使う場合は `stub/<pkg>/*.go` に最小スタブを置き、`typecheck_with_deps` に渡す。
4. **統合テスト**: `crates/guff-<crate>/tests/checks_test.rs` に、fixture を型チェックし
   `run_analyzer(analyzer(), &pkg)` の診断を assert するテストを足す。
5. `cargo test -p guff-<crate>`。
6. `guff-lint` レジストリ（`crates/guff-lint/src/registry.rs`）に登録し、プリセットに入れるか決める。

### 5.1 パターン DSL（`guff-pattern`）

staticcheck の `pattern` パッケージ相当。Go の AST ノードを s 式で書く:

```
(CallExpr (Builtin "append") [_])          # append(x) の 1 引数呼び出し（SA4021）
(SliceExpr x@(Object _) low (CallExpr (Builtin "len") [x]) nil)   # s[x:len(s)]（S1010）
(BinaryExpr (IntegerLiteral _) "/" (IntegerLiteral _))            # 定数/定数（SA4025）
```

- `_` = ワイルドカード、`name@(...)` = 束縛、`[...]` = リスト、`(Object "名前")` /
  `(Builtin "名前")` / `(Symbol "pkg.Func")` = 型情報を使う照合。
- 実体は `crates/guff-pattern/src/{lexer,parser,pattern,match}.rs`。
- **注意（2026-07-14 の教訓）**: サブパターンの照合結果を必ず `?` で伝播すること。
  過去に `match_expr_node` 等が結果を捨てて常に成功し、`CallExpr` の関数部が未検証になって
  SA4021 が全ファイル誤検出した。ワイルドカード `_` は各 matcher で明示対応が要る。

---

## 8. ロードマップ — 「golangci-lint 互換の高速 linter」を名乗るために

このセクションが本書の核心。**「互換」と「高速」を主張できる状態**を、検証可能なマイルストーン
A〜G に分解し、各タスク（R番号）に「目的 / なぜ必要 / どこを触る / 参考 / 手順 / 完了条件 / テスト」を付けた。
経験の浅い作業者は、依存関係（各タスクの「前提」）を守り、R番号の小さい順に進めるとよい。

### 「互換」の定義（受け入れ基準の親）
1. 実在する `.golangci.yml` を読み込んでエラーにならない（設定互換）。
2. 同じ設定で golangci-lint と guff を同じコードに掛けたとき、**指摘集合がほぼ一致**する
   （差分は §8 Milestone G の差分テストで定量化し、既知差分として文書化できる）。
3. 出力フォーマット（少なくとも `text` colored と `json`）が CI で差し替え可能。
4. `//nolint` を尊重する。
5. `--fix` で自動修正できる（golangci-lint と同等の範囲）。

### 「高速」の定義
6. マルチコア並列で解析する。
7. 永続キャッシュで増分再解析ができる。
8. 代表的 OSS リポジトリで golangci-lint と同等以上の wall-clock（ベンチで実証）。

---

### Milestone A — ドロップイン CLI / 設定互換

> ゴール: 既存プロジェクトが `guff run ./...` に置き換えても「設定が効く」。

#### R1. 診断を stdout に出し、終了コードを設定可能にする ✅ 完了 (2026-07-14)
- **目的/なぜ**: golangci-lint は指摘を stdout に出し、CI は stdout をパースする。現状 guff は stderr。
  また `--issues-exit-code`（デフォルト 1）が無いと CI 制御に使えない。
- **どこ**: `crates/guff-lint/src/lib.rs`（`print_text`, `run_and_print`）, `src/main.rs`（`RunArgs`）。
- **手順**: (1) `print_text` の出力先を stdout に変更。(2) `--issues-exit-code <int>`（default 1）を追加し、
  指摘ありのとき返す。(3) 内部エラーは 2 のまま。
- **完了条件**: `guff run ... > out.txt` に指摘が入る。`--issues-exit-code 0` で指摘があっても 0 を返す。
- **テスト**: `tests/` で stdout をキャプチャして assert、終了コードを検証。
- **完了メモ**: `run_and_print` → stdout、`LintOptions::issues_exit_code` + CLI フラグ、
  `tests/run_output_test.rs`（ライブラリ + CLI バイナリ）で検証。

#### R2. `.golangci.yml` の完全パースと適用（issues / run / severity） ✅ 完了 (2026-07-14)
- **目的/なぜ**: 互換の中核。今は linter の enable/disable しか効かない。実プロジェクトは
  `issues.exclude-rules` などに強く依存している。
- **どこ**: `crates/guff-lint/src/config.rs`（`ConfigV2`/`ConfigV1` に構造体追加）、
  新規 `src/exclude.rs`（後処理フィルタ）。
- **参考**: golangci-lint の `pkg/config/*.go`（`Issues`, `Run`, `Severity`, `Output`）と
  `pkg/result/processors/*`（exclude, exclude-rules, nolint, max-per-linter, uniq-by-line, path-prettifier）。
- **手順**:
  1. `ConfigV2` に `issues`（`exclude`, `exclude-rules`[linters/path/path-except/text/source], `exclude-dirs`,
     `exclude-files`, `exclude-use-default`(bool), `max-issues-per-linter`, `max-same-issues`,
     `new`/`new-from-rev`）、`run`（`build-tags`, `tests`, `go`, `timeout`, `concurrency`）、`severity`、
     `output` を追加して serde でパース。
  2. 診断を出した後に通す**後処理パイプライン**を作る（Go の `result/processors` を順に再現）:
     path で除外 → exclude-rules で除外 → デフォルト除外（`exclude-use-default: true` のとき golangci の
     既定除外集合）→ linter/line ごとの上限 → severity 付与。
  3. `run.build-tags` は `guff-build`/`guff-packages` のビルドタグに渡す。
- **完了条件**: 代表的な `.golangci.yml` を食わせてパースエラー無し。`exclude-rules` で特定ファイルの
  指摘が消える。
- **テスト**: 設定 fixture（`testdata/config/*.yml`）+ その設定で期待される除外結果のスナップショット。
- **完了メモ**: `IssuesConfig`/`RunConfig`/`SeverityConfig`/`OutputConfig` を serde パース。
  `exclude.rs` の `IssueFilter` が dirs/files/text/rules/default-excludes/max-*/uniq-by-line/severity を適用。
  `run.build-tags` → `go list -tags=...`、`run.tests` → `Config.tests`。
  ~~DEFERRED: v2 `linters.exclusions`~~ → **完了**（2026-07-16；`paths`/`paths-except`/`rules`/`presets` を
  `ConfigFile::effective_issues` で折り込み。v2 は既定除外なし。`warn-unused` / `generated` は DEFERRED）。
  DEFERRED: `issues.new`/`new-from-rev`（git diff）。`run.timeout` は R5 で実効化。
  `run.concurrency` の真の並列は R9。
  テスト: `v2_full_issues.yml` パース、`v2_exclude_errcheck_bad.yml` / `v2_linters_exclusions.yml` +
  `tests/exclude_test.rs` / `tests/config_test.rs`。

#### R3. `//nolint` ディレクティブと `nolintlint` ✅ 完了 (2026-07-14)
- **目的/なぜ**: 互換の必須項目。既存コードは `//nolint:...` で意図的に抑制している。
- **前提**: AST がコメント位置を保持していること（**R32 の recording/ノード ID に依存する場合あり**。
  最低限、コメントの行番号が取れれば実装可能）。
- **どこ**: 新規 `crates/guff-lint/src/nolint.rs`、後処理で診断をフィルタ。`nolintlint` は
  `guff-lint` 内 or `guff-style` の analyzer として。
- **参考**: golangci-lint `pkg/result/processors/nolint.go`、`pkg/golinters/nolintlint`。
- **手順**: (1) 各ファイルのコメントを走査し、`//nolint`（全 linter）と `//nolint:a,b`（特定）を、
  行末指定/直上行指定/ブロック指定の規則で収集。(2) 診断を (行, linter名) で突き合わせて抑制。
  (3) `nolintlint`: 使われていない nolint・書式不正・説明必須(`require-explanation`) を報告。
- **完了条件**: `//nolint:staticcheck` の行の staticcheck 指摘が消える。未使用 nolint を報告できる。
- **テスト**: nolint 付き fixture で抑制を確認、`nolintlint` の testdata。
- **完了メモ**: `NolintIndex` が `PARSE_COMMENTS` で再パースし、同一行 + 直前行の AST 展開レンジを構築。
  `IssueFilter::apply` の exclude 後段で抑制。`--enable nolintlint` で未使用ディレクティブを報告。
  DEFERRED: NeedsMachineOnly / NeedsExplanation / NeedsSpecific（書式・説明必須）。
  テスト: `tests/nolint_test.rs` + `testdata/run/nolint_{errcheck,unused}`。

#### R4. per-linter 設定（`linters-settings`）の各 analyzer への配線 ✅ 完了 (2026-07-14)
- **目的/なぜ**: `errcheck.check-blank`, `govet.enable/disable`, `staticcheck.checks`,
  `gocyclo.min-complexity` などが効かないと「互換」と言えない。
- **どこ**: `guff-analysis` の `Analyzer`/`Pass` に設定を渡す仕組みを新設（例: `Pass` に
  `settings: &LinterSettings` を持たせる、または analyzer 構築時にクロージャで束縛）。
  `guff-lint` が config から各 analyzer 用設定を組み立てて runner に渡す。
- **手順**: (1) 型付き設定構造体を linter ごとに定義。(2) `guff-lint` で config → 設定へ変換。
  (3) runner → Pass 経由で analyzer が参照。まず errcheck / govet / staticcheck の主要キーから。
- **完了条件**: `errcheck: check-blank: true` で `_ = f()` が検出されるようになる（設定で挙動が変わる）。
- **テスト**: 同じコードに対し設定違いで指摘が変わることを確認。
- **完了メモ**: `guff_analysis::SettingsBag` を `Pass` / `RunnerOptions` に配線。
  `guff-lint/src/settings.rs` が `linters.settings` / v1 `linters-settings` をパースし、
  errcheck は Pass-time（`check-blank` / `check-type-assertions` / **`exclude-functions`** /
  **`disable-default-exclusions`**）、govet は
  `enable`/`disable`/`disable-all`/`enable-all`、staticcheck は `checks`（`all`/`-SAxxxx`）で
  選択時フィルタ + Pass-time `initialisms` / `dot-import-whitelist` /
  `http-status-code-whitelist`（`stylecheck` YAML キーも merge）。~~DEFERRED: errcheck `exclude-functions` / `disable-default-exclusions`~~ →
  **完了**（2026-07-16）。DEFERRED（残）: errcheck `verbose`。
  ~~staticcheck initialisms 等~~ → **完了**（2026-07-16；`initialisms` /
  `dot-import-whitelist` / `http-status-code-whitelist` + `stylecheck` キー merge）。
  テスト: `tests/settings_test.rs` + `v2_errcheck_check_blank.yml` /
  `v2_errcheck_exclude_functions.yml` / `v2_staticcheck_stylecheck_settings.yml` +
  `guff-errcheck` exclude fixtures。

#### R5. 補助サブコマンドと run フラグ ✅ 完了 (2026-07-14)
- **目的/なぜ**: `guff linters`（利用可能/有効な linter 一覧）, `guff version`, `--timeout`,
  `-j/--concurrency`, `--build-tags` は移行時に必ず使われる。
- **どこ**: `crates/guff-lint/src/main.rs`。
- **完了条件**: `guff linters` が enabled/available を表示。`guff version` がバージョンを出す。
- **テスト**: 各サブコマンドの出力を assert。
- **完了メモ**: `version`（`--short`）/ `linters`（config・`--preset`/`--enable`/`--disable` 反映の
  Enabled/Disabled 一覧）。`--timeout` + `run.timeout`（Go duration、既定 `1m`、`0` で無効、超過で
  exit 4）。`-j/--concurrency` + `run.concurrency`（`1` → sequential）。`--build-tags` は既存。
  DEFERRED（当時）: concurrency > 1 の真の並列（→ **R9 で完了**）。
  テスト: `tests/cli_test.rs`。

---

### Milestone B — 出力フォーマット互換

> ゴール: CI が期待するフォーマットで出せる。少なくとも colored text と JSON。

#### R6. formatter 抽象 + テキスト整形の移設 ✅ 完了 (2026-07-14)
- **どこ**: 新規 `crates/guff-lint/src/format/mod.rs`（`trait Formatter { fn print(&self, issues, w) }`）。
  既存 `format.rs` を `format/text.rs` に移す。`--out-format <name>`（複数指定可）を追加。
- **完了条件**: `--out-format text` が現行と同じ出力。抽象越しに動く。
- **完了メモ**: `Formatter` + `OutputFormatKind::Text` / `TextFormatter`。`--out-format`
  （`text` / `line-number` / `colored-line-number`→text）。config `output.formats`/
  `output.format` もベストエフォート適用（未実装名は stderr で無視）。
  DEFERRED（当時）: JSON（→ R7）、色付き・ソース行下線（→ R8 で完了）、~~`format:path` 書き出し~~ → **完了**（2026-07-16）。
  テスト: `tests/format_test.rs` + `format` ユニットテスト。

#### R7. JSON 出力（golangci-lint スキーマ準拠） ✅ 完了 (2026-07-14)
- **なぜ**: 最も使われる機械可読フォーマット。互換の要。
- **参考**: golangci-lint `pkg/printers/json.go`。トップレベル `{"Issues": [...], "Report": {...}}`、
  各 Issue に `FromLinter`, `Text`, `Severity`, `SourceLines`, `Pos{Filename,Offset,Line,Column}`,
  `ExpectNoLint` / `ExpectedNoLintLinter`（`SuggestedFixes`/`LineRange` は omitempty）。
- **完了条件**: `--out-format json` が golangci-lint と同じキー構造を出す（フィールド名一致）。
- **テスト**: JSON をパースしてキー/値を検証。可能なら golangci-lint 実出力とのスナップショット比較。
- **完了メモ**: `format/json.rs` の `JsonFormatter`。`OutputFormatKind::Json`。空 Issues は `[]`、
  `Report` は現状常に `null`（`JsonReport` 型は用意）。`SourceLines` は単一行キャプチャ時のみ配列。
  DEFERRED: Report への warnings/linter 一覧埋め込み、`SuggestedFixes` JSON 化、golangci 実機スナップショット。
  テスト: `format/json.rs` ユニット + `tests/format_test.rs` CLI。

#### R8. その他フォーマット（colored-line-number, checkstyle, sarif, tab, github-actions） ✅ 完了 (2026-07-14)
- **参考**: golangci-lint `pkg/printers/*.go` に各実装がある。優先度: colored-line-number（既定）→
  github-actions（CI 注釈）→ checkstyle/sarif（企業 CI）→ tab。
- **完了条件**: 各フォーマットが対応ツールで読める。
- **完了メモ**: `TextFormatter::colored`（ANSI bold/red + source/`^` caret）、
  `GithubActionsFormatter`（`::error file=…`；v2 では削除済みだが CI 向けに復元）、
  `CheckstyleFormatter`（XML 5.0）、`SarifFormatter`（2.1.0・driver=`guff`）、
  `TabFormatter` / `colored-tab`。~~`format:path` への実ファイル書き出し~~ → **完了**（2026-07-16；`OutputSpec` + 親 dir 作成・`stdout`/`stderr`・config v2 map/`{format,path}`）。
  テスト: 各 formatter ユニット + `tests/format_test.rs` CLI（`json:path` 含む）。

---

### Milestone C — 高速（性能）

> ゴール: 「fast」を数字で主張できる。

#### R9. 真のパッケージ/analyzer 並列実行（PL11） ✅ 完了 (2026-07-14)
- **目的/なぜ**: 現在は実質シングルスレッド（`action.rs` の並列分岐も同一スレッド実行）。
  「fast」の前提。golangci-lint はワーカー並列。
- **障害**: 現状 `guff_packages::Package` / AST が `RefCell` を含み `!Sync`。
- **どこ**: `crates/guff-runner/src/action.rs`（`exec_all`）、`crates/guff-packages`（Package の共有可能化）。
- **手順**: (1) 解析中は AST/型情報を**不変共有**にする（`Arc` 化 or 解析用イミュータブルビューを作る）。
  (2) パッケージ単位で `rayon` などのスレッドプールに投げる（依存グラフの葉から）。
  (3) analyzer 内の `RefCell` 依存を洗い出して除去 or `Send+Sync` を満たす形へ。
- **完了条件**: マルチコアで wall-clock が対コア数でスケール。結果は逐次実行と**完全一致**（決定的）。
- **テスト**: 逐次 vs 並列で診断集合が一致することを大きめ fixture で確認。ベンチ（R11）で速度確認。
- **完了メモ**: `Ident::obj` を `RefCell` → `Mutex` にし `Package: Sync`。`exec_all` が依存ウェーブフロント
  + rayon プールで並列実行。`RunnerOptions.concurrency` / `-j` / `run.concurrency` をプールサイズに配線
  （未指定は `available_parallelism`）。結果の一致は診断多重集合で検証。
  DEFERRED: ロード/型チェック並列。wall-clock スケールは R11 で計測開始（現状は guff 側が遅い）。
  テスト: `crates/guff-runner/tests/parallel_test.rs`。

#### R10. 永続キャッシュ（増分再解析） ✅ 完了 (2026-07-14)
- **目的/なぜ**: 「fast」の本命。2 回目以降が速くないと golangci-lint に勝てない。
- **参考**: golangci-lint `internal/cache`（`GOLANGCI_LINT_CACHE`）。
- **どこ**: 新規 `crates/guff-runner/src/cache.rs`。
- **手順**: (1) キャッシュキー = ファイル内容ハッシュ + 有効 analyzer 集合 + 設定 + guff バージョン +
  go バージョン + ビルドタグ。(2) パッケージ単位で診断結果を保存/復元し、未変更パッケージは再解析しない。
  (3) 保存先は `$GOLANGCI_LINT_CACHE` or OS キャッシュディレクトリ。`guff cache clean` も用意。
- **完了条件**: 2 回目の実行が大幅に短縮。ファイルを変えたパッケージだけ再解析される。
- **テスト**: 1 ファイル変更後、そのパッケージのみ再解析されることをログ/計測で確認。
- **完了メモ**: `IssueCache` が SHA-256 コンテンツハッシュ（NeedAllDeps）+ salt（guff/go 版・analyzers・
  tags・settings）でキー化。hit パッケージは analysis スキップし診断を JSON から復元。
  `GUFF_CACHE` > `GOLANGCI_LINT_CACHE` > `{UserCacheDir}/guff`。`guff cache clean`/`status`、
  `--no-cache`。~~DEFERRED: ロード/型チェックのスキップ（ヒットでも load は走る）~~ → **R10.1 で実装済み**。
  DEFERRED（残）: facts 永続化（R24）。
  テスト: `cache.rs` ユニット + `tests/cache_test.rs` + `cli_test` cache サブコマンド。

#### R11. ベンチマークハーネス（対 golangci-lint） ✅ 完了 (2026-07-14)
- **目的/なぜ**: 「高速」を主張するには実測が要る。
- **どこ**: 新規 `benchmarks/`（スクリプト + 対象 OSS リポジトリのリスト）。
- **手順**: 数個の代表 OSS リポジトリで、同一プリセットで golangci-lint と guff の wall-clock を測り、
  コールド/ウォーム両方を記録。結果を §3 か README にテーブルで載せる。
- **完了条件**: 再現可能なベンチスクリプトと数値表。
- **テスト**: スクリプトが CI（or 手動）で回る。
- **完了メモ**: `benchmarks/run.sh`（cold/warm・median・`FAIL` 検知）+ `smoke.sh` + 共有
  `standard.yml`。デフォルト対象は `fixture/` と合成コーパス `local/`（〜3k LOC・
  SSA セーフ方言）。`--oss` で `repos.txt` を追加計測（現状 staticcheck→buildir で
  多く FAIL → R17）。数値表は `benchmarks/results/RESULTS.md` と README。
  所見（初版）: warm 比で guff は golangci-lint の ~5–6x（load/型チェックが毎回走るためキャッシュ恩恵が薄い）。
  → **その後の性能パス（R10.1）で解消。現状 guff は cold/warm とも golangci-lint より高速**
  （warm `local` 0.54x、`fixture` 0.77x）。詳細は R10.1 と `results/RESULTS.md`。
  テスト: `./benchmarks/smoke.sh`（オフライン）。

#### R10.1 性能パス（Rust 版が golangci-lint より遅い問題の解消） ✅ 完了 (2026-07-14)
- **目的/なぜ**: R11 の初回計測で warm が golangci-lint の ~5–6x だった。「Rust で書き直す意味」を
  成立させるための最優先課題。`sample` プロファイルで原因を実測特定。
- **原因（4 つ、いずれも言語ではなく実装/ビルドの問題）**:
  1. `[profile.release]` 未設定（LTO なし・`codegen-units=16`）。
  2. `typecheck_packages` が逐次ループ（10 コアでも 1 コアのみ使用）＝ warm の支配的コスト。
  3. キャッシュ salt が非決定的（`SettingsBag` の `Debug` が `HashMap` をランダム順で出力）→
     warm の約半分で**全ミス**。
  4. `NeedAllDeps` 依存ハッシュが非決定的（import グラフの深さが `connect_imports` の
     `HashMap` 走査順で変動）→ 全パッケージが毎回 hit↔miss でフリップ。
- **対策**:
  - `Cargo.toml` に fat LTO + `codegen-units=1`（`panic=unwind` は `catch_unwind` のワーカー
    分離のため維持）。
  - `typecheck_packages` を rayon で並列化（各 pkg は依存を `.a` export data から解決＝独立）。
  - `FileSet::add_file` の base 採番レースを修正（parser が `base=-1` を渡し、書き込みロック内で
    原子的採番）。副産物として errcheck の列バグ（`f00.go:14:7`→`14:9`、golangci 一致）も修正。
  - salt を決定的化（`SettingsBag` の `Debug` でキーをソート）。
  - `NeedAllDeps` を、脆弱な Arc 再帰ではなく `go list` の平坦・完全な `deps` ＋ 全パッケージの
    `id/pkg_path → self_hash` レジストリ（`IssueCache::set_dep_hashes`）から計算。
  - **遅延型チェック（R10 の DEFERRED「load/型チェックのスキップ」を実装）**: `run_linters` は
    まずメタデータのみロード（型抜きの `metadata_mode`）→ issues キャッシュを先に判定 →
    **ミスした root だけ** `guff_packages::typecheck_roots` で parse+型チェック。ヒットは
    `IssueCache::get_cached` + `exclude::issue_from_cached` で位置解決済みの `Issue` を直接復元
    （`FileSet` 不要）。`LintResult` に `cached_issues` を追加し `issues()` で統合。
- **結果**: guff が cold/warm とも golangci-lint より高速。warm `local` 1.76s→0.16s（0.54x）、
  `fixture` 0.92s→0.14s（0.77x）。完全 warm では型チェック 0 本。1 ファイル編集時は該当 pkg と
  その依存元のみ再解析（`GUFF_DEBUG_CACHE=1` で hit/miss と型チェック本数を表示）。
- **テスト**: 全ワークスペース 195 テスト green。cold 出力 = warm 出力 = golangci-lint 基準で一致。
- **発展余地（未実施）**: R24 を参照。

---

### Milestone D — 自動修正

#### R12. `--fix`（SuggestedFix / TextEdit の適用） ✅ 完了 (2026-07-14)
- **目的/なぜ**: golangci-lint 互換の重要機能。データ型（`Diagnostic.suggested_fixes`,
  `TextEdit`）は既にあるが誰も適用していない。
- **どこ**: `crates/guff-lint/src/fix.rs`（新規）、`main.rs` に `--fix`。
- **参考**: golangci-lint `pkg/result/processors/fixer.go`、go/analysis の `applyFixes`。
- **手順**: (1) 全診断の TextEdit を収集。(2) ファイルごとに**オフセット降順**でソートし、重なりを排除
  （重複時は最初の 1 つだけ適用）。(3) ファイルに書き戻す。(4) 修正した診断は出力から除く。
- **完了条件**: `guff run --fix` が SA1004 等の quickfix を実ファイルに適用する。
- **テスト**: 修正前/後のファイルスナップショット。
- **完了メモ**: `fix::apply_fixes` がフィルタ後の issues から最初の `SuggestedFix` を収集し、
  ファイルごとにオフセット降順・重なり排除で適用。`LintOptions.fix` + `--fix` フラグ。
  修正済み issue は stdout から除外し stderr に `fixed N issue(s)` を表示。
  DEFERRED: キャッシュヒット pkg の suggested_fixes 復元（現状空 Vec）、複数 fix 候補の選択。
  テスト: `fix.rs` ユニット + `tests/fix_test.rs`（SA1004 ライブラリ/CLI）。

---

### Milestone E — linter の網羅（golangci-lint のラインナップに追随）

> golangci-lint は 100+ linter を束ねる。全部は不要でも、主要どころを揃えないと「互換」の説得力が弱い。
> 各 linter は §5 の手順で 1 個ずつ追加する。**1 タスク = 数 linter** を目安に。

#### R13. go/analysis 系 linter（`guff-gostaticanalysis` ほか）🟡 部分完了 (2026-07-14)
- nilerr, nilnil, forcetypeassert, makezero, mirror, nilnesserr（`guff-gostaticanalysis`）。
- errorlint, errname, err113, errchkjson, wrapcheck, rowserrcheck（error 系）。
- bodyclose, noctx, contextcheck, sqlclosecheck, spancheck, fatcontext（context/resource 系）。
- gosec, gocritic, unparam, unconvert, exhaustive, exhaustruct, copyloopvar, perfsprint,
  usestdlibvars, usetesting, durationcheck, goconst, musttag, loggercheck, sloglint, testifylint ほか。
- **完了条件**: 各 linter に bad/ok testdata、`go vet`/golangci 相当の指摘、レジストリ登録。
  - **進捗メモ (2026-07-15)**:
  1. `guff-gostaticanalysis`: forcetypeassert / nilnil / makezero / **mirror**（butuzov/mirror；75 violation テーブル；`[]uint8`/`untyped string` 正規化；SuggestedFix は同一パッケージ/メソッドの単行のみ。マルチ行・AltPackage 跨ぎ import 書き換えは DEFERRED）
  2. `guff-error`: errname / err113 / durationcheck / **errorlint** / **wrapcheck** / **errchkjson**
     （golangci 既定 `omit-safe`；`check-error-free-encoding` / `report-no-exported` settings 配線済み；
     wrapcheck `ignore-sigs` / `extra-ignore-sigs` / `ignore-sig-regexps` / `ignore-package-globs` /
     `ignore-interface-regexps` / `report-internal-errors` settings 配線済み）
  3. `guff-context`: **noctx**（AST call-name; upstream は buildssa）/ **fatcontext**
  4. `guff-style`（新）: **copyloopvar**（`check-alias` settings 済）/ **usetesting**（per-check flags settings 済）/
     **usestdlibvars**（HTTP method/status 既定オン + optional tables settings 済）
     / **perfsprint**（fmt.Sprint/Sprintf/Errorf 既定オン; concat-loop/loop-other-ops/`err-error`/`int-conversion` 実効化済; fiximports は DEFERRED）
     / **goconst**（golangci 既定 min-len=3 / min-occurrences=3 / exclude call; find-duplicates 実効化済）
     / **dogsled**（golangci 既定 max-blank-identifiers=2）/ **asciicheck** / **goprintffuncname**
     / **funlen** / **gocyclo** / **lll** / **gocognit** / **nestif** / **cyclop**
     / **nakedret** / **nosprintfhostport** / **predeclared** / **whitespace** / **nlreturn** / **mnd**
     / **prealloc** / **tagalign** / **wsl**（R14）/ **unconvert**（`fast-math` settings 済；`safe` 親コンテキストは DEFERRED）
     / **exhaustruct**（`include` / `exclude` / `allow-empty` / `allow-empty-rx` / `allow-empty-returns` / `allow-empty-declarations` settings 済；コメントディレクティブは DEFERRED）
     / **exhaustive**（switch 既定；`check` / `default-signifies-exhaustive` / `default-case-required` / `ignore-enum-members` / `ignore-enum-types` / `package-scope-only` settings 済；map・コメントディレクティブは DEFERRED）
     / **musttag**（go-simpler/musttag；json/xml/yaml/toml/mapstructure/sqlx builtins + `linters.settings.musttag.functions` 済；iface whitelist はメソッド名ヒューリスティック）
     / **loggercheck**（timonwong/loggercheck；kitlog/klog/logr/slog/zap 奇数 KV + Attr/Field フィルタ；`linters.settings.loggercheck` 済）
     / **sloglint**（go-simpler/sloglint；`no-mixed-args` 既定；`linters.settings.sloglint` 済）
     / **testifylint**（Antonboom/testifylint；blank-import / bool-compare / compares / empty / error-nil / float-compare / len / nil-compare / zero / negative-positive / useless-assert / contains / equal-values / regexp / encoded-compare / error-is-as / expected-actual / **time-compare** / **formatter** / **suite-dont-use-pkg** / **suite-extra-assert-call** / **suite-subtest-run** / **suite-broken-parallel** / **suite-method-signature** / **suite-thelper**（既定オフ）/ **require-error** / **go-require** / **mock-expect**；`linters.settings.testifylint` 済）
     / **exptostd**（ldez/exptostd；`golang.org/x/exp/{maps,slices,constraints}` → std `maps`/`slices`/`cmp`；Clear/Ordered/import SuggestedFix 済；Keys/Values SuggestedFix は DEFERRED）
     / **modernize**（x/tools modernize；any / plusbuild / forvar / rangeint / minmax / fmtappendf / omitzero / slicessort / **stringscutprefix** / **slicescontains** / **stringsseq** / **waitgroupgo** / **mapsloop** / **slicesbackward** / **reflecttypefor** / **testingcontext** / **unsafefuncs** / **importcomment** / **stringscut**（Split/SplitN[0]）；`linters.settings.modernize.disable` 済）
     / **gocritic**（go-critic；**58** checkers: default 34 + enable-all extras 24: deferUnlambda / emptyDecl / emptyFallthrough / emptyStringTest / initClause / nilValReturn / octalLiteral / yodaStyleExpr / builtinShadow / builtinShadowDecl / commentedOutImport / dupImport / filepathJoin / paramTypeCombine / rangeAppendAll / weakCond / dupOption / methodExprCall / rangeExprCopy / regexpPattern / sortSlice / sqlQuery / typeAssertChain / **badRegexp**；`linters.settings.gocritic` enable-all/disable-all/enabled-checks/disabled-checks 済）
     / **godot** / **godox** / **dupword**（`guff-comment`）
     / **depguard** / **gomoddirectives** / **gomodguard**（新 `guff-import`）
   いずれも `--enable` で有効化。テストは各クレートの `tests/checks_test.rs`。
  **DEFERRED（同タスク内の残り）**:
   - `nilerr` / `nilnesserr` — SSA（→ R17）
   - ~~`mirror`~~ → **完了**（2026-07-16；butuzov/mirror 75 patterns + SuggestedFix 主要ケース。マルチ行・AltPackage import 書き換えは DEFERRED）
   - `errorlint` の errorf 既定オフ / allowed マップ完全版
   - ~~`wrapcheck` の package-glob / interface-regexp 設定~~ → **完了**（2026-07-16；ignore-sigs / extra-ignore-sigs / ignore-sig-regexps / report-internal-errors 含む）
   - ~~`usestdlibvars` の optional テーブル（`time-weekday` / `crypto-hash` 等）~~ → **完了**（2026-07-16）
   - `copyloopvar` / `usetesting` / `usestdlibvars`（HTTP）settings 配線は **完了**（2026-07-16）
   - `gocyclo` / `gocognit` / `nestif` / `dogsled` / `funlen` / `cyclop` / `lll` / `nakedret` / `nlreturn` / `predeclared` / `whitespace` / `mnd` / `prealloc` / `tagalign` / `wsl` / `perfsprint` / `goconst` の `linters.settings` 配線は **完了**（2026-07-16）
   - `whitespace` `multi-if` / `multi-func` 実効化は **完了**（2026-07-16）
   - `goconst` `match-constant` / `numbers` / `min` / `max` 実効化は **完了**（2026-07-16）
   - `perfsprint` concat-loop / `loop-other-ops` 実効化は **完了**（2026-07-16）
   - `goconst` `find-duplicates` 実効化は **完了**（2026-07-16）
   - `perfsprint` `err-error` / `int-conversion` 実効化は **完了**（2026-07-16；prometheus 設定互換）
   - `perfsprint` の fiximports
   - ~~`errchkjson` settings（`check-error-free-encoding` / `report-no-exported`）~~ → **完了**（2026-07-16）
   - `unconvert` の `-safe` 親コンテキスト判定（`isSafeContext`）
   - `exhaustruct` の `//exhaustruct:ignore` / `//exhaustruct:enforce` コメントディレクティブ
   - `exhaustive` の map チェック・`//exhaustive:ignore` / `//exhaustive:enforce`・`check-generated`
   - `musttag` の真の `types.Implements` iface whitelist・sqlx 全メソッドエントリ
   - ~~`loggercheck`~~ → **完了**（2026-07-16；rulefile・printf 完全パリティは DEFERRED）
   - ~~`sloglint`~~ → **完了**（2026-07-16；SuggestedFix・discard-handler Go 1.24 ゲートは DEFERRED）
   - ~~`testifylint`~~ → **完了**（2026-07-16；計 **28** checker；`mock-expect` 含む。SuggestedFix・formatter printf CheckPrintf は DEFERRED）
   - ~~`exptostd`~~ → **完了**（2026-07-16；Keys/Values SuggestedFix は DEFERRED）
   - ~~`modernize`（コア checker）~~ → **拡充**（2026-07-16；any / plusbuild / forvar / rangeint / minmax / fmtappendf / omitzero / slicessort + **stringscutprefix** / **slicescontains** / **stringsseq** / **waitgroupgo** / **mapsloop**（map→map `Copy`）+ **slicesbackward** / **reflecttypefor** / **testingcontext** + **unsafefuncs** / **importcomment** / **stringscut**（Split/SplitN[0]→Cut）+ `disable` settings。計 **19** checker。atomictypes / embedlit / errorsastype / newexpr / stditerators / stringsbuilder、stringscut Index/Contains/bytes、unsafefuncs Slice/String、importcomment Module==nil skip、mapsloop Insert/Collect/Clone、cutprefix pattern2/bytes、slicescontains ContainsFunc/break、waitgroupgo trailing-Done、reflecttypefor 複雑型・unused-var 削除、slicesbackward 変異完全解析、testingcontext typeindex sole-use、rangeint/minmax 完全パリティは DEFERRED）
   - ~~`gocritic`（コア + prometheus enable-all 主要 extras）~~ → **完了**（2026-07-16；初回 18 → 29 → 34 → 42 → 50 → 57 → **58** checks + enable-all/disabled-checks。残 enable-all extras: `ruleguard`；badRegexp dangling-anchor/flag 完全パリティ・per-check settings・SuggestedFix・caseOrder 式 switch 重なりは DEFERRED）
   - `rowserrcheck` / bodyclose / contextcheck / sqlclosecheck — SSA（→ R17）
   - gosec / unparam ほか（R13 続きセッションで数個ずつ）

#### R14. スタンドアロン linter 🟡 部分完了 (2026-07-15)
- `guff-revive`（独自ルールエンジン）, `guff-misspell`, `guff-dupl`。
- `guff-style` バンドル: funlen, gocyclo, gocognit, cyclop, nestif, lll, whitespace, wsl, nlreturn,
  nakedret, prealloc, predeclared, mnd, nosprintfhostport, tagalign。
  （クレート自体は R13 で新設済み。copyloopvar / usetesting / usestdlibvars / perfsprint / goconst /
  dogsled / asciicheck / goprintffuncname / **funlen**（60 lines / 40 stmts）/ **gocyclo**（min=30）/
  **lll**（120 / tab-width=1）/ **gocognit**（min=30）/ **nestif**（min=5）/ **cyclop**（max=10）/
  **nakedret**（max-func-lines=30）/ **nosprintfhostport** / **predeclared**（qualified=false）/
  **whitespace**（multi-if/multi-func 既定オフ）/ **nlreturn**（block-size=1）/ **mnd**（全 checks・0/1 無視）/
  **prealloc**（simple / range-loops 既定）/ **tagalign**（align+sort 既定）/
  **wsl**（v4 既定 cuddle の主要ルール）済。
  `gocyclo:ignore` / `gocognit:ignore`・PARSE_COMMENTS 前提の funlen コメント除外・
  SuggestedFix・tagalign StrictStyle・wsl 完全パリティ / `wsl_v5` は DEFERRED。
  `usestdlibvars` optional テーブル（`time-weekday` / `time-month` / `time-layout` / `crypto-hash` /
  `default-rpc-path` / `sql-isolation-level` / `tls-signature-scheme` / `constant-kind` /
  `time-date-month`）実効化済み。
  `copyloopvar` / `usetesting` / `usestdlibvars`（HTTP + optional）/ `gocyclo` / `gocognit` / `nestif` / `dogsled` / `funlen` / `cyclop` / `lll` / `nakedret` / `nlreturn` / `predeclared` / `whitespace` / `mnd` / `prealloc` / `tagalign` / `wsl` / `perfsprint` / `goconst` の `linters.settings` 配線済み。
  `perfsprint` concat-loop / `loop-other-ops` / `err-error` / `int-conversion`・`goconst` `find-duplicates` 実効化済み。`perfsprint` fiximports は DEFERRED。
- `guff-comment`: **godot**（declarations + period 既定；`scope`/`exclude`/`period`/`capital` settings 済）/ **godox**（TODO/BUG/FIXME；`keywords` settings 済）/ **dupword**
  （comments + string literals；`keywords`/`ignore`/`comments-only` settings 済）。
  SuggestedFix・godot `toplevel`/`noinline`・dupword 跨行 / `skip-raw-strings` は DEFERRED。
- `guff-import`: **depguard**（既定 `$gostd` only；`rules` / `list-mode` / `files` / `allow` / `deny` settings 済）/ **gomoddirectives**（replace 禁止；`replace-local` / `replace-allow-list` / `retract-allow-no-explanation` / `exclude-forbidden` / `toolchain-forbidden` / `tool-forbidden` / `go-debug-forbidden` settings 済）/ **gomodguard**（blocked・local-replace；v1 `blocked.modules` + `gomodguard_v2` blocked list settings 済）。
  allowed modules/domains・version constraints・`match-type`・depguard `${base-path}` / `${config-path}`・gomoddirectives `ignore-forbidden` / `toolchain-pattern` / `go-version-pattern` / `check-module-path` は DEFERRED。
- `guff-misspell`: **misspell**（golangci/misspell `DictMain` + locale US/UK；`mode=restricted` でコメントのみ）。
  `linters.settings.misspell` 配線済み（locale / ignore-words / extra-words / mode）。
- `guff-dupl`: **dupl**（golangci/dupl suffix-tree クローン検出；既定 threshold=150）。
  `linters.settings.dupl.threshold` YAML 配線済み。
- `guff-revive`: **revive**（golint-default **23 rules** + extended **77 rules**: … prior 69 … / comments-density / datarace / enforce-map-style / enforce-slice-style / enforce-switch-style / enforce-repeated-arg-type-style / package-directory-mismatch / forbidden-call-in-wg-go）。
  `linters.settings.revive` YAML 配線済み（rules リスト・arguments・global/per-rule severity・confidence・ignore-generated-header）。
  prometheus 互換 rule 引数: `context-as-argument` `allowTypesBefore`、`early-return`/`indent-error-flow`/`superfluous-else` の `preserveScope`（`allowJump` も配線）、`var-naming` allowlist/blocklist / `skipInitialismNameChecks` / `upperCaseConst`（`skipPackageNameChecks` 等は upstream 同様無視・`package-naming` へ）。

#### R15. formatter（`guff-fmt` + `guff fmt` サブコマンド, Milestone L5）
- gofmt, gofumpt, goimports, gci, golines。**別パイプライン**（解析ではなく整形）。
- golangci-lint v2 は `formatters` セクションを持つので、config 互換のためにも必要。

#### R16. staticcheck の ST*（stylecheck）/ QF*（quickfix）🟡 部分完了 (2026-07-16)
- 現在 `guff-staticcheck` は S* + SA* + **ST* 15** + **QF* 12**（ST1000 / ST1001 / ST1003 / ST1006 / ST1011 / ST1012 / ST1013 / ST1015 / ST1017 / ST1018 / ST1019 / ST1020 / ST1021 / ST1022 / ST1023 + **QF1001** / **QF1002** / **QF1003** / **QF1004** / **QF1005** / **QF1006** / **QF1007** / **QF1008** / **QF1009** / **QF1010** / **QF1011** / **QF1012**）。
- **進捗**: ST1000（package comment）/ ST1001（dot imports; `dot-import-whitelist` settings 済）/ ST1003（識別子命名・既定 initialisms; `initialisms` settings 済; `//export`/`//go:linkname` は `FuncDecl.doc` があるときのみ）/
  ST1006（receiver `self`/`this`/`_`; AST 版）/
  ST1011（`time.Duration` 単位 suffix; struct field は Defs 欠落時 AST フォールバック）/ ST1012（error var 命名）/
  ST1013（HTTP status magic numbers → `http.Status*`; 既定 whitelist 200/400/404/500; `http-status-code-whitelist` settings 済）/
  ST1015（switch default 位置）/ ST1017（Yoda conditions + SuggestedFix; `TrulyConstantExpression` `_` + `match_token_node` Or/Binding 修正）/
  ST1018（string literal の Cf/Cc; emoji ZWJ/variation selector 許容 + SuggestedFix）/
  ST1019（重複 import）/
  ST1020（exported func/method doc が名前で始まる; `PARSE_COMMENTS` 再パース）/
  ST1021（exported type doc が名前で始まる; 冠詞 A/An/The 許容; `PARSE_COMMENTS` 再パース）/
  ST1022（exported var/const doc が名前で始まる; `PARSE_COMMENTS` 再パース; 括弧グループ・複数名はスキップ）/
  ST1023（冗長な var 型; `CheckExpr` 無し近似・BasicLit 既定型 / 名前付き const 除外; SuggestedFix で型削除; syscall/unsafe import 時スキップ）。
  **QF1001**（De Morgan `!(a && b)` → `!a || !b`; SuggestedFix 非再帰/再帰; float 除外; SimplifyParentheses は DEFERRED）/
  **QF1002**（untagged switch → tagged; side-effect / 混在変数除外 + SuggestedFix）/
  **QF1003**（if/else-if → tagged switch; ≥2 if・break 除外 + SuggestedFix）/
  **QF1004**（`Replace`/`SplitN`/`SplitAfterN` + `n==-1` → `ReplaceAll`/`Split`/`SplitAfter`; strings+bytes; SuggestedFix 済; renamed import は DEFERRED）/
  **QF1005**（`math.Pow(x,0..3)` 展開 + SuggestedFix; `CheckExpr` float64 wrap は DEFERRED）/
  **QF1006**（`for { if cond { break } }` → `for !cond`; SuggestedFix 済）/
  **QF1007**（`x := false; if cond { x = true }` → `x := cond` + SuggestedFix）/
  **QF1008**（embedded field 省略 `a.B.F` → `a.F`; 連続 selector チェーン; 呼び出し割り込みチェーンは DEFERRED）/
  **QF1009**（`time.Time == time.Time` → `.Equal` + SuggestedFix）/
  **QF1010**（print 系へ渡す `[]byte` → `string(...)`; SuggestedFix 済; `fmt.Stringer` skip は DEFERRED）/
  **QF1011**（冗長な var 型; ST1023 相当で `could` + `flagHelpfulTypes`; SuggestedFix 済）/
  **QF1012**（`Write([]byte(fmt.Sprint*))` / `WriteString(fmt.Sprint*)` → `fmt.Fprint*`; Writer 判定は結果 arity 近似; SuggestedFix 済）。
- **残**: ST1005（IR）/ ST1008（IR）/ ST1016（IR）。ST1023 の真の `types.CheckExpr` パリティ。QF1005 の float64 wrap。QF1004 renamed import。QF1010 Stringer skip。QF1012 の真の `types.Implements`。QF1001 SimplifyParentheses。QF1008 呼び出し割り込みチェーン。
- テスト: `st1000` / `st1001` / `st1003` / `st1006` / `st1011` / `st1012` / `st1013` / `st1015` / `st1017` / `st1018` / `st1019` / `st1020` / `st1021` / `st1022` / `st1023` / `qf1001` / `qf1002` / `qf1003` / `qf1004` / `qf1005` / `qf1006` / `qf1007` / `qf1008` / `qf1009` / `qf1010` / `qf1011` / `qf1012` fixtures + `checks_test`（settings: whitelist / custom initialisms）+ `v2_staticcheck_stylecheck_settings.yml`。

---

### Milestone F — 土台の穴（breadth/speed を塞ぐ前提）

#### R17. SSA の残作業（`RangeStmt` ＋ メソッド機構 E25+）
- **なぜ**: `RangeStmt` 未実装のため SA1015 が buildir を default require できず inspect のみ。S1029 も
  AST 簡易版。さらに `methods.rs`/メソッドラッパ（`$thunk`/`$bound`）/`InstantiateGenerics`/メソッド呼び出し
  emit が未了（§3.2.1）。今後の IR ベース linter（gosec の一部等）に必要。
- **どこ**: `crates/guff-ssa`（builder / methods）。
- **完了条件**: `for k, v := range x` を SSA 化し、メソッド呼び出しを含む IR がビルドできる。
  SA1015 を buildir require に戻して green。golden 逆アセンブル比較を維持。

#### R18. `typeindex` の移植
- **なぜ**: 呼び出しサイトの高速索引。pattern 系全 linter と errcheck の性能最適化。機能ブロッカーでは
  ないが「高速」に効く。
- **参考**: staticcheck `go/ir` + `analysis/facts/typeindex`。
- **どこ**: `guff-analysis` にフレームワーク追加。

#### R19. 型チェッカの残り（initorder / recording / util）
- `initorder.rs`（Step 34）: パッケージ初期化順。init-cycle 検出や一部 linter が依存。
- `recording.rs`（Step 37）: **AST ノード ID の記録**。nolint の行対応（R3）や正確な位置情報に効く。
- `util.rs`（Step 39）残、および D01/D02/D03/D04/D07/D10/D13/D16 の未了分（旧 `MIGRATION.md` の
  deferral 表 = git 履歴参照）。

#### R20. オフライン/`go` 無し driver（PL02）と GOCACHE 管理（PL07）
- **なぜ**: `go` バイナリに依存しない環境・CI サンドボックスでの実行と、キャッシュ配置の整合。
- **どこ**: `guff-packages`（driver 抽象）、`guff-runner`（cache パス）。

#### R24. 性能フォローアップ（R10.1 の発展余地）
- **なぜ**: R10.1 で cold/warm とも golangci-lint 超えを達成したが、さらに詰められる余地がある。
  いずれも機能ブロッカーではなく、大規模リポジトリでの伸びしろ。
- **項目**:
  1. **facts の永続化**: analyzer 間 facts（`analysis.Fact`）をキャッシュに保存し、ミス pkg の
     再解析でも依存の facts 再計算を避ける（golangci `runner_action_cache.go` 相当）。R10 からの継続 DEFERRED。
  2. **サブパッケージ（ファイル）粒度のインクリメンタル型チェック**: 現状はパッケージ単位でミス→
     パッケージ丸ごと再型チェック。巨大パッケージで 1 ファイル変更時の再チェック範囲を狭める。
  3. **export data デコードの共有キャッシュ**: `typecheck_package` はパッケージごとに新規 `Checker`/
     `ExportImporter` を作り、共通の stdlib 依存（fmt 等）の export data をパッケージ数分デコードする
     （プロファイル上の `preload_exports` 重複）。並列化で wall-clock は隠せているが総 CPU は無駄。
     デコード済み型パッケージを共有アリーナで再利用できれば cold の総 CPU をさらに削減。
  4. **`go list` メタデータのキャッシュ/差分ロード**: 現状 warm でも毎回 `go list` を実行（~0.05s）。
     大規模ツリーではここも効いてくる。
- **どこ**: `guff-runner/src/cache.rs`（facts）、`guff-packages/src/typecheck.rs`（共有 importer・
  粒度）、`guff-packages/src/golist.rs`（メタデータキャッシュ）。

---

### Milestone G — 互換性の検証（「互換」を名乗る根拠）

> A〜F を作っても、**実測で一致を示さない限り「互換」とは言えない**。ここが主張の裏付け。

#### R21. 差分テストハーネス（guff vs golangci-lint）
- **目的**: 同一コーパス・同一設定で両者を実行し、指摘集合を diff。linter ごとに一致率（precision/recall）を出す。
- **どこ**: 新規 `compat/`（対象リポジトリ一覧、両ツール実行、正規化、diff レポート）。
- **手順**: (1) 数十リポジトリを固定。(2) golangci-lint（参照）と guff を standard プリセットで実行。
  (3) `file:line:linter:message` に正規化して差分を集計。(4) 既知差分を許容リストに登録し、
  新規差分が出たら CI で落とす。
- **完了条件**: 一致率レポートが生成され、CI ゲートになる。
- **テスト**: ハーネス自体のスモークテスト。

#### R22. `.golangci.yml` コーパスのパース検証
- 実在の `.golangci.yml`（有名 OSS のもの）を集めて、**パースエラー 0** を保証するテスト。
- **進捗メモ (2026-07-16)**: `crates/guff-lint/tests/testdata/config_corpus/` を追加し、
  Prometheus / Grafana 由来の golangci-lint v2 設定 snapshot を `parse_config_str` →
  `linter_selection` → `effective_issues` まで通す smoke test を追加。未知キー
  （`run.allow-parallel-runners` / `formatters.exclusions` 等）は serde の既存挙動で許容。
  **DEFERRED**: 実 OSS 数十件への拡張、CI ゲート化、出典 URL/更新手順の体系化。

#### R23. 互換性マトリクスの公開
- どの linter・どの設定キー・どの出力フォーマットが「対応済/部分/未対応」かを表にして README/本書に載せる。
- **これが揃って初めて「golangci-lint 互換の高速 linter」と公に主張できる。**

---

## 9. 付録

### 9.1 上流ソースの取得（参考実装を読むとき）
```bash
git clone --depth 1 https://github.com/golangci/golangci-lint.git   # CLI/config/printers/processors
git clone --depth 1 https://github.com/golang/tools.git             # go/analysis, go/packages, passes
git clone --depth 1 https://github.com/dominikh/go-tools.git        # staticcheck / simple / pattern / unused
git clone --depth 1 https://github.com/golangci/go-printf-func-name.git
git clone --depth 1 https://github.com/stbenjam/no-sprintf-host-port.git
# 型チェッカ: Go 本体の src/cmd/compile/internal/types2
```

### 9.2 マイルストーン → 主張の対応
| 主張 | 必要マイルストーン |
|------|--------------------|
| 「golangci-lint 互換」 | A（設定/CLI）+ B（出力）+ D（--fix）+ E（linter 網羅）+ G（差分実証） |
| 「高速」 | C（並列+キャッシュ+ベンチ）+ F（typeindex 等） |

### 9.3 進捗の更新場所
- 状況 → §3 の表。
- 残タスクの消化 → §8 の該当 R 番号に完了メモ/日付。
- 新しい linter → §3.3 の表に 1 行。

---

## 10. セッション記録（新しいものほど上）

| 日付 | 内容 |
|------|------|
| 2026-07-16 | **R22 開始**: `.golangci.yml` config corpus smoke を追加。Prometheus / Grafana 由来の golangci-lint v2 設定 snapshot を `crates/guff-lint/tests/testdata/config_corpus/` に置き、`parse_config_str` / `linter_selection` / `effective_issues` まで検証。未知キー（`run.allow-parallel-runners` / `formatters.exclusions` 等）もパース許容を確認。テスト: `cargo test -p guff-lint --test config_test` |
| 2026-07-16 | **R8 続き**: `--out-format format:path` / config `output.formats` の `path` でファイル書き出しを実装（親ディレクトリ作成・`stdout`/`stderr`）。`OutputSpec` 導入。golangci v2 map 形（`formats: { json: { path: … } }`）と sequence `{format, path}` をパース。テスト: `format` ユニット + `cli_out_format_path_writes_json_file_not_stdout` |
| 2026-07-16 | **R13 続き**: `mirror`（butuzov/mirror）を `guff-gostaticanalysis` に追加しレジストリ登録。strings/bytes/regexp/utf8/os/bufio/httptest/maphash の **75** violation テーブル。`[]uint8`/`untyped string` 正規化。SuggestedFix は同一パッケージ/メソッドの単行。テスト: `mirror/{bad,ok,regexp_bad}.go` + stubs |
| 2026-07-16 | **R16 続き**: quickfix QF* **4** 件追加（`QF1001` / `QF1002` / `QF1003` / `QF1008`）。計 **164** analyzers（S* 37 + SA* 100 + ST* 15 + QF* 12）。SuggestedFix 付き。QF1001 SimplifyParentheses・QF1008 呼び出し割り込みチェーン・残 IR 依存 ST* は DEFERRED。テスト: `qf1001` / `qf1002` / `qf1003` / `qf1008` |
| 2026-07-16 | **R16 続き**: quickfix QF* **4** 件追加（`QF1006` / `QF1010` / `QF1011` / `QF1012`）。計 **160** analyzers（S* 37 + SA* 100 + ST* 15 + QF* 8）。SuggestedFix 付き。QF1010 Stringer skip・QF1012 真の Implements・残 QF1001/2/3/8 と IR 依存 ST* は DEFERRED。テスト: `qf1006` / `qf1010` / `qf1011` / `qf1012` |
| 2026-07-16 | **R16 続き**: quickfix QF* 初回バッチ **4** 件（`QF1004` / `QF1005` / `QF1007` / `QF1009`）。計 **156** analyzers（S* 37 + SA* 100 + ST* 15 + QF* 4）。SuggestedFix 付き。QF1005 の float64 wrap・QF1004 renamed import・残 QF1001/2/3/6/8/10/11/12 と IR 依存 ST* は DEFERRED。テスト: `qf1004` / `qf1005` / `qf1007` / `qf1009` |
| 2026-07-16 | **R16 続き**: stylecheck settings 配線（`initialisms` / `dot-import-whitelist` / `http-status-code-whitelist`）。`linters.settings.staticcheck` + レガシー `stylecheck` キーを merge → `StylecheckOptions` → ST1001/ST1003/ST1013。空リストは upstream 既定（golangci 互換）。残 ST1005/ST1008/ST1016（IR）/ QF* / ST1023 CheckExpr は DEFERRED。テスト: settings fixtures + `v2_staticcheck_stylecheck_settings.yml` |
| 2026-07-16 | **R16 続き**: stylecheck ST* **3** 件追加（`ST1003` / `ST1022` / `ST1023`）。計 **152** analyzers（S* 37 + SA* 100 + ST* 15）。ST1003 は既定 initialisms（settings DEFERRED）。ST1022 は `PARSE_COMMENTS` 再パース。ST1023 は `CheckExpr` 無し近似 + SuggestedFix。残 ST1005/ST1008/ST1016（IR）/ QF* は DEFERRED。テスト: `st1003` / `st1022` / `st1023` |
| 2026-07-16 | **R16 続き**: stylecheck ST* **4** 件追加（`ST1013` / `ST1018` / `ST1020` / `ST1021`）。計 **149** analyzers（S* 37 + SA* 100 + ST* 12）。ST1013 は既定 whitelist・SuggestedFix 済（`http_status_code_whitelist` settings は DEFERRED）。ST1020/ST1021 は `Mode::NONE` 対策で `PARSE_COMMENTS` 再パース。残 ST1003/ST1005/ST1008/ST1016/ST1022/ST1023 / QF* は DEFERRED。テスト: `st1013` / `st1018` / `st1020` / `st1021` |
| 2026-07-16 | **R16 続き**: stylecheck ST* **4** 件追加（`ST1000` / `ST1011` / `ST1017` / `ST1019`）。計 **145** analyzers（S* 37 + SA* 100 + ST* 8）。`TrulyConstantExpression` の `_` ワイルドカード + `match_token_node` の Or/Binding 対応を修正（ST1017 前提）。ST1011 struct field は Defs 欠落時 AST フォールバック。残 ST1003/ST1005/ST1008/ST1013/ST1016/ST1018/ST1020–23 / QF* は DEFERRED。テスト: `st1000` / `st1011` / `st1017` / `st1019` |
| 2026-07-16 | **R16 開始**: stylecheck ST* の初回バッチ **4** 件（`ST1001` / `ST1006` / `ST1012` / `ST1015`）。計 **141** analyzers（S* 37 + SA* 100 + ST* 4）。`dot_import_whitelist`・ST1005/IR 依存・QF* は DEFERRED。テスト: `st1001` / `st1006` / `st1012` / `st1015` |
| 2026-07-16 | **R13 続き**: `modernize` に checker **3** 件追加（`unsafefuncs` / `importcomment` / `stringscut` Split/SplitN[0]→Cut）。計 **19** checker。残 atomictypes / newexpr / stringsbuilder / stringscut Index 等は DEFERRED。テスト: `unsafefuncs.go` / `importcomment.go` / `stringscut.go` |
| 2026-07-16 | **R13 続き**: `modernize` に checker **3** 件追加（`slicesbackward` / `reflecttypefor` / `testingcontext`）。計 **16** checker。prometheus `modernize` disable（`newexpr`/`omitzero`）互換は維持。残 atomictypes / newexpr / stringscut 等は DEFERRED。テスト: `slicesbackward.go` / `reflecttypefor.go` / `testingcontext.go` + reflect/context/testing stubs |
| 2026-07-16 | **R13 続き**: `modernize` に **`mapsloop`** を追加（`for k, v := range x { m[k] = v }` → `maps.Copy`；Go 1.23+・map→map）。計 **13** checker。Insert/Collect（iter.Seq2）/ Clone は DEFERRED。テスト: `mapsloop.go` + `stub/maps` |
| 2026-07-16 | **R13 続き**: `modernize` に checker **4** 件追加（`stringscutprefix` / `slicescontains` / `stringsseq` / `waitgroupgo`）。計 **12** checker。prometheus の `modernize` enable カバレッジ向上。残 atomictypes / mapsloop / reflecttypefor / stringscut 等は DEFERRED。テスト: `bad.go` 拡張 + `stringsseq.go` / `waitgroupgo.go` |
| 2026-07-16 | **R14 続き**: revive `var-naming` の rule 引数を実効化（allowlist / blocklist / `skipInitialismNameChecks` / `upperCaseConst`）。prometheus の `skip-package-name-checks` は upstream 同様パースのみ（無視；`package-naming` へ移行済み）。interface メソッドの params/results も検査。テスト: `var_naming_{skip_initialism,upper_case_const,lists}.go` + `v2_revive_prometheus_args.yml` |
| 2026-07-16 | **R14 続き**: revive の prometheus 互換 rule 引数を実効化。`context-as-argument` の `allowTypesBefore`（`*testing.T,testing.TB`）、`early-return` / `indent-error-flow` / `superfluous-else` の `preserveScope`（+ early-return `allowJump`）。upstream 同様 `preserveScope` 未指定時はスコープ拡大候補も報告。テスト: `context_allow.go` / `preserve_scope.go` + `v2_revive_prometheus_args.yml` |
| 2026-07-16 | **R13/R14 続き**: `perfsprint` の `err-error`（`error` 実装型 → `err.Error()`；既定オフ）と `int-conversion`（int8/uint 等のキャスト付き最適化；既定オン、親 `integer-format` オフ時は連動オフ）を実効化。prometheus `.golangci.yml` の `err-error: true` / `int-conversion: true` 互換。fiximports は DEFERRED。テスト: `err_error.go` / `int_conversion.go` + `v2_style_settings_extended.yml` |
| 2026-07-16 | **R13 続き**: `gocritic` に enable-all extra **`badRegexp`** を追加（`regex-syntax` AST で char-class dup/交差・suspicious range・alt anchor/dup・nested quantifier・flag 冗長/clear・dangling `^`）。計 **58** checker（default 34 + extras 24）。prometheus `enable-all` 向け残りは `ruleguard`。dangling-anchor/flag 完全パリティは DEFERRED。テスト: `gocritic_bad_regexp` + `extras.go` |
| 2026-07-16 | **R13 続き**: `gocritic` に enable-all extras **7** 件追加（dupOption / methodExprCall / rangeExprCopy / regexpPattern / sortSlice / sqlQuery / typeAssertChain）。計 **57** checker（default 34 + extras 23）。prometheus `enable-all` 向け残りは `badRegexp` / `ruleguard`。テスト: `gocritic/extras.go` + `gocritic_enable_all_extras` |
| 2026-07-16 | **R13 続き**: `gocritic` に enable-all extras **8** 件追加（builtinShadow / builtinShadowDecl / commentedOutImport / dupImport / filepathJoin / paramTypeCombine / rangeAppendAll / weakCond）。計 **50** checker（default 34 + extras 16）。prometheus `enable-all` 向け残りは badRegexp / dupOption / methodExprCall / rangeExprCopy / regexpPattern / ruleguard / sortSlice / sqlQuery / typeAssertChain。テスト: `gocritic/extras.go` + `gocritic_enable_all_extras` |
| 2026-07-16 | **R13 続き**: `gocritic` に enable-all extras **8** 件追加（deferUnlambda / emptyDecl / emptyFallthrough / emptyStringTest / initClause / nilValReturn / octalLiteral / yodaStyleExpr）。計 **42** checker（default 34 + extras 8）。prometheus `enable-all` カバレッジ向上。残 enable-all extras・SuggestedFix は DEFERRED。テスト: `gocritic/extras.go` + `gocritic_enable_all_extras` |
| 2026-07-16 | **R2 続き**: golangci-lint v2 `linters.exclusions`（`paths` / `paths-except` / `rules` / `presets` / `warn-unused`）をパースし `ConfigFile::effective_issues` → `IssueFilter` に配線。v2 は既定除外なし（presets で EXC* 相当を展開）。prometheus `.golangci.yml` の exclusions 形状に対応。`warn-unused` / `generated` は DEFERRED。テスト: `v2_linters_exclusions*.yml` + `exclude_test` / `config_test` |
| 2026-07-16 | **R4 続き**: `errcheck` の `linters.settings.exclude-functions` / `disable-default-exclusions` を YAML → `SettingsBag` → analyzer に配線（prometheus の `io.Copy` 等除外に必要）。`verbose` は DEFERRED。テスト: `exclude` / `default_exclude` fixtures + `v2_errcheck_exclude_functions.yml` |
| 2026-07-16 | **R13 続き**: `gocritic` に default checker **5** 件追加（caseOrder / codegenComment / commentFormatting / deprecatedComment / sloppyTypeAssert）。計 **34** checker。コメント系は PARSE_COMMENTS 再パース。残 enable-all extras・SuggestedFix は DEFERRED。テスト: `gocritic/{bad,ok}.go` 拡張 |
| 2026-07-16 | **R13 続き**: `gocritic` に default checker **11** 件追加（argOrder / badCond / dupBranchBody / dupSubExpr / flagName / mapKey / offBy1 / regexpMust / typeSwitchVar / unlambda / wrapperFunc）。計 **29** checker。prometheus `enable-all` カバレッジ向上。残 caseOrder / commentFormatting 等・型完全パリティは DEFERRED。テスト: `gocritic/{bad,ok}.go` 拡張 + regexp/sync stubs |
| 2026-07-16 | **R13 続き**: `gocritic`（go-critic）を `guff-style` に追加しレジストリ登録。実装 checker 18（appendAssign / assignOp / badCall / captLocal / defaultCaseOrder / dupArg / dupCase / elseif / exitAfterDefer / flagDeref / ifElseChain / newDeref / singleCaseSwitch / sloppyLen / switchTrue / underef / unslice / valSwap）。`linters.settings.gocritic`（`enable-all` / `disable-all` / `enabled-checks` / `disabled-checks`）YAML 配線（prometheus の enable-all+disabled-checks 互換）。残 default/extra checks・per-check settings・SuggestedFix は DEFERRED。テスト: `gocritic/{bad,ok}.go` + `v2_gocritic_settings.yml` |
| 2026-07-16 | **R13 続き**: `modernize`（golang.org/x/tools/go/analysis/passes/modernize）を `guff-style` に追加しレジストリ登録。実装 checker: any / plusbuild / forvar / rangeint / minmax / fmtappendf / omitzero / slicessort。`linters.settings.modernize.disable` YAML 配線（prometheus の omitzero/newexpr disable 互換）。残り modernize checker・完全パリティは DEFERRED。テスト: `modernize/{bad,ok,plusbuild}.go` + `v2_modernize_settings.yml` |
| 2026-07-16 | **R13 続き**: `exptostd`（ldez/exptostd）を `guff-style` に追加しレジストリ登録。`golang.org/x/exp/{maps,slices,constraints}` → std `maps`/`slices`/`cmp` を検出（Go バージョンゲート・slices は全置換時 import のみ報告）。Clear / Ordered / import の SuggestedFix 済；Keys/Values SuggestedFix は DEFERRED。テスト: `exptostd/{bad_maps,bad_slices,bad_constraints,ok}.go` |
| 2026-07-16 | **R13 続き**: `testifylint` に **`mock-expect`** を追加（計 **28** checker）。`mock.On("Method", …)` を `EXPECT().Method(…)` へ誘導（EXPECT 有無・引数 assignability・識別子メソッド名を検証）。SuggestedFix は DEFERRED。テスト: `mock_expect.go` + testify/mock stub + `testifylint_flags_mock_expect` |
| 2026-07-16 | **R13 続き**: `testifylint` に **`go-require`** を追加（計 **27** checker）。`go` / `sync.WaitGroup.Go` 内の require・`assert.FailNow`、入れ子ヘルパー呼び出し、HTTP handler（`net/http.HandlerFunc` シグネチャ）を検出。`linters.settings.testifylint.go-require.ignore-http-handlers` YAML 配線。SuggestedFix・間接コールバックは DEFERRED。残り `mock-expect`。テスト: `testifylint/{bad,ok}.go` + sync/http stubs + `testifylint_go_require_ignore_http_handlers` + `v2_testifylint_settings.yml` |
| 2026-07-16 | **R13 続き**: `testifylint` に advanced checker **`require-error`** を追加（計 26 checker）。if 条件・bool 式・NoError 連続・ブロック末尾・goroutine/`t.Cleanup`/suite teardown を upstream 準拠でスキップ。`linters.settings.testifylint.require-error.fn-pattern` YAML 配線。HTTP handler / WaitGroup.Go 文脈は DEFERRED。残り go-require / mock-expect。テスト: `testifylint/{bad,ok}.go` + `testifylint_require_error_fn_pattern` + `v2_testifylint_settings.yml` |
| 2026-07-16 | **R13 続き**: `testifylint` に suite checker **3** 件（`suite-broken-parallel` / `suite-method-signature` / `suite-thelper`）を追加。計 25 checker。`suite-thelper` は upstream 同様既定オフ（`enable` / `enable-all` で有効化）。SuggestedFix は DEFERRED。残り go-require / mock-expect / require-error。テスト: `testifylint/{bad,ok}.go` + suite iface stubs + `testifylint_suite_thelper_when_enabled` |
| 2026-07-16 | **R13 続き**: `testifylint` に suite checker **3** 件（`suite-dont-use-pkg` / `suite-extra-assert-call` / `suite-subtest-run`）を追加。計 22 checker。`linters.settings.testifylint.suite-extra-assert-call.mode`（`remove`/`require`）YAML 配線。CallMeta を `type_func_name` ベースにし Assertions メソッドを正しく認識。残り go-require / mock-expect / require-error / suite-broken-parallel / suite-method-signature / suite-thelper・SuggestedFix は DEFERRED。テスト: `testifylint/{bad,ok}.go` + suite stubs + `v2_testifylint_settings.yml` |
| 2026-07-16 | **R13 続き**: `testifylint` に checker **2** 件（`time-compare` / `formatter`）を追加。計 19 checker。`linters.settings.testifylint.time-compare.suppress-calls-pattern` / `formatter.{check-format-string,require-f-funcs,require-string-msg}` YAML 配線。formatter の printf CheckPrintf・SuggestedFix は DEFERRED。テスト: `testifylint/{bad,ok}.go` 拡張 + `v2_testifylint_settings.yml` |
| 2026-07-16 | **R13 続き**: `testifylint` に checker **3** 件（`encoded-compare` / `error-is-as` / `expected-actual`）を追加。計 17 checker。`linters.settings.testifylint.expected-actual.pattern` YAML 配線。テスト: `testifylint/{bad,ok}.go` 拡張 + `stub/errors` / `stub/encoding/json` / `stub/fmt` |
| 2026-07-16 | **R13 続き**: `testifylint` に checker **3** 件（`contains` / `equal-values` / `regexp`）を追加。計 14 checker。upstream 優先順に合わせて registry 順を更新。テスト: `testifylint/{bad,ok}.go` 拡張 + `stub/strings` / `stub/regexp` |
| 2026-07-16 | **R13 続き**: `testifylint` に checker **3** 件（`zero` / `negative-positive` / `useless-assert`）を追加。計 11 checker。upstream 優先順（zero → … → useless-assert）に合わせて registry 順を更新。テスト: `testifylint/bad.go` / `ok.go` 拡張 + `stub/time` |
| 2026-07-16 | **R13 続き**: `testifylint`（Antonboom/testifylint）を `guff-style` に追加しレジストリ登録。実装 checker: blank-import / bool-compare / compares / empty / error-nil / float-compare / len / nil-compare。`linters.settings.testifylint`（`enable-all` / `disable-all` / `enable` / `disable` / `bool-compare.ignore-custom-types`）YAML 配線。残り checker・SuggestedFix は DEFERRED。テスト: `testifylint/{bad,ok,blank,settings}.go` + `v2_testifylint_settings.yml` |
| 2026-07-16 | **R13 続き**: `sloglint`（go-simpler/sloglint）を `guff-style` に追加しレジストリ登録。`no-mixed-args` 既定オン。`linters.settings.sloglint`（`no-mixed-args` / `kv-only` / `attr-only` / `no-global` / `context` / `static-msg` / `msg-style` / `no-raw-keys` / `key-naming-case` / `allowed-keys` / `forbidden-keys` / `args-on-sep-lines` / `custom-funcs`）YAML 配線。SuggestedFix・discard-handler の Go 1.24 ゲートは DEFERRED。テスト: `sloglint/{bad,ok,settings}.go` + `v2_sloglint_settings.yml` |
| 2026-07-16 | **R13 続き**: `loggercheck`（timonwong/loggercheck）を `guff-style` に追加しレジストリ登録。kitlog/klog/logr/slog/zap の奇数 key-value を検出（slog Attr / zap Field フィルタ）。`linters.settings.loggercheck`（`kitlog`/`klog`/`logr`/`slog`/`zap` / `require-string-key` / `no-printf-like` / `rules`）YAML 配線。rulefile・printf 完全パリティは DEFERRED。テスト: `loggercheck/{bad,ok,custom,settings}.go` + `v2_loggercheck_settings.yml` |
| 2026-07-16 | **R13 続き**: `musttag`（go-simpler/musttag）を `guff-style` に追加しレジストリ登録。(un)marshal 先構造体のタグ欠落を検出（json/xml/yaml/toml/mapstructure/sqlx builtins）。`linters.settings.musttag.functions`（`name` / `tag` / `arg-pos`）YAML 配線。iface whitelist はメソッド名ヒューリスティック（真の Implements は DEFERRED）。テスト: `musttag/{bad,ok,custom}.go` + `v2_musttag_settings.yml` |
| 2026-07-16 | **R13 続き**: `exhaustive`（nishanths/exhaustive）を `guff-style` に追加しレジストリ登録。enum 型への switch の網羅性を検出（同一スコープの named+basic const）。`linters.settings.exhaustive`（`check` / `default-signifies-exhaustive` / `default-case-required` / `ignore-enum-members` / `ignore-enum-types` / `package-scope-only`）YAML 配線。map チェック・コメントディレクティブは DEFERRED。テスト: `exhaustive/{bad,ok,default_ok}.go` + `v2_exhaustive_settings.yml` |
| 2026-07-16 | **R13 続き**: `exhaustruct`（GaijinEntertainment/go-exhaustruct v4）を `guff-style` に追加しレジストリ登録。構造体リテラルの未初期化フィールドを検出（`exhaustruct:"optional"`・error return の空リテラル許容）。`linters.settings.exhaustruct`（`include` / `exclude` / `allow-empty` / `allow-empty-rx` / `allow-empty-returns` / `allow-empty-declarations`）YAML 配線。コメントディレクティブは DEFERRED。テスト: `exhaustruct/{bad,ok,include,empty_decl}.go` + `v2_exhaustruct_settings.yml` |
| 2026-07-16 | **R13 続き**: `unconvert`（mdempsky/unconvert）を `guff-style` に追加しレジストリ登録。同一型への不要な明示変換を検出（float/complex は既定スキップ）。`linters.settings.unconvert`（`fast-math` / `safe`）YAML 配線。`safe` の親コンテキスト判定は DEFERRED。テスト: `unconvert/{bad,ok,fast_math}.go` + `v2_unconvert_settings.yml` |
| 2026-07-16 | **R13/R14 続き**: `usestdlibvars` optional テーブル実効化（`time-weekday` / `time-month` / `time-layout` / `crypto-hash` / `default-rpc-path` / `sql-isolation-level` / `tls-signature-scheme` / `constant-kind` / `time-date-month`）+ `linters.settings` YAML 配線。テスト: `optional_*` fixtures + `v2_usestdlibvars_optional.yml` |
| 2026-07-16 | **R13 続き**: `wrapcheck` の `linters.settings` を YAML → `SettingsBag` に配線（`ignore-sigs` / `extra-ignore-sigs` / `ignore-sig-regexps` / `ignore-package-globs` / `ignore-interface-regexps` / `report-internal-errors`）。テスト: package-glob / extra-ignore-sigs / report-internal / interface-regexp fixtures + `v2_wrapcheck_settings.yml` |
| 2026-07-16 | **R14 続き**: `depguard` / `gomoddirectives` / `gomodguard`(+`gomodguard_v2`) の `linters.settings` を YAML → `SettingsBag` に配線（depguard `rules`/`list-mode`/`files`/`allow`/`deny`、gomoddirectives replace-local・allow-list・retract/exclude/toolchain/tool/godebug flags、gomodguard blocked + local-replace）。allowed modules/domains・version・match-type・path placeholders 等は DEFERRED。テスト: fixtures + `v2_import_settings.yml` |
| 2026-07-16 | **R14 続き**: `godot` / `godox` / `dupword` の `linters.settings` を YAML → `SettingsBag` に配線（godot `scope`/`exclude`/`period`/`capital`、godox `keywords`、dupword `keywords`/`ignore`/`comments-only`）。`toplevel`/`noinline`・SuggestedFix・跨行 dupword は DEFERRED。テスト: fixtures + `v2_comment_settings.yml` |
| 2026-07-16 | **R13 続き**: `errchkjson` の `check-error-free-encoding` / `report-no-exported` を `linters.settings` → `SettingsBag` → analyzer に配線（golangci: `omit-safe = !check-error-free-encoding`）。テスト: `check_error_free` / `no_exported` fixtures + `v2_errchkjson_settings.yml` |
| 2026-07-16 | **R13/R14 続き**: `copyloopvar` `check-alias`・`usetesting` per-check flags・`usestdlibvars` `http-method`/`http-status-code` を `linters.settings` → `SettingsBag` に配線（upstream / golangci キー準拠）。`usestdlibvars` optional テーブルは DEFERRED。テスト: settings fixtures + `v2_style_settings_extended.yml` |
| 2026-07-16 | **R13/R14 続き**: `goconst` `find-duplicates` 実効化（同一値の const を検出；upstream jgautheron/goconst / golangci メッセージ準拠）。`linters.settings.goconst.find-duplicates` YAML 配線。テスト: `find_duplicates_*` fixtures + settings integration |
| 2026-07-16 | **R13/R14 続き**: `perfsprint` concat-loop（ループ内文字列連結 → `strings.Builder` SuggestedFix）+ `loop-other-ops` 実効化（upstream catenacyber/perfsprint 準拠）。`linters.settings.perfsprint` に `concat-loop` / `loop-other-ops` YAML 配線。fiximports は DEFERRED。テスト: `concat_loop_*` fixtures + settings integration |
| 2026-07-16 | **R13/R14 続き**: `goconst` `match-constant` / `numbers` / `min` / `max` 実効化（upstream jgautheron/goconst 準拠）。`linters.settings.goconst` YAML 配線拡張。テスト: `numbers_*` / `match_constant_*` fixtures + settings integration tests |
| 2026-07-16 | **R13/R14 続き**: `whitespace` `multi-if` / `multi-func` 実効化（upstream ultraware/whitespace 準拠）。テスト: `multi_if_*` / `multi_func_*` fixtures + settings integration tests |
| 2026-07-16 | **R13/R14 続き**: `cyclop` `package-average` / `skip-tests`、`nakedret` `skip-test-files`、`predeclared` / `whitespace` / `mnd` / `prealloc` / `tagalign` / `wsl` / `perfsprint` / `goconst` の `linters.settings` YAML → `SettingsBag` 配線。テスト: `v2_style_settings_extended.yml` + `guff-style` settings integration tests |
| 2026-07-16 | **R13/R14 続き**: `linters.settings` を `cyclop` / `lll` / `nakedret` / `nlreturn` に配線（`max-complexity` / `line-length`・`tab-width` / `max-func-lines` / `block-size`）。テスト: `v2_style_settings.yml` 拡張 + `guff-style` settings integration tests |
| 2026-07-16 | **R13/R14 続き**: `linters.settings` を `gocyclo` / `gocognit` / `nestif` / `dogsled` / `funlen` に配線（`min-complexity` / `max-blank-identifiers` / `lines`・`statements`・`ignore-comments`）。テスト: `v2_style_settings.yml` + `guff-style` settings integration tests |
| 2026-07-16 | **R14 残完了**: `linters.settings.revive` の `confidence` / `ignore-generated-header`、`dupl.threshold`、`misspell`（locale / ignore-words / extra-words / mode=restricted）を YAML → `SettingsBag` に配線。テスト: `v2_revive_confidence.yml` / `v2_dupl_threshold.yml` / `v2_misspell_restricted.yml` + 各クレート integration tests |
| 2026-07-16 | **R14 続き**: `linters.settings.revive` の global/per-rule `severity` を YAML → `SettingsBag` → `Diagnostic.severity` → `Issue.severity` に配線。`severity: @linter` で revive 由来の severity を保持。テスト: `v2_revive_severity.yml` / `revive_applies_per_rule_and_global_severity` |
| 2026-07-16 | **R14 続き**: `guff-revive` に extended revive rules **8** 件（comments-density / datarace / enforce-map-style / enforce-slice-style / enforce-switch-style / enforce-repeated-arg-type-style / package-directory-mismatch / forbidden-call-in-wg-go）を追加。golint-default 23 + extended 77 = **100 rules**。`linters.settings.revive` YAML 配線（`guff-lint` → `SettingsBag` → `guff_revive::Settings`）。per-rule severity は DEFERRED |
| 2026-07-15 | **R14 続き**: `guff-revive` に extended revive rules **20** 件（confusing-naming / imports-blocklist / string-format / file-header / import-alias-naming / useless-break / useless-fallthrough / modifies-value-receiver / range-val-address / unsecure-url-scheme / banned-characters / file-length-limit / filename-format / multiline-if-init / package-naming / use-slices-sort / inefficient-map-lookup / redundant-test-main-exit / comment-spacings / epoch-naming）を追加。golint-default 23 + extended 69 = **92 rules**。`linters.settings.revive` と残 extended rules は DEFERRED |
| 2026-07-15 | **R14 続き**: `guff-revive` に extended revive rules **20** 件（flag-parameter / function-length / function-result-limit / use-any / use-fmt-print / unused-receiver / modifies-parameter / identical-branches / identical-ifelseif-branches / identical-ifelseif-conditions / identical-switch-branches / identical-switch-conditions / line-length-limit / max-control-nesting / nested-structs / unexported-naming / empty-lines / optimize-operands-order / range-val-in-closure / confusing-results）を追加。golint-default 23 + extended 49 = **72 rules**。`linters.settings.revive` と残 extended rules は DEFERRED |
| 2026-07-15 | **R14 続き**: `guff-revive` に extended revive rules **14** 件（atomic / bare-return / bool-literal-in-expr / call-to-gc / cyclomatic / duplicated-imports / if-return / string-of-int / time-equal / unchecked-type-assertion / unconditional-recursion / unnecessary-format / use-errors-new / waitgroup-by-value）を追加。golint-default 23 + extended 14 = **37 rules**。`config::with_extended_rules` + `extended_bad.go`/`extended_ok.go` で検証。`linters.settings.revive` と残 extended rules は DEFERRED |
| 2026-07-15 | **R14 続き**: `guff-revive` に golint-default 残り 14 rule（package-comments / exported / var-naming / range / errorf / error-return / unexported-return / context-as-argument / context-keys-type / indent-error-flow / superfluous-else / unused-parameter / unreachable-code / var-declaration）を追加。計 23 rule。`linters.settings.revive` と extended rules は DEFERRED |
| 2026-07-15 | **R14 完了**: 新 `guff-revive` に `revive`（golint-default 9 rules）を追加しレジストリ登録。`linters.settings.revive` YAML 配線と残 rule は DEFERRED |
| 2026-07-15 | **R14 続き**: 新 `guff-dupl` に `dupl`（golangci/dupl クローン検出・既定 threshold=150）を追加しレジストリ登録。`linters.settings.dupl.threshold` YAML 配線は DEFERRED |
| 2026-07-15 | **R14 続き**: 新 `guff-import` に `depguard`（既定 `$gostd` only）/ `gomoddirectives`（replace 禁止）/ `gomodguard`（blocked・local-replace）を追加しレジストリ登録。settings・gomodguard_v2 は DEFERRED |
| 2026-07-15 | **R14 続き**: 新 `guff-comment` に `godot`（宣言コメントの句点）/ `godox`（TODO/BUG/FIXME）/ `dupword`（重複語）を追加しレジストリ登録。settings・SuggestedFix・godot scope/capital は DEFERRED |
| 2026-07-15 | **R14 続き**: `guff-style` に `prealloc`（slice 事前確保）/ `tagalign`（struct tag 整列+sort）/ `wsl`（v4 既定 cuddle 主要ルール）を追加しレジストリ登録。settings・SuggestedFix・wsl 完全パリティ/`wsl_v5` は DEFERRED |
| 2026-07-15 | **R14 続き**: `guff-style` に `whitespace`（不要な先頭/末尾改行）/ `nlreturn`（return/branch 前の空行）/ `mnd`（マジックナンバー）を追加しレジストリ登録。wsl/prealloc/tagalign・settings・SuggestedFix は DEFERRED |
| 2026-07-15 | **R14 続き**: `guff-style` に `nakedret`（長い関数の naked return）/ `nosprintfhostport`（host:port の Sprintf）/ `predeclared`（予約識別子のシャドー）を追加しレジストリ登録。settings・SuggestedFix・qualified は DEFERRED |
| 2026-07-15 | **R14 続き**: `guff-style` に `gocognit`（認知的複雑度）/ `nestif`（深い if ネスト）/ `cyclop`（循環的複雑度 max=10）を追加しレジストリ登録。settings・ignore・package-average は DEFERRED |
| 2026-07-15 | **R14 続き**: `guff-style` に `funlen`（関数長）/ `gocyclo`（循環的複雑度）/ `lll`（行長）を追加しレジストリ登録。settings・`gocyclo:ignore` は DEFERRED |
| 2026-07-15 | **R14 続き**: `guff-style` に `dogsled`（blank ident 過多）/ `asciicheck`（非 ASCII 識別子）/ `goprintffuncname`（printf 系の `f` 接尾辞）を追加しレジストリ登録。`max-blank-identifiers` settings は DEFERRED |
| 2026-07-15 | **R13 続き**: `guff-error` に `errchkjson`（json.Marshal / MarshalIndent / Encoder.Encode；golangci 既定 omit-safe）。settings 配線と rowserrcheck/bodyclose 等（SSA）は DEFERRED |
| 2026-07-15 | **R13 続き**: `guff-style` に `perfsprint`（fmt 置換の既定チェック）と `goconst`（golangci 既定）を追加しレジストリ登録。concat-loop/fiximports・match-constant/numbers・settings は DEFERRED |
| 2026-07-15 | **R13 続き**: 新 `guff-style` に `copyloopvar` / `usetesting` / `usestdlibvars`（HTTP 既定）を実装しレジストリ登録。perfsprint/goconst・optional テーブル等は DEFERRED |
| 2026-07-14 | **R13 続き**: `errorlint` / `wrapcheck`（`guff-error`）+ `noctx` / `fatcontext`（新 `guff-context`）。レジストリ登録・testdata。noctx は AST 版（SSA 不要） |
| 2026-07-14 | **R13 続き**: `guff-error` クレート新設。`errname` / `err113` / `durationcheck` を実装しレジストリ登録（`--enable`）。errorlint ほかは DEFERRED |
| 2026-07-14 | **R13 部分完了**: `guff-gostaticanalysis` クレート新設。`forcetypeassert` / `nilnil` / `makezero` を実装し `guff-lint` レジストリに登録（`--enable`）。nilerr/nilnesserr（SSA）・mirror・その他多数は DEFERRED |
| 2026-07-14 | **R12 完了**: `--fix` で `SuggestedFix`/`TextEdit` をソースに適用（オフセット降順・重なり排除）。修正済み診断は出力から除外。SA1004（`time.Sleep`）で検証。`fix.rs` + `tests/fix_test.rs` |
| 2026-07-14 | **R10.1 完了（性能パス）**: warm が golangci の ~5–6x 遅かった原因を `sample` で特定し解消。①`[profile.release]` fat LTO+`codegen-units=1` ②`typecheck_packages` を rayon 並列化（+`FileSet` base 採番レース修正、errcheck 列バグも副産物で修正）③キャッシュ salt 決定化（`SettingsBag` Debug ソート）④`NeedAllDeps` を平坦 `deps`+self-hash レジストリで決定化 ⑤**遅延型チェック**（キャッシュ判定を先行、ミス pkg のみ parse+型チェック）。結果 **guff が cold/warm とも golangci 超え**（warm `local` 1.76s→0.16s=0.54x、`fixture` 0.77x）。全 195 テスト green・出力は golangci 基準一致。残余地は R24 |
| 2026-07-14 | **R11 完了**: `benchmarks/` ハーネス（guff vs golangci-lint、cold/warm、`standard.yml`）。`fixture`/`local` 計測 + `results/RESULTS.md`。`--oss` は SSA ギャップで FAIL しがち（R17）。（初回計測では warm が golangci の ~5–6x → R10.1 で逆転） |
| 2026-07-14 | **R10 完了**: パッケージ単位 issues 永続キャッシュ（`IssueCache`）。未変更 pkg は再解析スキップ。`GUFF_CACHE`/`GOLANGCI_LINT_CACHE`、`guff cache clean`/`status`、`--no-cache`。facts キャッシュと load スキップは DEFERRED。`tests/cache_test.rs` |
| 2026-07-14 | **R9 完了**: `Ident::obj` を `Mutex` 化して `Package: Sync`。action DAG を依存ウェーブフロント + rayon で並列実行。`-j`/`run.concurrency` をワーカー数に配線。逐次 vs 並列の診断一致を `tests/parallel_test.rs` で検証。wall-clock スケール実証は R11 DEFERRED |
| 2026-07-14 | **R8 完了**: colored-line-number / github-actions / checkstyle / sarif / tab / colored-tab。`format:path` 書き出しは当時 DEFERRED（→ 2026-07-16 完了）。`tests/format_test.rs` |
| 2026-07-14 | **R7 完了**: `--out-format json`（golangci `Issues`/`Report`/`FromLinter`/`Pos` 等キー一致）。`JsonFormatter` + `serde_json`。Report 埋め込み・SuggestedFixes は DEFERRED。`tests/format_test.rs` |
| 2026-07-14 | **R6 完了**: `Formatter` 抽象 + `format/text.rs` 移設、`--out-format text`（`line-number` 別名）。JSON/colored 等は R7/R8 DEFERRED。`tests/format_test.rs` |
| 2026-07-14 | **R5 完了**: `guff version` / `guff linters`、`--timeout`（`run.timeout`・既定 1m・exit 4）、`-j/--concurrency`（`1` → sequential）。真の並列は R9 DEFERRED。`tests/cli_test.rs` |
| 2026-07-14 | **R4 完了**: `linters.settings` 配線。`SettingsBag` → Pass、errcheck `check-blank`/`check-type-assertions`、govet enable/disable、staticcheck `checks`。exclude-functions 等は DEFERRED |
| 2026-07-14 | **R3 完了**: `//nolint` / `//nolint:linter` フィルタ（同一行・直前行 AST 展開）+ `nolintlint` 未使用報告。書式/説明必須は DEFERRED |
| 2026-07-14 | **R2 完了**: `.golangci.yml` の `issues`/`run`/`severity`/`output` パース + 除外後処理パイプライン（`exclude.rs`）。`exclude-rules` で指摘抑制、`run.build-tags`/`tests` を load に配線。diff 除外・timeout/concurrency 実効は DEFERRED |
| 2026-07-14 | **R1 完了**: 診断を stdout へ出力、`--issues-exit-code`（既定 1）追加。`tests/run_output_test.rs` で検証 |
| 2026-07-14 | 5 つの計画書（MIGRATION / PRE-LINTER-PLAN / LINTER-MIGRATION / STATICCHECK-MIGRATION / ADDING-ANALYZER）を本書に統合。§8 に golangci-lint 互換 + 高速化のロードマップ（R1–R23 / Milestone A–G）を追記 |
| 2026-07-14 | 独立リポジトリ化（`dakimura/guff`）後、`guff run` を実 Go プログラムで安定化。型チェッカ 2 バグ（再帰ジェネリックの subst 無限再帰 / 符号なし定数の `^` 精度）修正。pattern エンジンのサブパターン照合破棄バグ + ワイルドカード/pkg 関数シンボル修正（SA4021 等の誤検出解消）。printf を引数個数・型照合まで実装し `go vet` 一致。全 1806 テスト green |
| 2026-07-14 | standard プリセット完走: errcheck / ineffassign / unused 本実装、staticcheck 137 analyzers、govet 29/29 |
| 2026-07-13 | `guff-lint` CLI 骨格、standard プリセット、`migrate`、解析基盤（PRE-LINTER Phase 0–7）、型チェッカ Checker エンジン |
