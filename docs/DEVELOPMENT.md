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
| **CLI** | `guff-lint` (`bin: guff`) | 設定・linter 選択・診断表示・`migrate`・`custom` |
| **Plugins** | `guff-plugin`, `guff-plugin-example` | Module plugin API + サンプル（カスタムバイナリ用） |
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

> 最終更新: 2026-07-18。ワークスペース全体 **2663 tests green**。実装済み linter の一覧・件数・設定配線は §3.3 を、作業履歴は `SESSION-LOG.md` を参照。golangci-lint v2 との対応表は [`COMPATIBILITY.md`](COMPATIBILITY.md)（R23）。R24（facts / export seed / golist キャッシュ）完了。R25（Prometheus スケール修正）— R25.1（アリーナフットプリント、layered CoW で export seed を全 pkg 共有、**83s→11.9s / peak 56GB→5.8GB**）・R25.2（位置破損 = 決定論的 u32 オーバーフロー。fake export-data ファイルを実サイズ化して共有 fset の Pos 空間を u32 内に収め、依存ファイルへの誤マップ **226→0**・診断 234→2671）完了。残: §8 R25 の DEFERRED（隔離済み非致命 panic・govet unreachable の順序依存・R25.3 go list cold）。

### 3.1 型チェッカ（`guff-types`）
- 構造層（全 Type/Object 種別・述語・universe・ジェネリクス subst/instantiate/infer/unify・
  operand・conversions・assignments・typestring）**完了**。
- Checker エンジン本体もほぼ完走（`check_files` 到達、宣言・式・文・呼び出し・組込・ジェネリクス
  end-to-end・importer・unused/dot/blank import・mono・sizes・version）。
- **残**: D04（`any`-hijack; gotypesalias legacy・意図的非移植）、D07 の
  多行 cycle 診断、D13 の `interfacePtrError` 詳細ヒント、D16 の
  gcexportdata バイナリローダ（→ 必要になったら個別タスク）。initorder /
  recording / util / D01/D02/D03/D10 は **R19 で完了**。

