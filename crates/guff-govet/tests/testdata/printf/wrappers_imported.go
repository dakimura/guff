package imported

// The one shape from the wrapper grid that guff and golangci-lint disagree on,
// kept out of `wrappers.go` because the golden tier has no allowlist: it runs
// real golangci-lint over the fixture and a known divergence there is a red
// gate, not a record.
//
// guff analyses only the packages being linted and `printf` advertises no
// object facts, so a wrapper declared in another package is never recognised.
// Upstream exports an `isWrapper` fact from `sub` and reports
// `format %z has unknown verb z` on the call below; guff is silent. Silence is
// the direction guff already had, and the fan-out that facts would need is its
// own measurement — see the DEFERRED note in `printf_wrappers`.

import "example.com/govet/printf/sub"

func importedWrapper() { sub.ExportedWrapf("g %z", 1) }

// Same signature, forwards nothing: silent in both tools.
func importedNonWrapper() { sub.ExportedNotAWrapper("h %z", 1) }
