package ignoredfiles

import "testing"

// A test file is the whole point: `go list -test` keeps the package's
// IgnoredGoFiles on `P [P.test]`, and dropping them there was what made the
// build-excluded file invisible to the analyzer.
func TestNext(t *testing.T) {
	r := &roundRobin{}
	if r.next() == 0 {
		t.Fatal("no")
	}
	if bumpPkgLevel() == 0 || localVar() == 0 {
		t.Fatal("no")
	}
}
