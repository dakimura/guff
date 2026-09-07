package main

// A finding has to flow *through* a `range` over a pointer to an array, so the
// loop is proved lowered rather than merely proved not to panic.
//
// `range p` over `*Key` (a named array type) panicked in the SSA builder:
// `array_elem` does not unwrap, and the pointee arrives as the type as written.
// The length was wrong too, for the unnamed case as well — Go reads N off the
// type and never loads the pointer.

type Key [4]byte

var steps int

// Side-effecting on purpose: without it every dead assignment below is also an
// SA4017, which guff misses — a separate recall gap (COMPAT-HARDENING §4,
// 2026-09-07, continuation 250). This fixture measures SA4006.
func step(i int) int {
	steps++
	return i
}

func deadInNamedPtrRange(p *Key) int {
	total := 0
	for i := range p {
		v := step(i)
		total += v
		v = step(i + 1)
	}
	return total
}

func deadInNamedValueRange(k Key) int {
	total := 0
	for i := range k {
		v := step(i)
		total += v
		v = step(i + 1)
	}
	return total
}

func deadInUnnamedPtrRange(p *[4]byte) int {
	total := 0
	for i := range p {
		v := step(i)
		total += v
		v = step(i + 1)
	}
	return total
}
