package p

func Bad() {
	for i, v := range []int{1, 2, 3} {
		i := i
		v := v
		_, _ = i, v
	}
}
