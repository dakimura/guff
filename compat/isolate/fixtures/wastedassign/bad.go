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

// A local `const` declares no storage. go/ssa's `case *ast.DeclStmt` builds a
// cell only when `d.Tok == token.VAR`; guff built one for every ValueSpec, so
// the Store for an unread constant looked exactly like a wasted assignment.
// gitea hit it three times — migrations declare a nine-name `const (…)` block
// inside the migration function and read only some of the names.
//
// Five const shapes, all of which upstream is silent on.
func ConstIotaBlock() int {
	const (
		unreadFirst = iota + 1 // never read; the two below inherit its expression
		second
		third
	)
	return second + third
}

func ConstSingle() int {
	const unread = 1
	return 2
}

func ConstTyped() string {
	const unread string = "x"
	return "y"
}

func ConstMultiName() int {
	const unread, read = 1, 2
	return read
}

func ConstBesideType() int {
	type local struct{ f int }
	const unread = 3
	return local{f: 1}.f
}

// The `var` side of the same statement kind still builds its cell, so a wasted
// assignment next to a const declaration is still reported.
func VarBesideConst(cond bool) int {
	const tag = 1
	var n = 1
	n = 2
	if cond {
		n = 3
	}
	return n
}
