# guff — マルチ linter 移植計画

> **目的**: golangci-lint v2 が提供する linter / formatter を Rust に移植し、
> **1 本の解析パイプライン**でまとめて実行できるようにする。
> 後続セッションが「次に何を移植するか」を迷わないためのタスクリスト。
>
> **関連文書**:
> - [`PRE-LINTER-PLAN.md`](../PRE-LINTER-PLAN.md) — 解析基盤（Phase 0–8）の全体計画 ✅ 完了
> - [`docs/STATICCHECK-MIGRATION.md`](STATICCHECK-MIGRATION.md) — staticcheck 個別ルールの進捗
> - [`docs/ADDING-ANALYZER.md`](ADDING-ANALYZER.md) — analyzer 追加手順
>
> **リポジトリ**: `/Users/dakimura/projects/src/github.com/dakimura/me/projects/guff`
> **最終更新**: 2026-07-14

---

## 0. セッション開始チェックリスト

1. この文書の **§2 進捗サマリ** と **§4 フェーズ別タスク** を読む。
2. staticcheck 作業中なら [`STATICCHECK-MIGRATION.md`](STATICCHECK-MIGRATION.md) §2 も確認。
3. 着手する linter の **Go ソース**をローカル clone から `Read` する（§7 参照）。
4. 作業後:
   - §2 の進捗表を更新
   - §9 セッション記録に 1 行追加
   - 新しい deferral は §8 に追記
5. **コミットはユーザー依頼時のみ**。push しない。

```bash
. "$HOME/.cargo/env"
cd /Users/dakimura/projects/src/github.com/dakimura/me/projects/guff

# 基盤回帰
cargo test -p guff-analysis -p guff-runner -p guff-packages -q

# linter クレート（存在するもの）
cargo test -p guff-staticcheck -q
```

---

## 1. 設計原則（golangci-lint の批判を避ける）

guff が **遅く・重くならない**ための鉄則。移植のたびに確認する。

| # | 原則 | golangci-lint の問題 | guff の対策 |
|---|------|---------------------|------------|
| 1 | **パイプラインは 1 回** | linter ごとに load/typecheck が重なる | `guff_runner::run` を 1 回だけ呼ぶ |
| 2 | **linter は Analyzer の追加** | 各 linter が独自 walk | `requires` で `inspect` / `buildir` を共有 |
| 3 | **load mode は union** | 過剰ロード | `load_mode_for_analyzers` で最深 tier のみ |
| 4 | **横断ロジックは基盤へ** | errcheck と staticcheck が別 walk | `guff-analysis` に FW 昇格（§6） |
| 5 | **依存は export data** | stdlib 再 typecheck | `guff-exportdata`（PRE-LINTER-PLAN §2.3） |
| 6 | **解析後に trim** | メモリ積み上がり | `RunnerOptions::release_memory` |
| 7 | **並列はパッケージ単位** | linter 並列で重複解析 | PL11: パッケージ並列（同一 pass は DAG で 1 回） |

### 1.1 目標アーキテクチャ

```
go list (guff-packages)
  → typecheck（initial: source / deps: export data）
  → Pass (guff-analysis)
  → action graph (guff-runner)     ← 全 linter を 1 DAG で実行
  → Diagnostic 収集
       ↑
  guff-lint CLI（設定・有効化・診断フォーマット）
       ↑
  ┌─────────────┬──────────┬───────────────┬─────────────┐
  │guff-staticcheck│guff-govet│guff-errcheck│guff-unused │ …
  └─────────────┴──────────┴───────────────┴─────────────┘
```

### 1.2 解析 tier（コスト見積もり用）

| Tier | 必要なもの | 代表 linter | guff 共有 pass |
|------|-----------|------------|---------------|
| T0 | AST のみ | `asciicheck`, `dogsled`, `whitespace` | `inspect` |
| T1 | Types | `govet` 大半, `errname`, `misspell` | `inspect` + types |
| T2 | SSA | `errcheck`, `ineffassign`, `callcheck` 系 | `buildir` |
| T3 | Whole-program | `unused`, `unparam` | export facts + 全パッケージ |
| T4 | 非 go/analysis | `revive`, `dupl`, formatter 群 | 別パイプライン or 薄いラッパ |

**ルール**: 新 linter は可能な限り低 tier で実装し、T2 以上は `requires` で共有する。

---

## 2. 進捗サマリ

### 2.1 基盤（PRE-LINTER-PLAN）

| Phase | 状態 | メモ |
|-------|------|------|
| 0–7 linter 基盤 | ✅ 完了 | packages / analysis / runner |
| 8 任意ユーティリティ | 未着手 | gofmt 等 |
| PL06 メモリ trim 完全版 | 簡略版のみ | `trim_packages` |
| PL11 パッケージ並列 | 未着手 | 逐次 topo 実行 |
| PL07 GOCACHE 連携 | 未着手 | |

### 2.2 golangci-lint v2 `standard` プリセット（最優先）

golangci-lint v2 の `linters.default: standard` に含まれる linter:

