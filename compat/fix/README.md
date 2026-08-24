# The `--fix` gate

Every other tier here compares **what the two tools say**. This one compares
**what they write**.

A case is materialized twice, each tool is run over its own copy with `--fix`,
and the resulting tree is diffed against the pristine one. The key is that
unified diff, byte for byte.

## Why a whole tier for it

The golden key is `path:line:col:linter:severity:text` — the rendered
diagnostic. A suggested fix's replacement text appears nowhere in it, so a
linter can report perfectly and rewrite the file wrongly, or not at all, with
every existing gate green.

That is not hypothetical. Three independent `--fix` defects shipped under a
fully green harness ([COMPAT-HARDENING](../../docs/COMPAT-HARDENING.md),
2026-08-24 続き 37): a conflict rule that dropped the wrong side of an overlap,
an `errors.As` fix that was never built at all, and no gofmt pass after
applying. All three change bytes on disk and none of them changes a key.

## Quick start

```bash
cargo build --release -p guff-lint

./compat/fix/run.sh                  # check every case (CI gate)
./compat/fix/run.sh --case godot     # one case

./compat/fix/regen.sh                # re-record from golangci-lint --fix
./compat/fix/regen.sh godot
./compat/fix/regen.sh --pending      # re-record the known-missing gaps
```

## Layout

| Path | Role |
|------|------|
| `run.sh` | Gate, `--regen`, and `--record-pending` |
| `regen.sh` | Thin wrapper for both recording modes |
| `fixdiff.py` | Tree → normalized unified diff, record, compare |
| `expected/<case>.diff` | What `golangci-lint --fix` writes. Generated — do not hand-edit |
| `pending/<case>.diff` | What guff writes *today*, for a case whose parity is missing |
| `.work/<case>/` | Materialized module, one copy per tool (gitignored) |

The corpus is [`../golden/cases`](../golden/cases) — the same 193 cases the
golden tier gates, materialized through the same
[`materialize.sh`](../golden/materialize.sh). Two tiers, two questions, one
definition of what a case is.

## An absent file means strictly nothing

If `expected/<case>.diff` does not exist, upstream's `--fix` changes nothing
there and guff must change nothing either. Recording "no changes" as an empty
file would make it indistinguishable from a case nobody has regenerated, and
that is the same reasoning `compat/health.py` uses for a baseline row it
declines to write.

## `pending/` is a ledger, not an allowlist

Fifteen linters carry a `DEFERRED: SuggestedFix` note in their module doc.
Every one of them reports correctly and rewrites nothing, so a gate that failed
on all of them from day one would be turned off within a week — and one that
ignored them would measure nothing.

So a pending case records what guff writes today and prints what upstream writes
instead, on every run:

```
importas: pending — upstream writes 19 diff line(s), guff writes 13
```

Nothing is suppressed, and the file fails the gate the moment guff's output
moves in *either* direction — including once guff gets it right, so the ledger
cannot outlive the defect it records.

First measurement (2026-08-24): 143 of 193 cases matched. One of the 50 — the
one where guff *added* edits upstream does not make — was fixed in the same
change, so the tier lands at **144 matching, 49 pending**: 34 where guff writes
nothing at all, 15 where it writes some of the edits.

## Does it still build?

A `--fix` that rewrites `fmt.Sprint(i)` to `strconv.Itoa(i)` and does not add
the import writes code that does not compile. No finding-set comparison can
express that, and the byte diff only shows it to a reader who notices a missing
line — so the gate runs `go build ./...` over the fixed tree and counts.

It asks the pristine tree first: several fixtures are deliberately un-buildable
(the unused variable *is* the finding), and there is nothing to break in a tree
that was already broken.

The count is printed, not enforced, and the reason is the measurement itself.
Seven cases leave an un-buildable tree, and **three of them are byte-identical
to what golangci-lint wrote** — `dotimport` (rewrites a dot-import usage without
adding `errors`), `perfsprint` (`strconv` — upstream's own `fiximports` option
is off by default), `err113`. Upstream's `--fix` breaks the build on those
fixtures, guff reproduces it exactly, and a hard gate would be demanding that
guff be *incompatible*. The other four are guff's own: `importas`, `modernize`,
`rangeint`, `staticcheck-qf`.

## Two traps worth knowing

**golangci-lint's cache is keyed on content, not on path.** Run the same file
from a second directory and it will answer from the previous run's entry while
printing paths from a directory that no longer exists — which reads as "upstream
fixes nothing" and would be *recorded* as exactly that. Every run here gets its
own `GOLANGCI_LINT_CACHE`.

**The fixtures do not have to be gofmt-clean.** They did, once: before guff ran
the meta formatter after applying (続き 37), a gofmt-violating input produced a
diff all by itself. Both tools now normalize the same way, so a deliberately
misformatted input still comes out byte-identical.
