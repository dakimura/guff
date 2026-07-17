# guff 開発ガイド & ロードマップ（唯一の正典）

> このファイルは、以前分かれていた次の 5 つの計画書を **1 本に統合** したものです。
> `MIGRATION.md` / `PRE-LINTER-PLAN.md` / `docs/LINTER-MIGRATION.md` /
> `docs/STATICCHECK-MIGRATION.md` / `docs/ADDING-ANALYZER.md`。
> これらの原文は git 履歴に残っています（`git log -- docs/LINTER-MIGRATION.md` 等）。
> 以後、**設計・進捗・残タスク**の更新はこの 1 ファイルに集約してください。
> セッションごとの作業ログは肥大化するため [`SESSION-LOG.md`](SESSION-LOG.md) に分離しています（§10）。

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
| **Formatters** | `guff-fmt` | `guff fmt`（gofmt / gofumpt / goimports / gci / golines / swaggo） |
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

> 最終更新: 2026-07-17。ワークスペース全体 **2300+ tests green**。実装済み linter の一覧・件数・設定配線は §3.3 を、作業履歴は `SESSION-LOG.md` を参照。

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
  PL05（ctrlflow）, PL02（go 無し driver）（→ §8 各タスク）。
  PL11（真の並列実行）は **R9 で完了**。

### 3.2.1 SSA（`guff-ssa`, `go/ssa` 移植）
- naive SSA（lift 無し）→ dom/lift/blockopt → Milestone D/E/F 完了。**150+ tests green**。
- 型機構（subst/canonizer/typeset/instantiate）と builder コア、**RangeStmt**（slice/array/map/string/chan/int/func）、
  **methods.rs** / `$thunk`/`$bound` wrappers / `InstantiateGenerics` / メソッド呼び出し emit、
  **SwitchStmt / TypeSwitchStmt / SelectStmt / SendStmt / IncDecStmt / EmptyStmt** / compound assign / fallthrough
  まで移植済み。SA1015 は buildir require で green。
- **残（DEFERRED）**: range over non-array pointer、一部 expr/lvalue 端（`exprN` の未対応形、type-switch
  guard の一部）、`MakeInterface` 完全モデル、method instantiation wrapper（receiver / 0-result）、
  package-level n:1 init。IR ベース linter（nilerr / contextcheck 等）の追加配線は → R13。

### 3.3 実装済み linter

> 各行の「規模」は代表的な状態のみ。詳細な設定キー・DEFERRED は各 analyzer のコード内 `// DEFERRED:` と `SESSION-LOG.md` を参照。