| linter | guff クレート | 状態 | 備考 |
|--------|-------------|------|------|
| **staticcheck** | `guff-staticcheck` | ✅ 完了 (137 analyzers) | [`STATICCHECK-MIGRATION.md`](STATICCHECK-MIGRATION.md) |
| **govet** | `guff-govet` | ✅ 完了 (29/29 passes) | 全 pass 移植済み（`framepointer` 含む） |
| **errcheck** | `guff-errcheck` | ✅ 完了 | kisielk/errcheck 相当（excludes / blank / assert オプション） |
| **ineffassign** | `guff-ineffassign` | ✅ 完了 | gordonklaus CFG 移植 + generated 除外 |
| **unused** | `guff-unused` | ✅ 完了 | 単一パッケージ版（型・定数・メソッド・const グループ） |
| typecheck | （組み込み） | ✅ 相当 | typecheck 失敗時は解析スキップ |

### 2.3 オーケストレーション

| コンポーネント | 状態 | 備考 |
|--------------|------|------|
| `guff-lint` CLI | 🔄 骨格 | `run` + `standard` プリセット（staticcheck のみ配線） |
| 設定ファイル（`.golangci.yml` / `.guff.yml`） | ✅ 最小 | v1/v2 読み取り、`migrate` コマンド |
| `nolint` / 除外処理 | ⬜ 未着手 | golangci `nolintlint` 相当は後回し可 |

### 2.4 その他 linter（golangci-lint v2.1.6 収録 ~110 件）

| カテゴリ | 件数 | 状態 |
|---------|------|------|
| go/analysis ベース | ~65 | ⬜ 未着手（standard 5 件を除く） |
| スタンドアロン AST | ~25 | ⬜ 未着手 |
| Formatter | 5 | ⬜ 未着手（lint とは別 Phase） |
| 非推奨 / スキップ | ~5 | — |

---

## 3. golangci-lint プリセット対応表

golangci-lint v2 の `linters.default` 値と guff での扱い:

| プリセット | 内容 | guff 移植優先度 |
|-----------|------|---------------|
| `standard` | errcheck, govet, ineffassign, staticcheck, unused | **P0** — まずここを完走 |
| `fast` | standard から遅い linter を除外 | P0 完了後に設定対応 |
| `all` | 全 linter (~110) | P2 以降 |
| `none` | 無効（`enable` で個別指定） | 設定のみ |

---

## 4. フェーズ別タスク

### Phase L0 — staticcheck 完走（完了）

**ブロッカー**: 他 linter の品質基準になる。並行作業は L1 の設計のみ可。

- [x] staticcheck 残ルール移植（[`STATICCHECK-MIGRATION.md`](STATICCHECK-MIGRATION.md) §5 参照）
- [ ] `typeindex` 移植（pattern 系の性能改善、全 linter が恩恵）
- [x] staticcheck E2E（`stdlib_e2e` の `#[ignore]` 解除）

### Phase L1 — `guff-lint` CLI 骨格

standard 5 件を束ねる最小 CLI。**linter 移植と並行可能**。

| ID | タスク | 依存 | 成果物 | 状態 |
|----|--------|------|--------|------|
| L1-a | `crates/guff-lint` クレート新設 | Phase 7 ✅ | workspace member | ✅ |
| L1-b | analyzer レジストリ（名前 → `&Analyzer`） | L1-a | `registry.rs` | ✅ |
| L1-c | `standard` プリセット定義 | L1-b | 5 linter の名前リスト | ✅ |
| L1-d | CLI: `guff-lint run [packages...]` | L1-c, runner ✅ | `guff_runner::run` 呼び出し | ✅ |
| L1-e | 診断出力（text / JSON） | L1-d | 最低限 text | ✅（text のみ） |
| L1-f | 設定ファイル読み込み（YAML、最小） | L1-d | `enable` / `disable` / `default` | ✅（v1/v2 読み取り + migrate） |

**参考 Go ソース**: `golangci-lint/pkg/commands/run.go`, `pkg/config/`

### Phase L2 — standard プリセット（P0 linter）

#### L2-a: `guff-govet`

govet は `x/tools` の analysis pass 群。1 pass = 1 `Analyzer` が基本。

| ID | pass 名 | tier | 優先度 | 状態 |
|----|---------|------|--------|------|
| GV-01 | `printf` | T2 | 高 | ✅ (`guff-govet/src/printf.rs`) |
| GV-02 | `assign` | T1 | 高 | ✅ |
| GV-03 | `atomic` | T1 | 高 | ✅ |
| GV-04 | `bools` | T1 | 高 | ✅ |
| GV-05 | `buildtag` | T0 | 中 | ✅ |
| GV-06 | `cgocall` | T1 | 中 | ✅ |
| GV-07 | `composites` | T1 | 高 | ✅ |
| GV-08 | `copylocks` | T2 | 高 | ✅ |
| GV-09 | `defers` | T1 | 中 | ✅ |
| GV-10 | `directive` | T0 | 中 | ✅ |
| GV-11 | `errorsas` | T1 | 高 | ✅ |
| GV-12 | `framepointer` | T1 | 低 | ✅ |
| GV-13 | `httpresponse` | T2 | 中 | ✅ |
| GV-14 | `ifaceassert` | T1 | 中 | ✅ |
| GV-15 | `loopclosure` | T1 | 中 | ✅ |
| GV-16 | `lostcancel` | T2 | 高 | ✅ |
| GV-17 | `nilfunc` | T1 | 中 | ✅ |
| GV-18 | `printf`（本番品質） | T2 | 高 | ✅（GV-01 と統合。引数個数・型照合・`%[n]`/`*`/Fprintf/`%w`、`go vet` 一致） |
| GV-19 | `shift` | T1 | 高 | ✅ |
| GV-20 | `sigchanyzer` | T1 | 中 | ✅ |
| GV-21 | `slog` | T1 | 中 | ✅ |
| GV-22 | `stdmethods` | T1 | 中 | ✅ |
| GV-23 | `stringintconv` | T1 | 高 | ✅ |
| GV-24 | `structtag` | T1 | 高 | ✅ |
| GV-25 | `tests` | T1 | 中 | ✅ |
| GV-26 | `timeformat` | T1 | 中 | ✅ |
| GV-27 | `unmarshal` | T1 | 中 | ✅ |
| GV-28 | `unreachable` | T2 | 高 | ✅ |
| GV-29 | `unsafeptr` | T1 | 中 | ✅ |
| GV-30 | `unusedresult` | T1 | 高 | ✅ |

