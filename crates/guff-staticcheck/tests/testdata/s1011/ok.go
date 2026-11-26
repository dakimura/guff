package main

type holder struct{ items []string }

func makeSrc() []string { return nil }

func bar() map[int][]int { return nil }

// Not the loop variable.
func appendsSomethingElse(src []string) []string {
	var dst []string
	for _, v := range src {
		dst = append(dst, v+"x")
	}
	return dst
}

// The index is used for something, so the loop is not just a copy.
func usesTheIndex(src []string) []string {
	var dst []string
	for i, v := range src {
		if i > 0 {
			dst = append(dst, v)
		}
	}
	return dst
}

// Upstream's own example: "The lhs may be dynamic and return different values
// on each iteration." Silent before this change only because the destination
// had to be an identifier — now it is the side-effect guard that keeps it so.
func aDynamicDestination(x []int) {
	for i := range x {
		bar()[0] = append(bar()[0], x[i])
	}
}

// An index-based loop evaluates x every iteration, so an impure x disqualifies
// it. The value-based form of this is in bad.go, where x is evaluated once.
func indexBasedLoopOverACall(dst []string) []string {
	for i := range makeSrc() {
		dst = append(dst, makeSrc()[i])
	}
	return dst
}

var _ = holder{}
