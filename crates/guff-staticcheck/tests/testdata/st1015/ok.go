package pkg

func fn(x int) {
	switch x {
	default:
	case 1:
	}

	switch x {
	case 1:
	default:
	}

	switch x {
	case 1:
		fallthrough
	default:
	case 2:
	}
}
