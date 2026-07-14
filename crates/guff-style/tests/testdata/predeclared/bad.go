package predeclared

func len(x int) int {
	return x
}

func shadow() {
	error := "oops"
	_ = error
	true := false
	_ = true
}
