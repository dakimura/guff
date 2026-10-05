// Package sumdep is a module of its own: guff never analyses it, so the sum
// types its importers see come from its source, not from a fact.
package sumdep

//sumtype:decl
type Shape interface{ shape() }

type Circle struct{}

func (Circle) shape() {}

type Square struct{}

func (*Square) shape() {}

// An alias is not a variant.
type Round = Circle

// A generic type is skipped.
type Poly[T any] struct{ v T }

func (Poly[T]) shape() {}

// Not sealed: an error of this package's own pass, never shown to importers.
//
//sumtype:decl
type Open interface{ Area() float64 }

//sumtype:decl
type Event interface{ event() }

type Click struct{}

func (Click) event() {}

type key struct{}

func (key) event() {}

// NewKey hands out the unexported variant.
func NewKey() Event { return key{} }
