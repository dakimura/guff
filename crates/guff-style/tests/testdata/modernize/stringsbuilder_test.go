package modernize

// The file name is the whole point. Upstream's only gate is
// `within(pass, "strings", "runtime")` — the two packages where the fix would
// make an import cycle — and nothing in `stringsbuilder.go`, or anywhere else
// in modernize, looks at whether the file ends in `_test.go`. guff skipped
// every test file from the rule's first commit, with no reason recorded, and
// beats accumulates a status string in a loop in
// `heartbeat/monitors/wrappers/summarizer/summarizer_test.go:173`.

func builderInTestFile(xs []string) string {
	s := ""
	for _, x := range xs {
		s += x // FINDING
	}
	return s
}

// The same selection rule as in a non-test file: only the first candidate of a
// loop nest is reported.
func builderTwoInOneLoop(xs []string) (string, string) {
	a := ""
	b := ""
	for _, x := range xs {
		a += x // FINDING
		if x != "" {
			b += x
		} else {
			b += "_"
		}
	}
	return a, b
}
