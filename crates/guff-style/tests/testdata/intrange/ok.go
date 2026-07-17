package intrange

func ok() {
	for i := 2; i < 10; i++ {
		_ = i
	}

	for i := 0; i < 10; i += 2 {
		_ = i
	}

	for i := 0; i < 10; i++ {
		i++
	}

	for i := 0; i < 10; i++ {
		i += 1
	}

	for i := range 10 {
		_ = i
	}

	for range 10 {
	}

	s := []int{1, 2, 3}
	for i := range s {
		_ = i
	}

	for i := range len(s) / 2 {
		_ = i
	}

	m := map[int]int{1: 1}
	for i := range len(m) {
		_ = i
	}

	// <= with non-literal limit is not rewritten
	n := 9
	for i := 0; i <= n; i++ {
		_ = i
	}
}
