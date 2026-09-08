package p

import "testing"

func Testbad(t *testing.T) {}

// Upstream reports every checkExampleName finding at `fn.Pos()` — the `func`
// keyword — while the malformed-name findings below it come from
// `ReportRangef(fn.Name, ...)` and land on the identifier. guff used the name
// for both, and the hunt tier does not compare columns, so a wrong column read
// as a match. Only the golden tier looks at columns.
//
// `Example` and `Example_suffix` are deliberate: both return early from the
// identifier-resolution half of checkExampleName (the bare name short-circuits,
// and a lowercase suffix is a valid one), so what is pinned here is exactly the
// niladic/return-nothing pair and its column, with no dependency on the
// "refers to unknown identifier" check that guff does not implement.
func Example() int { return 1 }

func Example_suffix(n int) {}
