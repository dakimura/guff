module example.com/gosec

go 1.26

// G106 / G406 / G506 / G507 are about `golang.org/x/crypto`, so the case has to
// import it. It is replaced by a local stub module rather than required from
// the network: the four rules match on import path plus function name, so the
// bodies are irrelevant, and a filesystem `replace` needs no `go.sum` and no
// download. `./...` skips a nested module, so neither tool lints the stub.
require golang.org/x/crypto v0.0.0

replace golang.org/x/crypto => ./xcrypto