| linter | 状態 | 備考 |
|--------|------|------|
| `guff-staticcheck` | ✅ **167 analyzers**（simple S* 37 + SA* 100 + stylecheck ST* **18** + quickfix QF* **12**） | — |
| `guff-govet` | ✅ **29/29** passes（printf は引数個数・型照合まで、`go vet` 一致） | — |
| `guff-errcheck` | ✅（excludes / blank / assert） | `unchecked_call` FW 無しで実装 |
| `guff-ineffassign` | ✅（gordonklaus CFG + generated 除外） | — |
| `guff-unused` | ✅（単一パッケージ; 型・定数・メソッド・const グループ） | whole-program 版は未 |
| `guff-gostaticanalysis` | ✅ **4**（forcetypeassert / nilnil / makezero / mirror） | nilerr / nilnesserr は SSA 依存で DEFERRED（→ R17） |
| `guff-error` | ✅ **7**（errname / err113 / durationcheck / errorlint / wrapcheck / errchkjson / rowserrcheck） | 各 settings 配線済み。rowserrcheck は AST 近似（SSA 完全パリティは DEFERRED） |
| `guff-context` | ✅ **4**（noctx / fatcontext / bodyclose / sqlclosecheck） | bodyclose / sqlclosecheck は AST 近似（SSA 完全パリティは DEFERRED）。contextcheck は SSA 依存で DEFERRED（→ R17） |
| `guff-style` | ✅ **75**（copyloopvar / usetesting / usestdlibvars / perfsprint / goconst / gosec / gocritic / modernize / exptostd / testifylint / sloglint / loggercheck / exhaustive / exhaustruct / musttag / revive 系複雑度（funlen / gocyclo / gocognit / cyclop / nestif / lll / maintidx）ほか多数。全一覧はレジストリ `crates/guff-lint/src/registry.rs` 参照） | 主要 `linters.settings` キーを各 analyzer に配線済み。DEFERRED: 各 SuggestedFix 完全パリティ、コメントディレクティブ、wsl(v4) 完全パリティ、perfsprint fiximports 等 |
| `guff-comment` | ✅ **4**（godot / godox / dupword / godoclint） | settings 配線済み。SuggestedFix・godoclint strict/extra rules 等は DEFERRED |
| `guff-import` | ✅ **4**（depguard / gomoddirectives / gomodguard / importas） | settings 配線済み。allowed modules/domains・version constraints・path placeholders・use-site SuggestedFix 等は DEFERRED |
| `guff-misspell` | ✅ **1**（misspell） | `misspell` settings 配線済み（locale / ignore-words / extra-words / mode） |
| `guff-dupl` | ✅ **1**（dupl） | `dupl.threshold` 配線済み |
| `guff-revive` | ✅ **1**（revive；golint-default 23 + extended 77 = 計 **100 rules**） | `revive` settings 配線済み（rules・arguments・severity・confidence 等）。prometheus 互換の主要 rule 引数を実効化済み |

### 3.4 CLI / 設定 / 出力 / 実行（`guff-lint`, `guff-runner`）
現状は「薄いドライバ」。golangci-lint 互換にはほど遠い。**ここが §8 ロードマップの主戦場。**

| 項目 | 現状 | golangci-lint との差（ギャップ） |
|------|------|------------------------------------|
| サブコマンド | `run`, `fmt`, `migrate`, `version`, `linters`, `cache`（clean/status） | `help` 無し。`fmt` は gofmt / gofumpt / goimports / gci / golines / swaggo（`exclusions.generated` lax/strict/disable）。`run` でも `formatters.enable` があれば整形診断を出す |
| run フラグ | `-c`, `--no-config`, `--preset`, `--enable`, `--disable`, `--sequential`, `--issues-exit-code`, `--build-tags`, `--timeout`, `-j/--concurrency`, `--out-format`（`format` / `format:path`）, `--no-cache`, `--fix` | — |
| 設定ファイル | `.golangci.{yml,yaml}` / `.guff.{yml,yaml}` を上位まで探索。v1/v2 の linter 選択 + `issues` / `run` / `severity` / `output` をパースし、v2 `linters.exclusions`・`exclude-rules`・max-* ・severity を後処理適用。`run.build-tags` / `tests` を load へ、`run.timeout`（既定 `1m`）・`run.concurrency` / `-j` を実行に配線。`linters.settings` を各 analyzer に配線（キー詳細は §3.3・R13）。`output.formats` / `format` → `--out-format`。R22 の config corpus smoke で実 OSS の v2 設定 **14** 件をパース検証 | `issues.new` / `new-from-rev`（diff 除外）・exclusions `warn-unused` 実効化・`generated` モードは未 |
| プリセット | `standard` / `fast` / `all` / `none`。ただし `standard`==`all`（standard 5 系統）。追加 linter は `--enable <name>` で個別有効化（利用可能名は `guff linters` / §3.3） | 100+ linter を跨ぐ本来の `all` / `fast` / カテゴリプリセットに未対応 |
| 出力 | `Formatter` 抽象 + text（`line-number` 別名）/ colored-line-number / json / checkstyle / sarif / tab / colored-tab / github-actions。`format:path` / config `path` でファイル書き出し | — |
| nolint | ✅ `//nolint` / `//nolint:linter`（同一行・直前行の AST 展開）。`nolintlint` は `--enable nolintlint` | 書式/説明必須（NeedsMachineOnly / NeedsExplanation）は未 |
| キャッシュ | ✅ パッケージ単位の issues 永続キャッシュ（`$GUFF_CACHE` / `$GOLANGCI_LINT_CACHE` / `{UserCacheDir}/guff`）。未変更 pkg は再解析スキップ。`guff cache clean`/`status`、`--no-cache` | facts キャッシュは未（→ R24） |
| 並列 | ✅ action DAG を rayon で並列実行。`-j` / `run.concurrency` でワーカー数。型チェックも並列（R10.1） | — |
| ベンチ | ✅ `benchmarks/` ハーネス（cold/warm・`fixture` / `local`・`results/RESULTS.md`）。cold/warm とも golangci-lint より高速 | 実 OSS は一部 expr/lvalue DEFERRED で FAIL しがち（→ R17 DEFERRED） |
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
| `guff-fmt` | ✅ gofmt / gofumpt / goimports / gci / golines / swaggo（`guff fmt`） | — |

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

