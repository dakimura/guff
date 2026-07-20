# guff

**A Rust-native Go multi-linter** — one analysis pipeline, golangci-lint–compatible configs, built for fast local loops and AI agent sandboxes.

The CLI binary is **`guff`** (crate name: `guff-lint`).

---

## Why guff (vs golangci-lint)

Agents and CI run linters constantly. Wall-clock and peak memory dominate the feedback loop. guff keeps a single Rust process over package load → typecheck → analyzers, instead of orchestrating many Go tools.

**Cold runs are faster than golangci-lint** on prometheus’s own `.golangci.yml`
(~20 analyzers + formatters; empty tool caches **and** empty `GOCACHE` — the
agent / CI sandbox case). Hybrid dependency type-checking is on by default:

| Target | guff cold | golangci-lint cold | guff is |
|--------|----------:|-------------------:|--------:|
| prometheus `./tsdb/...` | **38.9s** | 54.6s | **1.4× faster** |

Medians of 3 samples (Darwin arm64, Go 1.26, golangci-lint 2.12, auto `-j`).
With a warm `GOCACHE` the gap shrinks (both skip export compiles). Full tables:
[`benchmarks/results/RESULTS.md`](benchmarks/results/RESULTS.md).

**Memory stays in the single-digit GB range** on large trees after arena / export-seed work (Prometheus full `./...`: ~**5.8 GB** peak RSS, down from ~56 GB). Smaller scopes (e.g. Prometheus `./tsdb/...`) land around **~1.8 GB**. Details: [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md), [`regress/`](regress/).

Drop in your existing `.golangci.yml` — **108 / 114** golangci-lint v2 linters are implemented. Matrix: [`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md).

---

## Getting Started

### Requirements

| Tool | Why |
|------|-----|
| [Rust](https://rustup.rs/) (edition 2021) | Build `guff` |
| [Go](https://go.dev/dl/) | Package resolution (`go list`) |

### Installation

**A. `cargo install --git` (no clone)**

```bash
cargo install --git https://github.com/dakimura/guff --locked guff-lint
```

**B. Clone and install (recommended)**

```bash
git clone https://github.com/dakimura/guff.git
cd guff
cargo install --path crates/guff-lint --locked
```

Either way installs `~/.cargo/bin/guff`. Ensure `~/.cargo/bin` is on your `PATH`.

```bash
guff --help
```

**C. Release binary only**

```bash
cargo build --release -p guff-lint
# artifact: target/release/guff
```

**D. Run from source**

```bash
cargo run -p guff-lint -- run ./...
```

### Usage

From a Go module root:

```bash
guff run .
# or
guff run ./...
```

With no config file, the golangci-lint v2 **`standard`** preset is enabled (five linters above).

```bash
# List what is on / off for the current config
guff linters

# Presets: standard | fast | all | none
guff run --preset standard .
guff run --preset fast .

# Enable / disable by name (repeatable)
guff run --enable misspell --enable revive --disable unused .

# Ignore discovered config
guff run --no-config .

# Migrate golangci-lint v1 YAML → v2
guff migrate
guff migrate -c .golangci.yml
```

Formatters (same six as golangci-lint v2 `formatters`):

```bash
guff fmt .
guff fmt --enable gofumpt --enable goimports .
```

### Configuration

guff walks from the current directory upward looking for:

- `.golangci.yml` / `.golangci.yaml`
- `.guff.yml` / `.guff.yaml`

It accepts a subset of golangci-lint v1 / v2 YAML. Point at a file explicitly with `-c`:

```bash
guff run -c .golangci.yml .
```

Minimal v2 example:

```yaml
version: "2"

linters:
  default: standard
  enable:
    - misspell
    - revive
  disable:
    - unused
  settings:
    errcheck:
      check-blank: true
    misspell:
      locale: US

formatters:
  enable:
    - gofmt
    - goimports

output:
  formats:
    text:
      path: stdout
```

`default` presets:

| Preset | Linters |
|--------|---------|
| `standard` | `staticcheck`, `govet`, `errcheck`, `ineffassign`, `unused` (default) |
| `fast` | `standard` without `staticcheck` |
| `all` | same base set as `standard` (enable extras via `enable` / `--enable`) |
| `none` | nothing until you `enable` |

Which keys / formats are supported: [`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md).

### Linter rules

**Default (`standard`)** — same five families as golangci-lint v2:

