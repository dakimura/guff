package main

func f() {
	switch 1 {
	case 1:
		println(1)
		break
	}
	return
}

func g() {
	fn := func() {
		return
	}
	_ = fn
}
