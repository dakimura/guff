# Per-linter isolate harness
#
# Runs guff and golangci-lint with **exactly one** linter enabled
# (`linters.default: none` + `enable: [<name>]`) on a tiny fixture module,
# then set-diffs findings via `compat/normalize.py`.
#
# This catches parity holes that multi-linter OSS configs hide (a bug in
# linter A can be invisible when only B/C are enabled on the corpus).

## Quick start

```bash
cargo build --release -p guff-lint

# All curated isolate targets (CI required gate)
./compat/run.sh --isolate

# Quick local pre-check: standard five only
./compat/run.sh --isolate --smoke

# One linter
./compat/run.sh --isolate --linter errcheck

# Refresh isolate allowlists from current diffs
./compat/run.sh --isolate --update-allowlist
```

## Layout

| Path | Role |
|------|------|
| `linters.txt` | Curated list (`<name> [smoke\|full]`) |
| `make_config.py` | Emit single-linter v2 YAML |
| `fixtures/<linter>/` | Tiny go module that should trigger the linter |
| `allowlists/` | Accepted diffs (`isolate-<linter>` targets) |

## Agent / PR checklist

Before finishing linter or analyzer changes:

1. `cargo build --release -p guff-lint`
2. `./compat/run.sh --isolate --linter <touched>` (and full `./compat/run.sh --isolate`)
3. Prefer fixing guff over `allowlists/` growth

CI runs **full isolate (all linters)** plus fixture smoke and OSS pr-tier on every
PR / main push. Local `--isolate --smoke` is only a quick pre-check.
See `.cursor/rules/compat-isolate.mdc`.

## Adding a linter

1. Create `fixtures/<name>/` with `go.mod` + Go that triggers ≥1 finding.
2. Optional: `fixtures/<name>/settings.yml` — options merged under
   `linters.settings.<name>` (needed when golangci defaults disable the check,
   e.g. `decorder`).
3. Append `<name>` (or `<name> smoke`) to `linters.txt`.
4. Run `./compat/run.sh --isolate --linter <name>` and fix guff or allowlist.

Prefer a **minimal** fixture (stdlib only) so both tools stay offline-friendly.
Aim for **≥2 distinct finding shapes** when the linter has multiple rules.
