# Migrate from golangci-lint in five minutes

Goal: run the same `.golangci.yml` with guff, keep an easy rollback, then optionally delete golangci-lint from CI.

## 1. Install guff (no Rust required)

```bash
curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/install.sh | sh
guff version
```

You need a Go toolchain on `PATH` (same as golangci-lint).

## 2. Keep your config

Do **not** rename or rewrite `.golangci.yml` yet.

```bash
# from your module root
guff run ./...
```

guff searches, in order:

```
.golangci.yml
.golangci.yaml
.guff.yml
.guff.yaml
```

v1 configs: `guff migrate` rewrites to v2 (with a backup). Prefer upgrading with golangci-lint first if you can.

## 3. Compare findings (optional but recommended)

On a quiet machine:

```bash
# same enable set — use your real config
golangci-lint run ./... > /tmp/golangci.txt || true
guff run ./... > /tmp/guff.txt || true
diff -u /tmp/golangci.txt /tmp/guff.txt | head
```

For per-linter parity work, this repo uses `./compat/run.sh --isolate` (maintainers). As a consumer, spot-check PRs and known-noisy linters (`revive`, `gosec`, SSA-heavy checks). See [COMPARE.md](COMPARE.md) for known partial gaps.

## 4. Wire CI

Replace the golangci Action (or binary install) with:

```yaml
- uses: actions/setup-go@v5
  with:
    go-version: stable

- uses: dakimura/guff@v0.4.0
  with:
    args: run --out-format=github-actions ./...
```

Or Docker: `ghcr.io/dakimura/guff:0.4.0`.

Run both tools in parallel for one release cycle if you want a safety net.

## 5. Local DX (optional)

```bash
# autofix where SuggestedFix exists
guff run --fix ./...

# formatters from config (gofmt / goimports / …)
guff fmt .

# re-lint on change
guff run --watch ./...

# second runs reuse the issues cache
guff cache status
```

pre-commit / editor snippets: [EDITORS.md](EDITORS.md). Agents: [AGENTS.md](AGENTS.md).

## Rollback

1. Point CI back at golangci-lint.
2. Leave `.golangci.yml` as-is.
3. Remove the binary: [INSTALL.md](INSTALL.md#uninstall--rollback).

Same config works for both tools — switching either direction should not require a config rewrite.

## When to keep golangci-lint around

- You need a **custom plugin** only available as a golangci plugin (guff custom plugins differ; see `guff custom --help`).
- You depend on a **DEFERRED** setting or comment directive listed in [COMPATIBILITY.md](COMPATIBILITY.md) (Japanese matrix) / [COMPARE.md](COMPARE.md).
- Policy requires a specific golangci-lint binary attestation you have not yet mirrored for guff ([SUPPLY-CHAIN.md](SUPPLY-CHAIN.md)).
