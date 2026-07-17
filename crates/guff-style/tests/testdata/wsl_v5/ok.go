package wsl_v5

func Ok() {
	b := 2
	if b == 2 {
		return
	}

	used := true
	if used {
		return
	}

	var a = 1

	var b2 = 2

	_ = a
	_ = b2

	err := doErr()
	if err != nil {
		return
	}
}

func doErr() error { return nil }

func Short() int {
	x := 1
	return x
}
