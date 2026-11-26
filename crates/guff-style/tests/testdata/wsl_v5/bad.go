package wsl_v5

func Bad() {
	a := 1
	b := 2
	if b == 2 {
		_ = a
		return
	}

	x := 1
	if true {
		_ = 2
		return
	}

	var c = 1
	var d = 2
	_ = c
	_ = d

	err := doErr()

	if err != nil {
		return
	}
}

func doErr() error { return nil }
