//go:build go1.26

// Package newexprshapes holds the constant-argument shapes of the newexpr
// "call of F(x) can be simplified to new(x)" arm.
//
// It is separate from newexpr.go because that file carries `variadic[int]()`,
// a call with no arguments, and upstream's newexpr indexes `call.Args[0]`
// without checking the length — x/tools v0.44.0 newexpr.go:152 panics on it,
// which is why the golden case cannot record newexpr.go at all. Nothing here
// calls a new-like wrapper with anything other than one argument, so this file
// *can* be measured against golangci-lint.
//
// The rule these shapes pin: `new(expr)` gives a constant its **default**
// type, so the rewrite is offered only when that default is the wrapper's
// element type. `int64Of(1)` is therefore silent — `new(1)` would be a *int.
package newexprshapes

import "math"

type myInt int

func intOf(i int) *int { return &i }

func int64Of(i int64) *int64 { return &i }

func float32Of(f float32) *float32 { return &f }

func complexOf(c complex128) *complex128 { return &c }

func float64Of(f float64) *float64 { return &f }

func myIntOf(m myInt) *myInt { return &m }

func stringOf(s string) *string { return &s }

func anyOf[T any](x T) *T { return &x }

const untypedInt = 5

const untypedStr = "k"

const typedInt64 int64 = 9

var (
	boolVar   = true
	intVarBox = 7
)

var (
	// Typed operands: the recorded type is already the right one.
	_ = anyOf(boolVar)
	_ = anyOf(intVarBox)
	_ = intOf(1)
	_ = stringOf("lit")

	// Untyped constants, named and not. `true` and `false` are Idents, not
	// literals, which is the form the corpus is full of (`boolPtr(true)`).
	_ = anyOf(true)
	_ = anyOf(false)
	_ = anyOf(1)
	_ = anyOf(1.5)
	_ = anyOf("s")
	_ = anyOf('a')
	_ = anyOf(1i)
	_ = anyOf(untypedInt)
	_ = anyOf(untypedStr)
	_ = anyOf(typedInt64)
	_ = anyOf((true))

	// Constant expressions. The kind is the one Go gives the expression on
	// its own: rune beats int, float beats rune, a comparison is bool, and a
	// shift takes its left operand's kind.
	_ = anyOf(1 + 2)
	_ = anyOf(-1)
	_ = anyOf('a' + 1)
	_ = anyOf(1 + 'a')
	_ = anyOf(1 + 1.5)
	_ = anyOf(1 == 1)
	_ = anyOf(!false)
	_ = anyOf(true && false)
	_ = anyOf(1 << 3)
	_ = anyOf("a" + "b")

	// Conversions are typed, so they take the converted type.
	_ = anyOf(myInt(1))
	_ = anyOf(float32(1.5))
	_ = anyOf(int64(123))

	// Silent: the constant's default type is not the element type.
	_ = int64Of(1)
	_ = int64Of(untypedInt)
	_ = float32Of(1.5)
	_ = myIntOf(1)
	_ = anyOf[int64](1)

	// Reported: a typed constant is compared on its recorded type, and an
	// untyped imaginary constant defaults to complex128.
	_ = int64Of(typedInt64)
	_ = complexOf(1i)

	// Reported: `len` and `int` are universe names, so the expression still
	// resolves where upstream evaluates it.
	_ = intOf(len("abc"))
	_ = intOf(int(untypedInt))

	// Silent: a *qualified* constant needs the file scope, and upstream
	// evaluates the argument in the package scope alone.
	_ = intOf(math.MaxInt8)
	_ = float64Of(math.Pi)
)

func localConstants() []any {
	const k = 1
	const typedK int64 = 2
	// All silent: a local constant does not resolve in the package scope
	// either, so upstream declines however obvious the rewrite looks.
	return []any{
		intOf(k),
		int64Of(typedK),
		intOf(1 + k),
		intOf(int(k)),
	}
}
