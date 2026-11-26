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

// The constant builtins are untyped constants too, so the declared type is only
// redundant when it matches their default. `complex(2, 3)` defaults to
// complex128, `real`/`imag` to float64, `min`/`max` to the widest argument.
// Without this the call read as a typed right-hand side and the default-type
// gate never ran — fiber's `state_test.go:339` is the first line below.
var floatVar float64

func builtinsNotReported() {
	var bnr1 complex64 = complex(2, 3)
	var bnr2 float32 = real(complex(2, 3))
	var bnr3 int64 = min(1, 2)
	_, _, _ = bnr1, bnr2, bnr3
}

func builtinsReported() {
	var br1 complex128 = complex(2, 3)
	var br2 float64 = real(complex(2, 3))
	var br3 int = min(1, 2)
	var br4 int = max(1, 2)
	var br5 float64 = imag(complex(2, 3))
	// A typed argument makes the call typed, so the type really is redundant.
	var br6 complex128 = complex(floatVar, 0)
	// `len` of a constant string is a *typed* int; the spec says so.
	var br7 int = len("abc")
	_, _, _, _, _, _, _ = br1, br2, br3, br4, br5, br6, br7
}
