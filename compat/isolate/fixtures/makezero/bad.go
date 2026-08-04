package p

func Bad() []int {
	x := make([]int, 10)
	x = append(x, 1)
	return x
}
