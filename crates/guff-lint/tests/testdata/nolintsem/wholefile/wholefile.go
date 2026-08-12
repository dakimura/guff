//nolint:errcheck // covers the whole file
package wholefile

// Upstream has no special "file level" rule: `ast.Walk` visits the *ast.File
// first, whose Pos() is the `package` keyword, so a directive on the line
// above it at the same column expands to the end of the file like any other
// node.

func mkerr() error { return nil }

func A() {
	mkerr()
}

func B() int {
	x := 1
	x = 2
	return x
}

// C carries a directive that suppresses nothing at all — and neither tool
// reports it unused, because the file-level directive above did suppress
// something. nolintlint emits an unused *candidate* per directive, and the
// filter cancels it through the same range loop every issue takes: any
// covering range whose own directive matched cancels the candidate with it.
// Take the `mkerr()` finding away and both directives get reported.
//
// (A comment line starting with the directive word and a space is itself a
// malformed directive to nolintlint, which is why this paragraph does not
// spell it out. Learned by regenerating the golden.)
func C() int {
	y := 1
	y = 2 //nolint
	return y
}
