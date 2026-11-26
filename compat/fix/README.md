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
| `divergent/<case>.diff` | What guff writes *on purpose* where upstream does not. Hand-written |
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

After seven more S checks (2026-08-26): 164 matching, 28 pending, and 21 of
`staticcheck-s`'s 22 files byte-identical — only `s1001` is left. The denominator
moved too: extending the S1005 fixture with the three range shapes it had never
held grew the case from 271 expected diff lines to 296, and immediately showed a
finding guff was not reporting at all. A fixture that exercises one shape of a
four-shape check reports "matches" about the one shape.

After S1001 (2026-08-26): **165 matching, 27 pending** — `staticcheck-s` closed,
and its ledger is gone. Extending that fixture first was again the whole job:
the seven loops it now holds found three defects at once, one of which was an
entire loop form guff never reported.

After goheader (2026-08-26): **166 matching, 26 pending**. The largest gap in the
ledger, 110 lines, closed on the first measurement — the whole of it was one
missing port (`generateFix`) plus the wrapper's span arithmetic. It is also the
eighth case whose rewritten tree no longer builds: upstream writes the raw
template, regex metacharacters and all, into every file.

After testifylint's argument-rewriting checkers (2026-08-26): 166 matching, 26
pending, and `testifylint-mock` at 45 of 97 lines. Seven of its checkers rewrite
the assertion's *name and arguments together*; the other nine are still silent,
which is deliberate — a rename without its argument edit writes
`assert.Empty(t, 0, len(arr))`, and the ledger would record a case that does not
compile.

After the other nine (2026-08-26): **167 matching, 25 pending** —
`testifylint-mock` closed, ledger deleted. Sixteen checkers carry fixes; the
seven that do not are the seven upstream leaves unfixed too.

After dupword's comments (2026-08-26): 167 matching, 25 pending, and `dupword`
at 33 of 36 lines. The three left are string literals, which need a Go-exact
`strconv.Quote` that currently lives in another crate.

After nlreturn and protogetter (2026-08-26): **169 matching, 23 pending**, both
cases closed on the first measurement. They had been deferred since 2026-08-19
on the grounds that their sources were not obtainable; `go mod download
<module>@<pinned>` fetches either of them in a second.

After ginkgolinter (2026-08-26): **170 matching, 22 pending**. Its suggestion
*is* its fix — the message already carries the rewritten assertion — so the port
was two struct fields and a helper.

After SA1004 / SA5004 / SA9002 (2026-08-26): 170 matching, 22 pending, and
`staticcheck-sa` at 8 of 16 files with **nothing over-written**. SA1004 was the
only case in the corpus where guff wrote a file upstream leaves alone: upstream
offers two competing fixes there, they conflict, and the conflict drops every
staticcheck edit for the file. Matching upstream meant emitting *more* so that
*less* is written.

After SA4013 (2026-08-26): 9 of 16. Its expected diff is a gofmt blank line and
nothing else — two competing fixes drop all of staticcheck's edits, and the file
is still written. Two rules from earlier entries meeting in one hunk.

After SA4026 / SA1013 (2026-08-26): **11 of 16**, nothing differing. SA4026's
replacement names `math` without importing it, so the tree stops compiling —
upstream's own choice, reproduced.

After SA1006 / SA6005 (2026-08-26): **173 matching, 19 pending** — three cases
closed at once (`staticcheck-checks-all`, `-default`, `-not-s`), none of them
touched directly. They run the same checks under different `checks:` settings,
so a check gained anywhere lands in all of them.

After SA4029 / SA1008 / SA9004 (2026-08-26): **174 matching, 18 pending** —
`staticcheck-sa` closed, ledger deleted. Eleven SA checks gained fixes across
four entries; the last three were the ones that rebuild a node rather than
replace a string.

After QF1005 (2026-08-27): 174 matching, **16 pending, 3 deliberately
divergent**. `staticcheck-qf`'s four differing lines were seven defects in one
check, found by running a 24-shape probe past both tools before touching the
fixture — including one that rewrote `math.Pow(g(), 0)` to `1.0` and deleted the
call, which upstream does not even report. With QF1005 byte-exact the case has
one difference left, the deliberate QF1004 one above, so it moved to
`divergent/`.

