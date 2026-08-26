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
change, so the tier landed at **144 matching, 49 pending**: 34 where guff writes
nothing at all, 15 where it writes some of the edits.

After the `refactor.AddImport` port (2026-08-25): **145 matching, 48 pending**
— 34 that write nothing at all, 14 that write some of the edits.
`modernize-atomictypes` reached upstream's exact bytes and its ledger file was
deleted; `modernize` moved and was re-recorded, gaining the four import
insertions it had been leaving out.

After rangeint / slicescontains / waitgroupgo (2026-08-25): **146 matching,
47 pending** — 34 that write nothing, 13 partial. `rangeint` reached upstream's
bytes and its ledger file was deleted.

After importas (2026-08-25): **147 matching, 46 pending**. `importas` is the
first case to *leave* the un-buildable list by being fixed rather than join it —
it renamed the import and not its uses.

After QF1012 (2026-08-25): 147 matching, 46 pending, and `staticcheck-qf` off
the un-buildable list too. `modernize` is the only guff-side tree left there.

After `DeleteStmt` / `DeleteUnusedVars` (2026-08-25): **148 matching, 45
pending**, and `modernize` reached upstream's bytes. **Every tree that no longer
builds is now byte-identical to what golangci-lint wrote** — the guff-side
column is empty for the first time.

After revive's `ReplacementLine` (2026-08-25): **156 matching, 36 pending, 1
deliberately divergent**. Nine `revive-*` cases are one linter under nine
configs, so a single mechanism closed all nine — and turned up the divergence
below.

After govet's `assign` / `unreachable` (2026-08-25): **160 matching, 32
pending**. Both are `refactor.DeleteStmt` upstream, so the port from the
modernize work carried straight over — and the tier caught a defect in it: a
trailing `// comment` was being stranded on a line whose statement had gone.

After `godot` (2026-08-26): **161 matching, 31 pending**.

After `whitespace` (2026-08-26): **162 matching, 30 pending**.

After gocritic's first three fixes (2026-08-26): still 162 matching, 30 pending
— the case needs a fourth thing guff will not do. Inside it, guff went from 0 to
29 of upstream's 76 diff lines.

After staticcheck's S1002 / S1004 / S1012 (2026-08-26): **163 matching, 29
pending**. Three checks closed a *fourth* case, `staticcheck-checks-glob`, which
enables a narrower set that those three happen to cover.

After S1003 / S1021 (2026-08-26): **164 matching, 28 pending**, and
`staticcheck-st` closed the same way. Five checks, two cases — and
`staticcheck-s` itself is still at 63 of 271.

After S1016 / S1028 / S1030 (2026-08-26): 164 matching, 28 pending — no case
moved this time, and `staticcheck-s` went 63 -> 113 of 271. Three checks
sometimes close a case and sometimes close none; the ledger line count is the
one that always moves.

After S1010 / S1033 / S1035 / S1037 / S1039 (2026-08-26): 164 matching, 28
pending, and `staticcheck-s` at 167 of 271. Two of the files that moved belong
to checks nobody touched: adding S1039 made `s1028` and `s1038` match, because
`--fix` output is a property of the whole *file*, not of one check. Every file
guff writes in that case is now byte-identical to upstream; it is silent on the
eight remaining ones.

## Does it still build?

A `--fix` that rewrites `fmt.Sprint(i)` to `strconv.Itoa(i)` and does not add
the import writes code that does not compile. No finding-set comparison can
express that, and the byte diff only shows it to a reader who notices a missing
line — so the gate runs `go build ./...` over the fixed tree and counts.

It asks the pristine tree first: several fixtures are deliberately un-buildable
(the unused variable *is* the finding), and there is nothing to break in a tree
that was already broken.

The count is printed, not enforced, and the reason is the measurement itself.
Six cases leave an un-buildable tree, and **all six are byte-identical to what
golangci-lint wrote**:

