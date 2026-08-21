# guff

<p align="center">
  <a href="README.md">English</a> |
  <a href="README.zh-CN.md">简体中文</a> |
  <a href="README.es.md">Español</a> |
  <a href="README.pt-BR.md">Português (Brasil)</a> |
  <a href="README.ja.md">日本語</a>
</p>

<p align="center">
  <b>⚡ 兼容 golangci-lint 的极速 Go 检查器</b>
</p>

<p align="center">
  让 Go lint 在数秒内完成，而不是数分钟。
</p>

<p align="center">
  <img src="assets/demo.gif" alt="golangci-lint 用时 22.1s；guff 用时 1.7s（helm，冷缓存）。" width="820" />
</p>

<p align="center">
  <a href="docs/MIGRATION.md">5 分钟迁移</a>
  ·
  <a href="docs/INSTALL.md">安装 / 卸载</a>
  ·
  <a href="docs/COMPARE.md">对比 golangci-lint</a>
  ·
  <a href="docs/AGENTS.md">AI 编程代理</a>
</p>

---

## 为什么选择 guff？

`golangci-lint` 是 Go 生态中标准的 linter 聚合器——而且非常出色。

但随着仓库变大，lint 往往成为开发循环中最慢的一环。

每次本地改动。  
每次拉取请求。  
每次 AI 编程代理迭代。

等待成本很高。

**guff 让 Go lint 重新变快。**

```
golangci-lint: 280s
guff:            20s

同一仓库。
同一配置。
同一结果。
```

---

## 🚀 性能

在真实开源仓库上，使用其现有的 `golangci-lint v2` 配置：

| Repository | golangci-lint | guff | Speedup |
|---|---:|---:|---:|
| grafana | 279.8s | **19.8s** | **14× faster** |
| consul | 38.0s | **5.2s** | **7× faster** |
| helm | 17.5s | **1.4s** | **13× faster** |
| k9s | 14.6s | **2.2s** | **7× faster** |
| caddy | 9.1s | **0.85s** | **11× faster** |
| containerd | 5.2s | **0.37s** | **14× faster** |
| gin | 3.9s | **0.38s** | **10× faster** |
| cobra | 1.4s | **0.23s** | **6× faster** |

Darwin arm64 冷缓存基准。

完整结果：

[`benchmarks/results/SCOREBOARD.md`](benchmarks/results/SCOREBOARD.md)

---

## 为什么这么快？

传统 lint 流水线会反复付出：

- 启动进程
- 加载包
- 解析源码
- 构建分析状态

guff 将整条分析流水线放在 **单个 Rust 进程** 中。

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

一条流水线。  
多个分析器。  
更少等待。

---

## 可直接替换的 golangci-lint 兼容性

已经有 `.golangci.yml`？

很好。

继续用。

```bash
guff run ./...
```

guff 会自动读取：

```
.golangci.yml
.golangci.yaml
.guff.yml
.guff.yaml
```

兼容性：

- ✅ 已实现 golangci-lint v2 的 114 / 114 个 linter
- ✅ 支持现有配置
- ✅ 多种输出格式
- ✅ GitHub Actions 注解

诚实对比（含已知部分差距）：

[`docs/COMPARE.md`](docs/COMPARE.md)

完整兼容矩阵：

[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md)

五分钟迁移与回滚：

[`docs/MIGRATION.md`](docs/MIGRATION.md)

---

## 为 AI 编程代理而生

AI 编程代理会频繁调用工具。

一次缓慢的 lint，就会拖慢整个开发循环。

guff 面向：

- Claude Code
- Cursor
- GitHub Copilot
- CI 流水线
- 本地开发

可复制的代理说明：[`docs/AGENTS.md`](docs/AGENTS.md) —— Claude Code 放进 `CLAUDE.md`，Cursor 放进 `.cursor/rules`，其他代理放进系统提示词。

---

## 立即试用

### 安装（无需 Rust）

```bash
curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/install.sh | sh
```

默认安装到 `~/.local/bin`。然后：

```bash
guff run ./...
```

就这样。现有的 `.golangci.yml` 可以直接使用。

其他安装方式：

```bash
# Homebrew
brew tap dakimura/guff https://github.com/dakimura/guff
brew install guff
```

Docker、aqua、Actions、cargo：[`docs/INSTALL.md`](docs/INSTALL.md)

### 卸载 / 回滚

```bash
curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/uninstall.sh | sh
```