After re-reading `parens` (2026-08-27): 174 matching, **17 pending, 2
deliberately divergent**, and no guff code changed. `parens` was the one pending
case where guff wrote *more* than upstream, and the extra hunk turned out to be
the `revive` divergence below arriving in a case upstream also writes to. The
refusal that exists to stop exactly that asked its question once per case rather
than once per line, so it never fired — and `git log` says the hunk arrived in
the very commit that built `divergent/` and wrote the defect down. Every
remaining pending case is now behind upstream, not ahead of it.

After `go/doc/comment` (2026-08-28): **189 matching, 0 pending, 4 deliberately
divergent** — `pending/` is empty and the directory is gone. The last case,
`gocritic`, was never a gocritic gap: go-critic's `commentFormatting` has
`//nolint` in its `equalPatterns` exemption list, so no checker of upstream's
ever emits that edit. The nine lines were written by the **gofmt pass that runs
after the fixes are applied**, whose `formatDocComment` sends every doc comment
through `go/doc/comment`'s parser and printer — a subsystem this repo had
stubbed out as a no-op. Porting it closed the case and, more to the point,
closed a formatter gap that no tier here could see: on GOROOT with the doc
comments deliberately un-formatted first, guff diverged from gofmt on 3,341 of
5,608 files while every gate stayed green (docs/COMPAT-HARDENING.md 続き 87).

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

`divergent/staticcheck-qf.diff` holds one hunk out of 481 lines, and it is a
place where **guff is right and upstream is not**, so closing it would be a
regression:

```go
import ( s "strings" )
func renamed() { s.Replace("", "", "", -1) }
```

golangci-lint's QF1004 rewrites that to `strings.ReplaceAll("", "", "")` and
adds no import, so it names a package that is not bound in the file. guff writes
`s.ReplaceAll(...)`, which compiles. That single hunk is why `staticcheck-qf`
builds after guff's `--fix` and would not after upstream's.

It lived in `pending/` until 2026-08-27, alongside the two real gaps the case
also had, because that was the only slot that could hold all three. QF1005
closed the gaps, so what was left was one decision, and `pending/` is the wrong
place for a decision: that ledger goes red the moment guff matches upstream, and
matching here means `--fix` starts breaking user builds. Same call as the
`revive` ratchet — a defect upstream ships is not a specification.

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
* **`# upstream-writes:` is mandatory too**, and must match what upstream writes
  today: `nothing`, or a line count and a digest. The moment upstream's own
  output moves, the declaration stops matching and somebody has to re-read the
  reason and decide again. This started life as "fails if upstream starts
  writing there", which only worked while the answer was *nothing* — see
  `parens` below.
* **A divergence must be a superset.** Every line upstream removes, guff removes
  too. Otherwise one `# why:` could cover a case that is ahead in one hunk and
  behind in another, and only the first half would ever be read.
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

`parens` is the second entry, the same defect, and the reason the slot had to
grow. It is one package holding one 6,241-byte file — the shape the sentence above
says gets fixed — and it is, if `revive` is the only linter enabled. Turn on
`govet`, or `staticcheck`, or `errorlint` alongside it and the same fix
disappears: the extra linter widens the `go/packages` load mode, dependency
sources enter the shared FileSet ahead of the fixture, and byte offset 4,693
stops landing in a file that used to start at base 1. The nine `revive-*` cases
keep their fix only because each of them enables `revive` and nothing else. So
`parens` and `revive-settings` ask for opposite things about the same rule on
the same code, and guff cannot serve both.

Upstream writes seven hunks in `parens` and guff writes those seven plus this
one. That is not the shape `divergent/` was built for — "upstream writes
nothing" was the whole test — and it is why it now declares what upstream
writes instead of assuming.

`staticcheck-qf` is the third entry and a third shape again. It touches no line
upstream leaves alone: it removes exactly what upstream removes and puts
different bytes back, in one hunk of 481 lines, for the QF1004 reason above. The
run line says so rather than reporting "rewrites 0 things upstream leaves
alone", which would read as a bug in the gate instead of the point of the
entry.

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