### Milestone A — ドロップイン CLI / 設定互換 ✅ 完了 (2026-07-14)

> ゴール: 既存プロジェクトが `guff run ./...` に置き換えても「設定が効く」。

- **R1** ✅ 診断を stdout に出力し、`--issues-exit-code`（既定 1）で終了コードを制御。
- **R2** ✅ `.golangci.yml` の完全パース（issues / run / severity / output）＋後処理フィルタ（exclude-rules / max-* / severity / v2 `linters.exclusions`）。**残**: `issues.new`/`new-from-rev`、exclusions `warn-unused` / `generated`。
- **R3** ✅ `//nolint` / `//nolint:linter`（同一行・直前行の AST 展開）＋ `nolintlint`。**残**: 書式/説明必須（NeedsMachineOnly / NeedsExplanation）。
- **R4** ✅ `linters.settings` を各 analyzer に配線（`SettingsBag` を Pass / Runner へ）。**残**: errcheck `verbose`。
- **R5** ✅ 補助サブコマンド（`version` / `linters`）＋ run フラグ（`--timeout` / `-j` / `--build-tags`）。

---

### Milestone B — 出力フォーマット互換 ✅ 完了 (2026-07-14)

> ゴール: CI が期待するフォーマットで出せる。

- **R6** ✅ `Formatter` 抽象 + text / colored-line-number、`format:path` 書き出し。
- **R7** ✅ JSON 出力（golangci-lint スキーマ準拠）。**残**: `Report` への warnings 埋め込み、`SuggestedFixes` の JSON 化。
- **R8** ✅ checkstyle / sarif / tab / colored-tab / github-actions。

---

### Milestone C — 高速（性能）✅ 完了 (2026-07-14)

> ゴール: 「fast」を数字で主張できる。

- **R9** ✅ action DAG を rayon ウェーブフロントで並列実行（`Ident::obj` を `Mutex` 化し `Package: Sync`）。結果は逐次と決定的一致。
- **R10** ✅ パッケージ単位の永続 issues キャッシュ（SHA-256 content hash + 決定的 salt）。`GUFF_CACHE` > `GOLANGCI_LINT_CACHE` > OS cache dir。`guff cache clean/status` / `--no-cache`。**残**: facts 永続化 → R24。
- **R11** ✅ ベンチハーネス `benchmarks/`（cold/warm・`run.sh` / `smoke.sh` / `results/RESULTS.md`）。実 OSS は一部 SSA DEFERRED で FAIL しがち → R17 DEFERRED。
- **R10.1** ✅ 性能パス（初回計測で warm が golangci の ~5–6x だった問題を解消）: fat LTO + `codegen-units=1`、`typecheck_packages` の rayon 並列化、キャッシュ salt / dep-hash の決定化、遅延型チェック（ミスした root だけ parse + 型チェック）。結果 guff が cold/warm とも golangci-lint より高速（warm `local` 0.54x / `fixture` 0.77x）。原因と対策の詳細はメモリ `guff-perf-cache-architecture` と git 履歴。発展余地 → R24。

