# guff

<p align="center">
  <a href="README.md">English</a> |
  <a href="README.zh-CN.md">简体中文</a> |
  <a href="README.es.md">Español</a> |
  <a href="README.pt-BR.md">Português (Brasil)</a> |
  <a href="README.ja.md">日本語</a>
</p>

<p align="center">
  <b>⚡ golangci-lint 互換の超高速 Go リンター</b>
</p>

<p align="center">
  Go の lint を、分単位ではなく秒単位で。
</p>

<p align="center">
  <img src="assets/demo.gif" alt="golangci-lint は 22.1s、guff は 1.7s（helm・コールドキャッシュ）。" width="820" />
</p>

<p align="center">
  <a href="docs/MIGRATION.md">5 分で移行</a>
  ·
  <a href="docs/INSTALL.md">インストール / アンインストール</a>
  ·
  <a href="docs/COMPARE.md">golangci-lint との比較</a>
  ·
  <a href="docs/AGENTS.md">AI エージェント向け</a>
</p>

---

## なぜ guff？

`golangci-lint` は Go の標準的なリンター集約ツールであり、優れたツールです。

しかしリポジトリが大きくなると、lint は開発ループで最も遅い工程のひとつになります。

ローカルの変更のたび。  
プルリクエストのたび。  
AI コーディングエージェントの反復のたび。

待ち時間は効いてきます。

**guff は、Go の lint を再び速くします。**

```
golangci-lint: 394s
guff:           24s

同じリポジトリ。
同じ設定。
同じ指摘。
```

---

## 🚀 パフォーマンス

既存の `golangci-lint v2` 設定を使った実 OSS リポジトリの計測:

| Repository | golangci-lint | guff | Speedup |
|---|---:|---:|---:|
| grafana | 394.8s | **23.8s** | **17× faster** |
| helm | 22.1s | **1.7s** | **13× faster** |
| caddy | 10.0s | **0.99s** | **10× faster** |
| gin | 4.2s | **0.38s** | **11× faster** |
| rclone | 6.3s | **0.64s** | **10× faster** |

Darwin arm64・コールドキャッシュ。

詳細:

[`benchmarks/results/SCOREBOARD.md`](benchmarks/results/SCOREBOARD.md)

---

## なぜ速いのか？

従来の lint パイプラインは、毎回次のコストを払い続けます:

- プロセス起動
- パッケージ読み込み
- ソースのパース
- 解析状態の構築

guff は解析パイプライン全体を **ひとつの Rust プロセス** に収めます。

```
Go source
   |
   v
Package loading
   |
   v
Type checking
   |
   v
Shared analysis pipeline
   |
   v
All linters
```

ひとつのパイプライン。  
多数のアナライザ。  
待ち時間は少なく。

---

## golangci-lint のドロップイン互換

すでに `.golangci.yml` がありますか？

そのまま使えます。

```bash
guff run ./...
```

guff が自動で読むファイル:

```
.golangci.yml
.golangci.yaml
.guff.yml
.guff.yaml
```

互換性:

- ✅ golangci-lint v2 の 114 / 114 リンターを実装
- ✅ 既存設定に対応
- ✅ 複数の出力フォーマット
- ✅ GitHub Actions アノテーション

正直な比較（既知の部分差分を含む）:

[`docs/COMPARE.md`](docs/COMPARE.md)

互換マトリクス:

[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md)

5 分移行とロールバック:

[`docs/MIGRATION.md`](docs/MIGRATION.md)

---

## AI コーディングエージェント向け

エージェントはツールを何度も回します。

遅い lint は、そのまま遅い開発ループになります。

guff は次のために設計されています:

- Cursor
- GitHub Copilot
- CI パイプライン
- ローカル開発

エージェント向けの定型文: [`docs/AGENTS.md`](docs/AGENTS.md)

---

## 今すぐ試す

### インストール（Rust 不要）

```bash
curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/install.sh | sh
```

既定では `~/.local/bin` に入ります。続けて:

```bash
guff run ./...
```

以上です。既存の `.golangci.yml` がそのままで動きます。

その他の入れ方:

```bash
# Homebrew
brew tap dakimura/guff https://github.com/dakimura/guff
brew install guff
```

Docker / aqua / Actions / cargo: [`docs/INSTALL.md`](docs/INSTALL.md)

### アンインストール / ロールバック

```bash
curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/uninstall.sh | sh
```

