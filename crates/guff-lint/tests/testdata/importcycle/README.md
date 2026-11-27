# importcycle

Import cycles, for the `typecheck-import-cycle*` golden cases. Every package
imports `example.com/cycle/...`, the module path of those cases' `go.mod`.

| dir | shape | golangci-lint 2.15.0 |
|-----|-------|----------------------|
| `pair` | `a` → `b` → `a` | `a/a.go:1: : import cycle not allowed: import stack: [… a … b … a]` |
| `self` | `a` → `a` | `a/a.go:1: : import cycle not allowed: import stack: [… a … a]` |
| `test` | `b_test.go` (package `b`) → `a` → `b` | `b/b.go:1: : import cycle not allowed in test: import stack: [… b … a … b]` |

`go list` gives the error an empty `Pos`, and golangci-lint's `extractErrors`
moves it to line 1 of the package's first Go file with the empty position kept
as a prefix — hence the `: ` before the message.
