package p

func Bad(a, b, c, d, e, f, g, h, i, j, k bool) {
	if a {
		if b {
			if c {
				if d {
					if e {
						if f {
							if g {
								if h {
									if i {
										if j {
											if k {
												return
											}
										}
									}
								}
							}
						}
					}
				}
			}
		}
	}
}

// cyclop walks func literals too, and reports them under the same message with
// the literal's own name — here the enclosing declaration's.
func AlsoBad(a, b, c, d, e, f, g, h, i, j, k bool) {
	if a && b && c && d && e && f && g && h && i && j && k {
		_ = 1
	}
	if a || b || c || d || e {
		_ = 2
	}
}
