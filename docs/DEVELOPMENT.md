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

> 最終更新: 2026-07-16。ワークスペース全体 **1900+ tests green**（`guff-revive` extended rules 計 **100 rules** + `linters.settings.revive` YAML 配線）。

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
| `guff-staticcheck` | ✅ **137 analyzers**（simple S* 37 + staticcheck SA* 100） | ST* / QF* は **未着手** |
| `guff-govet` | ✅ **29/29** passes（printf は引数個数・型照合まで, `go vet` 一致） | — |
| `guff-errcheck` | ✅（excludes / blank / assert） | `unchecked_call` FW 無しで実装 |
| `guff-ineffassign` | ✅（gordonklaus CFG + generated 除外） | — |
| `guff-unused` | ✅（単一パッケージ; 型・定数・メソッド・const グループ） | whole-program 版は未 |
| `guff-gostaticanalysis` | ✅ **3**（forcetypeassert / nilnil / makezero） | nilerr / nilnesserr / mirror ほかは **DEFERRED（R13 残）** |
| `guff-error` | ✅ **6**（errname / err113 / durationcheck / errorlint / wrapcheck / errchkjson） | rowserrcheck 等は **DEFERRED**（SSA） |
| `guff-context` | ✅ **2**（noctx / fatcontext） | bodyclose / contextcheck / sqlclosecheck 等は **DEFERRED**（SSA → R17） |
| `guff-style` | ✅ **23**（copyloopvar / usetesting / usestdlibvars / perfsprint / goconst / dogsled / asciicheck / goprintffuncname / funlen / gocyclo / lll / gocognit / nestif / cyclop / nakedret / nosprintfhostport / predeclared / whitespace / nlreturn / mnd / prealloc / tagalign / wsl） | settings・SuggestedFix は **DEFERRED** |
| `guff-comment` | ✅ **3**（godot / godox / dupword） | settings・SuggestedFix は **DEFERRED** |
| `guff-import` | ✅ **3**（depguard / gomoddirectives / gomodguard） | settings・gomodguard_v2 は **DEFERRED** |
| `guff-misspell` | ✅ **1**（misspell） | settings（locale / ignore-words / extra-words / mode）は **DEFERRED** |
| `guff-dupl` | ✅ **1**（dupl） | settings（`threshold` YAML 配線）は **DEFERRED** |
| `guff-revive` | ✅ **1**（revive） | golint-default **23 rules** + extended **77 rules**（計 100）；`linters.settings.revive` YAML 配線済み；per-rule severity は **DEFERRED** |

### 3.4 CLI / 設定 / 出力 / 実行（`guff-lint`, `guff-runner`）
現状は「薄いドライバ」。golangci-lint 互換にはほど遠い。**ここが §8 ロードマップの主戦場。**