**作業手順**: 各 pass の Go ソース（`golang.org/x/tools/go/analysis/passes/<name>/`）を読み、`guff-govet/src/<name>.rs` として移植。`requires` は pass ごとに最小限。

**テスト**: `crates/guff-govet/tests/testdata/<pass>/` + go vet 公式 testdata 流用可。

#### L2-b: `guff-errcheck`

| ID | タスク | 依存 | 状態 |
|----|--------|------|------|
| EC-01 | `guff-analysis` に `unchecked_call` FW 設計 | buildir ✅ | ⬜ |
| EC-02 | `guff-errcheck` クレート新設 | EC-01 | ✅ |
| EC-03 | 基本 errcheck（error 戻り値の未チェック） | EC-02 | ✅ |
| EC-04 | `exclude` / `ignore` 設定 | EC-03, L1-f | ✅（デフォルト excludes） |
| EC-05 | `blank` 判定（`_ = fn()`） | EC-03 | ✅（`analyzer_check_blank`） |
| EC-06 | type assertion チェック（オプション） | EC-03 | ✅（`analyzer_check_asserts`） |

**参考**: `github.com/kisielk/errcheck`, staticcheck S1040 / SA5009 との統合を検討。

#### L2-c: `guff-ineffassign`

| ID | タスク | 依存 | 状態 |
|----|--------|------|------|
| IA-01 | `guff-ineffassign` クレート新設 | buildir ✅ | ✅ |
| IA-02 | CFG 上の ineffectual assignment 検出 | IA-01 | ✅（`cfg.rs`、gordonklaus 移植） |
| IA-03 | テスト（公式 testdata） | IA-02 | ✅（基本 + switch/named return/generated） |

**参考**: `github.com/gordonklaus/ineffassign`（go/analysis ラッパあり）

#### L2-d: `guff-unused`

| ID | タスク | 依存 | 状態 |
|----|--------|------|------|
| UN-01 | go-tools `unused` の fact 依存調査 | facts 基盤 ✅ | ✅（単一パッケージ版で代替） |
| UN-02 | `guff-unused` クレート新設 | UN-01 | ✅ |
| UN-03 | whole-program 未使用検出（関数・型・定数・変数） | UN-02, export data ✅ | ✅（単一パッケージ） |
| UN-04 | `//lint:ignore` / generated 除外 | facts/generated ✅ | ✅（generated 除外） |

**参考**: `honnef.co/go/tools/unused`（staticcheck と同リポジトリ）

#### L2-e: standard 統合

| ID | タスク | 依存 | 状態 |
|----|--------|------|------|
| SE-01 | `guff-lint` の `standard` プリセットに 5 linter 登録 | L2-a–d | ✅ |
| SE-02 | 結合 E2E（既知モジュールで golangci 出力と diff） | SE-01 | ⬜ |
| SE-03 | `linters.default: fast` 対応（遅い linter 除外） | SE-02 | ⬜ |

### Phase L3 — 高価値 go/analysis linter（P1）

`WithLoadForGoAnalysis` の golangci 収録 linter。upstream リポジトリ単位で移植。

#### L3-a: `gostaticanalysis` ファミリ（1 クレート `guff-gostaticanalysis` 推奨）

| linter | リポジトリ | 状態 |
|--------|----------|------|
| nilerr | `github.com/gostaticanalysis/nilerr` | ⬜ |
| nilnil | `github.com/Antonboom/nilnil` | ⬜ |
| forcetypeassert | `github.com/gostaticanalysis/forcetypeassert` | ⬜ |
| makezero | `github.com/ashanbrown/makezero` | ⬜ |
| mirror | `github.com/butuzov/mirror` | ⬜ |
| nilnesserr | `github.com/alingse/nilnesserr` | ⬜ |

#### L3-b: error 系

| linter | リポジトリ | 状態 |
|--------|----------|------|
| errorlint | `github.com/polyfloyd/go-errorlint` | ⬜ |
| errname | `github.com/Antonboom/errname` | ⬜ |
| err113 | `github.com/Djarvur/go-err113` | ⬜ |
| errchkjson | `github.com/breml/errchkjson` | ⬜ |
| wrapcheck | `github.com/tomarrell/wrapcheck` | ⬜ |
| rowserrcheck | `github.com/jingyugao/rowserrcheck` | ⬜ |
| nilerr | （L3-a 参照） | ⬜ |

