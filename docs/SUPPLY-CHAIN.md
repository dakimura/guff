# Supply chain & release verification

## What we publish

Each [GitHub Release](https://github.com/dakimura/guff/releases) includes:

| Artifact | Purpose |
|---|---|
| `guff_<ver>_<os>_<arch>.tar.gz` | Prebuilt `guff` binary |
| `guff_<ver>_<os>_<arch>.tar.gz.sha256` | SHA-256 checksum (GNU `sha256sum` format) |
| Container `ghcr.io/dakimura/guff:<ver>` | Image with Go + guff |

Install script: [`scripts/install.sh`](../scripts/install.sh) downloads the matching asset for your OS/arch.

## Verify a download

```bash
VERSION=0.4.0
OS=darwin   # or linux
ARCH=arm64  # or amd64
BASE=guff_${VERSION}_${OS}_${ARCH}.tar.gz

curl -fsSL -O "https://github.com/dakimura/guff/releases/download/v${VERSION}/${BASE}"
curl -fsSL -O "https://github.com/dakimura/guff/releases/download/v${VERSION}/${BASE}.sha256"
shasum -a 256 -c "${BASE}.sha256"   # macOS
# sha256sum -c "${BASE}.sha256"    # Linux
```

GitHub Actions composite action installs via the same script and inherits Actions’ token for private-repo fetches.

## Reproducible-ish builds

Release binaries are produced by [`.github/workflows/release.yml`](../.github/workflows/release.yml) with `cargo build --release --locked` per target. `--locked` pins `Cargo.lock`.

## SBOM

The release workflow attaches a **CycloneDX JSON SBOM** for the Cargo workspace (`guff.cdx.json`) alongside binary assets. Generate locally:

```bash
cargo install cargo-cyclonedx --locked
cargo cyclonedx --format json --all -o guff.cdx.json
```

## Not yet

| Control | Status |
|---|---|
| Sigstore / cosign signatures on binaries | Planned |
| SLSA provenance attestation | Planned |
| Homebrew bottles (bottled builds) | Formula is source URL → release tarball |
| Windows assets | Not shipped yet |

Track requests via GitHub Issues if your compliance program needs cosign/SLSA before adoption.
