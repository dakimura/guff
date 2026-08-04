package p

func Bad(a, b, c, d, e, f bool) int {
	n := 0
	if a {
		n++
	}
	if b {
		n++
	}
	if c {
		n++
	}
	if d {
		n++
	}
	if e {
		n++
	}
	if f {
		n++
	}
	return n
}