#### L3-c: context / resource 系

| linter | リポジトリ | 状態 |
|--------|----------|------|
| bodyclose | `github.com/timakin/bodyclose` | ⬜ |
| noctx | `github.com/sonatard/noctx` | ⬜ |
| contextcheck | `github.com/kkHAIKE/contextcheck` | ⬜ |
| sqlclosecheck | `github.com/ryanrolds/sqlclosecheck` | ⬜ |
| spancheck | `github.com/jjti/go-spancheck` | ⬜ |
| fatcontext | `github.com/Crocmagnon/fatcontext` | ⬜ |

#### L3-d: その他 go/analysis（よく使われる順）

| linter | リポジトリ | 状態 |
|--------|----------|------|
| gosec | `github.com/securego/gosec` | ⬜ |
| gocritic | `github.com/go-critic/go-critic` | ⬜ |
| unparam | `github.com/mvdan/unparam` | ⬜ |
| unconvert | `github.com/mdempsky/unconvert` | ⬜ |
| exhaustive | `github.com/nishanths/exhaustive` | ⬜ |
| exhaustruct | `github.com/GaijinEntertainment/go-exhaustruct` | ⬜ |
| copyloopvar | `github.com/karamaru-alpha/copyloopvar` | ⬜ |
| perfsprint | `github.com/catenacyber/perfsprint` | ⬜ |
| usestdlibvars | `github.com/sashamelentyev/usestdlibvars` | ⬜ |
| usetesting | `github.com/ldez/usetesting` | ⬜ |
| exptostd | `github.com/ldez/exptostd` | ⬜ |
| durationcheck | `github.com/charithe/durationcheck` | ⬜ |
| goconst | `github.com/jgautheron/goconst` | ⬜ |
| musttag | `github.com/go-simpler/musttag` | ⬜ |
| loggercheck | `github.com/timonwong/loggercheck` | ⬜ |
| sloglint | `github.com/go-simpler/sloglint` | ⬜ |
| testifylint | `github.com/Antonboom/testifylint` | ⬜ |
| ginkgolinter | `github.com/nunnatsa/ginkgolinter` | ⬜ |
| asasalint | `github.com/alingse/asasalint` | ⬜ |
| bidichk | `github.com/breml/bidichk` | ⬜ |
| containedctx | `github.com/sivchari/containedctx` | ⬜ |
| canonicalheader | `github.com/lasiar/canonicalheader` | ⬜ |
| forbidigo | `github.com/ashanbrown/forbidigo` | ⬜ |
| importas | `github.com/julz/importas` | ⬜ |
| iface | `github.com/uudashr/iface` | ⬜ |
| ireturn | `github.com/butuzov/ireturn` | ⬜ |
| intrange | `github.com/ckaznocha/intrange` | ⬜ |
| protogetter | `github.com/ghostiam/protogetter` | ⬜ |
| reassign | `github.com/curioswitch/go-reassign` | ⬜ |
| recvcheck | `github.com/raeperd/recvcheck` | ⬜ |
| nonamedreturns | `github.com/firefart/nonamedreturns` | ⬜ |
| paralleltest | `github.com/kunwardeep/paralleltest` | ⬜ |
| thelper | `github.com/kulti/thelper` | ⬜ |
| tparallel | `github.com/moricho/tparallel` | ⬜ |
| wastedassign | `github.com/sanposhiho/wastedassign` | ⬜ |
| varnamelen | `github.com/blizzy78/varnamelen` | ⬜ |
| zerologlint | `github.com/ykadowak/zerologlint` | ⬜ |
| tagliatelle | `github.com/ldez/tagliatelle` | ⬜ |
| gochecknoglobals | `github.com/leighmcculloch/gochecknoglobals` | ⬜ |
| gochecksumtype | `github.com/alecthomas/go-check-sumtype` | ⬜ |
| gosmopolitan | `github.com/xen0n/gosmopolitan` | ⬜ |

### Phase L4 — スタンドアロン linter（P2）

go/analysis ではない、または独自エンジンを持つ linter。

