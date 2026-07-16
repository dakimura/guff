package pkg

func foo() bool { return true }

func fn() {
	x := foo()
	_ = x

	y := false
	if true {
		y = false
	}
	_ = y

	z := true
	if true {
		z = true
	}
	_ = z
}
