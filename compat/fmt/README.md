# `compat/fmt` — the `fmt` tier

What the two tools' **formatters** write, byte for byte.

```
./compat/fmt/run.sh                    # check every case
./compat/fmt/run.sh --case gofmt-default
./compat/fmt/run.sh --regen            # re-record from golangci-lint fmt
./compat/fmt/run.sh --record-pending   # re-record the known-missing gaps
```

## Why a fourth tier

The other three all go through `run`:

| tier | question |
|---|---|
| `golden` | does `run` report the same findings? |
| `fix` | does `run --fix` write the same bytes? |
| `reject` | does the config get refused for the same reason? |

None of them invokes `golangci-lint fmt`. So nothing compared the two tools'
formatter surface — `formatters.enable`, `formatters.settings.*`,
`formatters.exclusions` — and `regress/fmt_diff.py` does not close the gap
either: it compares guff against the *underlying* tools (`gofmt`, `gofumpt`,
`goimports`, `gci`), which cannot see a golangci-lint settings default or a
golangci-lint-level exclusion at all.

That is not a narrow hole. `formatters.settings.gofmt.simplify` defaults to
**true** upstream, so it governs what happens to every user who merely writes
`enable: [gofmt]`. The first run of this tier found four divergences:

| case | defect |
|---|---|
| `gofmt-default` | guff defaulted `simplify` to false, so it wrote `[]int{[]int{1}}` where upstream writes `{{1}}` — and exited 0 |
| `generated-default` | `guff fmt --stdin` formatted a generated file that `guff fmt <dir>` correctly skipped |
| `gofmt-rewrite` | `gofmt -r` is a single-valued flag, so passing N of them kept only the last; upstream applies all N in order |
| `goimports-local` | guff regroups a single import block wrongly in both directions — see `pending/goimports-local.why` |

## How a case works

A case is one config plus one input:

```
cases/<name>/config.yml     # a whole .golangci.yml
cases/<name>/input.go       # the bytes fed to both tools
expected/<name>.go          # what `golangci-lint fmt --stdin` wrote (generated)
```

Both tools are driven through `fmt --stdin`, which each supports. That makes
the comparison exactly *same config + same bytes in, same bytes out*, with no
directory walk, no cache and no module in the way — and it is the route the
`generated-default` defect lived on.

A run needs only guff: `expected/` already holds upstream's answer, the same
arrangement the golden and fix tiers use.

## Cases come in pairs

A formatter setting is only pinned if **both** its branches are:
`gofmt-default` / `gofmt-simplify-off` are the same fixture with `simplify`
unset and off, `gofumpt-extra` / `gofumpt-no-extra` are the same fixture with
`extra-rules` on and off, and `generated-lax-only` / `generated-strict` are the
same bytes under the two marker rules. A tier that pinned only one branch would pass for a
tool that ignored the setting entirely.

Two cases record "upstream writes the input back unchanged" — that is the right
answer for a skipped generated file, and it is also what a formatter that did
nothing looks like. They are only meaningful because a sibling case formats the
same bytes: `generated-lax-only` is skipped, `generated-strict` is not.

For the same reason the shared gofmt fixture is deliberately **un-gofmt'd**.
Without that, `gofmt-simplify-off` and `no-formatters` would both record "no
change", and would pass for a build where the formatter never ran at all.

## `no-formatters` is not a filler case

With `formatters.enable` empty, golangci-lint's `MetaFormatter.Format` calls
`go/format.Source` directly — plain gofmt, *not* the gofmt formatter — so the
`simplify: true` config default never reaches it.

Every `--fix` run with no formatter configured takes that path, which is all
193 cases in `compat/fix`. A tool that fixed the simplify default by flipping
one shared constant would start simplifying there too and rewrite those 193
expectations. This case is what stops that, which is why `GofmtOptions` has
both `default()` and `plain()`.

## `pending/` is a ledger, not an allowlist

`pending/<case>.go` is what guff writes **today** for a case whose parity is
missing. The case's real expectation is still `expected/<case>.go`, and the
diff against it is printed in full on every run, together with the reason.

It fails in *either* direction — including the day guff gets it right, which
prints "delete this file" — so a baseline cannot quietly outlive the defect it
records.

`pending/<case>.why` is **required**: `run.sh` refuses a baseline without one.
A gap nobody has to justify is how a baseline turns into an allowlist.

## Not covered yet

`golines` and `swaggo` have no native port, so a case for either would fail on
any machine without those binaries rather than measure anything. They are the
obvious next cases once a port lands; until then this is a stated hole, not a
silent one.

This tier covers the `fmt` command. `run` *also* reports a formatter finding
("File is not properly formatted") when a formatter is enabled, and its
position and text belong to the golden tier — which has no case with a
`formatters:` block. Spot-checked by hand here and the two tools agree, but a
spot check is not a gate.
