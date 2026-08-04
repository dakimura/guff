package p

func Bad() int {
	x := 1
	x = 2 // ineffectual assignment
	_ = x
	return x
}