---

### Milestone D — 自動修正 ✅ 完了 (2026-07-14)

- **R12** ✅ `--fix`（SuggestedFix / TextEdit をオフセット降順・重なり排除で適用、修正済み診断は出力から除外）。**残**: キャッシュヒット pkg の fix 復元、複数 fix 候補の選択。

---

### Milestone E — linter の網羅（golangci-lint のラインナップに追随）🟡 進行中

> golangci-lint は 100+ linter を束ねる。全部は不要でも主要どころを揃えないと「互換」の説得力が弱い。
> **実装済み linter の一覧・件数・設定キーは §3.3 の表が正典**（重複を避け、ここには目的と残作業だけ書く）。
> 各 linter は §5 の手順で 1 個ずつ追加し、設計判断・DEFERRED は各 analyzer のコード内 `// DEFERRED:` と `SESSION-LOG.md` に残す。

#### R13. go/analysis 系 linter（`guff-gostaticanalysis` ほか）🟡 部分完了
- **目的**: gostaticanalysis / error / context 系ほか go/analysis ベースの linter を揃える。実装済みは §3.3。
- **完了条件**: 各 linter に bad/ok testdata、golangci 相当の指摘、レジストリ登録。
- **残（SSA / ctrlflow 依存 → R17 / PL05）**: nilerr / nilnesserr / contextcheck / wastedassign / spancheck / zerologlint。
- **残（パリティ）**: errorlint の errorf 既定オフ / allowed マップ、gosec の未実装ルール（G113 / G115–G118 / G201–G202 / G304–G305 / G307 / G601 等）・severity/confidence、AST 近似 linter（bodyclose / sqlclosecheck / rowserrcheck 等）の SSA 完全パリティ、各 linter の SuggestedFix / コメントディレクティブ（`//exhaustruct:ignore` など）。

#### R14. スタンドアロン linter 🟡 部分完了
- **目的**: `guff-revive`（独自ルールエンジン）/ `guff-misspell` / `guff-dupl` / `guff-style` バンドル / `guff-comment` / `guff-import`。実装済みは §3.3。
- **残**: revive の未実装 rule、`gocyclo:ignore` / `gocognit:ignore` 等のコメント除外、tagalign StrictStyle、wsl(v4) 完全パリティ、各種 SuggestedFix。

#### R15. formatter（`guff-fmt` + `guff fmt` サブコマンド）✅ 完了 (2026-07-17)
- gofmt / gofumpt / goimports / gci / golines / **swaggo**（`swag fmt` 経由）をシステムバイナリで実装。enable 順チェーン、`formatters.enable` / `settings` / `exclusions.paths` / `exclusions.generated`（`lax` 既定 / `strict` / `disable`）配線。フラグ: `-E/--enable` / `-d/--diff`（TTY で ANSI 色付け・`--no-color`）/ `--stdin` / `-c` / `--no-config`。enable 空 → gofmt フォールバック。
- **gofumpt `-lang`**: `run.go`（または `settings.gofumpt.lang`）から注入。
- **gci `no-inline-comments` / `no-prefix-comments`**: gci `print` CLI が未対応のため import ブロックの後処理で実現。
- **`guff run` 時の formatter 診断**: `formatters.enable` があると各フォーマッタを個別に走査し未整形ファイルを `File is not properly formatted (<formatter>)` として issue 化。`--fix` でチェーン整形して書き換え（issue は出さない）。パターン→パス変換は `./...`→`.` 等。
- **DEFERRED**: 診断ごとの `SuggestedFix`（JSON への TextEdit 埋め込み）、`guff run` の formatter パターンが実パスでない場合（モジュールパス）のマッピング。

