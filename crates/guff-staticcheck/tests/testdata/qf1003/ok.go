package pkg

func fn() {
	var x, y int

	if x == 1 || x == 2 {
	} else if y == 3 {
	}

	if x == 1 || x == 2 {
	}

	for {
		if x == 1 || x == 2 {
		} else if x == 3 {
			break
		}
	}

	switch x {
	case 1, 2:
	case 3:
	}
}
