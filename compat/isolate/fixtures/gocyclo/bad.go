package p

func Bad(a, b, c, d, e int) int {
	if a > 0 {
		if b > 0 {
			if c > 0 {
				if d > 0 {
					if e > 0 {
						return 1
					}
					return 2
				}
				return 3
			}
			return 4
		}
		return 5
	}
	return 0
}

// gocyclo reports a func literal assigned to a package-level var under the name
// of that var, but positions it at the literal's `func` keyword — a different
// node from a FuncDecl.
var Lit = func(a, b, c, d, e int) int {
	n := 0
	if a > 0 {
		n++
	}
	if b > 0 {
		n++
	}
	if c > 0 {
		n++
	}
	if d > 0 {
		n++
	}
	if e > 0 {
		n++
	}

	return n
}
