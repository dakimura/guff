# Install & uninstall

guff ships prebuilt binaries for macOS and Linux (amd64 / arm64). A Go toolchain on `PATH` is required at runtime for package resolution.

## Quick install (recommended)

```bash
curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/install.sh | sh
```

Default location: `~/.local/bin/guff`. Put that directory on your `PATH`, then:

```bash
guff version
guff run ./...
```

Pin a version:

```bash
curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/install.sh \
  | sh -s -- -b ~/.local/bin v0.4.1
```

## Homebrew

```bash
brew tap dakimura/guff https://github.com/dakimura/guff
# Homebrew 5+ may ask you to trust the tap once:
#   brew trust dakimura/guff
brew install guff
```

Formula: [`Formula/guff.rb`](../Formula/guff.rb) (update shas on each release).

Uninstall: `brew uninstall guff` then optionally `brew untap dakimura/guff`.
## aqua

Draft registry package: [`packaging/aqua/guff.yaml`](../packaging/aqua/guff.yaml).

Until it lands in [aqua-registry](https://github.com/aquaproj/aqua-registry), either:

- open a registry PR using that YAML as the package body, or
- use the curl installer / Homebrew below.

After it is in the standard registry:

```yaml
# aqua.yaml
registries:
  - type: standard
    ref: v4.430.0 # use a tag that includes dakimura/guff

packages:
  - name: dakimura/guff@v0.4.1
```

```bash
aqua install
```

## mise

Prefer the curl installer or Homebrew until aqua-registry carries guff. Example pinned bootstrap in onboarding docs:

```bash
curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/install.sh \
  | sh -s -- -b "$HOME/.local/bin" v0.4.1
```

CI should use the GitHub Action or Docker image so local mise/aqua drift does not affect gates.

## GitHub Actions

```yaml
- uses: actions/setup-go@v5
  with:
    go-version: stable

- uses: dakimura/guff@v0.4.1
  with:
    args: run --out-format=github-actions ./...
```

## Docker

```bash
docker run --rm -v "$PWD":/app -w /app ghcr.io/dakimura/guff:0.4.1 run ./...
```

## cargo (from source)

Requires a Rust toolchain:

```bash
cargo install --git https://github.com/dakimura/guff --locked guff-lint
```

## Manual download

Release assets: <https://github.com/dakimura/guff/releases>

```bash
# example: darwin arm64
curl -fsSL -o guff.tgz \
  https://github.com/dakimura/guff/releases/download/v0.4.1/guff_0.4.1_darwin_arm64.tar.gz
curl -fsSL -o guff.tgz.sha256 \
  https://github.com/dakimura/guff/releases/download/v0.4.1/guff_0.4.1_darwin_arm64.tar.gz.sha256
shasum -a 256 -c guff.tgz.sha256
tar -xzf guff.tgz
install -m 755 guff ~/.local/bin/guff
```

## Uninstall / rollback

### Binary from `install.sh`

```bash
curl -sSfL https://raw.githubusercontent.com/dakimura/guff/main/scripts/uninstall.sh | sh
# or: ./scripts/uninstall.sh -b ~/.local/bin
```

### Other installers

| How you installed | Uninstall |
|---|---|
| Homebrew | `brew uninstall guff` |
| aqua | `aqua rm guff` / remove from `aqua.yaml` |
| cargo | `cargo uninstall guff-lint` |
| Docker / Action | stop using the image / remove the workflow step |

### Project files

guff does **not** rewrite your config by default. Keep or delete:

- `.golangci.yml` / `.golangci.yaml` (unchanged; golangci-lint still works)
- `.guff.yml` / `.guff.yaml` (optional guff-specific config)

Clear the analysis cache:

```bash
guff cache clean
```

### CI rollback

1. Revert the workflow step to `golangci/golangci-lint-action` (or your previous command).
2. Keep the same `.golangci.yml` — no migration needed to go back.

## Updating the Homebrew formula after a release

1. Download each `*.tar.gz.sha256` from the GitHub Release.
2. Update `version`, `url`, and `sha256` in [`Formula/guff.rb`](../Formula/guff.rb).
3. Smoke-test:

```bash
brew tap dakimura/guff https://github.com/dakimura/guff
brew reinstall guff
guff version
```
