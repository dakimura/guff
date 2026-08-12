// Package generics carries the generic *forms* the OSS corpus does not.
//
// corpus/shapes.py measured it: no gated target declares a method on a
// generic type, and none declares a generic type alias at all. Three bugs
// found on 2026-08-12 lived in exactly that gap — revive rendered a generic
// receiver by debug-printing the AST, revive's private-receiver skip never
// fired because that debug string starts with an upper-case letter, and
// nilerr never saw method bodies at all.
package generics

import "errors"

// Box is an exported generic type.
type Box[T any] struct{ v T }

// GetOK has a comment, so `exported` stays quiet about it.
func (b *Box[T]) GetOK() T { return b.v }

func (b *Box[T]) Get() T { return b.v }

func (b Box[T]) Peek() T { return b.v }

// Pair is a generic type with two parameters (an IndexListExpr receiver,
// which upstream unpacks the same way and confusing-naming does not).
type Pair[K comparable, V any] struct {
	k K
	v V
}

func (p *Pair[K, V]) First() K { return p.k }

func (p *Pair[K, V]) Second() V { return p.v }

// hidden is unexported: `exported` skips methods on it even though the method
// names are exported. Rendering the receiver as anything upper-case turns this
// into a false positive on every generic type in a package.
type hidden[T any] struct{ v T }

func (h *hidden[T]) Exported() T { return h.v }

// Number is a type set with tilde terms and a union.
type Number interface {
	~int | ~int64 | ~float64
}

// Last returns the final element, or the zero value.
//
// The body deliberately does no arithmetic on T. guff's type checker has no
// type-set-aware `allNumeric`/`allOrdered` (guff-types' predicates say so:
// "Type-set-aware variants (allX) are deferred"), so `total += x` here would
// make the whole package ill-typed and silently drop every type-dependent
// finding in this case. A generic type alias (`type Alias[T any] = Box[T]`,
// Go 1.24) does the same — "undefined: T". Both are recorded with minimal
// repros in docs/COMPAT-HARDENING.md; put them back here once they type-check.
func Last[T Number](xs []T) T {
	var last T
	for _, x := range xs {
		last = x
	}
	return last
}

// zeroParam dereferences new() of a type parameter: go-critic's newDeref
// returns early for those (go-critic #1272), so nothing is reported here.
func zeroParam[T any]() T { return *new(T) }

// zeroInt is the same expression on a concrete type, where newDeref does fire.
func zeroInt() int { return *new(int) }

// zeroBox instantiates the generic type first; the suggestion names the
// instantiated type.
func zeroBox() Box[int] { return *new(Box[int]) }

var errFailed = errors.New("failed")

func check(v int) error {
	if v < 0 {
		return errFailed
	}
	return nil
}

// Load returns nil after a non-nil error — nilerr's shape, inside a method on
// a generic type. Every SSA-based checker reads SrcFuncs, and guff's SrcFuncs
// held package-level functions only, so this whole body was invisible.
func (b *Box[T]) Load(v int) error {
	if err := check(v); err != nil {
		return nil
	}
	return nil
}

// Store is the same shape in a plain function, for contrast.
func Store(v int) error {
	if err := check(v); err != nil {
		return nil
	}
	return nil
}
