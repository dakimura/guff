# Adding a new analyzer to guff

This guide walks through adding a `go/analysis`-style linter to the guff
pipeline: define an [`Analyzer`](crates/guff-analysis/src/analyzer.rs), register
it with the runner, add fixture sources, and verify diagnostics with
`cargo test`.

The smoke analyzers [`printast`](crates/guff-analysis/src/passes/printast.rs) and
[`printf`](crates/guff-analysis/src/passes/printf.rs) are the reference
implementations from Phase 7.

## 1. Define the analyzer

Create a module under `crates/guff-analysis/src/passes/`:

```rust
use std::sync::OnceLock;

use crate::analyzer::{AnalysisResult, Analyzer, RunError, RunFn};
use crate::pass::Pass;

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    // Read AST via pass.files(), types via pass.types_info(), etc.
    pass.reportf(pos, "message");
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "myanalyzer",
        doc: "short description",
        url: "",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],           // or vec![inspect_analyzer()]
        fact_types: vec![],
    })
}
```

Export it from `crates/guff-analysis/src/passes/mod.rs`.

### Depends on another analyzer

List prerequisites in `requires`. The runner builds a DAG and runs dependencies
first on the same package. Use `pass.result_of::<T>(other_analyzer())` to read a
dependency's result (see `printast` / `printf`).

### Needs type information

Analyzers that call `pass.types_info()` need packages loaded with
`LoadMode::LOAD_SYNTAX` (or `guff_packages::load_for_go_analysis()`). When using
[`guff_runner::run`], pass a load-mode override:

```rust
use std::collections::HashMap;
use guff_runner::{run, types_load_mode, RunnerOptions};

let overrides = HashMap::from([("myanalyzer", types_load_mode())]);
let result = run(&cfg, &patterns, &[my_analyzer()], &overrides, &RunnerOptions::default())?;
```

[`infer_load_mode`](crates/guff-runner/src/load_mode.rs) defaults to AST-only
unless the analyzer exports facts.

## 2. Validate the analyzer graph

Call `guff_analysis::validate::validate(&[analyzer()])` in a unit test (or rely
on the runner, which validates before execution). Cycles in `requires` are
rejected.

## 3. Add testdata

Place a mini Go module under:

```
crates/guff-runner/tests/testdata/smoke/<name>/
  go.mod
  main.go
```

Keep fixtures small and focused on one diagnostic.

## 4. Write an integration test

Add a test in `crates/guff-runner/tests/` (see
[`smoke_test.rs`](crates/guff-runner/tests/smoke_test.rs)):

1. Type-check the fixture with `typecheck_package` and `LoadMode::LOAD_SYNTAX`.
2. Run `guff_runner::run_on_packages` with your analyzer.
3. Assert `result.diagnostics()` contains the expected message.

For the full `go list` → load → run pipeline (requires `go` on `PATH`), add a
second test marked `#[ignore]` and run:

```bash
cargo test -p guff-runner -- --ignored
```

## 5. Run tests

```bash
cd projects/guff
cargo test -p guff-analysis
cargo test -p guff-runner
```

## Quick checklist

| Step | Location |
|------|----------|
| Analyzer `run` function | `guff-analysis/src/passes/<name>.rs` |
| Export | `guff-analysis/src/passes/mod.rs` |
| Fixture sources | `guff-runner/tests/testdata/smoke/<name>/` |
| E2E test | `guff-runner/tests/smoke_test.rs` (or new file) |
| Load-mode override (if types needed) | `HashMap` passed to `guff_runner::run` |

## Next steps

With Phase 7 complete, individual staticcheck rules and other linters can be
ported as separate analyzers following this same pattern.

**Staticcheck-specific progress and remaining tasks**:
[`STATICCHECK-MIGRATION.md`](STATICCHECK-MIGRATION.md)