| 項目 | 現状 | golangci-lint との差（ギャップ） |
|------|------|------------------------------------|
| サブコマンド | `run`, `migrate`, `version`, `linters`, `cache`（clean/status） | `help`/`fmt` 無し |
| run フラグ | `-c`, `--no-config`, `--preset`, `--enable`, `--disable`, `--sequential`, `--issues-exit-code`, `--build-tags`, `--timeout`, `-j/--concurrency`, `--out-format`, `--no-cache`, `--fix` | `format:path` 書き出し・errcheck exclude-functions 等は未 |
| 設定ファイル | `.golangci.{yml,yaml}` / `.guff.{yml,yaml}` を上位ディレクトリまで探索。v1/v2 の linter 選択 + `issues`/`run`/`severity`/`output` をパース。`issues.exclude*` / `exclude-rules` / max-* / severity を後処理で適用。`run.build-tags`・`run.tests` を load に渡す。`run.timeout` を全体タイムアウトに適用（既定 `1m`）。`run.concurrency` / `-j` で rayon ワーカー数（`1` → sequential）。`linters.settings`（errcheck check-blank / check-type-assertions、govet enable/disable、staticcheck checks）を Pass / 選択に配線。`output.formats`/`format` → `--out-format`（text / colored / json / checkstyle / sarif / tab / github-actions） | `issues.new`/`new-from-rev`（diff 除外）・`format:path` 書き出し・errcheck exclude-functions 等は未 |
| プリセット | `standard`/`fast`/`all`/`none`。ただし `standard`==`all`（5 系統）。追加系は `--enable`（forcetypeassert/nilnil/makezero/errname/err113/durationcheck/errorlint/wrapcheck/errchkjson/noctx/fatcontext/copyloopvar/usetesting/usestdlibvars/perfsprint/goconst/dogsled/asciicheck/goprintffuncname/funlen/gocyclo/lll/gocognit/nestif/cyclop/nakedret/nosprintfhostport/predeclared/whitespace/nlreturn/mnd/prealloc/tagalign/wsl/godot/godox/dupword/depguard/gomoddirectives/gomodguard/misspell） | 100+ linter を跨ぐ本来の `all`/`fast`/カテゴリプリセットに未対応 |
| 出力 | `Formatter` 抽象 + `--out-format text`（`line-number` 別名）/ `colored-line-number` / `json` / `checkstyle` / `sarif` / `tab` / `colored-tab` / `github-actions`。stdout | `format:path` へのファイル書き出しは DEFERRED |
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
  DEFERRED: `issues.new`/`new-from-rev`（git diff）。`run.timeout` は R5 で実効化。
  `run.concurrency` の真の並列は R9。
  テスト: `v2_full_issues.yml` パース、`v2_exclude_errcheck_bad.yml` + `tests/exclude_test.rs`。

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
  errcheck は Pass-time（`check-blank` / `check-type-assertions`）、govet は
  `enable`/`disable`/`disable-all`/`enable-all`、staticcheck は `checks`（`all`/`-SAxxxx`）で
  選択時フィルタ。DEFERRED: errcheck `exclude-functions` / `disable-default-exclusions`、
  staticcheck initialisms 等、他 linter の settings。
  テスト: `tests/settings_test.rs` + `testdata/config/v2_errcheck_check_blank.yml` 等。

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
  DEFERRED（当時）: JSON（→ R7）、色付き・ソース行下線（→ R8 で完了）、`format:path` 書き出し（未）。
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
  `TabFormatter` / `colored-tab`。`format:path` への実ファイル書き出しは DEFERRED。
  テスト: 各 formatter ユニット + `tests/format_test.rs` CLI。

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
  1. `guff-gostaticanalysis`: forcetypeassert / nilnil / makezero
  2. `guff-error`: errname / err113 / durationcheck / **errorlint** / **wrapcheck** / **errchkjson**
     （golangci 既定 `omit-safe`；`report-no-exported` / `check-error-free-encoding` settings は DEFERRED）
  3. `guff-context`: **noctx**（AST call-name; upstream は buildssa）/ **fatcontext**
  4. `guff-style`（新）: **copyloopvar** / **usetesting** / **usestdlibvars**（HTTP method/status 既定オン）
     / **perfsprint**（fmt.Sprint/Sprintf/Errorf 既定オン; concat-loop/fiximports は DEFERRED）
     / **goconst**（golangci 既定 min-len=3 / min-occurrences=3 / exclude call）
     / **dogsled**（golangci 既定 max-blank-identifiers=2）/ **asciicheck** / **goprintffuncname**
     / **funlen** / **gocyclo** / **lll** / **gocognit** / **nestif** / **cyclop**
     / **nakedret** / **nosprintfhostport** / **predeclared** / **whitespace** / **nlreturn** / **mnd**
     / **prealloc** / **tagalign** / **wsl**（R14）
     / **godot** / **godox** / **dupword**（`guff-comment`）
     / **depguard** / **gomoddirectives** / **gomodguard**（新 `guff-import`）
  いずれも `--enable` で有効化。テストは各クレートの `tests/checks_test.rs`。
  **DEFERRED（同タスク内の残り）**:
  - `nilerr` / `nilnesserr` — SSA（→ R17）
  - `mirror` — 大テーブル
  - `errorlint` の errorf 既定オフ / allowed マップ完全版、`wrapcheck` の package-glob / interface-regexp 設定
  - `usestdlibvars` の optional テーブル / `usetesting`・`copyloopvar`・`perfsprint`・`goconst`・`dogsled` の settings 配線
  - `perfsprint` の concat-loop / fiximports、`goconst` の match-constant / numbers / find-duplicates
  - `errchkjson` settings（`check-error-free-encoding` / `report-no-exported`）
  - `rowserrcheck` / bodyclose / contextcheck / sqlclosecheck — SSA（→ R17）
  - gosec ほか（R13 続きセッションで数個ずつ）

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
  settings・`gocyclo:ignore` / `gocognit:ignore`・PARSE_COMMENTS 前提の funlen コメント除外・
  cyclop package-average・nakedret SuggestedFix / skip-test-files・predeclared ignore/qualified・
  whitespace SuggestedFix / multi-*・nlreturn SuggestedFix・mnd settings・prealloc settings・
  tagalign StrictStyle / SuggestedFix・wsl 完全パリティ / `wsl_v5`・settings は DEFERRED。）
- `guff-comment`: **godot**（declarations + period 既定）/ **godox**（TODO/BUG/FIXME）/ **dupword**
  （comments + string literals）。settings・SuggestedFix・godot scope/capital・dupword keyword
  フィルタは DEFERRED。
- `guff-import`: **depguard**（既定 `$gostd` only）/ **gomoddirectives**（replace 禁止）/ **gomodguard**（blocked・local-replace）/
  settings（rules / replace-local / allowed・blocked・version・gomodguard_v2）は DEFERRED。
- `guff-misspell`: **misspell**（golangci/misspell `DictMain` + US locale；既定 mode=plain text）。
  settings（locale UK / ignore-words / extra-words / mode=restricted）は DEFERRED。
- `guff-dupl`: **dupl**（golangci/dupl suffix-tree クローン検出；既定 threshold=150）。
  `linters.settings.dupl.threshold` YAML 配線は DEFERRED。
- `guff-revive`: **revive**（golint-default **23 rules** + extended **77 rules**: … prior 69 … / comments-density / datarace / enforce-map-style / enforce-slice-style / enforce-switch-style / enforce-repeated-arg-type-style / package-directory-mismatch / forbidden-call-in-wg-go）。
  `linters.settings.revive` YAML 配線済み（rules リスト・arguments）。per-rule severity / ignore-generated-header は **DEFERRED**。
- R14 残: revive per-rule severity 配線（DEFERRED）。

#### R15. formatter（`guff-fmt` + `guff fmt` サブコマンド, Milestone L5）
- gofmt, gofumpt, goimports, gci, golines。**別パイプライン**（解析ではなく整形）。
- golangci-lint v2 は `formatters` セクションを持つので、config 互換のためにも必要。

#### R16. staticcheck の ST*（stylecheck）/ QF*（quickfix）
- 現在 `guff-staticcheck` は S* + SA* のみ。ST1xxx / QF1xxx を追加すると staticcheck 完全互換に近づく。

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
| 2026-07-14 | **R8 完了**: colored-line-number / github-actions / checkstyle / sarif / tab / colored-tab。`format:path` 書き出しは DEFERRED。`tests/format_test.rs` |
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
