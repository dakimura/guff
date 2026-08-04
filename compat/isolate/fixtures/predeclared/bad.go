package p

func len(x int) int { return x }

func Bad() {
	error := "oops"
	_ = error
}