#### R16. staticcheck の ST*（stylecheck）/ QF*（quickfix）✅ 完了 (2026-07-17)
- 実装済み: ST* **18**（ST1000/1001/1003/**1005**/1006/**1008**/1011/1012/1013/1015/**1016**/1017/1018/1019/1020/1021/1022/1023）+ QF* **12**（QF1001–QF1012）。
- ST1005/ST1008/ST1016 は upstream が buildir 依存だが、AST/types 近似で実装（ST1006 と同様）。
- QF1008 割り込みチェーン・QF1012 `types.Implements`（io 未ロード時は arity フォールバック）・ST1023 CheckExpr 相当の孤立型再構築まで消化。
- テスト: 各 `stNNNN` / `qfNNNN` fixtures + `checks_test` + `v2_staticcheck_stylecheck_settings.yml`。

---

### Milestone F — 土台の穴（breadth/speed を塞ぐ前提）

#### R17. SSA の残作業（`RangeStmt` ＋ メソッド機構 E25+ ＋ 文カバレッジ）✅ 完了 (2026-07-17)
- **なぜ**: IR ベース linter（SA1015 等）と実 OSS 上の buildir を default で駆動するため。
- **実装**: `RangeStmt`（slice/array/map/string/chan/int/func）、`methods.rs` / `$thunk`/`$bound` /
  `InstantiateGenerics` / メソッド呼び出し emit、および statement builder の穴埋め
  （`SwitchStmt` / `TypeSwitchStmt` / `SelectStmt` / `SendStmt` / `IncDecStmt` / `EmptyStmt` +
  compound assign + fallthrough）。`Select`/`Send` 命令を具現化。
- **完了条件**: `for k, v := range x` とメソッド呼び出しを含む IR がビルドできる。SA1015 は
  buildir require で green。golden / `switch_select_test` 逆アセンブル比較を維持。
- **DEFERRED**: range over non-array pointer、一部 expr/lvalue 端、`MakeInterface` 完全モデル、
  method instantiation wrapper（receiver / 0-result）。

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

#### R22. `.golangci.yml` コーパスのパース検証 🟡 部分完了 (2026-07-17)
- 実在の `.golangci.yml`（有名 OSS のもの）を集めて、**パースエラー 0** を保証するテスト。
- **進捗メモ (2026-07-16)**: `crates/guff-lint/tests/testdata/config_corpus/` を追加し、
  Prometheus / Grafana 由来の golangci-lint v2 設定 snapshot を `parse_config_str` →
  `linter_selection` → `effective_issues` まで通す smoke test を追加。未知キー
  （`run.allow-parallel-runners` / `formatters.exclusions` 等）は serde の既存挙動で許容。
- **進捗メモ (2026-07-17)**: コーパスを **14** 件に拡張（Gitea / MinIO / NATS Server /
  Tailscale / Vitess / HashiCorp Consul / Helm / Moby / golangci-lint / Basecamp CLI /
  Kargo / Telegraf 追加）。各 snapshot は upstream
  `.golangci.yml` をそのまま取り込み、先頭に出典コメントを付与。`parse_golangci_config_corpus`
  が全件を走査。v1 設定（Traefik 等）は v2 専用コーパスから除外。
  **DEFERRED**: 数十件へのさらなる拡張、CI ゲート化、出典 URL/更新手順の体系化。

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
- 残タスクの消化 → §8 の該当 R 番号の「残」/「DEFERRED」を更新（完了なら該当行を消す）。
- 新しい linter → §3.3 の表に 1 行。
- そのセッションで何をしたか → [`SESSION-LOG.md`](SESSION-LOG.md) の表の先頭に 1 行。
- 冗長になりがちな詳細（設定キー全列挙・完了履歴）は本書に書かず、コード内 `// DEFERRED:` と `SESSION-LOG.md` / git に委ねる。

---

## 10. セッション記録

作業ログは [`SESSION-LOG.md`](SESSION-LOG.md) に分離した（一次情報は `git log`）。
新しいセッションの記録はそちらの表の先頭に 1 行追記する。
