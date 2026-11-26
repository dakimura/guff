package p

func Bad(n int) bool {
	if n > 42 {
		return true
	}
	switch n {
	case 7:
		return true
	}
	return n == 99
}
