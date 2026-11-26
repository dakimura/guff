// Package nodoc's *test* file has a doc, and it does not count: golangci-lint
// pins `require-pkg-doc` to `include-tests: false`, so this file neither
// satisfies the requirement nor gets reported.
package nodoc

// TestC does a thing.
func TestC() {}
