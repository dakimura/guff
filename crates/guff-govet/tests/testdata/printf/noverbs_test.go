package printf

import "testing"

// `Logf` is on upstream's table and was not on guff's short-name heuristic, so
// this shape went unreported. The receiver named in the message is the
// *concrete* one the method is declared on.
func TestNoVerbs(t *testing.T) {
	t.Errorf("no verbs here", 1)
	t.Logf("no verbs here", 1)
	t.Fatalf("no verbs here", 1)
}
