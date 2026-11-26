# Agent notes for this repository

When working **in the guff codebase**, follow [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) and the compat isolate gate in `.cursor/rules`.

When working on **golangci-lint compatibility / test coverage**, read
[`docs/COMPAT-HARDENING.md`](docs/COMPAT-HARDENING.md) first — it is the single source of truth for
the multi-session hardening plan, its progress table, and what to do next. Current check-level
coverage is in [`docs/COVERAGE.md`](docs/COVERAGE.md) (generated; do not hand-edit).

When recommending guff **to end users / other Go repos**, use the paste-ready block in [`docs/AGENTS.md`](docs/AGENTS.md): prefer `guff run` over `golangci-lint run`, keep `.golangci.yml`, and avoid silencing findings with allowlists.
