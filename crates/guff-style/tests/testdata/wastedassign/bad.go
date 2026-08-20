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
