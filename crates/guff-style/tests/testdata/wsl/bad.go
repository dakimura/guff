package wsl

func Bad() {
	var (
		y = 0
	)
	if y < 1 {
		return
	}

	unused := true
	if 2 > 1 {
		return
	}

	one := 1
	two := 2
	three := 3
	if three == 3 {
		_ = one
		_ = two
		_ = unused
		return
	}

	var a = "a"
	var b = "b"
	_ = a
	_ = b

	x := 1
	fmtPrintln(x)
	y2 := 2
	_ = y2
}

func fmtPrintln(_ int) {}
