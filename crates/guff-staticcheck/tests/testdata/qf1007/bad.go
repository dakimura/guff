package pkg

func foo() bool { return true }

var bar bool
var baz bool

func fn() {
	x := false
	if foo() || (bar && !baz) {
		x = true
	}

	b := true
	if foo() {
		b = false
	}

	y := false
	if true {
		y = true
		println(y)
	}

	z := false
	if true {
		z = false
	}

	_ = x
	_ = b
	_ = y
	_ = z
}
