package main

// SA4006 after a conversion to an instantiated generic type.
//
// `Fn[int](f)` is a conversion, not a call. guff's `index_expr` had a
// `DEFERRED (generics)` arm that made `T[A]` in expression position an invalid
// operand — silently, with no error, so the package still counted as
// well-typed. The converted value then had no type, `p.Apply` recorded no
// selection, the SSA builder resolved `Apply` as a bare identifier, and `p`
// ended up with no referrers at all: a dead store that is nothing of the kind.
// grafana/tempo's `modules/frontend/pipeline/pipeline.go:152` builds its async
// middleware exactly this way.
//
// The generic *struct* and *map* forms were never affected — a composite
// literal and a two-argument `T[A, B](v)` take other paths — so the fixture
// pins the shapes that were broken next to the ones that were not.

type Fn[T any] func(T) T

func (f Fn[T]) Apply(x T) T { return f(x) }

type Vec[T any] []T

func (v Vec[T]) Len() int { return len(v) }

type Pair[K comparable, V any] map[K]V

func (p Pair[K, V]) Size() int { return len(p) }

// A conversion to a generic func type, then a method call on the result.
func genericFuncConv(g func(int) int) int {
	p := Fn[int](g)
	return p.Apply(1)
}

// The same through an intermediate variable.
func genericFuncConvVia(g func(int) int) int {
	p := Fn[int](g)
	q := p
	return q.Apply(1)
}

// A conversion to a generic slice type.
func genericSliceConv(xs []int) int {
	p := Vec[int](xs)
	return p.Len()
}

// Two type arguments: an `IndexListExpr`, which already worked.
func genericMapConv(m map[string]int) int {
	p := Pair[string, int](m)
	return p.Size()
}

// A genuinely dead store of the same shape, so "silent" cannot mean "the
// checker never looks at this function".
func genericDead(g func(int) int) int {
	p := Fn[int](g)     // FINDING: this value of p is never used
	p = Fn[int](func(x int) int { return x - 1 })
	return p.Apply(1)
}
