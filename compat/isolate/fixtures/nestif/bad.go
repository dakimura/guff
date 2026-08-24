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