| linter | 方式 | クレート案 | 難易度 | 状態 |
|--------|------|----------|--------|------|
| **revive** | 独自 rule engine | `guff-revive` | 高 | ⬜ |
| misspell | 辞書ベース | `guff-misspell` | 低 | ⬜ |
| dupl | トークン類似 | `guff-dupl` | 中 | ⬜ |
| funlen | AST メトリクス | `guff-style`（束ねる） | 低 | ⬜ |
| gocyclo | AST メトリクス | `guff-style` | 低 | ⬜ |
| gocognit | AST メトリクス | `guff-style` | 低 | ⬜ |
| cyclop | AST メトリクス | `guff-style` | 低 | ⬜ |
| nestif | AST メトリクス | `guff-style` | 低 | ⬜ |
| lll | 行長 | `guff-style` | 低 | ⬜ |
| whitespace | AST 整形検出 | `guff-style` | 低 | ⬜ |
| wsl | 空行ルール | `guff-style` | 低 | ⬜ |
| nlreturn | 空行ルール | `guff-style` | 低 | ⬜ |
| godot | コメント句読点 | `guff-comment` | 低 | ⬜ |
| godox | TODO 検出 | `guff-comment` | 低 | ⬜ |
| dupword | コメント重複語 | `guff-comment` | 低 | ⬜ |
| dogsled | ブランク識別子過多 | `guff-style` | 低 | ⬜ |
| nakedret | naked return | `guff-style` | 低 | ⬜ |
| prealloc | slice 事前確保 | `guff-style` | 低 | ⬜ |
| predeclared | 予約語シャドウ | `guff-style` | 低 | ⬜ |
| decorder | 宣言順序 | `guff-style` | 低 | ⬜ |
| grouper | import グルーピング | `guff-style` | 低 | ⬜ |
| goheader | ライセンスヘッダ | `guff-style` | 低 | ⬜ |
| depguard | import 禁止 | `guff-import` | 中 | ⬜ |
| gomodguard | module 禁止 | `guff-import` | 中 | ⬜ |
| gomoddirectives | go.mod 検査 | `guff-import` | 中 | ⬜ |
| mnd | マジックナンバー | `guff-style` | 低 | ⬜ |
| inamedparam | インターフェース命名 | `guff-style` | 低 | ⬜ |
| interfacebloat | interface 肥大 | `guff-style` | 低 | ⬜ |
| maintidx | 保守性指数 | `guff-style` | 低 | ⬜ |
| goprintffuncname | Printf 関数名 | `guff-style` | 低 | ⬜ |
| nosprintfhostport | Sprintf host:port | `guff-style` | 低 | ⬜ |
| promlinter | Prometheus メトリクス命名 | `guff-style` | 低 | ⬜ |
| testableexamples | Example 関数テスト | `guff-test` | 低 | ⬜ |
| testpackage | `_test` パッケージ命名 | `guff-test` | 低 | ⬜ |
| gocheckcompilerdirectives | コンパイラ directive | `guff-style` | 低 | ⬜ |
| gochecknoinits | init 禁止 | `guff-style` | 低 | ⬜ |
| funcorder | 関数順序 | `guff-style` | 低 | ⬜ |
| tagalign | struct tag 整列 | `guff-style` | 低 | ⬜ |
| asciicheck | 非 ASCII 検出 | `guff-style` | 低 | ⬜ |
| nolintlint | nolint コメント検証 | `guff-lint` 内 | 中 | ⬜ |

### Phase L5 — Formatter（lint とは別パイプライン）

golangci-lint v2 では linter と formatter が分離。guff でも **別 Phase** とする。

| formatter | リポジトリ | 状態 |
|-----------|----------|------|
| gofmt | `cmd/gofmt`（stdlib） | ⬜ |
| gofumpt | `github.com/mvdan/gofumpt` | ⬜ |
| goimports | `golang.org/x/tools/cmd/goimports` | ⬜ |
| gci | `github.com/daixiang0/gci` | ⬜ |
| golines | `github.com/segmentio/golines` | ⬜ |

**方針**: `guff-fmt` クレート。lint runner とは別コマンド `guff-lint fmt` 推奨。

### Phase L6 — 性能・運用（横断）

| ID | タスク | 優先度 | 状態 |
|----|--------|--------|------|
| PL11 | パッケージ並列実行 | 中 | ⬜ |
| PL06 | `decUse` 完全版メモリ管理 | 中 | ⬜ |
| PL07 | GOCACHE / build cache 連携 | 低 | ⬜ |
| INF-01 | `typeindex` 移植 | 中 | ⬜ |
| INF-02 | `nolint` プリプロセッサ | 中 | ⬜ |
| INF-03 | 診断の `SuggestedFix` 統合 | 低 | ⬜ |
| INF-04 | golangci 設定ファイル互換レイヤ | 低 | ✅ 最小（linters + migrate） |

---

## 5. 移植しない / 後回し

| linter | 理由 |
|--------|------|
| **gosimple** | staticcheck に統合済み（golangci でも alias） |
| **stylecheck** | staticcheck に統合済み |
| **megacheck** | staticcheck に統合済み |
| **exportloopref** | Go 1.22+ では `copyloopvar` に置換 |
| **deadcode** | `unused` に置換 |
| **execinquery** | アーカイブ済み |
| **exhaustivestruct** | `exhaustruct` に置換 |
| **golint** | 廃止（`revive` が後継） |
| **interfacer** | 廃止 |
| **structcheck** / **varcheck** | `unused` に統合 |

---

## 6. 共有インフラ拡張（linter 横断）

linter 移植前に検討すべき `guff-analysis` 拡張:

| FW | 用途 | 恩恵する linter | 状態 |
|----|------|---------------|------|
| **unchecked_call** | error / multi-value の未使用検出 | errcheck, govet/unusedresult, staticcheck S1040 | ⬜ |
| **typeindex** | 呼び出しサイト索引 | pattern 系全般, errcheck 最適化 | ⬜ |
| **facts/purity** | 純粋性 fact | staticcheck 一部, unused | ⬜ |
| **whole_program** | 全パッケージ横断解析 | unused, unparam | ⬜ |
| **nolint** | 診断抑制 | 全 linter | ⬜ |
| **generated** | 生成コード除外 | 全 linter | ✅ 基本実装あり |

