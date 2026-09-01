package wastedassign

// A variable whose address is taken is heap-allocated by go/ssa, and
// `finishBody` drops heap allocs from `Function.Locals` — the set wastedassign
// walks. Every store below looks dead and none of them is a finding, because
// the pointer can write the cell from anywhere.
//
// The two controls at the bottom are the only findings in this file.

func fillInt(p *int) { *p = 1 }

type memish struct{ n int }

func fillMemish(p *memish) { p.n = 1 }

// syncthing cmd/syncthing/perfstats_unix.go: the address is taken before the
// loop, the whole struct is assigned at the tail of it, and it is never read.
func escapesStructInLoop() {
	var cur, prev memish
	fillMemish(&prev)
	for i := 0; i < 3; i++ {
		fillMemish(&cur)
		_ = cur.n
		prev = cur
	}
}

func escapesIntInLoop() {
	var cur, prev int
	fillInt(&prev)
	for i := 0; i < 3; i++ {
		fillInt(&cur)
		_ = cur
		prev = cur
	}
}

func escapesOnce() {
	var prev int
	fillInt(&prev)
	prev = 3
}

func escapesTwice() {
	var prev int
	fillInt(&prev)
	prev = 3
	prev = 4
}

// Not flow-sensitive: the address is taken after the wasted store.
func escapesAfterTheWaste() {
	x := 1
	x = 2
	fillInt(&x)
}

// A closure that reads the variable captures its cell, which escapes too.
func capturedAndRead() func() int {
	x := 1
	x = 2
	return func() int { return x }
}

// Control 1: a closure that never mentions `x` captures nothing, so the cell
// stays a local and the wasted store is reported.
func capturedByNothing() int {
	x := 1
	x = 2
	f := func() int { return 7 }
	return f() + x
}

// Control 2: no pointer anywhere.
func plainWaste() int {
	y := 1
	y = 2
	return y
}
