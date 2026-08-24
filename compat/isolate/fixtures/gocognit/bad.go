package p

func Bad(a, b, c, d, e int) int {
	if a > 0 {
		if b > 0 {
			if c > 0 {
				if d > 0 {
					if e > 0 {
						return 1
					}
				}
			}
		}
	}
	switch a {
	case 1:
		return 10
	case 2:
		return 20
	default:
		return 0
	}
}

// The message carries the computed complexity, so a second function over the
// limit with a different score is a different sentence.
func AlsoBad(a, b, c int) int {
	n := 0
	for i := 0; i < a; i++ {
		if b > 0 {
			switch c {
			case 1:
				n++
			case 2:
				n += 2
			default:
				n--
			}
		} else if b < 0 {
			n -= 2
		}
	}

	return n
}
