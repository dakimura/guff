package makezero

func bad() []int {
	x := make([]int, 8)
	return append(x, 1)
}
