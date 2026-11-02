// Package vname is imported as example.com/vname/v2: its name is not the
// last element of its path.
package vname

type R struct{}

func (R) N[T ~int](n T) T { return n }

func IntN(n int) int { return n }
