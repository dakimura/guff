package p

func Bad() (n int) {
	n = 1
	return
}

// nakedret names the function and the line count, so two functions over the
// limit are two different sentences.
func AlsoBad() (s string, err error) {
	s = "x"
	err = nil

	return
}

// Grouped result names are one AST field with two Names, and upstream's fix
// loops over `Results.List` *and then* `result.Names`. A field-only loop
// renders `return a` and silently drops `b` (COMPAT-HARDENING 続き 79).
func GroupedNames() (a, b int) {
	a = 1
	b = 2

	return
}
