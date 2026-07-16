package pkg

func done() bool { return false }

var a, b int
var x bool

func fn() {
	for {
		if done() {
			break
		}
	}

	for {
		if !done() {
			break
		}
	}

	for {
		if a > b || b > a {
			break
		}
	}

	for {
		if x && (a == b) {
			break
		}
	}

	for {
		if done() {
			break
		}
		println()
	}
}
