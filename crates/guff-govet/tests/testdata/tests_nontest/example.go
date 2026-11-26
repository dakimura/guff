// Package testsnontest holds an ORDINARY file — its name does not end in
// `_test.go`.
//
// Upstream's `tests` analyzer opens with a per-file
// `if !strings.HasSuffix(<this file>, "_test.go") { continue }`, so none of the
// Example/Test signature rules apply here. guff asked instead whether *any*
// file in the package was a test file, so every ordinary file in a package that
// also had tests got the rules applied to it. go-ethereum's
// `metrics/internal/sampledata.go` is exactly that: an ordinary file declaring
// `func ExampleMetrics() metrics.Registry`, sitting in a package whose other
// file is a test.
//
// These are the same two shapes as tests/bad_test/bad_test.go, so the pair is a
// control: reported there, silent here.
package testsnontest

func Example() int { return 1 }

func Example_suffix(n int) {}
