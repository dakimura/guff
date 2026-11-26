package p

func helper(used int, unused string) int {
	return used + 1
}

func Bad() int {
	return helper(1, "x")
}
