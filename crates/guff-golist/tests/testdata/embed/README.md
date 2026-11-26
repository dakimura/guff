# `//go:embed` shapes

Every shape here was measured against `go list -e -json=EmbedPatterns,EmbedFiles,Error`
and golangci-lint 2.12.2 (which reports the error as its `typecheck` pseudo
linter) before it was written down. The Rust test that reads this tree is
`crates/guff-golist/tests/embed_shapes.rs`; `compat/golden/cases/typecheck-embed`
lints two of the same files with both tools.

There is deliberately **no `go.mod`**: these directories are read by
`Context::import_dir`, which does not need one, and a nested module inside the
Rust workspace would be picked up by `go` tooling run at the repo root.
