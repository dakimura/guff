package p

func One(a, b, c, d, e, f int) int {
	x := a + b
	y := c + d
	z := e + f
	if x > y {
		return x + z
	}
	if y > z {
		return y + x
	}
	return z + x + y
}

func Two(a, b, c, d, e, f int) int {
	x := a + b
	y := c + d
	z := e + f
	if x > y {
		return x + z
	}
	if y > z {
		return y + x
	}
	return z + x + y
}