不会改动配置——随时把 CI 切回 golangci-lint。详情：[`docs/INSTALL.md`](docs/INSTALL.md#uninstall--rollback)

---

## 常用命令

```bash
# 按配置运行
guff run ./...

# 显示已启用的 linter
guff linters

# 使用 fast 预设
guff run --preset fast ./...

# 额外启用 linter
guff run \
  --enable revive \
  --enable misspell \
  ./...

# 应用建议修复
guff run --fix ./...

# 格式化（来自配置的 gofmt / goimports / …）
guff fmt .

# 监视变更并重新 lint（保持分析状态温热）
guff run --watch ./...

# issues 缓存
guff cache status
guff cache clean
```

编辑器、pre-commit、lefthook：[`docs/EDITORS.md`](docs/EDITORS.md)

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

      - uses: dakimura/guff@v0.6.0
        with:
          args: run --out-format=github-actions ./...
```

该 Action 会在多次运行之间保留 guff 的分析缓存，因此 pull request 只重新检查改
动过的部分。在 GitHub 托管 runner 上对 113 个包的模块实测：冷启动 7.9 秒，无改
动 0.2 秒，修改一个被广泛引用的文件后 4.2 秒。矩阵构建、缓存体积与自托管 runner
请见 [`docs/CI.md`](docs/CI.md)。

---

## Docker

包解析需要 Go 工具链。

官方 Docker 镜像已包含 Go。

```bash
docker run --rm \
  -v "$PWD":/app \
  -w /app \
  ghcr.io/dakimura/guff:0.6.0 \
  run ./...
```

可选：在多次运行之间持久化 Go 缓存：

```bash
docker run --rm \
  -v "$PWD":/app \
  -w /app \
  -v "$(go env GOMODCACHE)":/go/pkg/mod \
  -v "$(go env GOCACHE)":/root/.cache/go-build \
  -e GOMODCACHE=/go/pkg/mod \
  -e GOCACHE=/root/.cache/go-build \
  ghcr.io/dakimura/guff:0.6.0 \
  run ./...
```

---

# 配置

guff 支持现有的 `golangci-lint` 配置文件。

搜索顺序：

```
.golangci.yml
.golangci.yaml
.guff.yml
.guff.yaml
```

示例：

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

运行：

```bash
guff run .
```

或指定配置：

```bash
guff run -c .golangci.yml .
```

v1 → v2：`guff migrate`

---

# 支持的 Linter

guff 实现了完整的 golangci-lint v2 linter 集合。

当前兼容性：

```
114 / 114 linters supported
```

示例：

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

额外启用：

```bash
guff run \
  --enable revive \
  --enable gosec \
  ./...
```

完整矩阵：

[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md)

---

# 输出格式

支持的格式：

- text
- colored-line-number
- json
- checkstyle
- sarif
- tab
- colored-tab
- github-actions

示例：

```bash
guff run \
  --out-format github-actions \
  ./...
```

GitHub Actions 会自动为拉取请求添加注解。

---

# 架构

guff 围绕一条共享分析流水线构建。

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

与传统 linter 聚合器不同，guff 避免为每个工具反复重建分析状态。

效果：

- 更少的启动开销
- 更低的内存占用
- 更快的反馈循环

---

# 开发

依赖：

- Go
- Rust（edition 2021）

构建：

```bash
cargo build
```

测试：

```bash
cargo test
```

本地运行：

```bash
cargo run -p guff-lint -- run ./...
```

---

## 基准测试

构建 release 二进制：

```bash
cargo build --release -p guff-lint
```

运行基准：

```bash
./benchmarks/smoke.sh

./benchmarks/run.sh
```

开源仓库基准：

```bash
./benchmarks/run.sh \
  --oss \
  --tier pr,nightly,weekly
```

结果：

[`benchmarks/results/SCOREBOARD.md`](benchmarks/results/SCOREBOARD.md)

---

## 兼容性测试

guff 持续将结果与 golangci-lint 对比。

```bash
./compat/run.sh \
  --oss \
  --tier pr
```

按 linter 隔离（一次只启用一个）：

```bash
./compat/run.sh --isolate --smoke
./compat/run.sh --isolate
```

目标：

> 同一配置。同一结果。快得多的执行。

---

## Prometheus 回归门禁

guff 包含针对 Prometheus 的回归套件。

检查：

- 执行时间
- peak RSS 内存
- 结果差异

```bash
./regress/run.sh
```

完整配置：

```bash
./regress/run.sh \
  --profile full
```

---

# 源码结构

Cargo workspace：

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

主要组件：

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

# 许可证

GPL-3.0

在 CI 或本地使用 `guff` CLI **不会**让你的 Go 应用变成 GPL。详情：[`docs/LICENSE-FAQ.md`](docs/LICENSE-FAQ.md)

发布校验 / SBOM：[`docs/SUPPLY-CHAIN.md`](docs/SUPPLY-CHAIN.md)

guff 包含来自多个上游 Go 项目的分析器移植与适配。

参见：

- [`LICENSE`](LICENSE)
- [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md)
