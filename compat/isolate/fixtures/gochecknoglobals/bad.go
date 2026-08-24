package p

var MyGlobal = 1

const Ok = 2

func Bad() { _ = MyGlobal }

// gochecknoglobals reports the global by name, so each one is its own sentence.
// A `var` block, an error sentinel and a slice are separate nodes; `const` and
// the allowed error/regexp shapes are the negatives.
var (
	Second = 2
	third  = 3
)

var table = []int{1, 2}

// Sentinel errors and compiled regexps are exempt upstream.
var ErrSentinel = newError()

func newError() error { return nil }
