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

// The message carries three computed numbers, so a second function scores
// differently and reads differently.
func AlsoBad(a, b, c int) int {
	n := 0
	if a > 0 {
		n++
	}
	if b > 0 {
		n += 2
	}
	if c > 0 {
		n += 3
	}
	for i := 0; i < a; i++ {
		n += i
	}

	return n
}
