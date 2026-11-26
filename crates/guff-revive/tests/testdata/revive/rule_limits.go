// Package rulelimits exercises the six revive rules whose limit is a number.
//
// Each of them reads `arguments[0]` (function-length reads two) and falls back
// to a default. guff had every one of those defaults baked in as a `const` and
// never read the argument, so no configuration could move them. The two golden
// cases over this file — `revive-limits-default` and
// `revive-limits-configured` — differ only in the arguments, and if the
// arguments were still ignored the two would be identical.
//
// No function here may have an empty body: upstream's function-length bails out
// of the *whole file* on the first one (`return nil`, not `continue`).
package rulelimits

// argument-limit: 8 by default.
func sixArguments(a, b, c, d, e, f int) int { return a + b + c + d + e + f }

func nineArguments(a, b, c, d, e, f, g, h, i int) int {
	return a + b + c + d + e + f + g + h + i
}

// function-result-limit: 3 by default.
func twoResults() (int, int) { return 1, 2 }

func fourResults() (int, int, int, int) { return 1, 2, 3, 4 }

// cyclomatic: 10 by default. Each `if` and each `&&` adds one to the base of 1.
func complexityFour(a, b, c bool) int {
	if a {
		return 1
	}
	if b {
		return 2
	}
	if c {
		return 3
	}
	return 4
}

func complexityEleven(a, b, c, d, e, f, g, h, i, j bool) int {
	if a {
		return 1
	}
	if b {
		return 2
	}
	if c {
		return 3
	}
	if d {
		return 4
	}
	if e {
		return 5
	}
	if f {
		return 6
	}
	if g {
		return 7
	}
	if h {
		return 8
	}
	if i {
		return 9
	}
	if j {
		return 10
	}
	return 11
}

// max-control-nesting: 5 by default.
func nestingThree(a, b, c bool) int {
	if a {
		if b {
			if c {
				return 1
			}
		}
	}
	return 0
}

func nestingSix(a, b, c, d, e, f bool) int {
	if a {
		if b {
			if c {
				if d {
					if e {
						if f {
							return 1
						}
					}
				}
			}
		}
	}
	return 0
}

// line-length-limit: 80 by default. The next line is 96 characters.
func aFunctionWhoseSignatureIsDeliberatelyLongerThanEightyCharacters(value int) int { return value }

// function-length: 50 statements / 75 lines by default. This one has 52.
func fiftyTwoStatements() int {
	n := 0
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	n++
	return n
}
