# guff

<p align="center">
  <b>⚡ A blazing-fast golangci-lint compatible Go linter</b>
</p>

<p align="center">
  Run your Go linters in seconds, not minutes.
</p>

<p align="center">
  <img src="assets/demo.gif" alt="golangci-lint run finishes in 22.1s; guff run finishes in 1.7s (helm cold-cache)." width="820" />
</p>

---

## Why guff?

`golangci-lint` is the standard Go linter aggregator — and it is excellent.

But as Go repositories grow, linting becomes one of the slowest parts of the development loop.

Every local change.  
Every pull request.  
Every AI coding agent iteration.

Waiting matters.

**guff makes Go linting fast again.**

```
golangci-lint: 394s
guff:           24s

Same repository.
Same config.
Same findings.
```

---

## 🚀 Performance

Real-world open-source repositories with their existing `golangci-lint v2` configurations:

| Repository | golangci-lint | guff | Speedup |
|---|---:|---:|---:|
| grafana | 394.8s | **23.8s** | **17× faster** |
| helm | 22.1s | **1.7s** | **13× faster** |
| caddy | 10.0s | **0.99s** | **10× faster** |
| gin | 4.2s | **0.38s** | **11× faster** |
| kubernetes | 6.3s | **0.64s** | **10× faster** |

Cold-cache benchmarks on Darwin arm64.

Full benchmark results:

[`benchmarks/results/SCOREBOARD.md`](benchmarks/results/SCOREBOARD.md)

---

## Why is guff fast?

Traditional lint pipelines repeatedly pay the cost of:

- starting processes
- loading packages
- parsing source code
- building analysis state

guff keeps the entire analysis pipeline inside a single Rust process.

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

One pipeline.  
Many analyzers.  
Less waiting.

---

## Drop-in golangci-lint compatibility

Already have a `.golangci.yml`?

Good.

Keep it.

```bash
guff run ./...
```

guff automatically reads:

```
.golangci.yml
.golangci.yaml
.guff.yml
.guff.yaml
```

Compatibility:

- ✅ 114 / 114 golangci-lint v2 linters implemented
- ✅ Existing configurations supported
- ✅ Multiple output formats
- ✅ GitHub Actions annotations

Full compatibility matrix:

[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md)

---

## Built for AI coding agents

AI coding agents run tools constantly.

A slow lint command becomes a slow development loop.

guff is designed for:

- Cursor
- GitHub Copilot
- CI pipelines
- local development

Fast feedback means faster iteration.

---

## Try it now

### Install

```bash
cargo install --git https://github.com/dakimura/guff --locked guff-lint
```

### Run

From your Go module:

```bash
guff run ./...
```

That's it.

Your existing Go lint workflow now runs faster.

---

## Common commands

```bash
# Run configured linters
guff run ./...

# Show enabled linters
guff linters

# Use fast preset
guff run --preset fast ./...

# Enable additional linters
guff run \
  --enable revive \
  --enable misspell \
  ./...

# Run formatters
guff fmt .
```

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

      - uses: dakimura/guff@v0.2.0
        with:
          args: run --out-format=github-actions ./...
```

---

## Docker

guff requires a Go toolchain for package resolution.

The official Docker image already includes Go.

```bash
docker run --rm \
  -v "$PWD":/app \
  -w /app \
  ghcr.io/dakimura/guff:0.2.0 \
  run ./...
```

Optional: persist Go caches between runs:

```bash
docker run --rm \
  -v "$PWD":/app \
  -w /app \
  -v "$(go env GOMODCACHE)":/go/pkg/mod \
  -v "$(go env GOCACHE)":/root/.cache/go-build \
  -e GOMODCACHE=/go/pkg/mod \
  -e GOCACHE=/root/.cache/go-build \
  ghcr.io/dakimura/guff:0.2.0 \
  run ./...
```

---

# Configuration

guff supports existing `golangci-lint` configuration files.

Search order:

```
.golangci.yml
.golangci.yaml
.guff.yml
.guff.yaml
```

Example:

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

Run with:

```bash
guff run .
```

or specify a config:

```bash
guff run -c .golangci.yml .
```

---

# Supported Linters

guff implements the full golangci-lint v2 linter set.

Current compatibility:

```
114 / 114 linters supported
```

Examples:

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

Enable additional linters:

```bash
guff run \
  --enable revive \
  --enable gosec \
  ./...
```

Full matrix:

[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md)

---

# Output formats

Supported formats:

- text
- colored-line-number
- json
- checkstyle
- sarif
- tab
- colored-tab
- github-actions

Example:

```bash
guff run \
  --out-format github-actions \
  ./...
```

GitHub Actions will automatically annotate pull requests.

---

# Architecture

guff is built around one shared analysis pipeline.

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

Unlike traditional linter aggregators, guff avoids repeatedly rebuilding analysis state for every tool.

The result:

- less startup overhead
- lower memory usage
- faster feedback loops

---

# Development

Requirements:

- Go
- Rust (edition 2021)

Build:

```bash
cargo build
```

Test:

```bash
cargo test
```

Run locally:

```bash
cargo run -p guff-lint -- run ./...
```

---

## Benchmarking

Build release binary:

```bash
cargo build --release -p guff-lint
```

Run benchmarks:

```bash
./benchmarks/smoke.sh

./benchmarks/run.sh
```

OSS repository benchmarks:

```bash
./benchmarks/run.sh \
  --oss \
  --tier pr,nightly,weekly
```

Results:

[`benchmarks/results/SCOREBOARD.md`](benchmarks/results/SCOREBOARD.md)

---

## Compatibility Testing

guff continuously compares findings against golangci-lint.

Run compatibility checks:

```bash
./compat/run.sh \
  --oss \
  --tier pr
```

The goal:

> Same config. Same findings. Much faster execution.

---

## Prometheus Regression Gate

guff includes a regression suite against Prometheus.

It checks:

- execution time
- peak RSS memory
- finding differences

Run:

```bash
./regress/run.sh
```

Full profile:

```bash
./regress/run.sh \
  --profile full
```

---

# Source Layout

Cargo workspace structure:

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
└── docs/
```

Main components:

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

# License

GPL-3.0

guff includes ports and adaptations of analyzers from multiple upstream Go projects.

See:

- [`LICENSE`](LICENSE)
- [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md)

for attribution and license information.
