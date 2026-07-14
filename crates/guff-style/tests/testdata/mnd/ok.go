package mnd

func take(n int) int { return n }

func ok(x int) int {
	if x > 0 {
		return take(1)
	}
	switch x {
	case 1:
		return 0
	}
	y := x + 1
	_ = y
	return -1
}