| case | why the fixed tree does not build |
|------|-----------------------------------|
| `dotimport` | rewrites a dot-import usage without adding `errors` |
| `perfsprint` | `strconv.Itoa` without the import — upstream's own `fiximports` is off by default |
| `err113` | the rewritten call is short an argument |
| `modernize-atomictypes` | rewrites the only use of an aliased second import of `sync/atomic`, leaving `myatomic` unused |
| `rangeint` | `for i = 0` whose body never reads `i` becomes `for i = range n`, leaving `var i int` unread |
| `modernize` | `testingcontext` rewrites to `ctx := t.Context()` and does not drop the now-unused `"context"` import |

Upstream's `--fix` breaks the build on all six, guff reproduces each exactly,
and a hard gate would be demanding that guff be *incompatible*. There is no
guff-side entry left.

That is what this count is for. It never went to zero and it was never supposed
to: it went from "six trees, four of them our bug" to "six trees, none of them
ours" without the total moving at all. Reading the total alone would have shown
nothing happening.

## One difference is deliberate, and it is not in `pending/`'s sense

`pending/staticcheck-qf.diff` holds three hunks. Two are gaps. The third is a
place where **guff is right and upstream is not**, and closing it would be a
regression:

```go
import ( s "strings" )
func renamed() { s.Replace("", "", "", -1) }
```

golangci-lint's QF1004 rewrites that to `strings.ReplaceAll("", "", "")` and
adds no import, so it names a package that is not bound in the file. guff writes
`s.ReplaceAll(...)`, which compiles. That single hunk is why `staticcheck-qf`
builds after guff's `--fix` and would not after upstream's.

The ledger has no slot for "ahead", only for "behind", so it is recorded there
with the two real gaps. If someone later makes this hunk match, the gate goes
red — read this section before deleting the entry, because matching here means
`--fix` starts breaking user builds. Same call as the `revive` ratchet: a defect
upstream ships is not a specification.

Two of the five joined that list by being *fixed*. `modernize-atomictypes` used
to write a prefix guff invented, which happened to keep the alias used;
`rangeint` used to leave an unused `i` bound in the range clause, and the
fixture that pins the `=` spelling exposes the same defect one level up.
Matching upstream made both trees stop building. **The count going up is not
always the direction it looks like**, which is exactly why it is printed rather
than gated.

Fixing a parse error can also *reveal* one. `modernize` used to fail at
`waitgroupgo`, which left `wg.Go(func() {…}()` — syntax, so `go build` stopped
there. With that closed, the case reports two real failures underneath it:
`reflecttypefor` not deleting a now-unused `var zero`, and `testingcontext`
leaving `"context"` imported and unused. A tree that does not parse hides
however many type errors follow it.

## `divergent/` — the one door out of the refusal above

`pending` refuses, always, to hold a case where guff writes and upstream does
not. That refusal is the whole reason `omitempty` -> `omitzero` cannot happen
twice quietly, and relaxing it in general would give back exactly what it
bought.

`divergent/<case>.diff` is the narrow exception, and it is built to be hard to
use:

* **Hand-written.** Nothing in `regen.sh` produces one. The recorder still
  refuses. The only way a file appears here is a person writing it.
* **`# why:` is mandatory.** Without one the gate fails — a deliberate
  divergence nobody explained is an allowlist entry with better manners.
* **It fails if guff's bytes move**, like `pending`. It records one decision,
  not permission to write anything.
* **It fails if upstream starts writing there.** The reason is always of the
  form "upstream writes nothing, and here is why that is wrong"; if upstream
  starts writing, the premise is gone and somebody has to decide again.
* The reason is printed on every run, next to the case.

One entry today, `revive`. golangci-lint's revive `--fix` is inert in almost
every real repository, and by accident: its wrapper looks the file up with
`Fset.File(token.Pos(failure.Position.Start.Offset))` — a byte offset handed to
a lookup that wants a FileSet-wide position — then drops the fix unless the file
it found is the failure's own. Measured with 2.12.2: one package with `a.go` and
`b.go` fixes `a.go` and leaves `b.go`; two packages fix nothing. Which findings
keep their fix depends on FileSet layout neither tool guarantees, so it cannot
be reproduced — running the same computation against guff's FileSet suppressed
the nine `revive-*` cases that *do* match, because guff loads dependency files
first. The full argument is in the file's own header.

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
