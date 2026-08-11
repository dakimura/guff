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
