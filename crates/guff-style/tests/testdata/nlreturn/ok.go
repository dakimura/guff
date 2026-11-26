package nlreturn

func alone() int {
	if true {
		return 1
	}

	return 0
}

func withBlank() int {
	x := 1
	if x > 0 {
		y := 2

		return y
	}

	return x
}