| Name | What it covers |
|------|----------------|
| `staticcheck` | Staticcheck / simple / style / quickfix (S\* / SA\* / ST\* / QF\* — **167** analyzers) |
| `govet` | `go vet` passes (**29/29**) |
| `errcheck` | Unchecked `error` returns |
| `ineffassign` | Ineffectual assignments |
| `unused` | Unused package-level declarations |

**Beyond the default:** **108 / 114** golangci-lint v2 linters are implemented (enable with `--enable <name>` or config). Examples: `revive`, `gosec`, `misspell`, `gocritic`, `errname`, `bodyclose`, `dupl`, …

```bash
guff linters --no-config          # standard five on; others listed as disabled
guff linters --enable revive      # see revive in the enabled list
```

Full ✅ / 🟡 / ❌ matrix: [`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md).

### Output formats

`--out-format`: `text` (`line-number`), `colored-line-number`, `json`, `checkstyle`, `sarif`, `tab`, `colored-tab`, `github-actions`.

Use `format:path` to write files or emit several formats at once.

---

## Architecture (short)

```
go list (guff-packages)
  → typecheck (source / export data)
  → Pass (guff-analysis)
  → action graph (guff-runner)   ← all linters in one DAG
  → Diagnostic
       ↑
  guff CLI (config, enablement, printers)
```

---

## Development

```bash
cargo build
cargo test

cargo test -p guff-lint
cargo run -p guff-lint -- run ./path/to/go/module
```

Canonical guide (architecture, status, roadmap): [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md).

### Benchmarks (vs golangci-lint)

```bash
cargo build --release -p guff-lint
./benchmarks/smoke.sh          # offline smoke
./benchmarks/run.sh            # fixture + local corpus
```

Latest numbers: [`benchmarks/results/RESULTS.md`](benchmarks/results/RESULTS.md). Harness: [`benchmarks/README.md`](benchmarks/README.md).

### Prometheus regression gate (local)

Checks wall time, peak RSS, and finding-set delta vs golangci-lint against `baseline.json` (uses prometheus’s own `.golangci.yml`). Default scope targets ~24 GB machines (`./tsdb/...`, auto concurrency, warm `GOCACHE`, 12 GiB RSS cap):

```bash
./regress/run.sh --update-baseline   # first time / intentional baseline bump
./regress/run.sh                     # gate
```

Details: [`regress/README.md`](regress/README.md).

---

## License

**GPL-3.0** — see [`LICENSE`](LICENSE).

guff ports analyzers from many upstream Go projects (mostly MIT / Apache-2.0 / BSD-3-Clause) and a few that are **GPL-3.0**. Those GPL-derived analyzers are linked into the single `guff` binary, so the distributed work is GPL-3.0.

Upstream attributions and original licenses: [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md).

Go stdlib / `x/tools`–derived crates additionally retain BSD-3-Clause notices (e.g. [`crates/guff-ast/LICENSE`](crates/guff-ast/LICENSE)).

---

## Source layout

Cargo workspace. One binary (`guff`); everything else is library crates.

| Layer | Crates | Role |
|-------|--------|------|
| **CLI** | `guff-lint` (`bin: guff`) | Config, linter selection, printers, `migrate` |
| **Linters** | `guff-staticcheck`, `guff-govet`, `guff-errcheck`, `guff-ineffassign`, `guff-unused`, `guff-gostaticanalysis`, `guff-error`, `guff-context`, `guff-style`, `guff-comment`, `guff-import`, `guff-misspell`, `guff-dupl`, `guff-revive` | Analyzer bundles |
| **Formatters** | `guff-fmt` | `guff fmt` |
| **Driver** | `guff-runner` | Analyzer DAG (parallel) |
| **Framework** | `guff-analysis`, `guff-pattern` | `go/analysis` + Staticcheck pattern DSL |
| **SSA** | `guff-ssa` | `go/ssa` |
| **Load / types** | `guff-packages`, `guff-build`, `guff-exportdata`, `guff-types`, `guff-constant` | Load, typecheck, export data |
| **AST** | `guff-ast` | `go/token` / scanner / ast / parser |

```
guff/
├── Cargo.toml
├── LICENSE
├── THIRD_PARTY_LICENSES.md
├── benchmarks/             # vs golangci-lint wall-clock harness
├── compat/                 # finding-set diff harness
├── regress/                # prometheus RSS / wall / finding-set gate
├── crates/
│   ├── guff-lint/          # CLI (bin: guff)
│   ├── guff-runner/
│   ├── guff-analysis/
│   ├── guff-packages/
│   ├── guff-types/
│   ├── guff-ast/
│   ├── guff-ssa/
│   └── …
└── docs/                   # DEVELOPMENT.md, COMPATIBILITY.md, …
```
