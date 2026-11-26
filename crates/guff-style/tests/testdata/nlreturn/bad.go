package nlreturn

func bad() int {
	x := 1
	if x > 0 {
		y := 2
		return y
	}
	return x
}
