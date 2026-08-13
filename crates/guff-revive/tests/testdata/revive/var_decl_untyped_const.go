// Package vardeclconst covers `var-declaration`'s untyped-constant gate.
//
// Upstream drops the finding only when the declared type is not the constant's
// *default* type: `var b int = 1` is reported, `var e int64 = 1` is not. guff
// carried an extra gate with no upstream counterpart which fired on every
// literal — a literal's `Types` entry always carries the type the assignment
// gave it — so only a non-constant right-hand side was ever reported.
package vardeclconst

func reported() {
	var a string = "x"
	var b int = 1
	var c float64 = 1.5
	var d bool = true
	var g string = mk()
	_, _, _, _, _ = a, b, c, d, g
}

func notReported() {
	// The default type of `1` is `int`, not `int64`.
	var e int64 = 1
	var f uint8 = 2
	_, _ = e, f
}

func mk() string { return "" }
