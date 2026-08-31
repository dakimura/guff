package pkg

import (
	"math"
	"time"
)

const untypedInt = 10
const untypedRune = 'a'
const typedInt32 int32 = 7

// Upstream re-type-checks the right-hand side on its own (`types.CheckExpr`)
// and only then asks whether the declared type is redundant: an untyped
// right-hand side keeps its type unless the declaration spells out exactly the
// type it would default to. Two things that answer falls out of are easy to get
// wrong, and both are here:
//
//   - `types.Default` of an untyped rune is `rune`, compared by identity, so
//     `var v int32 = 'a'` keeps its type while `var v rune = 'a'` does not.
//   - a shift takes its left operand's type, so `1 << uint(x)` is an untyped
//     int no matter how typed the count is.
//
// The other axis is which AST shapes survive `flagHelpfulTypes = false`: only
// literals and predeclared identifiers, and only while the right-hand side is
// untyped. A typed right-hand side is flagged whatever its shape.
func shapes(x, y int, ch chan int, b1, b2 bool) {
	var c01 int = 1
	var c02 int64 = 1
	var c03 rune = 'a'
	var c04 int32 = 'a'
	var c05 byte = 'a'
	var c06 string = "s"
	var c07 float64 = 1.5
	var c08 int = untypedInt
	var c09 int64 = untypedInt
	var c10 int32 = typedInt32
	var c11 bool = true
	var c12 int = math.MaxInt8
	var c13 time.Duration = time.Second
	var c14 int = x
	var c15 int = (1)
	var c16 int = -1
	var c17 int = 1 + 1
	var c18 int64 = 1 + 1
	var c19 rune = 'a' + 1
	var c20 int32 = 'a' + 1
	var c21 int = x + y
	var c22 int64 = int64(untypedInt) * 10
	var c23 time.Duration = 5 * time.Second
	var c24 int32 = typedInt32 * 2
	var c25 bool = x == y
	var c26 bool = b1 && b2
	var c27 int = <-ch
	var c28 int = len("abc")
	var c29 int64 = int64(x)
	var c30 float64 = 1
	var c31 int32 = untypedRune
	var c32 rune = untypedRune
	var c33 int = 1 << 3
	var c34 uint = 1 << uint(x)
	var _ int = 1
	var c35 *int = nil
	var c36 int = int(untypedInt)
	_, _, _, _, _, _, _, _, _, _ = c01, c02, c03, c04, c05, c06, c07, c08, c09, c10
	_, _, _, _, _, _, _, _, _, _ = c11, c12, c13, c14, c15, c16, c17, c18, c19, c20
	_, _, _, _, _, _, _, _, _, _ = c21, c22, c23, c24, c25, c26, c27, c28, c29, c30
	_, _, _, _, _, _, _ = c31, c32, c33, c34, c35, c36, x
}

// The constant builtins. `complex(2, 3)` over untyped constants is an untyped
// *complex* constant, `real`/`imag` of one are untyped floats, and `min`/`max`
// over untyped constants are untyped — the spec says so and `types.CheckExpr`
// knows it, so upstream gets it for free. Reading the right-hand side as typed
// skipped the untyped branch entirely: both the default-type test and the
// expression-kind gate went unrun, and every one of these was reported.
// fiber's `state_test.go:339` is `var c complex64 = complex(2, 3)`.
//
// `len` is deliberately here as the control: the spec makes `len("abc")` a
// *typed* `int` constant, so it stays on the typed path.
var floatVar float64

func constBuiltins() {
	var b01 complex64 = complex(2, 3)
	var b02 complex128 = complex(2, 3)
	var b03 float64 = real(complex(2, 3))
	var b04 float32 = real(complex(2, 3))
	var b05 float64 = imag(complex(2, 3))
	var b06 int = min(1, 2)
	var b07 int64 = min(1, 2)
	var b08 int = max(1, 2)
	var b09 complex128 = complex(floatVar, 0)
	var b10 int = len("abc")
	_, _, _, _, _, _, _, _, _, _ = b01, b02, b03, b04, b05, b06, b07, b08, b09, b10
}
