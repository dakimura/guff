package comments

// gofumpt adds a space after `//`, but only when *no* line of the comment
// group looks like a directive or like code. Upstream decides "looks like
// code" by decoding the first rune of the body and asking whether it is a
// letter, a number or a space — and for a bare `//` that decode yields
// RuneError, which is none of the three. So one empty line disqualifies the
// whole group.

func interior() {
	//solo
	_ = 1

	// A bare `//` in the group: gofumpt leaves every line alone, including
	// the unspaced URL. celestia-node has two of these.
	//
	//https://example.com/some/long/path
	_ = 2

	// A directive anywhere in the group disqualifies it too.
	//go:noinline
	//unspaced-after-directive
	_ = 3

	// A line that could be code disqualifies it.
	//{
	//unspaced-after-brace
	_ = 4

	// Nothing disqualifying: every line gets the space.
	// spaced
	//unspaced
	_ = 5
}