---

## 7. ローカル clone 一覧

移植作業用の参照ソース。推奨ディレクトリ: `~/guff-linter-sources/`（任意）。

### 7.1 必須（最初に clone）

これだけあれば standard プリセット + 大部分の go/analysis linter が読める。

```bash
mkdir -p ~/guff-linter-sources && cd ~/guff-linter-sources

# guff 本体（作業リポジトリ）
# 既に: /Users/dakimura/projects/src/github.com/dakimura/me/projects/guff

# golangci-lint — 統合パターン・testdata・設定
git clone --depth 1 https://github.com/golangci/golangci-lint.git

# Go 公式 tools — govet passes, go/analysis 基盤, goimports
git clone --depth 1 https://github.com/golang/go.git
git clone --depth 1 https://github.com/golang/tools.git

# staticcheck / unused（同一 org）
git clone --depth 1 https://github.com/dominikh/go-tools.git
```

| ディレクトリ | 用途 |
|------------|------|
| `golangci-lint/` | linter 統合・設定・公式 testdata（`pkg/golinters/*/testdata/`） |
| `go/` | `src/cmd/vet/`, `src/cmd/gofmt/` |
| `tools/` | `go/analysis/passes/*`（govet 各 pass）, `go/packages`, `go/ssa` |
| `go-tools/` | staticcheck, unused, pattern, callcheck, facts |

### 7.2 standard プリセット用（P0）

```bash
cd ~/guff-linter-sources

git clone --depth 1 https://github.com/kisielk/errcheck.git
git clone --depth 1 https://github.com/gordonklaus/ineffassign.git
```

| リポジトリ | linter | 主な読むパス |
|----------|--------|------------|
| `kisielk/errcheck` | errcheck | `errcheck/`, `internal/` |
| `gordonklaus/ineffassign` | ineffassign | `pkg/ineffassign/` |
| `go-tools`（上で取得済） | unused | `unused/` |

### 7.3 go/analysis ファミリ（P1、リポジトリ単位）

```bash
cd ~/guff-linter-sources

# gostaticanalysis org
git clone --depth 1 https://github.com/gostaticanalysis/nilerr.git
git clone --depth 1 https://github.com/gostaticanalysis/forcetypeassert.git
git clone --depth 1 https://github.com/ashanbrown/makezero.git
git clone --depth 1 https://github.com/ashanbrown/forbidigo.git

# よく使われる linter
git clone --depth 1 https://github.com/securego/gosec.git
git clone --depth 1 https://github.com/go-critic/go-critic.git
git clone --depth 1 https://github.com/mvdan/unparam.git
git clone --depth 1 https://github.com/mvdan/unconvert.git
git clone --depth 1 https://github.com/polyfloyd/go-errorlint.git
git clone --depth 1 https://github.com/nishanths/exhaustive.git
git clone --depth 1 https://github.com/timakin/bodyclose.git
git clone --depth 1 https://github.com/sonatard/noctx.git
git clone --depth 1 https://github.com/kkHAIKE/contextcheck.git
git clone --depth 1 https://github.com/charithe/durationcheck.git
git clone --depth 1 https://github.com/jgautheron/goconst.git
git clone --depth 1 https://github.com/go-simpler/musttag.git
git clone --depth 1 https://github.com/go-simpler/sloglint.git
git clone --depth 1 https://github.com/Antonboom/testifylint.git
git clone --depth 1 https://github.com/Antonboom/errname.git
git clone --depth 1 https://github.com/Antonboom/nilnil.git
git clone --depth 1 https://github.com/breml/errchkjson.git
git clone --depth 1 https://github.com/breml/bidichk.git
git clone --depth 1 https://github.com/catenacyber/perfsprint.git
git clone --depth 1 https://github.com/Crocmagnon/fatcontext.git
git clone --depth 1 https://github.com/karamaru-alpha/copyloopvar.git
git clone --depth 1 https://github.com/ldez/exptostd.git
git clone --depth 1 https://github.com/ldez/usetesting.git
git clone --depth 1 https://github.com/ldez/tagliatelle.git
git clone --depth 1 https://github.com/ldez/gomoddirectives.git
git clone --depth 1 https://github.com/sashamelentyev/usestdlibvars.git
git clone --depth 1 https://github.com/jjti/go-spancheck.git
git clone --depth 1 https://github.com/ryanrolds/sqlclosecheck.git
git clone --depth 1 https://github.com/jingyugao/rowserrcheck.git
git clone --depth 1 https://github.com/tomarrell/wrapcheck.git
git clone --depth 1 https://github.com/Djarvur/go-err113.git
git clone --depth 1 https://github.com/GaijinEntertainment/go-exhaustruct.git
git clone --depth 1 https://github.com/alingse/asasalint.git
git clone --depth 1 https://github.com/alingse/nilnesserr.git
git clone --depth 1 https://github.com/sivchari/containedctx.git
git clone --depth 1 https://github.com/lasiar/canonicalheader.git
git clone --depth 1 https://github.com/julz/importas.git
git clone --depth 1 https://github.com/uudashr/iface.git
git clone --depth 1 https://github.com/butuzov/ireturn.git
git clone --depth 1 https://github.com/butuzov/mirror.git
git clone --depth 1 https://github.com/ckaznocha/intrange.git
git clone --depth 1 https://github.com/ghostiam/protogetter.git
git clone --depth 1 https://github.com/curioswitch/go-reassign.git
git clone --depth 1 https://github.com/raeperd/recvcheck.git
git clone --depth 1 https://github.com/firefart/nonamedreturns.git
git clone --depth 1 https://github.com/kunwardeep/paralleltest.git
git clone --depth 1 https://github.com/kulti/thelper.git
git clone --depth 1 https://github.com/moricho/tparallel.git
git clone --depth 1 https://github.com/sanposhiho/wastedassign.git
git clone --depth 1 https://github.com/blizzy78/varnamelen.git
git clone --depth 1 https://github.com/ykadowak/zerologlint.git
git clone --depth 1 https://github.com/timonwong/loggercheck.git
git clone --depth 1 https://github.com/nunnatsa/ginkgolinter.git
git clone --depth 1 https://github.com/leighmcculloch/gochecknoglobals.git
git clone --depth 1 https://github.com/alecthomas/go-check-sumtype.git
git clone --depth 1 https://github.com/xen0n/gosmopolitan.git
```

