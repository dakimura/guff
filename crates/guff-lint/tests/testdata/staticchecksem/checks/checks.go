package checks

import "fmt"

// The `staticcheck.checks` cases need findings from several *categories*,
// because the selector grammar is category-aware: `S*` matches S1002 and not
// SA1006, since a name's category is the part before its first digit. One
// finding per category is enough, plus one check (SA9003) that exists to
// measure what the *default* list turns off.

// SCategory has an `== true` comparison: S1002.
func SCategory(b bool) bool {
	if b == true {
		return true
	}
	return false
}

// SACategory calls a printf-style function with a dynamic format string and no
// arguments: SA1006.
func SACategory(s string) {
	fmt.Printf(s)
}

// STCategory writes the comparison the wrong way round: ST1017.
func STCategory(x int) bool {
	if 1 == x {
		return true
	}
	return false
}

// QFCategory can have its `if/else if` chain rewritten: QF1003.
func QFCategory(x int) string {
	if x == 1 {
		return "one"
	} else if x == 2 {
		return "two"
	} else if x == 3 {
		return "three"
	}
	return fmt.Sprint(x)
}

// EmptyBranch is here for the default check list rather than for a category:
// SA9003 ("empty branch") is *not* one of the six checks golangci-lint turns
// off when `checks` is unset — that list is all-ST.
func EmptyBranch(x int) {
	if x == 1 {
	}
}
