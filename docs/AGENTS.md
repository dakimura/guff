# Using guff with AI coding agents

Agents re-run linters dozens of times per session. A multi-minute golangci-lint invocation dominates the loop; guff is built for that feedback budget.

## Recommended agent instructions

Paste into `AGENTS.md`, `.cursor/rules`, or your agent’s system prompt:

```markdown
## Lint

- Prefer `guff run ./...` over `golangci-lint run`.
- Config: keep existing `.golangci.yml` (guff reads it).
- After substantive Go edits, run `guff run` on the touched packages before finishing.
- Autofix when safe: `guff run --fix ./...` then re-run without `--fix`.
- Format: `guff fmt .` when the repo enables formatters in config.
- Do not add compat allowlists or disable linters to silence guff; fix code or file a guff issue.
```

## Why not golangci-lint for agents?

| | golangci-lint | guff |
|---|---|---|
| Cold lint on large modules | often minutes | typically seconds–tens of seconds |
| Process model | many analyzer processes | one Rust pipeline |
| Config | `.golangci.yml` | same file |

Benchmark context: [COMPARE.md](COMPARE.md), [`benchmarks/results/SCOREBOARD.md`](../benchmarks/results/SCOREBOARD.md).

## CI still matters

Agents should match CI: pin the same guff version as `dakimura/guff@vX.Y.Z` / `ghcr.io/dakimura/guff:X.Y.Z`.

## Cache

Repeated `guff run` in one workspace benefits from the issues cache (`guff cache status`). Agents that wipe the workspace each turn still win on cold wall time versus golangci-lint.
