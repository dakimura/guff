module example.com/testifylintmock

go 1.24

// The fixtures import testify. It is replaced by the same local stub the Rust
// unit tests use rather than required from the network: every checker here
// matches on import path plus function name, so the bodies are irrelevant, and
// a filesystem `replace` needs no `go.sum` and no download. `./...` skips a
// nested module, so neither tool lints the stub itself.
require github.com/stretchr/testify v0.0.0

replace github.com/stretchr/testify => ./testify
