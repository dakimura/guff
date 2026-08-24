package p

func multi() (int, int, int, int) { return 1, 2, 3, 4 }

func Bad() {
	_, _, _, _ = multi()
}

func three() (int, int, int) { return 1, 2, 3 }

// The message names how many blanks it found, so three and four are different
// sentences rather than the same one twice.
func ThreeBlanks() {
	_, _, _ = three()
}

func InAssign() {
	var a, b, c, d int
	_ = a
	_, _, _, _ = b, c, d, a
}
