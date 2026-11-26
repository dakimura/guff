package main

import "bytes"

const zero = 0

// S1004 and S1019 ask honnef's `IntegerLiteral` too — a syntactic pattern, so
// a named constant that holds zero is not one.
func compares(a, b []byte) {
	// Reported.
	_ = bytes.Compare(a, b) == 0
	_ = bytes.Compare(a, b) == (0)
	// Silent.
	_ = bytes.Compare(a, b) == zero
	_ = bytes.Compare(a, b) == 1-1
}

func makes() {
	// Reported: `make(chan T, 0)` is the shape S1019 simplifies.
	_ = make(chan int, 0)
	// Silent.
	_ = make(chan int, zero)
}