### 7.4 スタンドアロン / 大型（P2）

```bash
cd ~/guff-linter-sources

# 大型・独自エンジン
git clone --depth 1 https://github.com/mgechev/revive.git
git clone --depth 1 https://github.com/mibk/dupl.git

# スタイル系（小〜中）
git clone --depth 1 https://github.com/golangci/misspell.git
git clone --depth 1 https://github.com/ultraware/funlen.git
git clone --depth 1 https://github.com/fzipp/gocyclo.git
git clone --depth 1 https://github.com/uudashr/gocognit.git
git clone --depth 1 https://github.com/bkielbasa/cyclop.git
git clone --depth 1 https://github.com/nakabonne/nestif.git
git clone --depth 1 https://github.com/ultraware/whitespace.git
git clone --depth 1 https://github.com/bombsimon/wsl.git
git clone --depth 1 https://github.com/ssgreg/nlreturn.git
git clone --depth 1 https://github.com/tetafro/godot.git
git clone --depth 1 https://github.com/matoous/godox.git
git clone --depth 1 https://github.com/Abirdcfly/dupword.git
git clone --depth 1 https://github.com/alexkohler/dogsled.git
git clone --depth 1 https://github.com/alexkohler/nakedret.git
git clone --depth 1 https://github.com/alexkohler/prealloc.git
git clone --depth 1 https://github.com/nishanths/predeclared.git
git clone --depth 1 https://github.com/OpenPeeDeeP/depguard.git
git clone --depth 1 https://github.com/ryancurrah/gomodguard.git
git clone --depth 1 https://github.com/tommy-muehle/go-mnd.git
git clone --depth 1 https://github.com/macabu/inamedparam.git
git clone --depth 1 https://github.com/sashamelentyev/interfacebloat.git
git clone --depth 1 https://github.com/yagipy/maintidx.git
git clone --depth 1 https://github.com/golangci/go-printf-func-name.git
git clone --depth 1 https://github.com/stbenjam/no-sprintf-host-port.git
git clone --depth 1 https://github.com/yeya24/promlinter.git
git clone --depth 1 https://github.com/maratori/testableexamples.git
git clone --depth 1 https://github.com/maratori/testpackage.git
git clone --depth 1 https://github.com/leighmcculloch/gocheckcompilerdirectives.git
git clone --depth 1 https://github.com/manuelarte/funcorder.git
git clone --depth 1 https://github.com/4meepo/tagalign.git
git clone --depth 1 https://github.com/tdakkota/asciicheck.git
git clone --depth 1 https://github.com/leonklingele/grouper.git
git clone --depth 1 https://github.com/denis-tingaikin/go-header.git
git clone --depth 1 https://gitlab.com/bosi/decorder.git
```

### 7.5 Formatter 用

```bash
cd ~/guff-linter-sources

git clone --depth 1 https://github.com/mvdan/gofumpt.git
git clone --depth 1 https://github.com/daixiang0/gci.git
git clone --depth 1 https://github.com/segmentio/golines.git
# gofmt → go/src/cmd/gofmt（7.1 で取得済）
# goimports → tools/cmd/goimports（7.1 で取得済）
```

### 7.6 clone の優先度まとめ

| 優先度 | clone するもの | タイミング |
|--------|--------------|----------|
| **P0 必須** | `golangci-lint`, `go`, `tools`, `go-tools`, `errcheck`, `ineffassign` | 今すぐ |
| **P1 推奨** | `gosec`, `go-critic`, `unparam`, `errorlint`, `bodyclose`, `revive` | L2 着手時 |
| **P2 按需** | §7.3 / §7.4 の残り | 該当 linter 移植直前 |
| **P3 formatter** | `gofumpt`, `gci`, `golines` | L5 着手時 |

---

## 8. Deferral 一覧

