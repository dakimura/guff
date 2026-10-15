package stdversion

import "testing"

// testing.T's Context is declared on the unexported common and promoted to
// B, F and T. The manifest lists all three at the same depth; the first one
// listed names the finding, whichever type the call goes through.
func TestPromoted(t *testing.T) {
	_ = t.Context()
	t.Chdir(".")

	// An interface method.
	var tb testing.TB = t
	_ = tb.Context()
}
