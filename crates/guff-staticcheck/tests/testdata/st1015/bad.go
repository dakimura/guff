package pkg

func fn(x int) {
	switch x {
	case 1:
	default:
	case 2:
	}

	switch x {
	case 1:
		fallthrough
	default:
	case 2:
	}
}
