package pkg

func fn() {
	var a, b bool
	var h, i float64

	_ = !a
	_ = a && b
	_ = !(a && h > i)
}
