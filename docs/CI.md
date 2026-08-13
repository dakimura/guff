# Running guff in CI

The short version: use the Action, pin a version, and let it keep its cache.

```yaml
name: lint

on:
  pull_request:

jobs:
  guff:
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-go@v5
        with:
          go-version: stable

      - uses: dakimura/guff@v0.4.1
        with:
          args: run --out-format=github-actions ./...
```

Caching is on by default, so this is already the fast configuration. The rest of
this page explains what that buys, and the two settings worth touching.

## What the cache changes

guff keeps its analysis results in `$GUFF_CACHE`, keyed by content: the `go list`
output, resolved package metadata, per-package analyzer facts, formatter check
results, and the issues themselves. A run whose inputs did not change does no
work beyond confirming that.

CI is the case that benefits most, because CI re-lints a tree that is almost
entirely identical to the last one it linted. Without a persisted cache, every
run pays full cold cost for a diff of a handful of files.

Measured on Prometheus (118 root packages, 1616 total) with guff 0.4.1 and
go1.26.5, on an Apple M4. The cache was copied from an archive before each trial
rather than reused in place, so these approximate what a runner sees after a
cache restore:

| `$GUFF_CACHE` | no source change | one core file changed |
|---|---:|---:|
| empty (no caching) | 17.0s | 17.0s |
| restored, default (27 MB) | 2.2s | 5.0s |
| restored, `cache-seed: true` (171 MB) | 1.4s | 3.9s |

Two things worth reading off that table. A warm cache is worth roughly 3–8× on
this module, and the win survives a real change, because only the packages
affected by that change are recomputed.

The cache also makes guff largely independent of the Go build cache: with
`$GUFF_CACHE` restored and `GOCACHE` empty, an unchanged tree still finished in
under a second locally, because nothing downstream of `go list` had to run.

Findings are identical in every configuration above — the cache is a
memoization, not an approximation. If you ever suspect otherwise, `--no-cache`
forces a cold run.

## Safety of a stale cache

Every sub-cache validates itself against the content it was derived from, so a
cache restored from an older commit cannot produce a stale finding: entries whose
inputs still match are reused, and everything else is recomputed. That is why
the Action restores on a **prefix** match rather than requiring an exact key —
a near-miss cache is nearly as good as an exact one, and far better than none.

The practical consequence is that you do not need `go.sum` or `.golangci.yml` in
the cache key, and you should not add them. Doing so throws the whole cache away
on a dependency bump, where a prefix match would have kept most of it.

## Settings

| Input | Default | When to change it |
|---|---|---|
| `cache` | `true` | Set `false` to always run cold. Mostly useful when reproducing a bug. |
| `cache-dir` | `${{ runner.temp }}/guff/cache` | Keep it outside the checkout. |
| `cache-key-suffix` | `""` | Two jobs linting the same directory with different configs. |
| `cache-seed` | `false` | See below. |

### `cache-key-suffix` and matrices

The working directory is already part of the cache key, so a matrix over several
modules gets one cache per module without any configuration:

```yaml
strategy:
  matrix:
    service: [alpha, beta]

steps:
  - uses: dakimura/guff@v0.4.1
    with:
      working-directory: backend/${{ matrix.service }}
```

`cache-key-suffix` is for the case the directory does not distinguish: the same
tree linted twice under different configs, which would otherwise share one entry
and evict each other every run.

### `cache-seed`

The type-checking seed overlays are the largest part of the cache by a wide
margin — 144 MB of the 171 MB above — and they save about a second once the
issue cache is warm. The Action leaves them out by default and sets
`GUFF_SEED_PERSIST=0` so the run does not write them at all.

That default is about the repository's 10 GB cache budget rather than about the
second. A five-service matrix storing 171 MB per leg per commit exhausts it in
roughly a dozen commits, and GitHub evicts least-recently-used entries across the
whole repository — including the Go module cache the same workflow depends on.
Turn it on for a single large module where the budget is otherwise unspent:

```yaml
- uses: dakimura/guff@v0.4.1
  with:
    cache-seed: "true"
```

## Pin a version

`version: latest` (the default when the Action is not pinned to a `v*` tag) costs
a GitHub API round-trip to resolve the release. Referencing the Action as
`dakimura/guff@v0.4.1` skips it: the tag names the release asset directly, so the
install is a single unauthenticated download.

Pinning also keeps the cache key stable. The key includes the resolved guff
version, so a floating `latest` discards the cache on every release.

## Binary or container?

Prefer the Action, which installs a ~20 MB static binary. The published image
(`ghcr.io/dakimura/guff`) ships a full Go toolchain and is far larger to pull —
worth it only when the job has no Go toolchain of its own, which for a Go
repository's CI is unusual.

Whichever you use, guff needs `go` on `PATH`: `go list` resolves the package
graph. `actions/setup-go` is enough, and its module cache is worth keeping even
though a warm `$GUFF_CACHE` makes guff itself much less sensitive to it.

## Self-hosted runners

Nothing above assumes GitHub-hosted runners, with one exception: if your runners
have a persistent working volume, point `cache-dir` at it and set `cache: false`.
The cache is then simply always there, and you skip the restore and upload
entirely. That is the fastest configuration available, and it is the one local
development already uses.
