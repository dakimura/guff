package p

func Bad(cond bool) int {
	n := 1
	if cond {
		n = 2
	}
	n = 3
	return n
}

// A labelled `break` leaves its loop, so the outer loop's condition reads
// `line` again and the assignment before the break is live. Resolving that
// break to the loop's *head* instead — which guff did while the label's break
// target was recorded only after the body was built — turns this into a
// finding neither tool should have.
func LiveAcrossLabelledBreak(read func() string) int {
	n := 0
	line := read()
	for {
		if line == "" {
			return n
		}
	curFileLoop:
		for {
			line = read()
			n++
			switch {
			case line == "end":
				line = read() + "!"
				break curFileLoop
			}
		}
	}
}

// The other direction: breaking the *outer* loop reaches the return, so nothing
// reads `line` again and the assignment is wasted.
func WastedAcrossLabelledBreak(read func() string) int {
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
