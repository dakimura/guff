package copyloopvar

func Bad() {
	for i, v := range []int{1, 2, 3} {
		i := i
		v := v
		_, _ = i, v
	}

	for i := 0; i < 3; i++ {
		i := i
		_ = i
	}
}
