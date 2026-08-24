package p

func Bad(a, b, c, d bool) {
	if a {
		if b {
			if c {
				if d {
					_ = 1
				}
			}
		}
	}
}

// The message quotes the `if` condition and the computed complexity, so a
// second nested block reads differently.
func AlsoBad(x, y, z, w bool) {
	if x || y {
		if y {
			if z {
				if w {
					_ = 1
				}
			}
		}
	}
}

// The condition is rendered with go/printer, which drops the blanks around a
// higher-precedence operator nested under a lower one: `len(a)/2 + len(b)`, not
// `len(a) / 2 + len(b)`. A hand-rolled renderer that puts blanks around every
// binary operator agrees with upstream on `a && b` and parts company here —
// same shape the prealloc fixture pins for the capacity expression.
func PrecedenceInCond(a, b []int, x, y, z, w bool) {
	if len(a)/2+len(b) > 0 {
		if x {
			if y {
				if z {
					if w {
						_ = 1
					}
				}
			}
		}
	}
}
