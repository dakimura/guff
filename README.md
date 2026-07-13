# guff

Go 向けマルチ linter を **1 本の解析パイプライン**で動かす Rust 実装です。  
golangci-lint v2 の `standard` プリセット相当（staticcheck / govet / errcheck / ineffassign / unused）を、パッケージロード → 型チェック → `go/analysis` 実行まで一度にこなします。

CLI バイナリ名は **`guff`** です（クレート名は `guff-lint`）。

## 必要条件

| ツール | 用途 |
|--------|------|
| [Rust](https://rustup.rs/) (edition 2021) | `guff` のビルド |
| [Go](https://go.dev/dl/) | 解析対象のパッケージ解決（内部で `go list` を利用） |

## インストール

次のいずれかで入れます。

### 方法 A: `cargo install --git`（clone 不要）

```bash
cargo install --git https://github.com/dakimura/guff --locked guff-lint
```

### 方法 B: clone してローカルからインストール（推奨）

```bash
git clone https://github.com/dakimura/guff.git
cd guff
cargo install --path crates/guff-lint --locked
```

いずれも `~/.cargo/bin/guff` が入ります。`PATH` に `~/.cargo/bin` が入っていることを確認してください。

```bash
guff --help
```

### 方法 C: リリース用にバイナリだけビルド

```bash
cargo build --release -p guff-lint
# 成果物: target/release/guff
cp target/release/guff /usr/local/bin/   # 好みの場所へ
```

### 方法 D: ソースから都度実行

```bash
cargo run -p guff-lint -- run ./...
```

## 使い方

### 基本

Go モジュールのルートで:

```bash
guff run .
# または
guff run ./...
```

設定ファイルがなければ、golangci-lint v2 の **`standard`** プリセット（上記 5 linter）が有効になります。

### 設定ファイル

カレントから親ディレクトリへ向かって次を探します。

- `.golangci.yml` / `.golangci.yaml`
- `.guff.yml` / `.guff.yaml`

golangci-lint v1 / v2 の YAML のサブセットを読めます。明示する場合:

```bash
guff run -c .golangci.yml .
guff run --no-config .          # 設定を無視
```

### CLI で linter を切り替える

```bash
# プリセット（standard / fast / all / none）
guff run --preset standard .
guff run --preset fast .

# 追加・除外（繰り返し可）
guff run --enable staticcheck --disable unused .
```

### 設定の v1 → v2 移行

```bash
guff migrate
guff migrate -c .golangci.yml
```

## 対応 linter

| 名前 | 内容 |
|------|------|
| `staticcheck` | Staticcheck / simple 系ルール |
| `govet` | `go vet` 相当の解析 |
| `errcheck` | 未チェックの error 戻り値 |
| `ineffassign` | 無効な代入 |
| `unused` | 未使用のパッケージレベル定義 |

プリセット:

- **`standard`** — 上記すべて（デフォルト）
- **`fast`** — `standard` から `staticcheck` を除いたもの

## アーキテクチャ（ざっくり）

```
go list (guff-packages)
  → typecheck（ソース / export data）
  → Pass (guff-analysis)
  → action graph (guff-runner)   ← 全 linter を 1 DAG で実行
  → Diagnostic
       ↑
  guff CLI（設定・有効化・表示）
```

クレート構成の詳細は下の「ソース構成」を参照してください。

## 開発

```bash
# ワークスペース全体
cargo build
cargo test

# CLI だけ
cargo test -p guff-lint
cargo run -p guff-lint -- run ./path/to/go/module
```

開発ガイド・アーキテクチャ・現状・残タスク（golangci-lint 互換の高速 linter へのロードマップ）・
analyzer 追加手順は、すべて 1 本にまとめてあります:

- [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) — **開発の唯一の正典**

> 以前分かれていた `MIGRATION.md` / `PRE-LINTER-PLAN.md` / `docs/LINTER-MIGRATION.md` /
> `docs/STATICCHECK-MIGRATION.md` / `docs/ADDING-ANALYZER.md` / `projects/guff-ssa-MIGRATION.md`
> は上記に統合しました（原文は git 履歴に残っています）。

## ライセンス

BSD-3-Clause（各クレートの移植元 Go ツールチェーン / go-tools のライセンス方針に合わせています）。

---

## ソース構成

Cargo workspace。バイナリは `guff` のみで、あとはライブラリクレートです。

### レイヤ俯瞰

| 層 | クレート | 役割（Go 相当） |
|----|----------|-----------------|
| **CLI** | `guff-lint` (`bin: guff`) | 設定・linter 選択・診断表示 |
| **Linters** | `guff-staticcheck`, `guff-govet`, `guff-errcheck`, `guff-ineffassign`, `guff-unused` | 各 linter の Analyzer 群 |
| **Driver** | `guff-runner` | Analyzer の DAG 実行（並列） |
| **Framework** | `guff-analysis`, `guff-pattern` | `go/analysis` + Staticcheck のパターン DSL |
| **SSA** | `guff-ssa` | `go/ssa` |
| **Load / types** | `guff-packages`, `guff-build`, `guff-exportdata`, `guff-types`, `guff-constant` | パッケージロード・型検査・export data |
| **AST** | `guff-ast` | `go/token` / `scanner` / `ast` / `parser` |
| **Version helpers** | `guff-version`, `guff-gover`, `guff-goversion`, `guff-types-errors` | Go バージョン・型エラーコード |

依存の流れ（下から上へ）:

```
guff-ast / guff-constant / guff-version*
        ↓
guff-types ← guff-exportdata
        ↓
guff-build → guff-packages
        ↓
guff-ssa / guff-pattern / guff-analysis
        ↓
guff-runner
        ↓
guff-{staticcheck,govet,errcheck,ineffassign,unused}
        ↓
guff-lint  (bin: guff)
```

### ディレクトリ

```
guff/
├── Cargo.toml              # workspace 定義
├── crates/
│   ├── guff-lint/          # CLI エントリ (bin: guff)
│   ├── guff-runner/        # 解析ドライバ
│   ├── guff-analysis/      # Analyzer / Pass / Diagnostic
│   ├── guff-packages/      # go list + ロード
│   ├── guff-types/         # 型チェッカ
│   ├── guff-ast/           # 字句・構文・AST
│   ├── guff-ssa/           # SSA
│   ├── guff-staticcheck/   # …
│   └── …
└── docs/                   # 移植メモ・開発ガイド
```
