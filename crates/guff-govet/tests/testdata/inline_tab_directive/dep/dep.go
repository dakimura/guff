// Package dep declares `//go:fix` directives separated by whitespace other
// than one space.
package dep

const New = 1

//go:fix	inline
const Old = New

type T int

//go:fix  inline
type A = T

// G is generic.
//
//go:fix	inline
func G[X any](x X) *X {
	return &x
}

// Not a directive: no whitespace between name and args.
//
//go:fixinline
const Glued = New
