package p

func Bad(a, b, c, d, e int) int {
	if a > 0 {
		if b > 0 {
			if c > 0 {
				if d > 0 {
					if e > 0 {
						return 1
					}
					return 2
				}
				return 3
			}
			return 4
		}
		return 5
	}
	return 0
}
