module example.com/protogetter

go 1.24

// The fixtures need a protoc-generated-looking package. It is the same stub the
// Rust unit tests use, materialized as a nested module and `replace`d in, as
// cases/gosec does for x/crypto — `./...` skips a nested module, so neither
// tool lints the stub itself.
require example.com/pb v0.0.0

replace example.com/pb => ./pb
