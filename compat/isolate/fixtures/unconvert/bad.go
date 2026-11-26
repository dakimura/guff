package p

func Bad(x int) int {
	return int(x)
}

// unconvert reports the conversion, so each redundant one is its own site —
// including conversions inside calls and on the result of a call.
func Redundant(n int, s string) {
	_ = int(n)
	_ = string(s)
	_ = int(len(s))
}
