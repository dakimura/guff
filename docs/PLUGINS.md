# Custom linters (module plugins)

guff supports repository-specific linters through the same workflow golangci-lint
uses for [module plugins](https://golangci-lint.run/plugins/module-plugins/):
you write a plugin, declare it in `.custom-gcl.yml`, build a custom binary, and
enable it from `.golangci.yml`.

The difference is the language. A golangci-lint module plugin is a Go module
implementing `register.LinterPlugin`. A guff plugin is a **Rust crate**
implementing `guff_plugin::LinterPlugin`. Everything else — the build config file,
the `linters.settings.custom` block, the resulting binary — is deliberately the
same shape.

Why bother writing a rule as a linter instead of a natural-language rule in
`CLAUDE.md` or `.cursor/rules`: a linter is deterministic and runs in
milliseconds, on every save and every CI run, with the same answer every time.

## Requirements

`guff custom` shells out to `cargo`, so unlike golangci-lint's plugin flow it needs:

- a Rust toolchain (`cargo` on `PATH`)
- the guff **source workspace**, located either because your `guff` binary was
  built from it, or via `GUFF_SRC=/path/to/guff`

```bash
git clone https://github.com/dakimura/guff
export GUFF_SRC="$PWD/guff"
```

This is a real cost compared to upstream, and it is the main reason plugins are
not part of the five-minute migration path. Everything below assumes you accept
a one-time toolchain setup in exchange for a plugin that runs at guff speed.

## 1. Write the plugin crate

A plugin is an ordinary Rust library crate. The complete working example lives at
[`crates/guff-plugin-example`](../crates/guff-plugin-example) — it mirrors
[golangci/example-plugin-module-linter](https://github.com/golangci/example-plugin-module-linter)
and reports `// TODO:` comments with no author.

`Cargo.toml`:

```toml
[package]
name = "my-linter"
version = "0.1.0"
edition = "2021"

[lib]
name = "my_linter"
path = "src/lib.rs"

[dependencies]
guff-plugin = { path = "/path/to/guff/crates/guff-plugin" }
guff-ast    = { path = "/path/to/guff/crates/guff-ast" }
serde       = { version = "1", features = ["derive"] }
serde_yaml  = "0.9"
```

`src/lib.rs`:

```rust
use std::sync::OnceLock;

use guff_plugin::guff_analysis::passes::inspect;
use guff_plugin::{
    decode_settings, AnalysisResult, Analyzer, LinterPlugin, Pass, PluginError, RunError, RunFn,
};
use serde::Deserialize;
use serde_yaml::Value;

// Registers the factory under the linter name used in .golangci.yml.
// Equivalent to Go's init() + register.Plugin.
guff_plugin::register!("mylinter", new_plugin);

// Required: keeps the linker from dropping this crate in the generated binary.
pub const FORCE_LINK: () = ();

#[derive(Debug, Clone, Default, Deserialize)]
struct MySettings {
    #[serde(default)]
    forbidden_prefix: String,
}

struct MyPlugin {
    settings: MySettings,
}

// Factory — golangci's `func(any) (register.LinterPlugin, error)`.
fn new_plugin(settings: &Value) -> Result<Box<dyn LinterPlugin>, PluginError> {
    let settings = decode_settings::<MySettings>(settings)?;
    Ok(Box::new(MyPlugin { settings }))
}

impl LinterPlugin for MyPlugin {
    fn build_analyzers(&self) -> Result<Vec<&'static Analyzer>, PluginError> {
        Ok(vec![analyzer()])
    }

    fn description(&self) -> &'static str {
        "reject identifiers with a forbidden prefix"
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    // Settings reach the run function through the pass settings bag.
    let raw = pass.settings::<Value>("mylinter").cloned().unwrap_or(Value::Null);
    let opts = decode_settings::<MySettings>(&raw).unwrap_or_default();

    let _inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "mylinter requires the inspect analyzer".to_string())?;

    // ... walk the AST, and for each violation:
    // pass.reportf(pos, format!("identifier starts with {:?}", opts.forbidden_prefix));

    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "mylinter",
        doc: "reject identifiers with a forbidden prefix",
        url: "https://example.com/mylinter",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
```

Three things are mandatory and easy to forget:

1. `guff_plugin::register!("<name>", <factory>)` at the crate root. The name is
   what you write in `.golangci.yml`.
2. `pub const FORCE_LINK: () = ();`. `guff custom` emits a reference to it in the
   generated `main.rs`; without it the linker discards the crate and the plugin
   silently never registers.
3. `requires: vec![inspect::analyzer()]` if you use the shared AST traversal.

## 2. Declare it in `.custom-gcl.yml`

Same file name and schema as golangci-lint. `.custom-guff.yml` is also accepted;
`.custom-gcl.yml` wins if both exist. Discovery walks up from the working directory.

```yaml
version: "0.6.0"          # informational; a mismatch is a warning, not an error
name: custom-guff         # output binary name (default: custom-guff)
destination: ./bin        # output directory (default: .)

plugins:
  # local crate during development
  - module: my-linter
    path: ./tools/my-linter

  # or fetched from git by tag
  - module: github.com/you/my-linter
    version: v0.1.0
```

| Key | Meaning |
|---|---|
| `module` | Crate identity. With `version`, a `github.com/...` value is fetched as a git dependency. |
| `path` | Local path to the crate. Relative paths resolve against the config file's directory. |
| `version` | Git tag (or crates.io version) when `path` is absent. |
| `import` | Overrides the crate name derived from the last path segment of `module`. |

Exactly one of `path` or `version` is required per plugin.

## 3. Build

```bash
guff custom            # discovers .custom-gcl.yml
guff custom -c path/to/.custom-gcl.yml
guff custom -v         # verbose cargo output
```

This generates a small Cargo project under `<destination>/.guff-custom-build`,
runs `cargo build --release`, and copies the binary to `<destination>/<name>`.
The first build compiles the whole guff workspace and is slow; later builds are
incremental.

## 4. Enable it in `.golangci.yml`

Identical to golangci-lint. `type` must be `module`.

```yaml
version: "2"

linters:
  enable:
    - mylinter
  settings:
    custom:
      mylinter:
        type: module
        description: reject identifiers with a forbidden prefix
        settings:
          forbidden-prefix: tmp_
```

Then run the binary you built, not the stock `guff`:

```bash
./bin/custom-guff run ./...
./bin/custom-guff linters | grep mylinter
```

`linters.settings.custom.<name>.settings` is handed to your factory verbatim as
YAML, so `decode_settings::<T>()` maps it onto any `serde::Deserialize` type.
`description` is used by `guff linters` when the plugin returns an empty one.

## API reference

Everything below is re-exported from `guff_plugin`.

| Item | golangci-lint counterpart |
|---|---|
| `LinterPlugin` trait | `register.LinterPlugin` |
| `build_analyzers()` | `BuildAnalyzers()` |
| `register!(name, factory)` | `register.Plugin(name, New)` |
| `decode_settings::<T>()` | `register.DecodeSettings[T]()` |
| `Analyzer` | `*analysis.Analyzer` |
| `Pass` | `*analysis.Pass` |
| `Diagnostic`, `SuggestedFix`, `TextEdit` | same names in `go/analysis` |

Useful `Pass` methods:

- `pass.reportf(pos, msg)` — report a diagnostic at a byte position
- `pass.files()` — parsed `ast::File`s for the package
- `pass.pkg()` — the type-checked package (`compiled_go_files`, type info)
- `pass.fset()` — the `FileSet` for position → line/column
- `pass.result_of::<T>(analyzer)` — depend on another analyzer's result
- `pass.settings::<T>(name)` — this linter's settings from config

Autofix works the same way as built-in linters: attach a `SuggestedFix` with
`TextEdit`s to your `Diagnostic` and `guff run --fix` will apply it.

## Differences from golangci-lint

- **Language**: Rust crate, not a Go module. Existing Go plugins do not port
  automatically; the analyzer logic has to be rewritten.
- **Build inputs**: needs `cargo` and the guff source workspace (see Requirements).
- **`type: goplugin`** (the `.so` flow) is **not supported**. guff parses the key,
  prints `linters.settings.custom.<name>: type "goplugin" is not supported` to
  stderr, and skips that entry — the run continues, so watch stderr rather than
  the exit code when porting such a config.
- Plugin instances are cached per name for the lifetime of the process, so `New`
  runs once even across many packages.

## Troubleshooting

**`plugin "x" is not registered in this binary`**
`register!` is missing, the crate name in `.custom-gcl.yml` does not match, or
`FORCE_LINK` is absent and the linker dropped the crate.

**`cannot locate guff source`**
Set `GUFF_SRC` to a guff checkout, or use a `guff` built from source.

**`plugin <module>: need path or version`**
Every entry in `plugins:` needs one of the two.

**`guff: linters.settings.custom.x: type "goplugin" is not supported`**
A stderr warning, not a failure. Only `module` plugins exist in guff; the entry
is skipped. See Differences above.

**The linter builds but never fires**
Confirm it is in `linters.enable`, and that you are running the custom binary
rather than the stock `guff` on your `PATH`.
