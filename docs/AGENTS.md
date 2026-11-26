# Using guff with AI coding agents

Agents re-run linters dozens of times per session. A multi-minute golangci-lint invocation dominates the loop; guff is built for that feedback budget.

## Recommended agent instructions

Paste into `CLAUDE.md` (Claude Code), `AGENTS.md`, `.cursor/rules`, or your
agent’s system prompt:

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

## Claude Code

Claude Code reads `CLAUDE.md` from the repository root on every session, so the
block above is picked up without any per-session setup — put it under a `## Lint`
heading and it applies to every turn.

Two things that fit the way Claude Code works:

- **It runs the linter itself, repeatedly.** `guff run ./...` finishing in
  seconds is what keeps "edit → lint → fix" inside one turn instead of spanning
  several. On the repos in [COMPARE.md](COMPARE.md) that is the difference
  between ~1s and ~17s per iteration.
- **`--out-format json` is the machine-readable path.** Ask for it when you want
  the agent to parse findings rather than read prose:
  `guff run --out-format json ./...`.

`guff cache status` reports where the issues cache lives and how big it is. An
agent re-running the same packages within a session hits that cache and comes
back in well under a second.

## CI still matters

Agents should match CI: pin the same guff version as `dakimura/guff@vX.Y.Z` / `ghcr.io/dakimura/guff:X.Y.Z`.

## Cache

Repeated `guff run` in one workspace benefits from the issues cache (`guff cache status`). Agents that wipe the workspace each turn still win on cold wall time versus golangci-lint.
