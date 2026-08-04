package p

func Bad(a, b, c, d bool) {
	if a {
		if b {
			if c {
				if d {
					_ = 1
				}
			}
		}
	}
}
