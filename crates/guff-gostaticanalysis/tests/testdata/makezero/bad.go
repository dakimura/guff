package makezero

func bad() []int {
	x := make([]int, 8)
	return append(x, 1)
}

// The `make` is seen first, so the append after it is reported.
func appendAfterMake(seen []string, name string) []string {
	old := seen
	seen = make([]string, len(old)+1)
	seen = append(seen, name)
	return seen
}

// The same one closure in.
func appendAfterMakeInClosure(n int) []string {
	s := make([]string, n)
	f := func() { s = append(s, "x") }
	f()
	return s
}