| ID | 内容 | 理由 |
|----|------|------|
| LM-D01 | golangci 設定 100% 互換 | 最初は `standard` プリセットのみで十分 |
| LM-D02 | `revive` 全ルール | 独自 rule engine のため L4 で独立フェーズ |
| LM-D03 | `gosec` 全ルール | ルール数が多い。カテゴリ単位で段階移植 |
| LM-D04 | `gocritic` 全チェッカ | 同上 |
| LM-D05 | plugin / module プラグイン | golangci v2 plugin は後回し |
| LM-D06 | pure-Rust module loader | PRE-LINTER-PLAN PL02 と同一 |
| LM-D07 | macOS / Windows CI matrix | まず Linux + go list 前提 |
| LM-D08 | stdlib `go list` E2E（`errors` + SA1019） | ✅ 完了（`renameTParams` + `*ppe` addressability） |

---

## 9. セッション記録

| 日付 | 内容 |
|------|------|
| 2026-07-14 | 独立リポジトリ化（`dakimura/guff`）後、`guff run` を実 Go プログラムで安定化。型チェッカ 2 バグ修正: `subst_named` を `instantiate()`（context キャッシュ）経由にし再帰ジェネリック（`type T[P] struct{ next *T[P] }` 系）のスタックオーバーフローを解消 / 型付き符号なし定数の `^`（ビット補数）を型幅でマスク（`1<<(^uintptr(0)>>63)` 等の OOM 解消）。pattern エンジン修正: サブパターン照合結果の破棄バグ + `(Object _)`/`(IntegerLiteral _)` ワイルドカード + pkg 関数シンボル解決（SA4021 等の誤検出解消、SA4009/S1010/S1024/S1028/SA4025 が正確化）。printf（GV-01/18）本番品質化。全ワークスペース 1806 テスト ✅ |
| 2026-07-14 | standard プリセット完走: errcheck 本実装（excludes/blank/assert）、ineffassign（generated 除外 + 追加 testdata）、unused（型・定数・メソッド・const グループ）、staticcheck 137 analyzers ✅ |
| 2026-07-14 | govet 完走: `framepointer`（amd64/arm64 アセンブリ解析、linux/darwin のみ）。govet 29/29 ✅ |
| 2026-07-14 | govet +14: `buildtag`, `cgocall`, `directive`, `ifaceassert`, `loopclosure`, `sigchanyzer`, `slog`, `stdmethods`, `tests`, `timeformat`, `unmarshal`, `unsafeptr`, `httpresponse`, `lostcancel`。共有ヘルパー `govet_util.rs`、各 pass testdata + `checks_test`（55 件）。govet 28/30（残り `framepointer`） |
| 2026-07-14 | govet +3: `composites`（他パッケージ struct の無キー composite literal）、`nilfunc`（関数と nil の比較）、`unreachable`（return 後の到達不能コード）。govet 14/30 |
| 2026-07-13 | govet +2: `copylocks`（lockpath + sync.noCopy）、`printf`（fmt/log 書式検証）。ineffassign: gordonklaus CFG 移植、`uses` マップで変数参照解決。govet 11/30 |
| 2026-07-13 | sa5011 ブロッカー解消: SSA `address` の `StarExpr` 実装。errorsas: `api_assignable_to` で `*ConcreteError` の implements 判定。govet +2: `atomic`, `defers`（stub testdata 付き） |
| 2026-07-13 | LM-D08 完走: `star_expr` addressability、再帰 generic call の `renameTParams`、`load.rs` fset 保持、runner 依存パッケージ fset フォールバック。stdlib E2E `#[ignore]` 解除。L2-a: errorsas/bools/structtag/unusedresult。L2-e: guff-errcheck/ineffassign/unused 骨格 + `guff-lint` standard 5 linter 配線 |
| 2026-07-13 | `guff-lint` CLI 骨格（L1-a–e）: registry、`standard` プリセット、text 診断。`guff-exportdata` ureader 修正（Type 前方宣言順・Func 二重 bind）。stdlib E2E は LM-D08 で継続 |
| 2026-07-13 | 初版作成。standard プリセット 5 linter + 全 linter フェーズ計画 |

---

## 10. クレート命名規約

| パターン | 例 | いつ使う |
|---------|-----|---------|
| `guff-<linter名>` | `guff-errcheck`, `guff-govet` | golangci と 1:1 対応する主要 linter |
| `guff-<upstream名>` | `guff-gostaticanalysis` | 同一 org の小 linter を束ねる |
| `guff-<カテゴリ>` | `guff-style`, `guff-comment` | 小さなスタンドアロン linter 群 |
| `guff-lint` | — | CLI + レジストリ + nolint |
| `guff-fmt` | — | formatter 群 |

**ルール**: 1 クレート = `analyzers() -> Vec<&Analyzer>` を公開。runner への登録は `guff-lint` が行う。

---

## 11. 作業サイクル（1 セッション = 1 タスク）

[`ADDING-ANALYZER.md`](ADDING-ANALYZER.md) と [`STATICCHECK-MIGRATION.md`](STATICCHECK-MIGRATION.md) と同じ:

1. Go ソースを読む（§7 の clone から）
2. `guff-<name>` に `Analyzer` を定義（`requires` は最小限）
3. `tests/testdata/<case>/` に fixture
4. `cargo test -p guff-<name>`
5. `guff-lint` レジストリに追加（L1 完了後）
6. §2 / §9 を更新
