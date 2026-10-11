package operands

import "fmt"

// A func-typed field of a generic struct becomes a printf wrapper through an
// assignment on one instance, and is checked when called on another: the
// wrapper is recorded against the declared field (`origin`), as upstream's
// fact is. Shape from x/tools v0.50 printf testdata a/a.go.
type GS[T any] struct {
	printf func(x T, format string, args ...any)
}

type GM[T any] struct{}

func (GM[T]) Logf(format string, args ...any) { fmt.Printf(format, args...) }

func genericFields() {
	new(GS[bool]).printf = func(_ bool, format string, args ...any) {
		println(fmt.Sprintf(format, args...))
	}
	new(GS[int]).printf(0, "%s", 123)
	var m GM[string]
	m.Logf("%d", "x")
}
