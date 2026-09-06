package main

func f(ch chan int) {
	v := <-ch
	_ = v
}

// A type assertion matches neither arm of the pattern, so neither tool reports
// it — the guard against widening "comma-ok with a blank" too far.
func typeAssertion(x any) int {
	v, _ := x.(int)
	return v
}

// The second value is used.
func keepsTheOk(m map[string]int) (int, bool) {
	v, ok := m["k"]
	return v, ok
}
