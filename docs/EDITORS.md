# Editors, pre-commit, and local hooks

guff does not yet ship a dedicated VS Code / GoLand extension. Use it as an external tool the same way many teams use golangci-lint.

## VS Code / Cursor

Recommended: a workspace task (reliable; no flag translation surprises):

```json
{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "guff",
      "type": "shell",
      "command": "guff run ./...",
      "group": "test",
      "presentation": { "reveal": "always", "panel": "shared" }
    }
  ]
}
```

Experimental: point the Go extension’s golangci-lint binary at guff (`go.lintTool` = `golangci-lint`, `go.alternateTools.golangci-lint` = `guff`). This only works when the extension’s flags are a subset guff accepts — prefer the task if lint fails to start.

## GoLand / IntelliJ

**Settings → Tools → File Watchers** or an **External Tool**:

- Program: `guff`
- Arguments: `run $FileDir$` or `run ./...`
- Working directory: `$ProjectFileDir$`

For on-save formatting: `guff fmt $FilePath$` (or `guff fmt .`).

## pre-commit

This repository exposes hooks via [`.pre-commit-hooks.yaml`](../.pre-commit-hooks.yaml).

In your Go repo’s `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: https://github.com/dakimura/guff
    rev: v0.4.0
    hooks:
      - id: guff
      - id: guff-fmt
```

Install guff on the machine (or in CI) first — hooks use `language: system`.

## lefthook

```yaml
# lefthook.yml
pre-commit:
  commands:
    guff:
      run: guff run ./...
      glob: "*.go"
```

## watch mode

For a long-lived local session:

```bash
guff run --watch ./...
```

Keeps the package graph warm and re-runs on changes. Pair with your editor’s save, not instead of CI.
