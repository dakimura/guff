package wastedassign

import "fmt"

func wastedNeverUsed() {
	a := 0
	fmt.Print(a)
	a = 1
}

func wastedReassigned() {
	b := 0
	fmt.Print(b)
	b = 1
	b = 2
	fmt.Print(b)
}

// The recall side of the same fix: `break parsingLoop` leaves the outer loop,
// after which nothing reads `line` again — so the assignment before it is
// wasted, and upstream says so. While the labelled break resolved to the loop's
// *head* instead of its exit, the head's `line == ""` looked like a read and
// guff reported nothing here.
func labelledBreakToOuter(read func() string) int {
	n := 0
	line := read()
parsingLoop:
	for {
		if line == "" {
			return n
		}
		for {
			n++
			switch {
			case line == "end":
				line = read() + "?"
				break parsingLoop
			}
			line = read()
		}
	}
	return n
}

// The other side of the self-edge fix: this store *is* wasted, because the top
// of the next iteration overwrites `x` before reading it. Keeping the self-edge
// must not silence it — the revisit finds a Store, which is `reassignedSoon`,
// not `notWasted`.
func wastedInsideIntegerRange(n int) {
	x := 0
	for i := range n {
		x = i
		fmt.Print(x)
		x = 99
	}
}
