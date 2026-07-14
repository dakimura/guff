package wsl

func Ok() {
	y := 0

	if y < 1 {
		return
	}

	used := true
	if used {
		return
	}

	three := 3
	if three == 3 {
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

func Short() int {
	x := 1
	return x
}
