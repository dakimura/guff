package pkg

func fn() {
	var a, b, c bool
	var e, f, g int
	var h, i float64

	_ = !(a && b && (!c || e > f) && g == f)
	_ = !(a && h > i)
}
