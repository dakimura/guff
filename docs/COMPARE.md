# guff vs golangci-lint (honest comparison)

**Tagline:** Same config. Same findings (with documented gaps). Much faster.

guff is a Rust-native reimplementation of the golangci-lint v2 analyzer set. It reads `.golangci.yml` and aims for finding-set parity, not a new lint philosophy.

## Headline numbers

Cold-cache wall time on Darwin arm64 (see [`benchmarks/results/SCOREBOARD.md`](../benchmarks/results/SCOREBOARD.md)):

| Repository | golangci-lint | guff | Speedup |
|---|---:|---:|---:|
| grafana | 290.4s | 17.8s | 16× |
| consul | 39.4s | 4.7s | 8× |
| helm | 17.4s | 1.3s | 13× |
| caddy | 8.7s | 0.91s | 10× |

## Compatibility snapshot

| Area | Status |
|---|---|
| golangci-lint v2 linters | **114 / 114 implemented** (97 full ✅, 17 partial 🟡) |
| Config files | `.golangci.yml` / `.guff.yml` (v2; v1 via `guff migrate`) |
| Output formats | text, json, checkstyle, sarif, github-actions, … |
| Formatters | gofmt, gofumpt, goimports, gci, golines, swaggo |
| Autofix | `--fix` where SuggestedFix exists (coverage still growing) |
| Cache / watch | package issues cache, `guff run --watch` |

Full matrix (detailed, JP): [COMPATIBILITY.md](COMPATIBILITY.md).

## Partial (🟡) areas worth knowing

These ship, but can diverge from golangci-lint on edge cases:

| Linter / area | Why it may differ |
|---|---|
| bodyclose, rowserrcheck, sqlclosecheck, spancheck | AST approximation; SSA/ctrlflow parity deferred |
| contextcheck | cross-package HTTP handler facts deferred |
| gosec | some G* rules deferred |
| revive | large rule set; some rules not implemented |
| unused | single-package; whole-program deferred |
| depguard / gomod* / errorlint / … | subset of settings deferred |
| comment directives (`//gocyclo:ignore`, …) | often deferred; `//nolint` works at runner layer |

**Prefer fixing guff over silencing diffs** when adopting. If a gap blocks you, open an issue with a minimal repro.

## When guff is the better default

- CI or local lint is a multi-minute wait.
- AI coding agents re-run lint constantly ([AGENTS.md](AGENTS.md)).
- You already have a golangci-lint v2 config and want drop-in speed.

## When to keep golangci-lint

- You need an upstream-only plugin or behavior marked ❌ / blocking 🟡 in the matrix.
- Your org’s security process has not yet accepted guff’s release artifacts ([SUPPLY-CHAIN.md](SUPPLY-CHAIN.md)).
- You are debugging a parity bug and need a known-good oracle (this is also how guff’s `compat/` suite works).

## Migration

Five-minute path: [MIGRATION.md](MIGRATION.md). Rollback: [INSTALL.md](INSTALL.md#uninstall--rollback).

## License note

guff is **GPL-3.0**. Using the CLI in CI/local does not GPL your Go code. Linking guff as a library into a proprietary binary is a different question — see [LICENSE-FAQ.md](LICENSE-FAQ.md).
