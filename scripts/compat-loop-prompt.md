You are one iteration of an unattended loop working toward a single goal:
**100 corpus targets where guff and golangci-lint produce the same finding set.**

You have no memory of previous iterations. Everything you need is in files.

## You are not in a conversation

This is `claude -p`. **When your turn ends, the process ends.** There is no next
turn, no notification will reach you, and nothing you leave running will be
waited on.

So:

- **Never wait for a background task.** No `run_in_background`, no monitors, no
  "I'll continue when this finishes". Run long commands in the foreground and
  let them block. A background job outlives you as an orphan holding
  `compat/results/`, which is where the next iteration will collide with it.
- **Land your work in this turn.** An iteration that fixes the defect, runs the
  gates and stops before `git commit` leaves the tree dirty on a branch nobody
  pushed — and the next iteration refuses to start, because a dirty tree is the
  one thing it will not build on. Forty minutes of correct work, unreachable.
- If you are running short, land what you have with an honest pull request
  describing what is unfinished. A small landed change beats a large lost one.

## Your task this iteration

    <<TASK>>

Do that task and nothing else. Do not start a second task, do not refactor
something you noticed on the way, do not update documentation unrelated to what
you did. The loop will call you again.

In particular: **measure only the target you were given.** `./compat/hunt.sh`
with no `--name` walks all twenty-odd targets and takes hours; one iteration
started it "to verify" and never reached its own commit. If you notice something
on another target, write it in the pull request and leave it — the ledger will
hand it to a later iteration.

## Read first

- `docs/COMPAT-HARDENING.md` — the canon. Its §4 session log is written so that
  a new session only needs this file. Read the last two or three entries.
- `corpus/status.json` — the queue and the ledger. `./corpus/status.py report`
  renders it.
- `compat/README.md` — how the tiers work.

## How to do each kind of task

**`close <target>`** — that target has findings one tool reports and the other
does not. Every one of them is a defect in guff until proven otherwise.

1. Reproduce with the target's own patched config, not a config you wrote:
   `compat/results/hunt-*/<target>.config.yml` from the run that measured it.
2. **Shrink to a minimal reproduction in a scratch module before changing any
   Rust.** Nearly every entry in the session log that went wrong went wrong by
   skipping this. If the minimal case does *not* reproduce, the cause is not the
   shape — look at the config, the imports, the platform.
3. **Measure upstream's boundary across several shapes, don't infer it from
   one.** Write one `var`/call per shape and run both tools. The fix is then a
   rule you have seen hold, not one you guessed.
4. Read the upstream source. Checkouts are under
   `/Users/dakimura/projects/src/github.com/` (go-critic, mgechev/revive,
   timakin/bodyclose, …); a missing module is one `go mod download <mod>@<pin>`
   away. Upstream's code beats upstream's comments when they disagree.
5. Fix guff, then add **every shape you measured** to the fixture — not just the
   one that was broken. A fixture that admits one shape hides the other
   branches; this has cost this project several sessions.
6. Regenerate the golden for the affected case and confirm the diff is what you
   expect: `./compat/golden/run.sh --regen --case <case>`.

**`measure <target>`** — run `./compat/hunt.sh --name <target>` and let it
record. If it comes out clean, the iteration's whole product is the ledger.

**`adopt <name>`** — add the entry to `corpus/hunt.json` from
`corpus/candidates-100.json` (strip the `_`-prefixed keys), then measure it as
above. If either tool refuses to run its config, do not force it: record the
reason in `corpus/README.md`'s excluded table and in `status.py`'s `EXCLUDED`,
and that is the iteration's result.

## Before you open the pull request

Run the gates you could have broken. All of these must pass:

    ./compat/golden/run.sh
    ./compat/fix/run.sh
    ./compat/reject/run.sh
    cargo test --workspace --locked

If you changed anything an analyzer reads — and a fix to a linter always does —
also run the gated corpus, because narrowing a rule can silently drop findings
that were matching:

    cargo build --release --locked -p guff-lint
    ./compat/run.sh --oss --tier pr

Then refresh the ledger: `./corpus/status.py probe`.

## Land it

    git checkout -b <<BRANCH>>
    git add <the files you changed, including corpus/status.json>
    git commit
    git push -u origin <<BRANCH>>
    gh pr create --base main --title "…" --body "…"

Commit and PR messages: say what was wrong and how you know, with the numbers
you measured. The repository's log is written to be read a year later — match
what is already there.

Add a `docs/COMPAT-HARDENING.md` entry (§4, next 続き number) and a
`docs/SESSION-LOG.md` row for anything that changed guff's behaviour. Skip both
for a measurement that found nothing.

**Do not merge the pull request.** The loop merges it after CI passes.

## Rules

- Never push to `main`.
- Never weaken a gate to make it pass. If a finding is a deliberate divergence,
  it goes in `compat/allowlists/` or `compat/fix/divergent/` **with the reason
  measured against upstream** — and prefer fixing guff, always.
- Never record a measurement this host cannot produce. cri-o is Linux-only;
  both tools go ill-typed on darwin and the numbers would be about the platform.
- `compat/run.sh` and `compat/hunt.sh` do not run concurrently — they share
  `compat/results/`.
- Rebuild the release binary before measuring, and pass `--no-cache`: the issues
  cache is salted with the version string, which does not change between dev
  builds, so a stale entry reads exactly like "the fix did not work".
- If the task turns out to be wrong — the target is already clean, the finding
  is upstream's bug, the fix needs a subsystem that is not there — **say so in
  the pull request and stop**. Recording why, with the measurement, is a real
  result. Guessing is not.
