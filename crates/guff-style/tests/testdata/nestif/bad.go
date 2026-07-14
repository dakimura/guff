package nestif

func DeepNesting(a, b, c, d bool) {
	if a {
		if b {
			if c {
				if d {
					return
				}
			}
		}
	}
}
