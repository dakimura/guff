package p

func Bad(n int) {
	for i := 0; i < n; i++ {
		_ = i
	}
}
