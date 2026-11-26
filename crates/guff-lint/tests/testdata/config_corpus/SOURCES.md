# Config corpus sources (R22)

Real-world golangci-lint **v2** config snapshots used by
`parse_golangci_config_corpus` in `crates/guff-lint/tests/config_test.rs`.

Each `*.yml` / `*.yaml` file is an upstream `.golangci.yml` (or `.yaml`)
with a 3-line header:

```yaml
# Snapshot based on <org/repo> (golangci-lint v2).
# Source: <raw.githubusercontent.com URL>
# Captured: YYYY-MM-DD. Upstream may diverge; refresh via SOURCES.md.
```

v1 configs are intentionally excluded — this corpus exercises guff's v2
parser / `linter_selection` / `effective_issues` path.

## Refresh a snapshot

```bash
# From repo root; example: refresh caddy
name=caddy
url=$(awk '/^# Source:/{print $3; exit}' \
  crates/guff-lint/tests/testdata/config_corpus/${name}.yml)
tmp=$(mktemp)
curl -fsSL "$url" -o "$tmp"
# Keep only v2
grep -qE '^version:[[:space:]]*["'\'']?2' "$tmp"
{
  head -3 "crates/guff-lint/tests/testdata/config_corpus/${name}.yml" \
    | sed "s/^# Captured:.*/# Captured: $(date -u +%Y-%m-%d). Upstream may diverge; refresh via SOURCES.md./"
  cat "$tmp"
} > "crates/guff-lint/tests/testdata/config_corpus/${name}.yml"
rm -f "$tmp"
cargo test -p guff-lint --test config_test parse_golangci_config_corpus
```

## Add a new snapshot

1. Find a popular Go project whose config starts with `version: "2"`
   (or `version: 2`).
2. Copy it into this directory as `<short-name>.yml`.
3. Prepend the 3-line header (org/repo + raw Source URL + Captured date).
4. Run `cargo test -p guff-lint --test config_test parse_golangci_config_corpus`.
5. Update the count in `docs/DEVELOPMENT.md` §3.4 / §8 R22 if you care
   about the documented total.

## Inventory

The authoritative inventory is the files themselves (`# Source:` lines).
Count them with:

```bash
ls crates/guff-lint/tests/testdata/config_corpus/*.{yml,yaml} 2>/dev/null | wc -l
```
