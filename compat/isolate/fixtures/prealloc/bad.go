package p

func Bad(n int) []int {
	var out []int
	for i := 0; i < n; i++ {
		out = append(out, i)
	}
	return out
}

// The capacity is printed the way `go/printer` prints it: a higher-precedence
// operator nested under a lower one loses its blanks. dapr's
// `pkg/runtime/hotreload/differ` is `len(components)/2 + len(componentsDiff2)`,
// which guff used to spell with blanks around the `/` — the same finding under
// two different names, counted once as a miss and once as an extra.
func NestedPrecedence(a []int, b []int) []int {
	var out []int
	for i := range len(a)/2 + len(b) {
		out = append(out, i)
	}
	return out
}

func MulUnderAdd(n int, a []int) []int {
	var out []int
	for i := 0; i < n*2+len(a); i++ {
		out = append(out, i)
	}
	return out
}

// One level deep keeps its blanks.
func SingleLevel(a []int) []int {
	var out []int
	for _, v := range a[:len(a)-1] {
		out = append(out, v)
	}
	return out
}

// A range loop is the shape `range-loops` (on by default) reaches; the four
// above are `for-loops`. Added while widening — the precedence cases are the
// point of this fixture and were restored after being overwritten.
func RangeLoop(xs []int) []int {
	var out []int
	for _, x := range xs {
		out = append(out, x)
	}

	return out
}
