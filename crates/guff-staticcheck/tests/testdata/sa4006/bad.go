package main

// Upstream judges the assignment by the value of its right-hand *expression*,
// so only non-constant values that nothing reads are findings. The shapes that
// look similar but are not reported live in ok.go.

func two(s string) (string, bool) { return s, true }

func overwrittenFromParam(m int) {
	var n int
	n = m // want
	n = 2
	_ = n
}

func overwrittenFromCall() {
	f, _ := two("x") // want
	f, _ = two("y")
	_ = f
}

func selfAppendUnused(x, y []int) {
	x = append(x, y...) // want
}

func realConversionUnused(b []byte) {
	s := "a"
	_ = s
	s = string(b) // want
}
