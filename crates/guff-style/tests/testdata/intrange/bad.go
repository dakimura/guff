package intrange

func calculate(i int) int {
	return i
}

func bad() {
	for i := 0; i < 10; i++ {
		_ = i
	}

	for i := 0; i < 10; i++ {
	}

	for i := 0; i <= 9; i++ {
		_ = i
	}

	for i := 0; 10 > i; i++ {
		_ = i
	}

	for i := 0; i < 10; i += 1 {
		_ = i
	}

	for i := 0; i < 10; i = i + 1 {
		_ = i
	}

	for i := 0; i < 10; i = 1 + i {
		_ = i
	}

	s := []int{1, 2, 3}
	for i := range len(s) {
		_ = i
	}

	for range len(s) {
	}

	for i := 0; i < calculate(10); i++ {
		_ = i
	}

	i := 0
	for i = 0; i < 10; i++ {
		_ = i
	}
}
