package p

func Bad(cond bool) int {
	n := 1
	if cond {
		n = 2
	}
	n = 3
	return n
}
