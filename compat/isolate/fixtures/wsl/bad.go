package p

func Bad() {
	one := 1
	two := 2
	three := 3
	if three == 3 {
		_ = one
		_ = two
		return
	}
	four := 4
	_ = four
}
