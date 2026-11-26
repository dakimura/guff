package pkg

func foo() int { return 0 }

func fn() {
	var x, y int

	switch x {
	case 1, 2, 3:
	case 4:
	}

	switch {
	case x == 1 || x == 2, y == 3:
	case x == 4:
	}

	switch {
	case x == 1 || x == 2, x == foo():
	case x == 4:
	}

	switch {
	case x == 1 && x == 2:
	}

	switch {
	default:
	}
}
