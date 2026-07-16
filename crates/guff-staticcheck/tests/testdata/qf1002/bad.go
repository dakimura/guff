package pkg

func fn() {
	var x, y int
	var a bool

	switch {
	case x == 4:
	case x == 1 || x == 2, x == 3:
	}

	switch {
	case x == 1 || x == 2, x == 3:
	case x == 4:
	default:
	}

	switch {
	case a == (x == y) || a == (x != y):
	}
}
