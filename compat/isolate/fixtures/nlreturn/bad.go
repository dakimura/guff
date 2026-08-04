package p

func Bad(cond bool) int {
	if cond {
		return 1
	}
	x := 2
	return x
}
