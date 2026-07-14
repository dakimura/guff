package whitespace

func ok() int {
	x := 1
	return x
}

func nested() {
	if true {
		_ = 1
	}
	for i := 0; i < 1; i++ {
		_ = i
	}
}
