package ok

func withValues(xs []int) []int {
	return append(xs, 1)
}

func spread(xs, ys []int) []int {
	return append(xs, ys...)
}

// A local named `append` shadows the builtin, so `typeutil.Callee` does not
// answer `*types.Builtin` and upstream is silent.
func shadowed(xs []int) []int {
	append := func(a []int) []int { return a }
	return append(xs)
}
