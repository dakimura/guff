package modernize

// The file name is the whole point. golangci-lint 2.12.2 (x/tools v0.44)
// reported both loops below; 2.14.0 (x/tools v0.50) skips every file ending in
// `_test.go` — "suggested fixes may increase verbosity, and performance
// doesn't matter as much" (go.dev/issue/78613). The markers below are what
// 2.12.2 said; the golden case records that 2.14.0 says nothing. beats
// accumulates a status string in a loop in
// `heartbeat/monitors/wrappers/summarizer/summarizer_test.go:173`.

func builderInTestFile(xs []string) string {
	s := ""
	for _, x := range xs {
		s += x // reported by 2.12.2 only
	}
	return s
}

// The same selection rule as in a non-test file: only the first candidate of a
// loop nest is reported.
func builderTwoInOneLoop(xs []string) (string, string) {
	a := ""
	b := ""
	for _, x := range xs {
		a += x // reported by 2.12.2 only
		if x != "" {
			b += x
		} else {
			b += "_"
		}
	}
	return a, b
}