設定ファイルは触りません。いつでも CI を golangci-lint に戻せます。詳細: [`docs/INSTALL.md`](docs/INSTALL.md#uninstall--rollback)

---

## よく使うコマンド

```bash
# 設定どおりに実行
guff run ./...

# 有効なリンター一覧
guff linters

# fast プリセット
guff run --preset fast ./...

# 追加で有効化
guff run \
  --enable revive \
  --enable misspell \
  ./...

# 提案された修正を適用
guff run --fix ./...

# フォーマッタ（設定の gofmt / goimports / …）
guff fmt .

# 変更を監視して再 lint（解析を温存）
guff run --watch ./...

# issues キャッシュ
guff cache status
guff cache clean
```

エディタ / pre-commit / lefthook: [`docs/EDITORS.md`](docs/EDITORS.md)

---

## GitHub Actions

```yaml
name: lint

on:
  pull_request:

jobs:
  guff:
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-go@v5
        with:
          go-version: stable

      - uses: dakimura/guff@v0.3.0
        with:
          args: run --out-format=github-actions ./...
```

---

## Docker

パッケージ解決には Go ツールチェーンが必要です。

公式 Docker イメージには Go が入っています。

```bash
docker run --rm \
  -v "$PWD":/app \
  -w /app \
  ghcr.io/dakimura/guff:0.3.0 \
  run ./...
```

任意: 実行間で Go キャッシュを共有:

```bash
docker run --rm \
  -v "$PWD":/app \
  -w /app \
  -v "$(go env GOMODCACHE)":/go/pkg/mod \
  -v "$(go env GOCACHE)":/root/.cache/go-build \
  -e GOMODCACHE=/go/pkg/mod \
  -e GOCACHE=/root/.cache/go-build \
  ghcr.io/dakimura/guff:0.3.0 \
  run ./...
```

---

# 設定

既存の `golangci-lint` 設定ファイルに対応しています。

探索順:

```
.golangci.yml
.golangci.yaml
.guff.yml
.guff.yaml
```

例:

```yaml
version: "2"

linters:
  default: standard

  enable:
    - revive
    - misspell

  disable:
    - unused

  settings:
    errcheck:
      check-blank: true

formatters:
  enable:
    - gofmt
    - goimports
```

実行:

```bash
guff run .
```

設定を指定:

```bash
guff run -c .golangci.yml .
```

v1 → v2: `guff migrate`

---

# 対応リンター

guff は golangci-lint v2 のリンター一式を実装しています。

現状:

```
114 / 114 linters supported
```

例:

| Linter | Description |
|---|---|
| staticcheck | Static analysis suite |
| govet | Go vet analyzers |
| errcheck | Unchecked errors |
| ineffassign | Ineffectual assignments |
| unused | Unused declarations |
| revive | Go style checker |
| gosec | Security checks |
| misspell | Spelling mistakes |
| gocritic | Code quality checks |
| dupl | Duplicate code detection |

追加で有効化:

```bash
guff run \
  --enable revive \
  --enable gosec \
  ./...
```

マトリクス全体:

[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md)

---

# 出力フォーマット

対応フォーマット:

- text
- colored-line-number
- json
- checkstyle
- sarif
- tab
- colored-tab
- github-actions

例:

```bash
guff run \
  --out-format github-actions \
  ./...
```

GitHub Actions が PR に自動アノテーションします。

---

# アーキテクチャ

guff は共有の解析パイプラインを中心に構成されています。

```
go list
  |
  v
Package loading
  |
  v
Type checking
  |
  v
Analysis passes
  |
  v
Dependency-aware execution graph
  |
  v
Diagnostics
```

従来のリンター集約と異なり、ツールごとに解析状態を作り直すことを避けます。

結果として:

- 起動オーバーヘッドが小さい
- メモリ使用量が低い
- フィードバックが速い

---

# 開発

必要環境:

- Go
- Rust（edition 2021）

ビルド:

```bash
cargo build
```

テスト:

```bash
cargo test
```

ローカル実行:

```bash
cargo run -p guff-lint -- run ./...
```

---

## ベンチマーク

リリースビルド:

```bash
cargo build --release -p guff-lint
```

ベンチマーク実行:

```bash
./benchmarks/smoke.sh

./benchmarks/run.sh
```

OSS リポジトリベンチマーク:

```bash
./benchmarks/run.sh \
  --oss \
  --tier pr,nightly,weekly
```

結果:

[`benchmarks/results/SCOREBOARD.md`](benchmarks/results/SCOREBOARD.md)

---

## 互換性テスト

guff は golangci-lint との指摘差分を継続的に比較します。

```bash
./compat/run.sh \
  --oss \
  --tier pr
```

リンター単位の isolate（1 つずつ有効化）:

```bash
./compat/run.sh --isolate --smoke
./compat/run.sh --isolate
```

目標:

> 同じ設定。同じ指摘。はるかに速い実行。

---

## Prometheus リグレッションゲート

Prometheus に対するリグレッションスイートがあります。

確認内容:

- 実行時間
- peak RSS
- 指摘差分

```bash
./regress/run.sh
```

フルプロファイル:

```bash
./regress/run.sh \
  --profile full
```

---

# ソース構成

Cargo ワークスペース:

```
guff/
├── crates/
│   ├── guff-lint/
│   ├── guff-runner/
│   ├── guff-analysis/
│   ├── guff-packages/
│   ├── guff-types/
│   ├── guff-ast/
│   ├── guff-ssa/
│   └── ...
│
├── benchmarks/
├── compat/
├── regress/
├── packaging/          # aqua registry draft
├── Formula/            # Homebrew tap formula
└── docs/
```

主な層:

| Layer | Responsibility |
|---|---|
| CLI | Config, commands, output |
| Runner | Parallel analyzer execution |
| Analysis | Shared analysis framework |
| Packages | Go package loading |
| Types | Type checking |
| SSA | Go SSA implementation |
| AST | Go parser/token support |

---

# ライセンス

GPL-3.0

CI やローカルで `guff` CLI を使うだけでは、あなたの Go アプリは GPL にはなりません。詳細: [`docs/LICENSE-FAQ.md`](docs/LICENSE-FAQ.md)

リリース検証 / SBOM: [`docs/SUPPLY-CHAIN.md`](docs/SUPPLY-CHAIN.md)

guff は複数の upstream Go プロジェクトからアナライザを移植・適応しています。

- [`LICENSE`](LICENSE)
- [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md)
