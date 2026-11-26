package pkg

func done() bool { return false }

func fn() {
	for {
		println()
		if done() {
			break
		}
	}

	for {
		if done() {
			println()
			break
		}
	}

	for done() {
	}
}
