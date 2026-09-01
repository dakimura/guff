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

// A variable whose address is taken is heap-allocated by go/ssa, and
// `finishBody` drops heap allocs from `Function.Locals` — the set wastedassign
// walks. So none of the shapes below is a finding, however dead the store
// looks: the pointer can write the cell from anywhere.
//
// syncthing `cmd/syncthing/perfstats_unix.go` is EscapesStructInLoop:
// `runtime.ReadMemStats(&prevMem)` before the loop, `prevMem = curMem` at the
// tail of it, and `prevMem` never read.

func fillInt(p *int) { *p = 1 }

type memish struct{ n int }

func fillMemish(p *memish) { p.n = 1 }

func EscapesStructInLoop() {
	var cur, prev memish
	fillMemish(&prev)
	for i := 0; i < 3; i++ {
		fillMemish(&cur)
		_ = cur.n
		prev = cur
	}
}

func EscapesIntInLoop() {
	var cur, prev int
	fillInt(&prev)
	for i := 0; i < 3; i++ {
		fillInt(&cur)
		_ = cur
		prev = cur
	}
}

func EscapesOnce() {
	var prev int
	fillInt(&prev)
	prev = 3
}

func EscapesTwice() {
	var prev int
	fillInt(&prev)
	prev = 3
	prev = 4
}

// The mark is not flow-sensitive, in either tool: the address is taken after
// the wasted store and the store is still not reported.
func EscapesAfterTheWaste() {
	x := 1
	x = 2
	fillInt(&x)
}

// Captured by a closure that reads it — also escaping, also silent.
func CapturedAndRead() func() int {
	x := 1
	x = 2
	return func() int { return x }
}

// Captured by a closure that does *not* mention it: nothing escapes, so the
// wasted store is still a finding. This is the control for the four above.
func CapturedByNothing() int {
	x := 1
	x = 2
	f := func() int { return 7 }
	return f() + x
}

// A field and a slice element are addressed through FieldAddr/IndexAddr, not
// through an Alloc in Locals, so neither tool reports these either.
func FieldStore() memish {
	var b memish
	b.n = 1
	b.n = 2
	return b
}

func ElemStore() []int {
	s := make([]int, 2)
	s[0] = 1
	s[0] = 2
	return s
}