### 3.2 解析フレームワーク（PRE-LINTER Phase 0–7）
- Phase 0（types 仕上げ）〜Phase 7（E2E smoke）**完了**。
- **残**: Phase 8（gofmt / go/doc 等の付帯ユーティリティ）, PL05（ctrlflow）
  （→ §8 各タスク）。
  PL11（真の並列実行）は **R9 で完了**。typeindex は **R18 で完了**。
  PL02（go 無し driver）/ PL07（GOCACHE 管理）は **R20 で完了**。
  差分テストハーネスは **R21 で完了**。

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
| `guff-gostaticanalysis` | ✅ **6**（forcetypeassert / nilnil / makezero / mirror / nilnesserr / nilerr） | |
| `guff-error` | ✅ **7**（errname / err113 / durationcheck / errorlint / wrapcheck / errchkjson / rowserrcheck） | 各 settings 配線済み。rowserrcheck は AST 近似（SSA 完全パリティは DEFERRED） |
| `guff-context` | ✅ **5**（noctx / fatcontext / bodyclose / sqlclosecheck / contextcheck） | bodyclose / sqlclosecheck は AST 近似。contextcheck は SSA + パッケージ内 facts（cross-pkg DEFERRED） |
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
| サブコマンド | `run`, `fmt`, `migrate`, `version`, `linters`, `cache`（clean/status）, **`custom`**（module plugin バイナリ生成） | `help` 無し。`fmt` は gofmt / gofumpt / goimports / gci / golines / swaggo（`exclusions.generated` lax/strict/disable）。`run` でも `formatters.enable` があれば整形診断を出す |
| run フラグ | `-c`, `--no-config`, `--preset`, `--enable`, `--disable`, `--sequential`, `--issues-exit-code`, `--build-tags`, `--timeout`, `-j/--concurrency`, `--out-format`（`format` / `format:path`）, `--no-cache`, `--fix` | — |
| 設定ファイル | `.golangci.{yml,yaml}` / `.guff.{yml,yaml}` を上位まで探索。v1/v2 の linter 選択 + `issues` / `run` / `severity` / `output` をパースし、v2 `linters.exclusions`・`exclude-rules`・max-* ・severity・`new*` / `whole-files`・`generated` を後処理適用。`run.build-tags` / `tests` を load へ、`run.timeout`（既定 `1m`）・`run.concurrency` / `-j` を実行に配線。`linters.settings` を各 analyzer に配線（キー詳細は §3.3・R13）。`output.formats` / `format` → `--out-format`。R22 の config corpus smoke で実 OSS の v2 設定 **52** 件をパース検証（CI ゲート） | exclusions `warn-unused` 実効化は未 |
| プリセット | `standard` / `fast` / `all` / `none`。ただし `standard`==`all`（standard 5 系統）。追加 linter は `--enable <name>` で個別有効化（利用可能名は `guff linters` / §3.3） | 100+ linter を跨ぐ本来の `all` / `fast` / カテゴリプリセットに未対応 |
| 出力 | `Formatter` 抽象 + text（`line-number` 別名）/ colored-line-number / json / checkstyle / sarif / tab / colored-tab / github-actions。`format:path` / config `path` でファイル書き出し | — |
| nolint | ✅ `//nolint` / `//nolint:linter`（同一行・直前行の AST 展開）。`nolintlint` は `--enable nolintlint` | 書式/説明必須（NeedsMachineOnly / NeedsExplanation）は未 |
| キャッシュ | ✅ パッケージ単位の issues 永続キャッシュ（`$GUFF_CACHE` / `$GOLANGCI_LINT_CACHE` / `{UserCacheDir}/guff`）。未変更 pkg は再解析スキップ。**facts 永続化**（analyzer×package、objectpath キー、`facts/`；R24.1）。**`go list` メタデータキャッシュ**（`golist/`；R24.4）。**export seed clone**（並列型チェックで共通 deps を再デコードしない；R24.3）。`guff cache clean`/`status`（GOCACHE も表示）、`--no-cache`。`go list` に `GOCACHE` を明示注入；診断の GOCACHE 配下パスは除外（cgo） | 真のファイル粒度インクリメンタル型チェックは未（Checker はパッケージ全体；R24.2 DEFERRED）。ignored ファイルは self_hash から除外済み（R24.2 I1） |
| 並列 | ✅ action DAG を rayon で並列実行。`-j` / `run.concurrency` でワーカー数。型チェックも並列（R10.1） | — |
| ベンチ | ✅ `benchmarks/` ハーネス（cold/warm・`fixture` / `local`・`results/RESULTS.md`）。cold/warm とも golangci-lint より高速 | 実 OSS は一部 expr/lvalue DEFERRED で FAIL しがち（→ R17 DEFERRED） |
| 互換差分 | ✅ `compat/` ハーネス（guff vs golangci JSON → `file:line:linter:message`、P/R、allowlist、`.github/workflows/compat.yml` ゲート） | OSS コーパス拡張・local パリティ改善は継続 |
| Prometheus 回帰 | ✅ `regress/` ローカルゲート（prometheus 本体 `.golangci.yml`・peak RSS / wall / finding-set を悪化のみ FAIL）。プロファイル: `tsdb`（`./tsdb/...` / `baseline.json` / RSS kill 12GiB）と `full`（`./...` / `baseline.full.json` / 18GiB）。暖機 GOCACHE・auto concurrency | CI 未接続。絶対一致は不要。hybrid 既定の full peak は ~10.7 GiB（export 経路の ~5.8 GB より高い） |
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
| `guff-lint` | — | CLI + レジストリ + nolint + `guff custom` |
| `guff-plugin` | — | Module plugin API（golangci `plugin-module-register` 相当） |
| `guff-plugin-example` | — | サンプル module plugin（デフォルト `guff` にはリンクしない） |
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

### 5.2 Module Plugin（golangci 互換）

