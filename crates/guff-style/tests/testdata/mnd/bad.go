package mnd

func take(n int) int { return n }

func bad(x int) int {
	if x > 3 {
		return take(4)
	}
	switch x {
	case 5:
		return 6
	}
	y := x + 7
	_ = y
	return -8
}
