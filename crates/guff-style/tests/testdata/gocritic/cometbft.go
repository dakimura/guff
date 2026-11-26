// Package cometbft carries the two gocritic shapes cometbft v0.40.0 turned up:
// a `dupBranchBody` false positive and an `ifElseChain` false negative.
package cometbft

func send(a, b int) {}

func assertErr(err error) (int, bool) { return 0, err != nil }

// `dupBranchBody` compares branch bodies structurally upstream
// (`astequal.Stmt`), so a `go`/`defer` statement's arguments count. guff
// rendered both forms as `go f(...);`, eliding every argument, and then called
// two branches identical when only the arguments differed —
// cometbft `consensus/byzantine_test.go:512` sends `proposal1` in one branch
// and `proposal2` in the other.
func dupBranchGoArgs(n, p1, p2, h1, h2 int) {
	// silent: the arguments differ
	if n < 2 {
		go send(p1, h1)
	} else {
		go send(p2, h2)
	}

	// silent: likewise for defer
	if n < 3 {
		defer send(p1, h1)
	} else {
		defer send(p2, h2)
	}

	// reported: genuinely the same body
	if n < 4 {
		go send(p1, h1)
	} else {
		go send(p1, h1)
	}
}

// `countIfelseLen` gives up on an `if` carrying an init statement, but marks
// the chain visited only *as it walks* — so giving up at the head leaves the
// rest unvisited, and the walker counts again from the first `else if`. guff
// marked the whole chain up front, so a head with an init swallowed it.
// cometbft `consensus/state.go` is this shape.
func ifElseChainWithInit(err error, n int) string {
	// reported at the first `else if` below, not at this `if`
	if v, ok := assertErr(err); ok {
		_ = v
		return "assert"
	} else if n == 1 {
		return "one"
	} else if n == 2 {
		return "two"
	} else {
		return "other"
	}
}

// The control: the same chain without an init statement, reported at the head.
func ifElseChainNoInit(a bool, n int) string {
	if a {
		return "a"
	} else if n == 1 {
		return "one"
	} else if n == 2 {
		return "two"
	} else {
		return "other"
	}
}
