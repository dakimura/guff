package makezero

func ok() []int {
	x := make([]int, 0, 8)
	return append(x, 1)
}

// Upstream walks each file once, in source order, against a set that only ever
// holds the `make`s already seen — so an `append` written *before* the `make`
// that gives the slice its length is not reported. guff used to collect every
// `make` in the file first and then look at the appends, which reports these.
// k6 `internal/dashboard/registry.go:78` is the closure form.
func appendBeforeMake(seen []string, name string) []string {
	seen = append(seen, name)
	old := seen
	seen = make([]string, len(old)+1)
	copy(seen, old)
	return seen
}

func appendBeforeMakeInClosure(seen []string, name string) ([]string, []string) {
	var names []string
	process := func(n string) {
		if len(seen) == 0 {
			seen = append(seen, n)
			names = append(names, n)
		}
		old := seen
		seen = make([]string, len(old)+1)
		copy(seen, old)
	}
	process(name)
	return seen, names
}

// A parameter is not a `make`, however it is appended to.
func appendToParam(seen []string, name string) []string {
	return append(seen, name)
}

// Nor is a plain declaration.
func appendToVar() []string {
	var s []string
	return append(s, "x")
}
