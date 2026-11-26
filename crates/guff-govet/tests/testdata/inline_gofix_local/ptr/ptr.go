// Package ptr is vitess's `go/ptr` in miniature: a generic helper carrying
// `//go:fix inline`, declared in a sibling package of the caller's module.
package ptr

// Of returns a pointer to the given value
//
//go:fix inline
func Of[T any](x T) *T {
	return &x
}

// Pair has two type parameters.
//
//go:fix inline
func Pair[A any, B any](a A, b B) (A, B) {
	return a, b
}

// Plain is not generic: upstream inlines it and says so, which is the other
// half of the `//go:fix` diagnostic and still deferred here.
//
//go:fix inline
func Plain(x int) int {
	return x
}

// NotMarked is generic but carries no directive.
func NotMarked[T any](x T) *T {
	return &x
}
