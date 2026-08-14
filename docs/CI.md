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

Measured on a GitHub-hosted `ubuntu-latest` runner (4 cores, 15 GB) against
Prometheus at 113 root packages, under that project's own `.golangci.yml` —
revive with two dozen rules, gocritic and govet at `enable-all`, staticcheck,
modernize. guff 0.4.1, go1.25.10:

| `$GUFF_CACHE` | no source change | one widely-imported file changed |
|---|---:|---:|
| empty (no caching) | 7.9s | 7.9s |
| restored, default | **0.2s** | **4.2s** |
| restored, `cache-seed: true` | 0.2s | 2.9s |

The win survives a real change, which is the part that matters: only the
packages affected by the edit are recomputed, so a pull request pays for its own
diff rather than for the repository.

The cache also makes guff largely independent of the Go build cache. Across two
runs of the same commit where the only difference was whether `actions/setup-go`
restored `GOCACHE`, guff's cold run moved from 8.09s to 7.88s. For comparison,
golangci-lint's moved from 120s to 70s on the same pair — worth knowing if you
are benchmarking either tool, because it is easily the largest source of
variance in that measurement.

Findings are identical in every configuration above — the cache is a
memoization, not an approximation. If you ever suspect otherwise, `--no-cache`
forces a cold run.

One caveat when you compare those outputs yourself: the *order* findings are
printed in can differ between a cached and an uncached run, because emission
follows the order work finished and serving packages from cache changes that.
Measured on the same module, 841 findings each way, equal once sorted but not in
the same sequence. Compare the set, not the transcript — `output.sort-order` in
your config pins the order if you want the transcripts to match too.

## Safety of a stale cache

The failure everyone is right to worry about is a cache that hides a finding:
you edit an exported signature, every caller now has a real problem, and the
run reports nothing because those callers were served from a cache keyed on
their own unchanged bytes.

guff's cache entries are not keyed that way. Every issue and fact lookup uses
the hash mode that folds in **each transitive dependency's content hash**, taken
from the flat `deps` list `go list` already produces. Changing a package
therefore changes the key of everything downstream of it, and all of it is
recomputed. `crates/guff-lint/tests/cache_dep_invalidation.rs` pins this
end-to-end: a package's dependency gains an error return, the dependent's own
source is asserted byte-identical, and the warm-cache run must produce the same
errcheck finding as `--no-cache` on the same tree.

That is what makes a **prefix** restore safe, rather than requiring an exact
key: entries whose inputs still match are reused, everything else is recomputed,
so a near-miss cache is nearly as good as an exact one and far better than none.

The practical consequence is that you do not need `go.sum` or `.golangci.yml` in
the cache key, and you should not add them. Doing so throws the whole cache away
on a dependency bump, where a prefix match would have kept most of it.

None of that is a proof of no bugs, which is why `cache-invalidation-interval`
defaults to 7 days: the key carries a bucket number, so once a week the
restore misses and the run is genuinely cold. It bounds how long a hypothetical
bad entry could survive. golangci-lint-action carries the same 7-day default for
the same reason.

If you ever suspect the cache, `--no-cache` reproduces a cold run in place and
the two outputs should be identical.

## What the cache costs to move

A warm run only wins if restoring the cache costs less than the work it skips,
and a wall-clock table shows only the second half. Since `actions/cache`
compresses before upload, the number that matters is the compressed size, and it
is not one you can guess from the directory listing:

| cache | on disk | compressed |
|---|---:|---:|
| default (seeds excluded) | 29 MB | **1.2 MB** |
| `cache-seed: true` | 126 MB | 16 MB |

Roughly a megabyte against an eight-second cold run is not a close call, and it
is the main reason the seeds are excluded by default: keeping them is a 13×
larger artifact to save one to three seconds on the incremental run. (How much
depends on how widely the changed package is imported — edit a leaf and the
seeds save almost nothing; edit something central and it is nearer three.)

For scale, golangci-lint's own cache on the same module is 76 MB on disk and
1.8 MB compressed. Neither tool has a transfer problem here.

That said, the win is not universal, and the crossover is about module size.
The round-trip is roughly fixed while the work saved shrinks with the module, so
on something small enough that a cold run is already a couple of seconds,
caching may be noise. `cache: false` is a perfectly reasonable setting there and
removes a moving part. What you should not do is leave caching on and never
look — read one run's log and see whether the restore bought anything.

## Settings

| Input | Default | When to change it |
|---|---|---|
| `cache` | `true` | Set `false` to always run cold — a small module, or reproducing a bug. |
| `cache-dir` | `${{ runner.temp }}/guff/cache` | Keep it outside the checkout. |
| `cache-key-suffix` | `""` | Two jobs linting the same directory with different configs. |
| `cache-invalidation-interval` | `7` | Days before a forced cold run. `0` disables it. |
| `verify-cache` | `false` | See below. |
| `cache-seed` | `false` | See below. |

### `verify-cache`

Everything above bounds the risk; this one measures it. With `verify-cache: true`
the Action lints the tree a second time with `--no-cache` and fails the job if
the two runs disagree, so a cache defect surfaces as a diff in your own
repository rather than as a finding nobody ever saw.

It compares the *set* of findings, sorted, rather than the two transcripts. The
emission order legitimately differs between a cached and an uncached run, so a
transcript diff would fail on every run and report a bug that is not there — and
a check that cries wolf gets switched off, which costs more than never having
added it. A finding that appears, disappears, moves or changes wording still
fails the job.

It roughly doubles the job's lint time, which is why it is off by default and
belongs on a nightly or main-branch build rather than on every pull request:

```yaml
- uses: dakimura/guff@v0.4.1
  with:
    verify-cache: ${{ github.ref == 'refs/heads/main' }}
```

The Action also exposes `cache-key` and `cache-restored` outputs. When a cache
is not behaving, print them: the key carries the guff version, the
working-directory scope and the invalidation bucket, so a key that moved
unexpectedly explains the miss by itself.

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
margin — 97 MB of the 126 MB above — and they save one to three seconds on an
incremental run, scaling with how many packages depend on the one you edited.
The Action leaves them out by default and sets `GUFF_SEED_PERSIST=0` so the run
does not write them at all.

That default is about the repository's 10 GB cache budget rather than about the
seconds. A five-service matrix storing 126 MB per leg per commit exhausts it in
under twenty commits, and GitHub evicts least-recently-used entries across the
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
