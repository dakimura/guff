package bad

func bad() {
	x := 1
	if true {
		x = 2
	} else {
		x = 3
	}
	print(x)
}
