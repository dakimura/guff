package fixture

// Deliberately un-gofmt'd spacing and indentation, so that the cases where
// simplify is *off* still assert that plain gofmt ran, rather than passing
// because nothing happened.
var spaced = 1 + 2

func badlyIndented() {
	_ = spaced
}

// Composite literals whose element type repeats the outer one.
var nested = [][]int{{1, 2}, {3}}

var mapped = map[string][]string{"a": {"x"}}

type point struct{ x, y int }

var pointers = []*point{{1, 2}}

// A slice expression whose high bound is len of the same identifier.
func tail(s []byte) []byte { return s[1:] }

// A range whose value (and then key) is blank.
func iterate(m map[string]int) {
	for k := range m {
		_ = k
	}
	for range m {
	}
}

// An empty declaration group.
const ()
