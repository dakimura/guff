# The reject tier — configs golangci-lint refuses to start on

Every other tier under `compat/` compares two finding sets. This one exists for
the configs where upstream produces no finding set at all.

golangci-lint validates its configuration before it lints anything
(`config.Config.Validate`, plus per-linter settings validators such as
gocritic's `validateOptionsCombinations`), and a config that fails validation
ends the process with exit code 3. guff used to accept every one of these and
run — which is worse than either alternative, because it lints with an enable
set the user never asked for and nothing downstream can tell:

```yaml
linters:
  exclusions:
    rules:
      - linters: [errcheck]     # one condition: upstream will not start
```

golangci-lint calls that a config error. guff read it as "exclude every errcheck
finding" and printed a clean run.

## What a case is

```
cases/<name>/config.yml     the config
cases/<name>/expected.txt   golangci-lint's reason, recorded by --regen
cases/<name>/accepts        marker: a control case both tools must run
```

Cases are pointed at `compat/reject/` itself — one package, one function, no
findings. Neither tool should get as far as reading it; a case that started
passing because the *code* changed would be a case that stopped testing the
config.

## What is compared

The **reason**, not the rendering. golangci-lint prints config errors as
`Error: <reason>` and per-linter settings errors through its logger as
`level=error msg="[linters_context] <reason>"`; guff prefixes its own name.
`reject.py` strips the frame from each and compares what is left, so these two
lines are a match:

```
Error: can't load config: invalid preset: stdErrorHandling
guff: can't load config: invalid preset: stdErrorHandling
```

Three things are asserted per case: golangci-lint **still** refuses it, guff
refuses it, and the reasons are equal. The first of those matters as much as the
others — a recorded expectation upstream has stopped producing is a case that
has quietly stopped testing anything.

Expected reasons are generated, never hand-written (`--regen`), the same rule
`compat/golden` follows: nobody types an expected value, so no assumption can
smuggle itself in.

## `_control`

A config that exercises the same keys *validly* — presets, an exclude rule with
two conditions, a severity default, `path-mode: abs` — and that both tools must
run. Without it, a tier whose job is to see failures could pass while failing
everything.

## Running

```sh
./compat/reject/run.sh                    # every case (CI gate)
./compat/reject/run.sh --case output-path-mode-rel
./compat/reject/run.sh --regen            # re-record golangci-lint's reasons
```

`reject.py`'s reason extraction is unit-tested in `compat/tests/test_reject.py`;
the rules themselves are asserted at the unit level in
`crates/guff-lint/tests/config_validate_test.rs`.

## Rules covered

| case | upstream |
|---|---|
| `exclude-rule-one-condition` | `BaseRule.Validate(2)` |
| `exclude-rule-path-and-path-except` | `BaseRule.Validate` |
| `exclusion-preset-camel-case` | `LinterExclusions.Validate` (kebab-case vocabulary only) |
| `severity-rules-without-default` | `Severity.Validate` |
| `severity-default-v1-spelling` | same rule; `default-severity` is v1's key and sets nothing in v2 |
| `output-path-mode-rel` | `Output.validatePathMode` (only `""` and `abs` exist) |
| `formatter-under-linters` | `Linters.validateNoFormatters` |
| `linter-under-formatters` | `Formatters.Validate` |
| `gocritic-enable-all-and-enabled-tags` | `gocritic.validateOptionsCombinations` |
| `gocritic-disable-all-and-disabled-checks` | same |
| `gocritic-disable-all-enabling-nothing` | same |

Not covered, deliberately: upstream also compiles `path` / `path-except` /
`text` / `source` as **Go** regexes and rejects the config when one fails. guff
matches with Rust's `regex`, and the dialects disagree in both directions, so
porting that check would reject configs golangci-lint runs.

Also measured but not matched: upstream exits **3** on all of these, guff exits
**2** (its documented error code, `docs/COMPATIBILITY.md`). The tier asserts
"both refuse", not the number.
