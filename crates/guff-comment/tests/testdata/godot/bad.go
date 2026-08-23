package example

// Sum adds two integers
func Sum(a, b int) int {
	return a + b
}

// Bad lacks a period at the end
func Bad() {}

// godot's `declarations` scope is getBlockComments() ++ getDeclarationComments().
// guff had only the second half, so nothing below this line was ever checked.

const (
	// One tab in, so column 2 — checked
	A = 1
)

var (
	// Free-floating in the block, owned by no spec — still checked
	D = 4

	E = struct {
		X int
	}{
		// Two levels in, column 3 — skipped, and deliberately so: the block
		// itself is top level, so only its immediate contents count
		X: 1,
	}
)

// A single-line const has no Lparen, so no block to be inside of
const C = 3

func hasBody() {
	// Inside a func body, which is not a top-level block
	_ = 1
}

// Multi does a thing
// across two comment lines
func Multi() {}

// Blank does a thing
//
// Deprecated: the blank line above is a line, and the last non-empty one is
// this one
func Blank() {}

// Trailing does a thing
//
// and then a blank comment line follows
func Trailing() {}
