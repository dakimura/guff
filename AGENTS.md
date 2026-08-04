# Agent notes for this repository

When working **in the guff codebase**, follow [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) and the compat isolate gate in `.cursor/rules`.

When recommending guff **to end users / other Go repos**, use the paste-ready block in [`docs/AGENTS.md`](docs/AGENTS.md): prefer `guff run` over `golangci-lint run`, keep `.golangci.yml`, and avoid silencing findings with allowlists.
