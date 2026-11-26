package main

// SA4006 in a method body.
//
// Upstream walks `SrcFuncs`, which `buildssa`/`buildir` define as every named
// function declared in the package — methods included. guff's `expr_values`
// index followed `buildir_src_methods`, which is off outside contextcheck runs
// so that **SA5011** does not over-report (guff-ssa has no σ-nodes), and an
// expression inside a method body then resolved to no SSA value at all.
//
// The result was not a corner case: SA4006 fired in no method in the package,
// for any shape. The three bodies below are identical; only the receiver
// differs, and only the first one used to be reported.

type Recv struct{ n int }

var calls int

// `mk` writes to a package variable on purpose. Without a side effect, every
// dead assignment below is also an **SA4017** ("doesn't have side effects and
// its return value is ignored"), and guff misses all five of those — including
// the one in the plain function, so it is not this fix's blind spot but a
// separate SA4017 recall gap. Keeping it out of this fixture keeps the file
// measuring SA4006 and nothing else; the SA4017 gap is measured in its own
// right in COMPAT-HARDENING §4 (2026-09-07, continuation 250).
func mk(i int) int {
	calls++
	return i
}

// reported before and after
func plainOverwrite(n int) int {
	x := mk(n)
	x = mk(n + 1)
	return x
}

// pointer receiver
func (r *Recv) ptrOverwrite(n int) int {
	x := mk(n)
	x = mk(n + 1)
	return x
}

// value receiver
func (r Recv) valOverwrite(n int) int {
	x := mk(n)
	x = mk(n + 1)
	return x
}

// a method whose dead value sits inside a loop — the two halves of this fix
// meeting, since the loop guard is what continuation 249 corrected
func (r *Recv) loopInMethod(ns []int) int {
	total := 0
	for _, n := range ns {
		v := mk(n)
		total += v
		v = mk(n + 1)
	}
	return total
}

// a closure inside a method: `collect_anon_funcs` never reached these either
func (r *Recv) closureInMethod(n int) int {
	f := func() int {
		x := mk(n)
		x = mk(n + 1)
		return x
	}
	return f()
}
