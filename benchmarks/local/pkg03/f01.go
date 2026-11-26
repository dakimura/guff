package pkg03

func Ineff1() int {
	x := 1
	x = 2 // want ineffassign
	return x
}

func helperUnused1() int { // want unused
	return 4
}