社内・非公開 linter を **カスタムバイナリに静的リンク**する方式。golangci-lint の
[Module Plugin System](https://golangci-lint.run/docs/plugins/module-plugins/) と同じ運用手順。
（Go の既存 module / `.so` プラグインはそのままでは動かない。プラグイン本体は Rust。）

| 手順 | golangci-lint | guff |
|------|---------------|------|
| ビルド設定 | `.custom-gcl.yml` | 同じ（`.custom-guff.yml` も可） |
| ビルド | `golangci-lint custom` | `guff custom` |
| 実行時設定 | `.golangci.yml` → `linters.settings.custom` | 同じ |
| 有効化 | `linters.enable: [name]` | 同じ |
| 実行 | `./custom-gcl run` | `./custom-guff run` |

**1. プラグイン crate**（参考: `crates/guff-plugin-example`）:

```rust
use guff_plugin::{decode_settings, LinterPlugin, PluginError, Analyzer, /* ... */};

guff_plugin::register!("example", new_example);

pub const FORCE_LINK: () = (); // `guff custom` がリンクを強制するのに必要

fn new_example(settings: &serde_yaml::Value) -> Result<Box<dyn LinterPlugin>, PluginError> {
    let s = decode_settings::<MySettings>(settings)?;
    Ok(Box::new(PluginExample { settings: s }))
}

impl LinterPlugin for PluginExample {
    fn build_analyzers(&self) -> Result<Vec<&'static Analyzer>, PluginError> { /* ... */ }
    fn description(&self) -> &'static str { "find TODOs without an author" }
}
```

**2. `.custom-gcl.yml`**:

```yaml
version: "0.1.0"
name: custom-guff
destination: .
plugins:
  - module: guff-plugin-example
    path: ./crates/guff-plugin-example
```

**3. `guff custom`**（要: `GUFF_SRC` またはソースからビルドした `guff`）→ `./custom-guff`

**4. `.golangci.yml`**:

```yaml
version: "2"
linters:
  default: none
  enable: [example]
  settings:
    custom:
      example:
        type: module
        description: find TODOs without an author
        settings:
          one: yes
```

`type` は `"module"` のみサポート。nested `settings` はプラグインの `New` と
`SettingsBag`（`pass.settings::<serde_yaml::Value>("example")`）の両方に渡る。

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
- **R2** ✅ `.golangci.yml` の完全パース（issues / run / severity / output）＋後処理フィルタ（exclude-rules / max-* / severity / v2 `linters.exclusions` / `new*` / `whole-files` / `generated`）。**残**: exclusions `warn-unused`。
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
- **R10** ✅ パッケージ単位の永続 issues キャッシュ（SHA-256 content hash + 決定的 salt）。`GUFF_CACHE` > `GOLANGCI_LINT_CACHE` > OS cache dir。`guff cache clean/status` / `--no-cache`。facts 永続化は **R24.1 で完了**。
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
- **残（パリティ）**: contextcheck cross-pkg facts / HTTP handler 端、spancheck ctrlflow 完全パリティ、errorlint の errorf 既定オフ / allowed マップ、gosec の未実装ルール、AST 近似 linter（bodyclose / sqlclosecheck / rowserrcheck 等）の SSA 完全パリティ、各 linter の SuggestedFix / コメントディレクティブ。
- ~~**残（SSA / ctrlflow 依存）**: nilerr / contextcheck / wastedassign / spancheck / zerologlint~~ ✅ 2026-07-24 実装（spancheck/contextcheck は 🟡）。

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

#### R18. `typeindex` の移植 ✅ 完了 (2026-07-17)
- **なぜ**: 呼び出しサイトの高速索引。pattern 系全 linter と errcheck の性能最適化。
- **実装**: `guff-analysis` に `passes/typeindex`（`Index` + analyzer）。`Uses`/`Used`/`Def`/
  `Package`/`Object`/`Selection`/`Calls`（`for_each_call`）。`guff-pattern` に
  `IndexSymbol` / `root_call_symbols`。`matches()` の typeindex 高速パス。QF1004 / S1024 /
  S1028 / SA1001 を配線。runner に `Index` clone パス。
- **DEFERRED**: 汎用 field/method の `Origin()` 二重記録（`Func`/`Var` に Origin API が無い）。

#### R19. 型チェッカの残り（initorder / recording / util）✅ 完了 (2026-07-17)
- **実装**: `initorder` / `recording`（Defs/Uses/Types/Selections/Instances/Scopes/Implicits）は
  既に揃っていたことを確認。今回の回収:
  1. **D01** — `typeterm`/`termlist` が `identical` を使用（`identical_stub` 撤去）。Union
     同一性も `compute_union_type_set` + `termlist::equal`。
  2. **D02** — `intersect_term_lists` が `comparable_type` で非 comparable 項をフィルタ。
  3. **D03** — `lookup_ignoring_case` + 修飾セレクタの `(but have X)` ヒント。
  4. **D10** — `common_under` の `TypeError` 引数が `type_string`。
  5. **util.rs**（Step 39）— `has_dots` / `is_ddd_array` / `cmp_pos` / `start_pos` /
     `end_pos`；`[...]T` を複合リテラル外で拒否。
- **DEFERRED**: D04（`any`-hijack legacy）、D07 多行 cycle 診断、D13
  `interfacePtrError` 詳細、D16 gcexportdata バイナリローダ。
- **テスト**: `cargo test -p guff-types` green（777+）。

#### R20. オフライン/`go` 無し driver（PL02）と GOCACHE 管理（PL07）✅ 完了 (2026-07-17)
- **実装**:
  1. **PL02** — `OfflineDriver`（`guff-build` で `go.mod` + ファイルシステム解決）。
     `AutoDriver` が `go` 不在時にフォールバック。`.` / `./...` / モジュールパス対応。
     直接 import は GOROOT/モジュール内までロード；`NeedDeps` 時のみ再帰。
  2. **PL07** — `default_go_cache_dir` / `ensure_go_cache_env` / `is_under_go_cache`。
     `go list` に `GOCACHE` を明示注入。IssueFilter で GOCACHE 配下・`_cgo_gotypes.go`
     を除外（golangci Cgo processor 相当）。`guff cache status` に GOCACHE 表示。
- **DEFERRED**: 外部 module `require` 解決（module cache / `go mod download`）、
  offline 時の export data 生成。
- **テスト**: offline 2 + go_source imports 3 + runner go_cache 2 + lint filter/CLI。

#### R24. 性能フォローアップ（R10.1 の発展余地）✅ 完了 (2026-07-17)
- **なぜ**: R10.1 で cold/warm とも golangci-lint 超えを達成したが、さらに詰められる余地がある。
  いずれも機能ブロッカーではなく、大規模リポジトリでの伸びしろ。
- **完了**:
  1. **R24.1 facts 永続化** — `objectpath`（PO）+ `EncodedFact` + `IssueCache` `facts/` + クロスアリーナ remap。
  2. **R24.3 export seed clone** — `ExportSeed` / `Checker::from_seed`。`typecheck_roots` / `typecheck_packages` が依存 `.a` を一度だけデコードし、並列ワーカーはアリーナを clone。
  3. **R24.4 `go list` メタデータキャッシュ** — `$GUFF_CACHE/golist/`。キー = go.mod/sum + args + 関連 env；Export パス欠落時は miss。`--no-cache` / `GUFF_CACHE=off` で無効。
  4. **R24.2 I1** — `self_hash` から `ignored_files` を除外（build-tag 変更は salt で無効化）。
- **DEFERRED（R24.2 本体）**: 真のファイル粒度インクリメンタル型チェック。Checker はパッケージ全体；cross-file defs/methods/inits のため部分 `check_files` は不正。`typecheck_package_with_seed` に `// DEFERRED(R24.2)` コメントあり。
- **テスト**: objectpath / fact_codec / facts_put_get + `export_seed_roundtrip` + `golist_cache_key_*` / `export_paths_exist_*` + packages/runner 回帰。

#### R25. Prometheus スケール修正と残り性能（大規模リポジトリ）🟢 R25.1/R25.2 完了・R25.3 残 (2026-07-18)
- **なぜ**: フル `guff run ./...`（Prometheus 113 pkg）が >15分で未完 / OOM。実態は OOM だけでなく ①linter panic 非隔離で run 全滅 ②解析フェーズの二次コスト。128GB マシンでは OOM せず peak ~49GB。
- **完了**:
  1. **linter panic 隔離** — `guff-runner::action::execute` の `(analyzer.run)()` を `catch_unwind` で包み panic を action エラー化。1 linter の panic が rayon ワーカーごと巻き戻り「lint worker exited without a result」で run 全滅していたのを解消（`Cargo.toml` の `panic = "unwind"` 注記が謳う挙動を実装で復元）。
  2. **copylocks アリーナクローン除去** — `guff-govet::lockpath` に `LockChecker`（pkg あたり 1 回だけ `TypeArena` を scratch clone + lock-path をメモ化 + `*T` を作らず addressable method set で判定）。旧 `is_lock_by_value` は再帰走査の**各ノード**でアリーナ全体を clone していた。tsdb pkg **42s→0.02s**。
  3. **buildir O(pkg×objects) 除去** — `guff-ssa::create::imported_type_packages` はオブジェクトアリーナ全走査で「引数によらず全 pkg」を返す。これを pkg ごとに呼ぶ `create_import_packages`（旧 `create_imports_rec`）と `populate_imported_package_members` が共有型アリーナ巨大時に二次（~184s/buildir）。オブジェクトを 1 パスで pkg 別グループ化して線形に。tsdb/... buildir **550s→0.84s**。
  - 結果: フル run >15分（未完）→ **86.6s**、解析フェーズ 51.5s、peak RSS ~49GB、panic 35 件捕捉・abort 0。ワークスペース **2663 tests green**。
- **完了（R25.1、2026-07-18 続き）— アリーナフットプリント削減が本丸だった**:
  4. **layered CoW アリーナ（本命の修正）** — `guff-types::arena` の 4 アリーナ（Type/Object/Scope/Package）を `Layered<T> { base: Arc<Vec<T>>, overlay: Vec<T> }` 化。`alloc` は overlay に積むだけ（base 不変）、`get_mut(base_idx)` のみ `Arc::make_mut` で CoW。`Checker::from_seed` は `shared_clone`（Arc bump）で seed を共有し、`capture_export_seed` で `freeze`（overlay→base）。**これで R24.3 export seed（全 deps）が全 pkg で 1 個共有**になり、113 pkg 分の deep clone / buildir snapshot / analyzer clone が消滅。安全性は実測プローブで確認：SSA build は base 型を **0 回**変更、typecheck-after-seed も 113 pkg 中 base 変更は 4 pkg のみ（CoW で個別化）。共有は既に `&Arc<ExportSeed>` を par_iter で跨いでおり `Sync` 要件は充足済み。
  5. **buildir import-closure 限定** — `create_import_packages` / `populate_imported_package_members` を対象 pkg の推移的 import closure に限定（旧: export seed の全 pkg superset を SSA 化）。`Package.imports()` を source-checked pkg にも設定（`check.rs`、Go 準拠）して closure walk を可能に。1 ファイル pkg の buildir が 7.46s→即時に。
  6. **copylocks 読み取り専用プローブ** — `lockpath::find_method_ro`（`&TypeArena` のみ）で一般的な lock 形状（named 型・埋め込み struct）はクローン無しで method-set 判定。埋め込み interface / generic instance のみ従来の clone にフォールバック。
  7. **action result の即時解放** — `guff-runner::exec_all` で各 non-root action の result を最後の依存者完了時点で drop（旧: run 完了まで全 result 保持）。逆依存カウントで管理。
  - **結果（フル `guff run --no-cache ./...`、Prometheus 113 pkg、apples-to-apples）**: **83s→11.9s（7×）**、peak RSS **56GB→5.8GB（9.6×）**。typecheck 8.3s→1.8s、analyze 52s→4.2s、buildir(summed) 251s→0.9s、copylocks 179s→0.05s。ワークスペース **2663 tests green**、`-j 1` 逐次実行で HEAD と**バイト一致**（tsdb subtree 771 診断）。
- **完了（R25.2、2026-07-18 続き）— 位置破損は「並列競合」ではなく決定論的 u32 オーバーフローだった**:
  8. **fake export-data ファイルの実サイズ化（本命の修正）** — フル `./...`（Prometheus）で診断の **226/234 件が go/pkg/mod・GOROOT の依存ファイルへ誤マップ**し列番号が巨大化（例 `:60286`）していた。DEFERRED の「並列時の共有 FileSet / position offset 競合」という仮説は**誤り**。`RAYON_NUM_THREADS=1` の完全逐次でも全件破損して再現する（`-j` は action DAG のみ逐次化し typecheck の `par_iter` は常に並列なので切り分けになっていなかった）。真因は **`guff-types` が位置を `u32`（`ObjectMeta::pos` ほか）で保持する一方、`FakeFileSet`（`guff-exportdata`）が export data の依存ファイル 1 個につき `MAX_LINES=65536` Pos を予約**し、共有 `FileSet` の base が **~19,446 ファイル追加時点で `u32::MAX`(4,294,967,296) を突破**（プローブで実測）。それ以降に parse される prometheus ソース（base 55億）の位置が `u32` へ切り詰められ、低位の依存ファイル範囲に落ちていた。tsdb 単体は base が u32 内に収まり正常だったため「バイト一致」に見えていた。**修正**: `FakeFileSet` を、decode 中は `file_index*STRIDE+line`（`STRIDE=MAX_LINES+1`）の**暫定ハンドル**を返し、`finalize()` で各ファイルを**実際の最大行数ちょうど**のサイズで共有 fset に登録、`translate()` で暫定→実 offset に変換する方式に変更。暫定を付与したオブジェクトは `PkgState::prov_objs` に記録し finalize で書き戻す（`do_pkg` の再帰 import が同 arena にオブジェクトを差し込むため arena インデックス範囲では不可）。共有 fset の Pos 空間が ~5.5G→~50M に縮小し u32 に収まる。
  - **結果（フル `guff run --no-cache ./...`、Prometheus）**: 依存ファイルへの誤マップ **226→0**、診断総数 **234→2671**（破損で誤ファイルに埋もれていた本来の診断が正しく出るように）。ST1000 等が正しい `file:14:1` を指す。**並列 / `-j 1` / `RAYON_NUM_THREADS=1` の 3 モードで 2671 件一致**（残差 1 件は govet unreachable が別の有効行を選ぶ既存の解析器順序依存で、u32 破損とは無関係）。tsdb subtree は 771 件で修正前とバイト一致（退行なし）。性能/メモリは **10.88s / peak 5.73GB**（R25.1 の 11.9s / 5.8GB から微改善）。ワークスペース **tests green**（exit 0）。
- **DEFERRED（次セッションへの引き継ぎ）**:
  - **govet unreachable の順序依存** — 報告順は pos ソートで安定化（2026-07-20）。残差があれば解析器内部の到達判定ゆらぎ（優先度低）。
  - **R25.3 `go list` cold 23s** — warm は 1.3s（OS キャッシュ）。golist ディスクキャッシュ（R24.4）は cold 初回には効かない。優先度低。
  - ~~**R25.2 残・隔離済み非致命 panic**~~ — `int64(x)` 等の明示型変換未対応（`expr.rs` ident TypeName）と `as_signature` が Named を解けない件、MethodVal 誤タグ時の `recv_type` panic を修正。`./tsdb/...` は prometheus `.golangci.yml` 下で panic 0・完走を確認。
  - **hybrid peak RSS** — 依存 AST 早期破棄で full ~13.4 GiB→~10.7 GiB。さらなる削減は未着手。
- **計測**: `GUFF_DEBUG_CACHE=1` で phase 別時間 + per-analyzer 集計（`report_analyzer_timing`）+ slow buildir(>1s) pkg。`=2` でサブ phase の内訳（§9.3.1）。RSS は `/usr/bin/time -l`。base 溢れの切り分けは `position.rs::add_file` に一時プローブ（`next_base > u32::MAX` で eprintln）。
- **回帰ゲート（ローカル）**: `regress/` — prometheus 本体の `.golangci.yml` で guff の wall / peak RSS と golangci-lint との finding-set 差分を比較し、悪化時のみ FAIL（絶対一致は不要）。`tsdb`（既定）と `full`（`./...`）の 2 プロファイル。`./regress/run.sh` / `--profile full` / `--update-baseline`。詳細は [`regress/README.md`](../regress/README.md)。
- **完了（2026-07-20）— 決定性 + hybrid seed AST 早期破棄**: govet `unreachable` 報告順の pos ソート；ineffassign multi-var の `(pos, name)` ソート；`import_package` の `sources.remove`。`full` peak **~13.4 GiB→~10.7 GiB**。
- **テスト**: `cargo test --workspace`（green）+ `-j 1` / `RAYON_NUM_THREADS=1` diff で 3 モード一致確認。

---

### Milestone G — 互換性の検証（「互換」を名乗る根拠）✅ 完了 (2026-07-17)

> A〜F を作っても、**実測で一致を示さない限り「互換」とは言えない**。ここが主張の裏付け。
> R21（差分ハーネス）/ R22（設定コーパス）/ R23（互換性マトリクス）で完了。

#### R21. 差分テストハーネス（guff vs golangci-lint）✅ 完了 (2026-07-17)
- **目的**: 同一コーパス・同一設定で両者を実行し、指摘集合を diff。linter ごとに一致率（precision/recall）を出す。
- **実装**: 新規 `compat/` — `run.sh` / `smoke.sh` / `normalize.py` / `standard.yml` /
  `allowlist.txt` / `repos.txt`。キーは `relpath:line:linter:message`（errcheck /
  unused / staticcheck `QF####:` プレフィックスを正規化）。`issues.max-*-issues: 0`
  で切り捨て・非決定性を排除。golangci は `--path-mode abs`。guff の診断パスは
  `compiled_go_files` フルパスを FileSet に載せるよう修正。fixture P=80%/R=100%；
  local R=100%（guff 余剰は ST1000・ineffassign 多報告を allowlist）。
  CI ゲート: `.github/workflows/compat.yml`（fixture smoke + normalize 単体テスト）。
- **完了条件**: 一致率レポートが生成され、CI ゲートになる — 満たした。
- **DEFERRED**: `--oss` コーパスの本格拡張、ineffassign 多報告・ST1000 既定差のパリティ。
- **テスト**: `python3 -m unittest discover -s compat/tests` + `./compat/smoke.sh`。
- **追記 (2026-08-03)**: per-linter **isolate** モード（`./compat/run.sh --isolate`）。
  `linters.default: none` + 単一 `enable` で fixture を両ツール比較。OSS 交差では
  見えない穴を塞ぐ。`compat/isolate/`（`linters.txt` / `fixtures/` / `allowlists/`）。
  CI: `--isolate --smoke`（standard 5）。追加は fixture + `linters.txt` 1 行。

#### R22. `.golangci.yml` コーパスのパース検証 ✅ 完了 (2026-07-17)
- 実在の `.golangci.yml`（有名 OSS のもの）を集めて、**パースエラー 0** を保証するテスト。
- **実装**: `crates/guff-lint/tests/testdata/config_corpus/` に golangci-lint v2 設定
  snapshot **52** 件（Prometheus / Grafana / Gitea / MinIO / NATS / Tailscale / Vitess /
  Consul / Helm / Moby / golangci-lint / Basecamp CLI / Kargo / Telegraf / Caddy /
  Traefik / Cilium / Argo CD / Vault / Nomad / Loki / Mimir / Tempo / Thanos /
  containerd / buildkit / compose / Trivy / Cosign / goreleaser ほか）。各ファイル先頭に
  出典 URL + キャプチャ日。`SOURCES.md` に refresh / 追加手順。
  `parse_golangci_config_corpus` が全件を `parse_config_str` → `linter_selection` →
  `effective_issues` まで検証（下限 50 件）。CI ゲート:
  `.github/workflows/config-corpus.yml`。
- **完了条件**: パースエラー 0・コーパス拡張・CI ゲート・出典/更新手順 — 満たした。
- **DEFERRED**: さらに大規模な自動同期（定期 upstream pull）、v1 コーパス（migrate 用）の
  別ディレクトリ化。
- **テスト**: `cargo test -p guff-lint --test config_test parse_golangci_config_corpus`。

#### R23. 互換性マトリクスの公開 ✅ 完了 (2026-07-17)
- どの linter・どの設定キー・どの出力フォーマットが「対応済/部分/未対応」かを表にして公開。
- **実装**: [`docs/COMPATIBILITY.md`](COMPATIBILITY.md) に互換性マトリクスを新設。
  1. **Linter**: golangci-lint v2 の全 **114** linter を ✅/🟡/❌ で分類（**114 実装** = ✅ 97 + 🟡 17）。
     （2026-07-24: 残り 5 = nilerr / contextcheck / wastedassign / spancheck / zerologlint を実装し 109→114。）
  2. **設定キー**: `run` / `linters` / `linters.exclusions` / `formatters` / `issues` / `severity` /
     `output` の各キーを対応状況付きで一覧（パースのみ vs 実効を区別）。
  3. **出力フォーマット**: text / colored / json / checkstyle / sarif / tab / colored-tab /
     github-actions を一覧。
  - linter 一覧の出典は <https://golangci-lint.run/docs/linters/>（キャプチャ 2026-07-17）。
    README からもリンク。差分の実測は R21 の `compat/` で継続。
- **完了条件**: linter・設定キー・出力フォーマットの対応表を公開 — 満たした。
- **DEFERRED**: 表の自動生成（レジストリからの照合テスト）、上流 linter 追加の定期同期。

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
- 新しい linter → §3.3 の表に 1 行。あわせて [`COMPATIBILITY.md`](COMPATIBILITY.md) の該当行も更新。
- そのセッションで何をしたか → [`SESSION-LOG.md`](SESSION-LOG.md) の表の先頭に 1 行。
- 冗長になりがちな詳細（設定キー全列挙・完了履歴）は本書に書かず、コード内 `// DEFERRED:` と `SESSION-LOG.md` / git に委ねる。

### 9.3.1 phase タイマーの詳細レベル（`GUFF_DEBUG_CACHE=2`）

`GUFF_DEBUG_CACHE` を **`2` 以上**にすると、phase 合計の下に内訳が出る（`=1` の出力は不変）。
値が数値でない場合（`=1` / 空 / `=on` など）は従来どおりレベル 1。

```
guff:     golist cache probe 0.00s (miss)                    ← ディスクキャッシュ判定
guff:     golist subprocess 1.00s (14069597 bytes), cache store 0.00s
guff:     refine total 0.01s (1792 pkgs, 294 roots)
guff:     target check read 0.04s / parse 0.89s / seed-clone 0.02s / check_files 1.71s (summed across workers)
guff:     format collect_paths 0.00s (725 files, 1 roots)
guff:     format gofumpt 0.57s (0 unformatted)               ← formatter ごと
```

注意点:

- **`target check …` は wall ではなく全ワーカーの合計 CPU**（`PERF_TASKS.md` §1.6）。
  wall と混同しない。
- **`format …` の行は phase の行の間に割り込む。** format は専用プールの別スレッドで
  重畳しているので、**出力順は毎回変わる**（行は `eprintln!` のロックで壊れない）。
- 未設定時のオーバーヘッドはゼロ。ファイル単位の計測は `Option<Instant>` で、
  レベル 2 でなければ時計を読まない。
- 実装は `crates/guff-packages/src/debug.rs` と `crates/guff-lint/src/debug.rs`
  （2 crate に共通の下位 crate が無いので写しが 2 つある。レベルの決め方を変えるときは両方）。

### 9.4 プロファイリング（samply）

phase タイマー（`GUFF_DEBUG_CACHE=1` / `=2`）は「どの phase・どのサブ phase が遅いか」までしか
分からない。**関数レベルでどこに時間が行っているか**を見るには `samply`（macOS / Linux 両対応の
サンプリングプロファイラ、UI は Firefox Profiler）を使う。

```bash
cargo install samply                    # 初回のみ
cargo build --profile profiling         # → target/profiling/guff

cd /path/to/prometheus
CACHE=$(mktemp -d)
GUFF_CACHE="$CACHE" samply record -- \
  /path/to/guff/target/profiling/guff run --no-cache -c .golangci.yml ./... >/dev/null
rm -rf "$CACHE"                         # 一時キャッシュの後片付けを忘れない
```

`samply record` はプロファイル取得後にローカルサーバを立ち上げてブラウザを開く。
ブラウザを開かずファイルだけ残すなら `samply record --save-only -o profile.json.gz -- …`。

**専用プロファイル `[profile.profiling]`（ルート `Cargo.toml`）を使う理由**:
`[profile.release]` は `strip = true` なのでシンボルが消えてスタックが読めない。
`profiling` は `inherits = "release"` に `strip = false` / `debug = 1` を足しただけで、
最適化（`opt-level = 3` / `lto = "fat"` / `codegen-units = 1`）は release と同一。
**`[profile.release]` 自体は変更しないこと** — `lto` / `codegen-units` / `panic` は意図して
選ばれている（§43 のコメント参照）。

**落とし穴（守らないと結論を間違える）**:

- **`profiling` ビルドの wall を regress ゲートや PERF タスクの数値に使ってはいけない。**
  `strip=false` / `debug=1` でバイナリサイズが変わり、release と数字が一致しない。
  ゲートは常に `target/release/guff`。
- guff は並列実行なので、フレームグラフは**スレッドごとに分けて見る**。
  「全スレッドの合計 CPU」と「wall」を混同しない（合計 CPU が 6s でも wall は 1s かもしれない）。
- 計測前に `./scripts/perf-guard.sh` を通す（他プロセスに CPU を食われていると比率まで歪む）。

### 9.5 ローカル向け高速ビルド（`target-cpu=native` / PGO）

**配布・CI・regress baseline には使わない。** CI は素の `cargo build --release` で、
`.cargo/config.toml` に `target-cpu=native` を書いてコミットすると他マシンや CI で
不正命令になり得る。

**`target-cpu=native`（都度 `RUSTFLAGS`）:**

```bash
RUSTFLAGS='-C target-cpu=native' cargo build --release -p guff-lint
```

2026-07-29 の aarch64-apple-darwin 実測では wall 差は誤差帯（§A-8a NO-GO）。
x86-64 ローカルでは効く余地がある。

**PGO（Profile-Guided Optimization）:**

```bash
./scripts/build-pgo.sh
# → target/release/guff（PGO 済み）。元の generic は target/release/guff.generic.bak
```

手順の詳細と注意（`--update-baseline` 禁止）は [`PERF_TASKS_V2.md`](PERF_TASKS_V2.md) §A-8。

詳細な計測プロトコル（cold / warm の定義、findings 同一性の検証、GO/NO-GO 判定）は
[`PERF_TASKS.md`](PERF_TASKS.md) §1〜§2 と [`PERF_TASKS_V2.md`](PERF_TASKS_V2.md) §1〜§2 にある。

---

## 10. セッション記録

作業ログは [`SESSION-LOG.md`](SESSION-LOG.md) に分離した（一次情報は `git log`）。
新しいセッションの記録はそちらの表の先頭に 1 行追記する。
