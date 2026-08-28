package fixture

// The leading blank line is removed by plain gofumpt, so the `extra-rules:
// false` twin still asserts that the formatter ran rather than that nothing
// happened. The repeated parameter type is what `extra-rules` collapses.
func demo(a string, b string, c int) {

	_, _, _ = a, b, c
}

func grouped() {
	var x int
	var y string
	_, _ = x, y
}
